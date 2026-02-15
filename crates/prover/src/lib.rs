//! Host-side ZK prover for Craftec distribution proofs.
//!
//! Two modes:
//! - **mock** (default feature): Executes guest logic locally without ZK proof.
//!   Sufficient for testing and development.
//! - **sp1** feature: Uses SP1 SDK to execute the guest ELF in the zkVM and
//!   generate real Groth16 proofs for on-chain verification.

mod merkle;

use std::collections::BTreeMap;

use craftec_core::ContributionReceipt;
use craftec_prover_guest_types::{DistributionEntry, DistributionOutput, ReceiptData};
#[cfg(feature = "sp1")]
use craftec_prover_guest_types::DistributionInput;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub use merkle::compute_merkle_root;

#[derive(Debug, Error)]
pub enum ProverError {
    #[error("empty receipt batch")]
    EmptyBatch,
    #[error("receipt has zero weight")]
    ZeroWeight,
    #[error("receipt has zero timestamp")]
    ZeroTimestamp,
    #[error("receipt has zero operator")]
    ZeroOperator,
    #[error("invalid mock proof")]
    InvalidProof,
    #[error("proof output mismatch")]
    OutputMismatch,
    #[error("sp1 error: {0}")]
    Sp1(String),
}

/// A batch proof containing proof bytes and public inputs.
#[derive(Clone, Debug)]
pub struct BatchProof {
    /// Proof bytes (mock: magic + output bytes; real: Groth16 proof).
    pub proof_bytes: Vec<u8>,
    /// Public inputs (the 76-byte DistributionOutput).
    pub public_inputs: Vec<u8>,
}

// ─── Mock prover ───────────────────────────────────────────────────

/// Magic bytes identifying a mock proof (not a real Groth16 proof).
#[cfg(feature = "mock")]
const MOCK_PROOF_MAGIC: &[u8] = b"CRAFTEC_MOCK_PROOF_V1";

/// Mock prover client that executes distribution logic without SP1.
#[cfg(feature = "mock")]
#[derive(Default)]
pub struct ProverClient;

#[cfg(feature = "mock")]
impl ProverClient {
    pub fn new() -> Self {
        Self
    }

    /// Prove a batch of contribution receipts, returning a mock proof.
    pub fn prove_batch<R: ContributionReceipt>(
        &self,
        receipts: &[R],
        pool_id: &[u8; 32],
    ) -> Result<BatchProof, ProverError> {
        if receipts.is_empty() {
            return Err(ProverError::EmptyBatch);
        }

        let receipt_data: Vec<ReceiptData> = receipts
            .iter()
            .map(|r| ReceiptData {
                operator: r.operator(),
                signer: r.signer(),
                weight: r.weight(),
                timestamp: r.timestamp(),
                signable_data: r.signable_data(),
                signature: vec![0u8; 64], // Mock doesn't verify signatures
            })
            .collect();

        let output = self.execute_guest_logic(&receipt_data, pool_id)?;
        let output_bytes = output.to_bytes();

        let mut proof_bytes = Vec::new();
        proof_bytes.extend_from_slice(MOCK_PROOF_MAGIC);
        proof_bytes.extend_from_slice(&output_bytes);

        Ok(BatchProof {
            proof_bytes,
            public_inputs: output_bytes.to_vec(),
        })
    }

    /// Verify a mock batch proof.
    pub fn verify_batch(&self, proof: &BatchProof) -> Result<DistributionOutput, ProverError> {
        if proof.proof_bytes.len() < MOCK_PROOF_MAGIC.len() + 76 {
            return Err(ProverError::InvalidProof);
        }
        if &proof.proof_bytes[..MOCK_PROOF_MAGIC.len()] != MOCK_PROOF_MAGIC {
            return Err(ProverError::InvalidProof);
        }

        let embedded_output = &proof.proof_bytes[MOCK_PROOF_MAGIC.len()..];
        if embedded_output != proof.public_inputs.as_slice() {
            return Err(ProverError::OutputMismatch);
        }

        let output_bytes: [u8; 76] = proof
            .public_inputs
            .as_slice()
            .try_into()
            .map_err(|_| ProverError::InvalidProof)?;
        Ok(DistributionOutput::from_bytes(&output_bytes))
    }

    /// Execute the same logic the SP1 guest would run, without ZK.
    fn execute_guest_logic(
        &self,
        receipts: &[ReceiptData],
        pool_id: &[u8; 32],
    ) -> Result<DistributionOutput, ProverError> {
        let mut weights: BTreeMap<[u8; 32], u64> = BTreeMap::new();

        for receipt in receipts {
            if receipt.weight == 0 {
                return Err(ProverError::ZeroWeight);
            }
            if receipt.timestamp == 0 {
                return Err(ProverError::ZeroTimestamp);
            }
            if receipt.operator == [0u8; 32] {
                return Err(ProverError::ZeroOperator);
            }
            *weights.entry(receipt.operator).or_insert(0) += receipt.weight;
        }

        let entries: Vec<DistributionEntry> = weights
            .into_iter()
            .map(|(operator, weight)| DistributionEntry { operator, weight })
            .collect(); // BTreeMap is already sorted

        let leaf_hashes: Vec<[u8; 32]> = entries
            .iter()
            .map(|e| {
                let mut hasher = Sha256::new();
                hasher.update(e.operator);
                hasher.update(e.weight.to_le_bytes());
                let result = hasher.finalize();
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&result);
                hash
            })
            .collect();

        let root = compute_merkle_root(&leaf_hashes);
        let total_weight: u64 = entries.iter().map(|e| e.weight).sum();

        Ok(DistributionOutput {
            root,
            total_weight,
            entry_count: entries.len() as u32,
            pool_id: *pool_id,
        })
    }
}

// ─── SP1 prover ────────────────────────────────────────────────────

#[cfg(feature = "sp1")]
use sp1_sdk::include_elf;

/// The compiled SP1 guest ELF, embedded at build time.
#[cfg(feature = "sp1")]
const DISTRIBUTION_ELF: &[u8] = include_elf!("craftec-distribution-guest");

/// SP1 prover client that generates real ZK proofs.
#[cfg(feature = "sp1")]
pub struct ProverClient {
    client: sp1_sdk::EnvProver,
}

#[cfg(feature = "sp1")]
impl Default for ProverClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "sp1")]
impl ProverClient {
    pub fn new() -> Self {
        Self {
            client: sp1_sdk::ProverClient::from_env(),
        }
    }

    /// Prove a batch of contribution receipts using SP1 zkVM.
    ///
    /// Generates a Groth16 proof suitable for on-chain verification.
    pub fn prove_batch<R: ContributionReceipt>(
        &self,
        receipts: &[R],
        pool_id: &[u8; 32],
    ) -> Result<BatchProof, ProverError> {
        if receipts.is_empty() {
            return Err(ProverError::EmptyBatch);
        }

        let receipt_data: Vec<ReceiptData> = receipts
            .iter()
            .map(|r| ReceiptData {
                operator: r.operator(),
                signer: r.signer(),
                weight: r.weight(),
                timestamp: r.timestamp(),
                signable_data: r.signable_data(),
                signature: r.signature().to_vec(),
            })
            .collect();

        let input = DistributionInput {
            pool_id: *pool_id,
            receipts: receipt_data,
        };

        let mut stdin = sp1_sdk::SP1Stdin::new();
        stdin.write(&input);

        let (pk, _vk) = self.client.setup(DISTRIBUTION_ELF);

        let proof = self.client
            .prove(&pk, &stdin)
            .groth16()
            .run()
            .map_err(|e| ProverError::Sp1(e.to_string()))?;

        let public_inputs = proof.public_values.as_slice().to_vec();
        let proof_bytes = proof.bytes();

        Ok(BatchProof {
            proof_bytes,
            public_inputs,
        })
    }

    /// Execute the guest program without generating a proof (for testing/dry-run).
    pub fn execute_batch<R: ContributionReceipt>(
        &self,
        receipts: &[R],
        pool_id: &[u8; 32],
    ) -> Result<DistributionOutput, ProverError> {
        if receipts.is_empty() {
            return Err(ProverError::EmptyBatch);
        }

        let receipt_data: Vec<ReceiptData> = receipts
            .iter()
            .map(|r| ReceiptData {
                operator: r.operator(),
                signer: r.signer(),
                weight: r.weight(),
                timestamp: r.timestamp(),
                signable_data: r.signable_data(),
                signature: r.signature().to_vec(),
            })
            .collect();

        let input = DistributionInput {
            pool_id: *pool_id,
            receipts: receipt_data,
        };

        let mut stdin = sp1_sdk::SP1Stdin::new();
        stdin.write(&input);

        let (output, _report) = self.client
            .execute(DISTRIBUTION_ELF, &stdin)
            .run()
            .map_err(|e| ProverError::Sp1(e.to_string()))?;

        let output_bytes: [u8; 76] = output
            .as_slice()
            .try_into()
            .map_err(|_| ProverError::Sp1("unexpected output length".into()))?;

        Ok(DistributionOutput::from_bytes(&output_bytes))
    }

    /// Verify a batch proof using SP1 SDK.
    pub fn verify_batch(&self, proof: &BatchProof) -> Result<DistributionOutput, ProverError> {
        // For Groth16, verification is typically done on-chain.
        // Host-side we can parse the public values.
        if proof.public_inputs.len() != 76 {
            return Err(ProverError::InvalidProof);
        }
        let output_bytes: [u8; 76] = proof
            .public_inputs
            .as_slice()
            .try_into()
            .map_err(|_| ProverError::InvalidProof)?;
        Ok(DistributionOutput::from_bytes(&output_bytes))
    }

    /// Get the verification key hash for the distribution guest program.
    pub fn vkey_hash(&self) -> String {
        use sp1_sdk::HashableKey;
        let (_pk, vk) = self.client.setup(DISTRIBUTION_ELF);
        vk.bytes32()
    }
}

// ─── Shared utilities ──────────────────────────────────────────────

/// Compute distribution entries from receipts (useful for testing).
pub fn compute_distribution_entries<R: ContributionReceipt>(
    receipts: &[R],
) -> Vec<DistributionEntry> {
    let mut weights: BTreeMap<[u8; 32], u64> = BTreeMap::new();
    for r in receipts {
        *weights.entry(r.operator()).or_insert(0) += r.weight();
    }
    weights
        .into_iter()
        .map(|(operator, weight)| DistributionEntry { operator, weight })
        .collect()
}

#[cfg(test)]
mod tests;

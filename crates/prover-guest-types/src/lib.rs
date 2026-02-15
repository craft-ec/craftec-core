#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Input to the distribution guest program.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DistributionInput {
    /// The pool identifier (creator pubkey or pool pubkey).
    pub pool_id: [u8; 32],
    /// Batch of receipts to verify and aggregate.
    pub receipts: Vec<ReceiptData>,
}

/// A receipt in serialized form, generic across crafts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceiptData {
    /// Operator (node) public key — who earned this receipt.
    pub operator: [u8; 32],
    /// Signer public key — who issued/signed this receipt.
    pub signer: [u8; 32],
    /// Contribution weight (bytes forwarded, PDP proofs, etc.).
    pub weight: u64,
    /// Timestamp of the contribution.
    pub timestamp: u64,
    /// The signable payload (craft-specific canonical bytes).
    pub signable_data: Vec<u8>,
    /// Ed25519 signature over signable_data, by signer.
    pub signature: Vec<u8>,
}

/// Committed output from the guest program (76 bytes).
/// This is what the on-chain verifier reads after proof verification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistributionOutput {
    /// Merkle root of (operator, weight) leaves.
    pub root: [u8; 32],
    /// Sum of all operator weights.
    pub total_weight: u64,
    /// Number of unique operators in the distribution.
    pub entry_count: u32,
    /// Pool identifier — ties the proof to a specific pool.
    pub pool_id: [u8; 32],
}

impl DistributionOutput {
    /// Serialize to 76 bytes (deterministic layout).
    pub fn to_bytes(&self) -> [u8; 76] {
        let mut buf = [0u8; 76];
        buf[0..32].copy_from_slice(&self.root);
        buf[32..40].copy_from_slice(&self.total_weight.to_le_bytes());
        buf[40..44].copy_from_slice(&self.entry_count.to_le_bytes());
        buf[44..76].copy_from_slice(&self.pool_id);
        buf
    }

    /// Deserialize from 76 bytes.
    pub fn from_bytes(bytes: &[u8; 76]) -> Self {
        let mut root = [0u8; 32];
        root.copy_from_slice(&bytes[0..32]);
        let total_weight = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let entry_count = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        let mut pool_id = [0u8; 32];
        pool_id.copy_from_slice(&bytes[44..76]);
        Self {
            root,
            total_weight,
            entry_count,
            pool_id,
        }
    }
}

/// A leaf in the distribution Merkle tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistributionEntry {
    /// Operator public key.
    pub operator: [u8; 32],
    /// Aggregated weight for this operator.
    pub weight: u64,
}

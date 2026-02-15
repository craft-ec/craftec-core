//! Batch builder: groups receipts by pool, computes per-operator weights,
//! generates proofs via craftec-prover.

use std::collections::BTreeMap;

use craftec_core::ContributionReceipt;
use craftec_prover::{BatchProof, ProverClient};
use craftec_settlement::StorageReceipt;
use tracing::info;

use crate::AggregatorError;

/// Result of building a batch for a single pool.
#[derive(Debug, Clone)]
pub struct PoolBatch {
    /// The pool pubkey this batch belongs to.
    pub pool_pubkey: [u8; 32],
    /// Per-operator weights (sorted by operator pubkey).
    pub weights: BTreeMap<[u8; 32], u64>,
    /// Number of receipts in this batch.
    pub receipt_count: usize,
    /// Total weight across all operators.
    pub total_weight: u64,
    /// The ZK proof (mock or real).
    pub proof: BatchProof,
}

/// Builds batches from collected receipts and generates proofs.
pub struct BatchBuilder {
    prover: ProverClient,
}

impl BatchBuilder {
    pub fn new() -> Self {
        Self {
            prover: ProverClient::new(),
        }
    }

    /// Build a batch for a pool's receipts and generate a proof.
    pub fn build(
        &self,
        pool_pubkey: [u8; 32],
        receipts: &[StorageReceipt],
    ) -> Result<PoolBatch, AggregatorError> {
        if receipts.is_empty() {
            return Err(AggregatorError::EmptyBatch);
        }

        // Compute per-operator weights
        let mut weights = BTreeMap::new();
        for receipt in receipts {
            *weights.entry(receipt.operator()).or_insert(0u64) += receipt.weight();
        }

        let total_weight: u64 = weights.values().sum();

        info!(
            pool = hex::encode(pool_pubkey),
            operators = weights.len(),
            receipts = receipts.len(),
            total_weight,
            "building batch"
        );

        // Generate proof
        let proof = self
            .prover
            .prove_batch(receipts, &pool_pubkey)?;

        Ok(PoolBatch {
            pool_pubkey,
            weights,
            receipt_count: receipts.len(),
            total_weight,
            proof,
        })
    }
}

impl Default for BatchBuilder {
    fn default() -> Self {
        Self::new()
    }
}

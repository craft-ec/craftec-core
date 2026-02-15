//! Distribution poster: builds settlement transactions using craftec-settlement
//! instruction builders.

use craftec_settlement::instruction;
use tracing::info;

use crate::batch::PoolBatch;

/// Represents a built transaction ready for submission.
#[derive(Debug, Clone)]
pub struct BuiltTransaction {
    /// The serialized instruction data.
    pub instruction_data: Vec<u8>,
    /// Human-readable description for logging.
    pub description: String,
}

/// Builds settlement transactions from pool batches.
///
/// In production, this would also handle Solana RPC submission.
/// For now, it produces the serialized instruction data.
pub struct DistributionPoster {
    /// Program ID (settlement program).
    pub program_id: [u8; 32],
}

impl DistributionPoster {
    pub fn new(program_id: [u8; 32]) -> Self {
        Self { program_id }
    }

    /// Build a `post_distribution` transaction for a pool batch.
    pub fn build_post_distribution(&self, batch: &PoolBatch) -> BuiltTransaction {
        let root: [u8; 32] = {
            // Extract root from public_inputs (first 32 bytes)
            let pi = &batch.proof.public_inputs;
            if pi.len() >= 32 {
                let mut r = [0u8; 32];
                r.copy_from_slice(&pi[..32]);
                r
            } else {
                [0u8; 32]
            }
        };

        let data = instruction::post_distribution(
            batch.pool_pubkey,
            root,
            batch.total_weight,
            batch.proof.proof_bytes.clone(),
            batch.proof.public_inputs.clone(),
        );

        info!(
            pool = hex::encode(batch.pool_pubkey),
            total_weight = batch.total_weight,
            operators = batch.weights.len(),
            "built post_distribution transaction"
        );

        BuiltTransaction {
            instruction_data: data,
            description: format!(
                "post_distribution(pool={}, weight={}, operators={})",
                hex::encode(batch.pool_pubkey),
                batch.total_weight,
                batch.weights.len(),
            ),
        }
    }
}

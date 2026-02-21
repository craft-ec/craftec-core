//! Craftec Aggregator Service
//!
//! Collects `StorageReceipt`s from storage nodes via direct P2P push, batches them
//! per creator pool per epoch, generates ZK proofs via `craftec-prover`, and
//! posts distributions on-chain via `craftec-settlement` instruction builders.

pub mod batch;
pub mod collector;
pub mod poster;
pub mod service;
#[cfg(test)]
mod tests;

pub use collector::{ReceiptMessage, ReceiptCollector};
pub use batch::BatchBuilder;
pub use poster::{BuiltTransaction, DistributionPoster, DryRunSubmitter, SolanaRpcPoster, SubmitResult, TransactionSubmitter};
pub use service::{AggregatorConfig, AggregatorService};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AggregatorError {
    #[error("receipt signature verification failed")]
    InvalidSignature,
    #[error("receipt timestamp {0} outside epoch window [{1}, {2})")]
    TimestampOutOfRange(u64, u64, u64),
    #[error("duplicate receipt: {0}")]
    DuplicateReceipt(String),
    #[error("unknown pool for content_id")]
    UnknownPool,
    #[error("prover error: {0}")]
    Prover(#[from] craftec_prover::ProverError),
    #[error("no receipts for pool in epoch")]
    EmptyBatch,
    #[error("serialization error: {0}")]
    Serialization(String),
}

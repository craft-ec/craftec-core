//! Receipt collector: validates, deduplicates, and queues StorageReceipts by pool.

use std::collections::{HashMap, HashSet};

use craftec_settlement::StorageReceipt;
use ed25519_dalek::{Signature, VerifyingKey};
use tracing::{debug, warn};

use crate::AggregatorError;

/// Collects and validates StorageReceipts, grouping them by creator pool.
pub struct ReceiptCollector {
    /// Receipts grouped by pool pubkey (as hex string for simplicity).
    pool_receipts: HashMap<[u8; 32], Vec<StorageReceipt>>,
    /// Dedup set: SHA256 hashes of already-seen receipts.
    seen: HashSet<[u8; 32]>,
    /// Epoch time window: [epoch_start, epoch_end).
    epoch_start: u64,
    epoch_end: u64,
    /// Grace period in seconds after epoch_end.
    grace_secs: u64,
    /// CID → pool pubkey mapping.
    cid_to_pool: HashMap<[u8; 32], [u8; 32]>,
}

impl ReceiptCollector {
    /// Create a new collector for a given epoch window.
    pub fn new(epoch_start: u64, epoch_end: u64, grace_secs: u64) -> Self {
        Self {
            pool_receipts: HashMap::new(),
            seen: HashSet::new(),
            epoch_start,
            epoch_end,
            grace_secs,
            cid_to_pool: HashMap::new(),
        }
    }

    /// Register a CID → pool mapping.
    pub fn register_pool(&mut self, content_id: [u8; 32], pool_pubkey: [u8; 32]) {
        self.cid_to_pool.insert(content_id, pool_pubkey);
    }

    /// Number of unique receipts collected.
    pub fn receipt_count(&self) -> usize {
        self.pool_receipts.values().map(|v| v.len()).sum()
    }

    /// Get all pools that have receipts.
    pub fn pools(&self) -> Vec<[u8; 32]> {
        self.pool_receipts.keys().copied().collect()
    }

    /// Take all receipts for a given pool, draining them from the collector.
    pub fn take_receipts(&mut self, pool: &[u8; 32]) -> Vec<StorageReceipt> {
        self.pool_receipts.remove(pool).unwrap_or_default()
    }

    /// Validate and ingest a receipt. Returns Ok(()) if accepted.
    pub fn ingest(&mut self, receipt: StorageReceipt) -> Result<(), AggregatorError> {
        // 1. Timestamp check
        let deadline = self.epoch_end + self.grace_secs;
        if receipt.timestamp < self.epoch_start || receipt.timestamp >= deadline {
            return Err(AggregatorError::TimestampOutOfRange(
                receipt.timestamp,
                self.epoch_start,
                deadline,
            ));
        }

        // 2. Non-zero field checks
        if receipt.storage_node == [0u8; 32] || receipt.content_id == [0u8; 32] {
            return Err(AggregatorError::InvalidSignature);
        }

        // 3. Deduplication
        let dedup = receipt.dedup_hash();
        if self.seen.contains(&dedup) {
            return Err(AggregatorError::DuplicateReceipt(hex::encode(dedup)));
        }

        // 4. Signature verification
        verify_receipt_signature(&receipt)?;

        // 5. Pool mapping
        let pool = self
            .cid_to_pool
            .get(&receipt.content_id)
            .copied()
            .ok_or(AggregatorError::UnknownPool)?;

        // Accept
        self.seen.insert(dedup);
        self.pool_receipts.entry(pool).or_default().push(receipt);
        debug!(pool = hex::encode(pool), "receipt ingested");

        Ok(())
    }

    /// Reset for a new epoch.
    pub fn reset(&mut self, epoch_start: u64, epoch_end: u64) {
        self.pool_receipts.clear();
        self.seen.clear();
        self.epoch_start = epoch_start;
        self.epoch_end = epoch_end;
    }
}

/// Verify the challenger's ed25519 signature on a StorageReceipt.
fn verify_receipt_signature(receipt: &StorageReceipt) -> Result<(), AggregatorError> {
    let verifying_key = VerifyingKey::from_bytes(&receipt.challenger)
        .map_err(|_| AggregatorError::InvalidSignature)?;

    if receipt.signature.len() != 64 {
        return Err(AggregatorError::InvalidSignature);
    }

    let sig_bytes: [u8; 64] = receipt
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| AggregatorError::InvalidSignature)?;

    let signature =
        Signature::from_bytes(&sig_bytes);

    let signable = receipt.signable_bytes();

    verifying_key
        .verify_strict(&signable, &signature)
        .map_err(|_| {
            warn!(
                challenger = hex::encode(receipt.challenger),
                "signature verification failed"
            );
            AggregatorError::InvalidSignature
        })
}

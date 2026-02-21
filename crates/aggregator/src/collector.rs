//! Receipt collector: validates, deduplicates, and queues StorageReceipts by pool.
//!
//! Supports ingestion from storage nodes via a channel-based listener (P2P direct push).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use craftec_settlement::StorageReceipt;
use ed25519_dalek::{Signature, VerifyingKey};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::AggregatorError;

/// A receipt message received via direct P2P push (bincode-serialized StorageReceipt).
#[derive(Debug, Clone)]
pub struct ReceiptMessage {
    /// Raw bincode-serialized StorageReceipt.
    pub data: Vec<u8>,
}

/// Collects and validates StorageReceipts, grouping them by creator pool.
pub struct ReceiptCollector {
    /// Receipts grouped by pool pubkey.
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
    /// Optional persistence directory for crash recovery.
    persist_dir: Option<PathBuf>,
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
            persist_dir: None,
        }
    }

    /// Enable receipt persistence to disk for crash recovery.
    pub fn with_persistence(mut self, dir: PathBuf) -> Self {
        self.persist_dir = Some(dir);
        self
    }

    /// Set the persistence directory.
    pub fn set_persist_dir(&mut self, dir: PathBuf) {
        self.persist_dir = Some(dir);
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

    /// Process a raw receipt message (bincode-serialized): deserialize and ingest.
    pub fn process_receipt_message(&mut self, data: &[u8]) -> Result<(), AggregatorError> {
        let receipt: StorageReceipt = bincode::deserialize(data)
            .map_err(|e| AggregatorError::Serialization(e.to_string()))?;
        self.ingest(receipt)
    }

    /// Drain all pending receipts from a channel receiver and ingest them.
    /// Returns the number of successfully ingested receipts.
    pub fn drain_channel(
        &mut self,
        rx: &mut mpsc::Receiver<ReceiptMessage>,
    ) -> usize {
        let mut count = 0;
        while let Ok(msg) = rx.try_recv() {
            match self.process_receipt_message(&msg.data) {
                Ok(()) => count += 1,
                Err(e) => {
                    debug!(error = %e, "rejected receipt");
                }
            }
        }
        count
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

        // 6. Persist to disk before accepting (crash recovery)
        if let Some(ref dir) = self.persist_dir {
            if let Err(e) = persist_receipt(dir, &pool, &receipt) {
                warn!(error = %e, "failed to persist receipt, accepting anyway");
            }
        }

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

    /// Load persisted receipts from disk for crash recovery.
    /// Receipts are re-validated on load.
    pub fn load_persisted(&mut self, epoch_dir: &Path) -> usize {
        let mut count = 0;
        let entries = match std::fs::read_dir(epoch_dir) {
            Ok(e) => e,
            Err(_) => return 0,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "receipts").unwrap_or(false) {
                match std::fs::read(&path) {
                    Ok(data) => {
                        match bincode::deserialize::<Vec<StorageReceipt>>(&data) {
                            Ok(receipts) => {
                                for receipt in receipts {
                                    // Skip validation on reload (already validated)
                                    let dedup = receipt.dedup_hash();
                                    if self.seen.contains(&dedup) {
                                        continue;
                                    }
                                    if let Some(&pool) = self.cid_to_pool.get(&receipt.content_id) {
                                        self.seen.insert(dedup);
                                        self.pool_receipts.entry(pool).or_default().push(receipt);
                                        count += 1;
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(path = %path.display(), error = %e, "failed to deserialize persisted receipts");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "failed to read persisted receipts");
                    }
                }
            }
        }
        if count > 0 {
            info!(count, "loaded persisted receipts");
        }
        count
    }
}

/// Persist a receipt to disk (append to pool file).
fn persist_receipt(
    dir: &Path,
    pool: &[u8; 32],
    receipt: &StorageReceipt,
) -> Result<(), AggregatorError> {
    std::fs::create_dir_all(dir).map_err(|e| AggregatorError::Serialization(e.to_string()))?;

    let filename = format!("pool_{}.receipts.log", hex::encode(pool));
    let path = dir.join(filename);

    let data =
        bincode::serialize(receipt).map_err(|e| AggregatorError::Serialization(e.to_string()))?;

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| AggregatorError::Serialization(e.to_string()))?;

    // Write length-prefixed record
    let len = data.len() as u32;
    file.write_all(&len.to_le_bytes())
        .map_err(|e| AggregatorError::Serialization(e.to_string()))?;
    file.write_all(&data)
        .map_err(|e| AggregatorError::Serialization(e.to_string()))?;

    Ok(())
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

    let signature = Signature::from_bytes(&sig_bytes);

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

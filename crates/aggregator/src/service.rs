//! Top-level aggregator service with epoch loop.
//!
//! Integrates gossipsub receipt collection, batch building, proof generation,
//! and on-chain distribution posting.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use tokio::time;
use tracing::{error, info, warn};

use crate::batch::BatchBuilder;
use crate::collector::{GossipReceiptMessage, ReceiptCollector};
use crate::poster::{BuiltTransaction, DistributionPoster, TransactionSubmitter};
use crate::AggregatorError;

/// Configuration for the aggregator service.
#[derive(Debug, Clone)]
pub struct AggregatorConfig {
    /// Epoch duration in seconds (default: 3600 = 1 hour).
    pub epoch_duration_secs: u64,
    /// Grace period for late receipts in seconds (default: 300 = 5 min).
    pub grace_period_secs: u64,
    /// Settlement program ID.
    pub program_id: [u8; 32],
    /// Maximum receipts per batch.
    pub max_batch_size: usize,
    /// Optional data directory for receipt persistence.
    pub data_dir: Option<PathBuf>,
    /// Solana RPC URL (for the poster).
    pub solana_rpc_url: Option<String>,
    /// Whether to submit claims for operators (or let them claim themselves).
    pub submit_claims: bool,
}

impl Default for AggregatorConfig {
    fn default() -> Self {
        Self {
            epoch_duration_secs: 3600,
            grace_period_secs: 300,
            program_id: [0u8; 32],
            max_batch_size: 1000,
            data_dir: None,
            solana_rpc_url: None,
            submit_claims: false,
        }
    }
}

/// The main aggregator service. Runs an epoch loop that:
/// 1. Collects receipts during the epoch window (from gossipsub channel)
/// 2. At epoch boundary, builds batches and generates proofs
/// 3. Posts distributions on-chain via the submitter
pub struct AggregatorService {
    config: AggregatorConfig,
    collector: ReceiptCollector,
    batch_builder: BatchBuilder,
    poster: DistributionPoster,
    current_epoch: u64,
}

impl AggregatorService {
    pub fn new(config: AggregatorConfig) -> Self {
        let now = current_timestamp();
        let epoch_start = align_to_epoch(now, config.epoch_duration_secs);
        let epoch_end = epoch_start + config.epoch_duration_secs;

        let mut collector = ReceiptCollector::new(epoch_start, epoch_end, config.grace_period_secs);

        // Set up persistence if data_dir is configured
        if let Some(ref dir) = config.data_dir {
            let epoch_dir = dir.join(format!("epoch_{}", epoch_start / config.epoch_duration_secs));
            collector.set_persist_dir(epoch_dir);
        }

        let batch_builder = BatchBuilder::new();
        let poster = DistributionPoster::new(config.program_id);

        Self {
            current_epoch: epoch_start / config.epoch_duration_secs,
            config,
            collector,
            batch_builder,
            poster,
        }
    }

    /// Create a service with a specific epoch (for testing).
    pub fn new_with_epoch(config: AggregatorConfig, epoch_start: u64, epoch_end: u64) -> Self {
        let collector = ReceiptCollector::new(epoch_start, epoch_end, config.grace_period_secs);
        let batch_builder = BatchBuilder::new();
        let poster = DistributionPoster::new(config.program_id);

        Self {
            current_epoch: epoch_start / config.epoch_duration_secs,
            config,
            collector,
            batch_builder,
            poster,
        }
    }

    /// Get a mutable reference to the collector (for ingesting receipts).
    pub fn collector_mut(&mut self) -> &mut ReceiptCollector {
        &mut self.collector
    }

    /// Get the current epoch number.
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Process the end of an epoch: build batches, generate proofs, build transactions.
    /// Returns the built transactions (caller is responsible for submission).
    pub fn process_epoch_end(
        &mut self,
    ) -> Vec<Result<BuiltTransaction, AggregatorError>> {
        let pools = self.collector.pools();

        if pools.is_empty() {
            info!(epoch = self.current_epoch, "no receipts in epoch, skipping");
            self.advance_epoch();
            return vec![];
        }

        info!(
            epoch = self.current_epoch,
            pools = pools.len(),
            total_receipts = self.collector.receipt_count(),
            "processing epoch end"
        );

        let mut results = Vec::new();

        for pool in pools {
            let receipts = self.collector.take_receipts(&pool);
            if receipts.is_empty() {
                continue;
            }

            match self.batch_builder.build(pool, &receipts) {
                Ok(batch) => {
                    let tx = self.poster.build_post_distribution(&batch);
                    info!(
                        pool = hex::encode(pool),
                        description = tx.description,
                        "distribution transaction built"
                    );
                    results.push(Ok(tx));
                }
                Err(e) => {
                    error!(
                        pool = hex::encode(pool),
                        error = %e,
                        "failed to build batch"
                    );
                    results.push(Err(e));
                }
            }
        }

        self.advance_epoch();
        results
    }

    /// Advance to the next epoch, resetting the collector.
    fn advance_epoch(&mut self) {
        self.current_epoch += 1;
        let epoch_start = self.current_epoch * self.config.epoch_duration_secs;
        let epoch_end = epoch_start + self.config.epoch_duration_secs;
        self.collector.reset(epoch_start, epoch_end);

        // Update persistence directory for new epoch
        if let Some(ref dir) = self.config.data_dir {
            let epoch_dir = dir.join(format!("epoch_{}", self.current_epoch));
            self.collector.set_persist_dir(epoch_dir);
        }

        info!(epoch = self.current_epoch, "advanced to new epoch");
    }

    /// Run the epoch loop with a gossipsub receipt channel and a transaction submitter.
    ///
    /// This is the main entry point for production use. Receipts arrive via
    /// the `receipt_rx` channel (fed by a gossipsub listener), and built
    /// transactions are submitted via the `submitter`.
    pub async fn run_with_channel(
        &mut self,
        mut receipt_rx: mpsc::Receiver<GossipReceiptMessage>,
        submitter: Arc<dyn TransactionSubmitter>,
    ) {
        info!(
            epoch_duration = self.config.epoch_duration_secs,
            "aggregator service starting (channel mode)"
        );

        loop {
            // Calculate time until epoch end
            let now = current_timestamp();
            let epoch_end = (self.current_epoch + 1) * self.config.epoch_duration_secs;

            if now < epoch_end {
                let wait = Duration::from_secs(epoch_end - now);
                info!(wait_secs = wait.as_secs(), "waiting for epoch end");

                // Poll for receipts while waiting
                let poll_interval = Duration::from_secs(10);
                let mut remaining = wait;

                while remaining > Duration::ZERO {
                    let sleep_dur = remaining.min(poll_interval);
                    time::sleep(sleep_dur).await;

                    let ingested = self.collector.drain_channel(&mut receipt_rx);
                    if ingested > 0 {
                        info!(ingested, total = self.collector.receipt_count(), "drained receipts from channel");
                    }

                    remaining = remaining.saturating_sub(sleep_dur);
                }
            }

            // Grace period
            let grace = Duration::from_secs(self.config.grace_period_secs);
            time::sleep(grace).await;
            let _ = self.collector.drain_channel(&mut receipt_rx);

            // Process epoch and submit
            let results = self.process_epoch_end();
            for result in results {
                match result {
                    Ok(tx) => {
                        match submitter.submit(&tx).await {
                            Ok(result) => {
                                info!(
                                    signature = %result.signature,
                                    confirmed = result.confirmed,
                                    description = %tx.description,
                                    "distribution submitted"
                                );

                                // Optionally submit claims
                                if self.config.submit_claims {
                                    for (&operator, &weight) in &tx.weights {
                                        match submitter.submit_claim(
                                            tx.pool_pubkey,
                                            operator,
                                            weight,
                                            vec![],
                                            0,
                                        ).await {
                                            Ok(r) => info!(
                                                operator = hex::encode(operator),
                                                signature = %r.signature,
                                                "claim submitted"
                                            ),
                                            Err(e) => warn!(
                                                operator = hex::encode(operator),
                                                error = %e,
                                                "claim submission failed"
                                            ),
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!(
                                    error = %e,
                                    description = %tx.description,
                                    "distribution submission failed"
                                );
                            }
                        }
                    }
                    Err(e) => warn!(error = %e, "batch failed"),
                }
            }
        }
    }

    /// Run the epoch loop. This is the legacy entry point using a closure.
    /// The `receipt_source` closure is called to poll for new receipts.
    pub async fn run<F>(&mut self, mut receipt_source: F)
    where
        F: FnMut(&mut ReceiptCollector),
    {
        info!(
            epoch_duration = self.config.epoch_duration_secs,
            "aggregator service starting"
        );

        loop {
            let now = current_timestamp();
            let epoch_end = (self.current_epoch + 1) * self.config.epoch_duration_secs;

            if now < epoch_end {
                let wait = Duration::from_secs(epoch_end - now);
                info!(wait_secs = wait.as_secs(), "waiting for epoch end");

                let poll_interval = Duration::from_secs(10);
                let mut remaining = wait;

                while remaining > Duration::ZERO {
                    let sleep_dur = remaining.min(poll_interval);
                    time::sleep(sleep_dur).await;
                    receipt_source(&mut self.collector);
                    remaining = remaining.saturating_sub(sleep_dur);
                }
            }

            // Grace period
            let grace = Duration::from_secs(self.config.grace_period_secs);
            time::sleep(grace).await;
            receipt_source(&mut self.collector);

            // Process epoch
            let results = self.process_epoch_end();
            for result in &results {
                match result {
                    Ok(tx) => info!(description = tx.description, "ready to submit"),
                    Err(e) => warn!(error = %e, "batch failed"),
                }
            }
        }
    }
}

/// Current unix timestamp in seconds.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Align a timestamp to the start of its epoch.
fn align_to_epoch(timestamp: u64, epoch_duration: u64) -> u64 {
    (timestamp / epoch_duration) * epoch_duration
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_to_epoch() {
        assert_eq!(align_to_epoch(3650, 3600), 3600);
        assert_eq!(align_to_epoch(3600, 3600), 3600);
        assert_eq!(align_to_epoch(7199, 3600), 3600);
        assert_eq!(align_to_epoch(7200, 3600), 7200);
    }

    #[test]
    fn test_default_config() {
        let cfg = AggregatorConfig::default();
        assert_eq!(cfg.epoch_duration_secs, 3600);
        assert_eq!(cfg.grace_period_secs, 300);
        assert_eq!(cfg.max_batch_size, 1000);
        assert!(cfg.data_dir.is_none());
        assert!(cfg.solana_rpc_url.is_none());
        assert!(!cfg.submit_claims);
    }

    #[test]
    fn test_service_creation() {
        let config = AggregatorConfig::default();
        let service = AggregatorService::new(config);
        assert!(service.current_epoch() > 0);
    }

    #[test]
    fn test_empty_epoch_processing() {
        let config = AggregatorConfig::default();
        let mut service = AggregatorService::new(config);
        let results = service.process_epoch_end();
        assert!(results.is_empty());
    }
}

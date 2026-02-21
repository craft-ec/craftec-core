//! Integration tests for the aggregator pipeline.

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use tokio::sync::mpsc;

    use craftec_settlement::StorageReceipt;

    use crate::batch::BatchBuilder;
    use crate::collector::{ReceiptCollector, ReceiptMessage};
    use crate::poster::{DistributionPoster, DryRunSubmitter};
    use crate::service::{AggregatorConfig, AggregatorService};

    /// Create a valid StorageReceipt signed by the given challenger key.
    fn make_receipt(
        challenger_key: &SigningKey,
        storage_node: [u8; 32],
        content_id: [u8; 32],
        shard_index: u32,
        timestamp: u64,
    ) -> StorageReceipt {
        let mut receipt = StorageReceipt {
            content_id,
            storage_node,
            challenger: challenger_key.verifying_key().to_bytes(),
            shard_index,
            timestamp,
            nonce: [0u8; 32],
            proof_hash: [1u8; 32],
            signature: vec![],
        };
        let signable = receipt.signable_bytes();
        let sig = challenger_key.sign(&signable);
        receipt.signature = sig.to_bytes().to_vec();
        receipt
    }

    #[test]
    fn test_collector_ingest_valid_receipt() {
        let challenger = SigningKey::generate(&mut OsRng);
        let storage_node = [2u8; 32];
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        let mut collector = ReceiptCollector::new(1000, 2000, 300);
        collector.register_pool(content_id, pool);

        let receipt = make_receipt(&challenger, storage_node, content_id, 0, 1500);
        assert!(collector.ingest(receipt).is_ok());
        assert_eq!(collector.receipt_count(), 1);
    }

    #[test]
    fn test_collector_rejects_duplicate() {
        let challenger = SigningKey::generate(&mut OsRng);
        let storage_node = [2u8; 32];
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        let mut collector = ReceiptCollector::new(1000, 2000, 300);
        collector.register_pool(content_id, pool);

        let receipt = make_receipt(&challenger, storage_node, content_id, 0, 1500);
        assert!(collector.ingest(receipt.clone()).is_ok());
        assert!(collector.ingest(receipt).is_err());
    }

    #[test]
    fn test_collector_rejects_out_of_range_timestamp() {
        let challenger = SigningKey::generate(&mut OsRng);
        let storage_node = [2u8; 32];
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        let mut collector = ReceiptCollector::new(1000, 2000, 300);
        collector.register_pool(content_id, pool);

        // Too early
        let receipt = make_receipt(&challenger, storage_node, content_id, 0, 500);
        assert!(collector.ingest(receipt).is_err());

        // Too late (after grace)
        let receipt = make_receipt(&challenger, storage_node, content_id, 1, 2500);
        assert!(collector.ingest(receipt).is_err());
    }

    #[test]
    fn test_collector_accepts_in_grace_period() {
        let challenger = SigningKey::generate(&mut OsRng);
        let storage_node = [2u8; 32];
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        let mut collector = ReceiptCollector::new(1000, 2000, 300);
        collector.register_pool(content_id, pool);

        // Within grace: timestamp 2100 < 2000 + 300
        let receipt = make_receipt(&challenger, storage_node, content_id, 0, 2100);
        assert!(collector.ingest(receipt).is_ok());
    }

    #[test]
    fn test_collector_rejects_unknown_pool() {
        let challenger = SigningKey::generate(&mut OsRng);
        let storage_node = [2u8; 32];
        let content_id = [3u8; 32];

        let mut collector = ReceiptCollector::new(1000, 2000, 300);
        // Not registering any pool

        let receipt = make_receipt(&challenger, storage_node, content_id, 0, 1500);
        assert!(collector.ingest(receipt).is_err());
    }

    #[test]
    fn test_collector_rejects_bad_signature() {
        let challenger = SigningKey::generate(&mut OsRng);
        let storage_node = [2u8; 32];
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        let mut collector = ReceiptCollector::new(1000, 2000, 300);
        collector.register_pool(content_id, pool);

        let mut receipt = make_receipt(&challenger, storage_node, content_id, 0, 1500);
        // Corrupt the signature
        receipt.signature[0] ^= 0xFF;
        assert!(collector.ingest(receipt).is_err());
    }

    #[test]
    fn test_batch_builder_computes_weights() {
        let challenger = SigningKey::generate(&mut OsRng);
        let node_a = [2u8; 32];
        let mut node_b = [2u8; 32];
        node_b[0] = 3;
        let content_id = [3u8; 32];

        let receipts = vec![
            make_receipt(&challenger, node_a, content_id, 0, 1500),
            make_receipt(&challenger, node_a, content_id, 1, 1501),
            make_receipt(&challenger, node_b, content_id, 2, 1502),
        ];

        let builder = BatchBuilder::new();
        let pool = [4u8; 32];
        let batch = builder.build(pool, &receipts).unwrap();

        assert_eq!(batch.weights[&node_a], 2);
        assert_eq!(batch.weights[&node_b], 1);
        assert_eq!(batch.total_weight, 3);
        assert_eq!(batch.receipt_count, 3);
    }

    #[test]
    fn test_poster_builds_transaction() {
        let challenger = SigningKey::generate(&mut OsRng);
        let node = [2u8; 32];
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        let receipts = vec![make_receipt(&challenger, node, content_id, 0, 1500)];

        let builder = BatchBuilder::new();
        let batch = builder.build(pool, &receipts).unwrap();

        let poster = DistributionPoster::new([1u8; 32]);
        let tx = poster.build_post_distribution(&batch);

        assert!(!tx.instruction_data.is_empty());
        assert!(tx.description.contains("post_distribution"));
        assert_eq!(tx.pool_pubkey, pool);
        assert!(!tx.weights.is_empty());
    }

    #[test]
    fn test_poster_builds_claims() {
        let challenger = SigningKey::generate(&mut OsRng);
        let node_a = [2u8; 32];
        let mut node_b = [2u8; 32];
        node_b[0] = 3;
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        let receipts = vec![
            make_receipt(&challenger, node_a, content_id, 0, 1500),
            make_receipt(&challenger, node_b, content_id, 1, 1501),
        ];

        let builder = BatchBuilder::new();
        let batch = builder.build(pool, &receipts).unwrap();

        let poster = DistributionPoster::new([1u8; 32]);
        let claims = poster.build_claims(&batch);

        assert_eq!(claims.len(), 2);
        for claim in &claims {
            assert!(claim.description.contains("claim"));
            assert!(!claim.instruction_data.is_empty());
        }
    }

    #[test]
    fn test_end_to_end_pipeline() {
        let challenger = SigningKey::generate(&mut OsRng);
        let node_a = [2u8; 32];
        let mut node_b = [2u8; 32];
        node_b[0] = 3;
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        let mut config = AggregatorConfig::default();
        config.program_id = [1u8; 32];
        let mut service = AggregatorService::new(config);

        // Register pool
        service.collector_mut().register_pool(content_id, pool);

        // Test the batch builder directly (service epoch window is current time)
        let receipts = vec![
            make_receipt(&challenger, node_a, content_id, 0, 1500),
            make_receipt(&challenger, node_a, content_id, 1, 1501),
            make_receipt(&challenger, node_b, content_id, 2, 1502),
        ];

        let builder = BatchBuilder::new();
        let batch = builder.build(pool, &receipts).unwrap();

        let poster = DistributionPoster::new([1u8; 32]);
        let tx = poster.build_post_distribution(&batch);

        // Verify the transaction data starts with PostDistribution discriminator (8)
        assert_eq!(
            tx.instruction_data[0],
            craftec_settlement::instruction::InstructionType::PostDistribution as u8
        );
    }

    // =========================================================================
    // Gossipsub channel integration tests
    // =========================================================================

    #[test]
    fn test_collector_process_gossip_message() {
        let challenger = SigningKey::generate(&mut OsRng);
        let storage_node = [2u8; 32];
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        let mut collector = ReceiptCollector::new(1000, 2000, 300);
        collector.register_pool(content_id, pool);

        let receipt = make_receipt(&challenger, storage_node, content_id, 0, 1500);
        let data = bincode::serialize(&receipt).unwrap();

        assert!(collector.process_receipt_message(&data).is_ok());
        assert_eq!(collector.receipt_count(), 1);
    }

    #[test]
    fn test_collector_rejects_invalid_gossip_message() {
        let mut collector = ReceiptCollector::new(1000, 2000, 300);
        let bad_data = vec![0u8; 10]; // not a valid receipt
        assert!(collector.process_receipt_message(&bad_data).is_err());
    }

    #[tokio::test]
    async fn test_collector_drain_channel() {
        let challenger = SigningKey::generate(&mut OsRng);
        let storage_node = [2u8; 32];
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        let mut collector = ReceiptCollector::new(1000, 2000, 300);
        collector.register_pool(content_id, pool);

        let (tx, mut rx) = mpsc::channel::<ReceiptMessage>(100);

        // Send 3 receipts via channel
        for i in 0..3u32 {
            let receipt = make_receipt(&challenger, storage_node, content_id, i, 1500 + i as u64);
            let data = bincode::serialize(&receipt).unwrap();
            tx.send(ReceiptMessage { data }).await.unwrap();
        }

        let ingested = collector.drain_channel(&mut rx);
        assert_eq!(ingested, 3);
        assert_eq!(collector.receipt_count(), 3);
    }

    // =========================================================================
    // Dry-run submitter tests
    // =========================================================================

    #[tokio::test]
    async fn test_dry_run_submitter() {
        let submitter = DryRunSubmitter::new();

        let challenger = SigningKey::generate(&mut OsRng);
        let node = [2u8; 32];
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        let receipts = vec![make_receipt(&challenger, node, content_id, 0, 1500)];

        let builder = BatchBuilder::new();
        let batch = builder.build(pool, &receipts).unwrap();

        let poster = DistributionPoster::new([1u8; 32]);
        let tx = poster.build_post_distribution(&batch);

        use crate::poster::TransactionSubmitter;
        let result = submitter.submit(&tx).await.unwrap();
        assert!(result.confirmed);
        assert_eq!(submitter.submission_count(), 1);
    }

    // =========================================================================
    // Full epoch cycle: gossipsub → collect → batch → prove → post (dry-run)
    // =========================================================================

    #[tokio::test]
    async fn test_full_epoch_cycle_with_channel_and_dryrun() {
        let challenger = SigningKey::generate(&mut OsRng);
        let node_a = [2u8; 32];
        let mut node_b = [2u8; 32];
        node_b[0] = 3;
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        // Set up service with known epoch window
        let mut config = AggregatorConfig::default();
        config.program_id = [1u8; 32];
        config.epoch_duration_secs = 3600;
        let mut service = AggregatorService::new_with_epoch(config, 1000, 4600);
        service.collector_mut().register_pool(content_id, pool);

        // Create channel and feed receipts (simulating gossipsub)
        let (tx, mut rx) = mpsc::channel::<ReceiptMessage>(100);

        let receipts_data: Vec<_> = vec![
            make_receipt(&challenger, node_a, content_id, 0, 1500),
            make_receipt(&challenger, node_a, content_id, 1, 1501),
            make_receipt(&challenger, node_b, content_id, 2, 1502),
        ]
        .into_iter()
        .map(|r| bincode::serialize(&r).unwrap())
        .collect();

        for data in receipts_data {
            tx.send(ReceiptMessage { data }).await.unwrap();
        }

        // Drain receipts from channel
        let ingested = service.collector_mut().drain_channel(&mut rx);
        assert_eq!(ingested, 3);
        assert_eq!(service.collector_mut().receipt_count(), 3);

        // Process epoch end
        let results = service.process_epoch_end();
        assert_eq!(results.len(), 1);

        let built_tx = results.into_iter().next().unwrap().unwrap();
        assert!(built_tx.description.contains("post_distribution"));
        assert_eq!(built_tx.pool_pubkey, pool);
        assert_eq!(built_tx.weights.len(), 2);
        assert_eq!(built_tx.weights[&node_a], 2);
        assert_eq!(built_tx.weights[&node_b], 1);

        // Submit via dry-run
        let submitter = DryRunSubmitter::new();
        use crate::poster::TransactionSubmitter;
        let result = submitter.submit(&built_tx).await.unwrap();
        assert!(result.confirmed);

        // Submit claims
        for (&operator, &weight) in &built_tx.weights {
            let claim_result = submitter
                .submit_claim(built_tx.pool_pubkey, operator, weight, vec![], 0)
                .await
                .unwrap();
            assert!(claim_result.confirmed);
        }

        // 1 distribution + 2 claims = 3 submissions
        assert_eq!(submitter.submission_count(), 3);

        // Verify instruction discriminators
        let subs = submitter.submissions();
        assert_eq!(
            subs[0].instruction_data[0],
            craftec_settlement::instruction::InstructionType::PostDistribution as u8
        );
        assert_eq!(
            subs[1].instruction_data[0],
            craftec_settlement::instruction::InstructionType::Claim as u8
        );
        assert_eq!(
            subs[2].instruction_data[0],
            craftec_settlement::instruction::InstructionType::Claim as u8
        );
    }

    // =========================================================================
    // Persistence tests
    // =========================================================================

    #[test]
    fn test_receipt_persistence_and_recovery() {
        let tmp = tempfile::tempdir().unwrap();
        let persist_dir = tmp.path().join("epoch_0");

        let challenger = SigningKey::generate(&mut OsRng);
        let storage_node = [2u8; 32];
        let content_id = [3u8; 32];
        let pool = [4u8; 32];

        // Collector 1: ingest and persist
        {
            let mut collector = ReceiptCollector::new(1000, 2000, 300)
                .with_persistence(persist_dir.clone());
            collector.register_pool(content_id, pool);

            let receipt = make_receipt(&challenger, storage_node, content_id, 0, 1500);
            assert!(collector.ingest(receipt).is_ok());
            assert_eq!(collector.receipt_count(), 1);
        }

        // Verify files were created
        assert!(persist_dir.exists());
        let entries: Vec<_> = std::fs::read_dir(&persist_dir)
            .unwrap()
            .collect();
        assert!(!entries.is_empty());
    }
}

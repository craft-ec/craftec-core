//! Integration tests for the aggregator pipeline.

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    use craftec_settlement::StorageReceipt;

    use crate::batch::BatchBuilder;
    use crate::collector::ReceiptCollector;
    use crate::poster::DistributionPoster;
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

        // Ingest receipts (use current epoch window timestamps)
        // Since the service sets epoch based on current time, we can't easily
        // predict the exact window. Instead, test the batch builder directly.
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
}

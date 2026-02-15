use super::*;
use craftec_core::ContributionReceipt;

/// Test receipt type implementing ContributionReceipt.
#[derive(Clone)]
struct MockReceipt {
    operator: [u8; 32],
    signer: [u8; 32],
    weight: u64,
    timestamp: u64,
}

impl ContributionReceipt for MockReceipt {
    fn weight(&self) -> u64 {
        self.weight
    }
    fn signable_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.operator);
        data.extend_from_slice(&self.weight.to_le_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data
    }
    fn operator(&self) -> [u8; 32] {
        self.operator
    }
    fn signer(&self) -> [u8; 32] {
        self.signer
    }
    fn timestamp(&self) -> u64 {
        self.timestamp
    }
}

fn make_receipts(count: usize) -> Vec<MockReceipt> {
    (0..count)
        .map(|i| {
            let mut operator = [0u8; 32];
            operator[0] = (i % 5) as u8 + 1; // 5 unique operators
            let mut signer = [0u8; 32];
            signer[0] = 0xFF;
            signer[1] = i as u8;
            MockReceipt {
                operator,
                signer,
                weight: 100 + i as u64,
                timestamp: 1_000_000 + i as u64,
            }
        })
        .collect()
}

#[test]
fn test_mock_prove_batch_of_10() {
    let client = ProverClient::new();
    let receipts = make_receipts(10);
    let pool_id = [0xAA; 32];

    let proof = client.prove_batch(&receipts, &pool_id).unwrap();
    assert!(!proof.proof_bytes.is_empty());
    assert_eq!(proof.public_inputs.len(), 76);
}

#[test]
fn test_mock_verify_proof() {
    let client = ProverClient::new();
    let receipts = make_receipts(10);
    let pool_id = [0xAA; 32];

    let proof = client.prove_batch(&receipts, &pool_id).unwrap();
    let output = client.verify_batch(&proof).unwrap();

    assert_eq!(output.pool_id, pool_id);
    assert_eq!(output.entry_count, 5); // 10 receipts across 5 operators
    assert!(output.total_weight > 0);
    assert_ne!(output.root, [0u8; 32]);
}

#[test]
fn test_merkle_root_deterministic() {
    let client = ProverClient::new();
    let receipts = make_receipts(10);
    let pool_id = [0xBB; 32];

    let proof1 = client.prove_batch(&receipts, &pool_id).unwrap();
    let proof2 = client.prove_batch(&receipts, &pool_id).unwrap();

    let out1 = client.verify_batch(&proof1).unwrap();
    let out2 = client.verify_batch(&proof2).unwrap();
    assert_eq!(out1.root, out2.root);
}

#[test]
fn test_distribution_weights_sum() {
    let client = ProverClient::new();
    let receipts = make_receipts(10);
    let pool_id = [0xCC; 32];

    let proof = client.prove_batch(&receipts, &pool_id).unwrap();
    let output = client.verify_batch(&proof).unwrap();

    // Expected total: sum of 100..109 = 1045
    let expected_total: u64 = (0..10).map(|i| 100 + i as u64).sum();
    assert_eq!(output.total_weight, expected_total);
}

#[test]
fn test_distribution_entries() {
    let receipts = make_receipts(10);
    let entries = compute_distribution_entries(&receipts);

    assert_eq!(entries.len(), 5);
    let total: u64 = entries.iter().map(|e| e.weight).sum();
    let expected: u64 = (0..10).map(|i| 100 + i as u64).sum();
    assert_eq!(total, expected);
}

#[test]
fn test_empty_batch_error() {
    let client = ProverClient::new();
    let receipts: Vec<MockReceipt> = vec![];
    let pool_id = [0; 32];
    assert!(client.prove_batch(&receipts, &pool_id).is_err());
}

#[test]
fn test_zero_weight_error() {
    let client = ProverClient::new();
    let mut operator = [0u8; 32];
    operator[0] = 1;
    let receipts = vec![MockReceipt {
        operator,
        signer: [1u8; 32],
        weight: 0,
        timestamp: 1000,
    }];
    assert!(client.prove_batch(&receipts, &[0; 32]).is_err());
}

#[test]
fn test_output_serialization_roundtrip() {
    let output = DistributionOutput {
        root: [0xAA; 32],
        total_weight: 12345,
        entry_count: 42,
        pool_id: [0xBB; 32],
    };
    let bytes = output.to_bytes();
    let decoded = DistributionOutput::from_bytes(&bytes);
    assert_eq!(output, decoded);
}

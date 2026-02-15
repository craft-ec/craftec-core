//! SP1 guest program for Craftec distribution proof.
//!
//! Runs inside the SP1 RISC-V VM. Given a batch of contribution receipts:
//! 1. Verifies ed25519 signature on each receipt
//! 2. Validates fields (non-zero weight, timestamp, operator)
//! 3. Accumulates weight per operator
//! 4. Builds Merkle tree of (operator, weight) leaves (sorted by operator)
//! 5. Commits DistributionOutput (76 bytes) as public values
//!
//! The Merkle tree construction matches `compute_merkle_root` in
//! `crates/prover/src/merkle.rs`: odd-length levels duplicate the last element.

#![no_main]
sp1_zkvm::entrypoint!(main);

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use craftec_prover_guest_types::DistributionInput;

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

pub fn main() {
    let input = sp1_zkvm::io::read::<DistributionInput>();

    assert!(!input.receipts.is_empty(), "empty receipt batch");

    // 1. Verify every receipt and accumulate weights per operator
    let mut weights: BTreeMap<[u8; 32], u64> = BTreeMap::new();

    for receipt in &input.receipts {
        // Ed25519 signature verification (SP1 patches ed25519-dalek to use precompile)
        let verifying_key = VerifyingKey::from_bytes(&receipt.signer)
            .expect("invalid signer public key");

        let sig_bytes: [u8; 64] = receipt
            .signature
            .as_slice()
            .try_into()
            .expect("signature must be 64 bytes");
        let signature = Signature::from_bytes(&sig_bytes);

        use ed25519_dalek::Verifier;
        verifying_key
            .verify(&receipt.signable_data, &signature)
            .expect("invalid ed25519 signature");

        // Field validation
        assert!(receipt.weight > 0, "zero weight");
        assert!(receipt.timestamp > 0, "zero timestamp");
        assert!(receipt.operator != [0u8; 32], "zero operator");

        // Accumulate weight per operator
        *weights.entry(receipt.operator).or_insert(0) += receipt.weight;
    }

    // 2. Build sorted leaves (BTreeMap is already sorted by key)
    let leaf_hashes: Vec<[u8; 32]> = weights
        .iter()
        .map(|(operator, weight)| {
            let mut hasher = Sha256::new();
            hasher.update(operator);
            hasher.update(weight.to_le_bytes());
            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        })
        .collect();

    // 3. Compute Merkle root (matching host-side compute_merkle_root)
    let root = compute_merkle_root(&leaf_hashes);
    let total_weight: u64 = weights.values().sum();
    let entry_count = weights.len() as u32;

    // 4. Commit output (76 bytes): root(32) + total_weight(8) + entry_count(4) + pool_id(32)
    sp1_zkvm::io::commit_slice(&root);
    sp1_zkvm::io::commit_slice(&total_weight.to_le_bytes());
    sp1_zkvm::io::commit_slice(&entry_count.to_le_bytes());
    sp1_zkvm::io::commit_slice(&input.pool_id);
}

/// Compute Merkle root matching `crates/prover/src/merkle.rs`.
/// Odd-length levels duplicate the last element.
fn compute_merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    if leaves.len() == 1 {
        return leaves[0];
    }

    let mut current_level = leaves.to_vec();

    while current_level.len() > 1 {
        let mut next_level = Vec::with_capacity((current_level.len() + 1) / 2);

        for chunk in current_level.chunks(2) {
            let left = &chunk[0];
            let right = if chunk.len() == 2 {
                &chunk[1]
            } else {
                &chunk[0] // duplicate last for odd count
            };
            let mut hasher = Sha256::new();
            hasher.update(left);
            hasher.update(right);
            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            next_level.push(hash);
        }

        current_level = next_level;
    }

    current_level[0]
}

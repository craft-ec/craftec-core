//! SP1 guest program for Craftec distribution proof.
//!
//! Runs inside the SP1 RISC-V VM. Given pre-aggregated distribution entries:
//! 1. Sorts entries by operator pubkey (deterministic ordering)
//! 2. Builds Merkle tree: leaf = SHA256(operator_pubkey || weight_le)
//! 3. Pads to next power-of-2 with [0u8; 32], bottom-up SHA256(left || right)
//! 4. Commits 76 bytes: root(32) + total_weight(8) + entry_count(4) + pool_id(32)

#![no_main]
sp1_zkvm::entrypoint!(main);

use sha2::{Digest, Sha256};

use craftec_prover_guest_types::DistributionInput;

pub fn main() {
    let input = sp1_zkvm::io::read::<DistributionInput>();

    assert!(!input.entries.is_empty(), "empty distribution");

    // 1. Sort entries by operator pubkey for deterministic ordering
    let mut entries = input.entries.clone();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // 2. Compute total weight
    let total_weight: u64 = entries.iter().map(|(_, w)| w).sum();

    // 3. Build Merkle tree
    let leaves: Vec<[u8; 32]> = entries
        .iter()
        .map(|(pubkey, weight)| merkle_leaf(pubkey, *weight))
        .collect();

    let root = merkle_root(&leaves);

    // 4. Commit output fields individually for predictable byte layout
    sp1_zkvm::io::commit_slice(&root);
    sp1_zkvm::io::commit_slice(&total_weight.to_le_bytes());
    sp1_zkvm::io::commit_slice(&(entries.len() as u32).to_le_bytes());
    sp1_zkvm::io::commit_slice(&input.pool_id);
}

/// Compute a leaf hash: SHA256(operator_pubkey || weight.to_le_bytes())
fn merkle_leaf(pubkey: &[u8; 32], weight: u64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(pubkey);
    hasher.update(weight.to_le_bytes());
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Build Merkle root: pad to next power-of-2 with [0u8; 32], bottom-up SHA256(left || right).
fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    if leaves.len() == 1 {
        return leaves[0];
    }

    // Pad to next power of 2
    let n = leaves.len().next_power_of_two();
    let mut nodes: Vec<[u8; 32]> = Vec::with_capacity(n);
    nodes.extend_from_slice(leaves);
    while nodes.len() < n {
        nodes.push([0u8; 32]);
    }

    // Bottom-up merge
    while nodes.len() > 1 {
        let mut next = Vec::with_capacity(nodes.len() / 2);
        for i in (0..nodes.len()).step_by(2) {
            let mut hasher = Sha256::new();
            hasher.update(&nodes[i]);
            hasher.update(&nodes[i + 1]);
            let result = hasher.finalize();
            let mut parent = [0u8; 32];
            parent.copy_from_slice(&result);
            next.push(parent);
        }
        nodes = next;
    }

    nodes[0]
}

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Input to the distribution guest program.
///
/// Contains pre-aggregated (operator_pubkey, weight) entries and pool metadata.
/// Receipt verification happens off-chain in the host/aggregator — the guest
/// only builds the Merkle tree for on-chain compression.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DistributionInput {
    /// Pre-aggregated (operator_pubkey, weight) entries.
    pub entries: Vec<([u8; 32], u64)>,
    /// Pool identifier (creator pubkey or pool pubkey).
    pub pool_id: [u8; 32],
}

/// Committed output from the guest program (76 bytes).
/// This is what the on-chain verifier reads after proof verification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistributionOutput {
    /// Merkle root of (operator, weight) leaves.
    pub root: [u8; 32],
    /// Sum of all operator weights.
    pub total_weight: u64,
    /// Number of unique operators in the distribution.
    pub entry_count: u32,
    /// Pool identifier — ties the proof to a specific pool.
    pub pool_id: [u8; 32],
}

impl DistributionOutput {
    /// Serialize to 76 bytes (deterministic layout).
    pub fn to_bytes(&self) -> [u8; 76] {
        let mut buf = [0u8; 76];
        buf[0..32].copy_from_slice(&self.root);
        buf[32..40].copy_from_slice(&self.total_weight.to_le_bytes());
        buf[40..44].copy_from_slice(&self.entry_count.to_le_bytes());
        buf[44..76].copy_from_slice(&self.pool_id);
        buf
    }

    /// Deserialize from 76 bytes.
    pub fn from_bytes(bytes: &[u8; 76]) -> Self {
        let mut root = [0u8; 32];
        root.copy_from_slice(&bytes[0..32]);
        let total_weight = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let entry_count = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        let mut pool_id = [0u8; 32];
        pool_id.copy_from_slice(&bytes[44..76]);
        Self {
            root,
            total_weight,
            entry_count,
            pool_id,
        }
    }
}

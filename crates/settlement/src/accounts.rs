//! On-chain account types (client-side serialization).
//!
//! These types mirror the on-chain account structures. Serialized with
//! bincode for compact binary encoding matching on-chain layout.

use serde::{Deserialize, Serialize};

/// Storage tier for funded content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum StorageTier {
    Free = 0,
    Lite = 1,
    Standard = 2,
    Pro = 3,
    Enterprise = 4,
}

impl StorageTier {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Free),
            1 => Some(Self::Lite),
            2 => Some(Self::Standard),
            3 => Some(Self::Pro),
            4 => Some(Self::Enterprise),
            _ => None,
        }
    }

    /// Minimum shard ratio guaranteed by this tier.
    pub fn min_shard_ratio(&self) -> f64 {
        match self {
            Self::Free => 0.0,
            Self::Lite => 2.0,
            Self::Standard => 3.0,
            Self::Pro => 5.0,
            Self::Enterprise => 10.0,
        }
    }
}

/// Creator Pool account — one per creator, funds all their CIDs.
///
/// PDA seeds: `[b"creator_pool", creator_pubkey]`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatorPool {
    /// Creator's public key (32 bytes).
    pub creator: [u8; 32],
    /// Current USDC balance in lamports (6 decimals).
    pub balance: u64,
    /// Original balance snapshot at last distribution posting.
    pub original_balance: u64,
    /// Storage tier determining shard ratio guarantee.
    pub tier: StorageTier,
    /// Number of funded CIDs (actual CID list tracked off-chain or in separate accounts).
    pub funded_cid_count: u32,
    /// Whether a distribution has been posted for the current period.
    pub distribution_posted: bool,
    /// Merkle root of the latest distribution.
    pub distribution_root: [u8; 32],
    /// Total weight across all operators in the latest distribution.
    pub total_weight: u64,
    /// Creation timestamp.
    pub created_at: i64,
}

impl CreatorPool {
    /// Account discriminator.
    pub const DISCRIMINATOR: [u8; 8] = *b"creatpol";

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialization should not fail")
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

/// Subscription pool account (time-bound, per-user).
///
/// PDA seeds: `[b"pool", pool_pubkey]`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pool {
    /// Unique pool identifier.
    pub pool_pubkey: [u8; 32],
    /// Service type (0=Tunnel, 1=Data, etc.).
    pub service: u8,
    /// Subscription tier.
    pub tier: u8,
    /// Subscription start date (unix timestamp).
    pub start_date: i64,
    /// Creation timestamp.
    pub created_at: i64,
    /// Expiration timestamp.
    pub expires_at: i64,
    /// Current USDC balance.
    pub pool_balance: u64,
    /// Balance snapshot at distribution posting.
    pub original_pool_balance: u64,
    /// Total contribution weight.
    pub total_weight: u64,
    /// Merkle root of distribution.
    pub distribution_root: [u8; 32],
    /// Whether distribution has been posted.
    pub distribution_posted: bool,
}

impl Pool {
    pub const DISCRIMINATOR: [u8; 8] = *b"poolacct";

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialization should not fail")
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

/// Payment channel between a user and a storage node.
///
/// PDA seeds: `[b"payment_channel", user_pubkey, node_pubkey]`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentChannel {
    /// User (payer) public key.
    pub user: [u8; 32],
    /// Storage node (payee) public key.
    pub node: [u8; 32],
    /// Total USDC locked in the channel.
    pub locked_amount: u64,
    /// Cumulative amount in the latest redeemed voucher.
    pub redeemed_amount: u64,
    /// Highest nonce seen (prevents replay).
    pub nonce: u64,
    /// Whether the channel is open.
    pub is_open: bool,
    /// Channel creation timestamp.
    pub created_at: i64,
    /// Channel expiry (optional timeout for unilateral close).
    pub expires_at: i64,
}

impl PaymentChannel {
    pub const DISCRIMINATOR: [u8; 8] = *b"paychanl";

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialization should not fail")
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

/// A signed voucher for a payment channel.
///
/// Off-chain structure — the user signs incremental vouchers as data is served.
/// The storage node redeems the latest (highest nonce) voucher on settlement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentVoucher {
    /// Payment channel PDA address.
    pub channel: [u8; 32],
    /// Cumulative USDC amount (not incremental).
    pub amount: u64,
    /// Monotonically increasing nonce. Highest nonce wins.
    pub nonce: u64,
    /// User's ed25519 signature over `(channel, amount, nonce)`.
    pub signature: Vec<u8>,
}

impl PaymentVoucher {
    /// Canonical bytes for signing: `channel || amount_le || nonce_le`.
    pub fn signable_data(channel: &[u8; 32], amount: u64, nonce: u64) -> Vec<u8> {
        let mut data = Vec::with_capacity(48);
        data.extend_from_slice(channel);
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&nonce.to_le_bytes());
        data
    }
}

/// Storage receipt from PDP challenge (off-chain, submitted for claims).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageReceipt {
    /// Content ID (SHA-256 of content).
    pub content_id: [u8; 32],
    /// Storage node that proved possession.
    pub storage_node: [u8; 32],
    /// Challenger that issued the PDP challenge.
    pub challenger: [u8; 32],
    /// Shard index that was proven.
    pub shard_index: u32,
    /// Timestamp of the proof.
    pub timestamp: u64,
    /// Challenge nonce.
    pub nonce: [u8; 32],
    /// hash(shard_data || nonce).
    pub proof_hash: [u8; 32],
    /// Challenger's ed25519 signature.
    pub signature: Vec<u8>,
}

impl StorageReceipt {
    /// Dedup hash: `SHA256(content_id || shard_index || storage_node || challenger || timestamp)`.
    pub fn dedup_hash(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.content_id);
        hasher.update(self.shard_index.to_le_bytes());
        hasher.update(self.storage_node);
        hasher.update(self.challenger);
        hasher.update(self.timestamp.to_le_bytes());
        let result = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }

    /// Canonical bytes for signature verification.
    pub fn signable_bytes(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(32 + 32 + 32 + 4 + 8 + 32 + 32);
        data.extend_from_slice(&self.content_id);
        data.extend_from_slice(&self.storage_node);
        data.extend_from_slice(&self.challenger);
        data.extend_from_slice(&self.shard_index.to_le_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.extend_from_slice(&self.nonce);
        data.extend_from_slice(&self.proof_hash);
        data
    }
}

impl craftec_core::ContributionReceipt for StorageReceipt {
    fn weight(&self) -> u64 {
        1 // Each PDP pass = 1 unit of weight
    }

    fn signable_data(&self) -> Vec<u8> {
        self.signable_bytes()
    }

    fn operator(&self) -> [u8; 32] {
        self.storage_node
    }

    fn signer(&self) -> [u8; 32] {
        self.challenger
    }

    fn timestamp(&self) -> u64 {
        self.timestamp
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftec_core::ContributionReceipt;

    #[test]
    fn test_storage_tier_round_trip() {
        for i in 0..=4u8 {
            let tier = StorageTier::from_u8(i).unwrap();
            assert_eq!(tier as u8, i);
        }
        assert!(StorageTier::from_u8(5).is_none());
    }

    #[test]
    fn test_shard_ratios() {
        assert_eq!(StorageTier::Free.min_shard_ratio(), 0.0);
        assert_eq!(StorageTier::Lite.min_shard_ratio(), 2.0);
        assert_eq!(StorageTier::Standard.min_shard_ratio(), 3.0);
        assert_eq!(StorageTier::Pro.min_shard_ratio(), 5.0);
        assert_eq!(StorageTier::Enterprise.min_shard_ratio(), 10.0);
    }

    #[test]
    fn test_creator_pool_roundtrip() {
        let pool = CreatorPool {
            creator: [1u8; 32],
            balance: 1_000_000,
            original_balance: 1_000_000,
            tier: StorageTier::Standard,
            funded_cid_count: 5,
            distribution_posted: false,
            distribution_root: [0u8; 32],
            total_weight: 0,
            created_at: 1700000000,
        };
        let bytes = pool.to_bytes();
        let decoded = CreatorPool::from_bytes(&bytes).unwrap();
        assert_eq!(pool, decoded);
    }

    #[test]
    fn test_payment_channel_roundtrip() {
        let ch = PaymentChannel {
            user: [2u8; 32],
            node: [3u8; 32],
            locked_amount: 50_000_000,
            redeemed_amount: 0,
            nonce: 0,
            is_open: true,
            created_at: 1700000000,
            expires_at: 1703000000,
        };
        let bytes = ch.to_bytes();
        let decoded = PaymentChannel::from_bytes(&bytes).unwrap();
        assert_eq!(ch, decoded);
    }

    #[test]
    fn test_voucher_signable_data() {
        let channel = [1u8; 32];
        let data = PaymentVoucher::signable_data(&channel, 100, 1);
        assert_eq!(data.len(), 48);
    }

    #[test]
    fn test_storage_receipt_dedup() {
        let receipt = StorageReceipt {
            content_id: [1u8; 32],
            storage_node: [2u8; 32],
            challenger: [3u8; 32],
            shard_index: 0,
            timestamp: 1700000000,
            nonce: [4u8; 32],
            proof_hash: [5u8; 32],
            signature: vec![0u8; 64],
        };
        let hash1 = receipt.dedup_hash();
        let hash2 = receipt.dedup_hash();
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, [0u8; 32]);
    }

    #[test]
    fn test_storage_receipt_contribution() {
        let receipt = StorageReceipt {
            content_id: [1u8; 32],
            storage_node: [2u8; 32],
            challenger: [3u8; 32],
            shard_index: 0,
            timestamp: 1700000000,
            nonce: [4u8; 32],
            proof_hash: [5u8; 32],
            signature: vec![0u8; 64],
        };
        assert_eq!(receipt.weight(), 1);
        assert_eq!(receipt.operator(), [2u8; 32]);
        assert_eq!(receipt.signer(), [3u8; 32]);
        assert_eq!(ContributionReceipt::timestamp(&receipt), 1700000000);
    }

    #[test]
    fn test_pool_roundtrip() {
        let pool = Pool {
            pool_pubkey: [1u8; 32],
            service: 1,
            tier: 2,
            start_date: 1700000000,
            created_at: 1700000000,
            expires_at: 1703000000,
            pool_balance: 10_000_000,
            original_pool_balance: 10_000_000,
            total_weight: 0,
            distribution_root: [0u8; 32],
            distribution_posted: false,
        };
        let bytes = pool.to_bytes();
        let decoded = Pool::from_bytes(&bytes).unwrap();
        assert_eq!(pool, decoded);
    }
}

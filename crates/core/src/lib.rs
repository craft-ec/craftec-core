//! Craftec Core
//!
//! Generic types, errors, and traits shared by all Craftec projects.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Generic error types shared across all crafts
#[derive(Error, Debug)]
pub enum CraftError {
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Peer not found: {0}")]
    PeerNotFound(String),
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("Invalid public key")]
    InvalidPublicKey,
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Settlement error: {0}")]
    SettlementError(String),
    #[error("Timeout")]
    Timeout,
}

/// Node identity as 32-byte public key
pub type NodeId = [u8; 32];

/// Epoch for settlement periods
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Epoch {
    pub index: u64,
    pub start: i64,
    pub end: i64,
}

/// Pool identifier for settlement
pub type PoolId = [u8; 32];

/// Service type enum used by the settlement layer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ServiceType {
    Tunnel = 0,
    Data = 1,
    Compute = 2,
    Parallel = 3,
    Bundle = 4,
}

impl ServiceType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Tunnel),
            1 => Some(Self::Data),
            2 => Some(Self::Compute),
            3 => Some(Self::Parallel),
            4 => Some(Self::Bundle),
            _ => None,
        }
    }
}

/// Generic contribution receipt trait.
///
/// Each craft defines its own receipt type and implements this trait
/// so the settlement layer can process it uniformly.
pub trait ContributionReceipt {
    /// The weight of this contribution (bytes, cycles, etc).
    fn weight(&self) -> u64;
    /// Canonical bytes for signing/verification.
    fn signable_data(&self) -> Vec<u8>;
    /// Public key of the operator who earned this receipt.
    fn operator(&self) -> [u8; 32];
    /// Public key of the counterparty who signed the receipt.
    fn signer(&self) -> [u8; 32];
    /// Timestamp of the contribution.
    fn timestamp(&self) -> u64;
    /// Ed25519 signature over signable_data, by signer.
    /// Required for SP1 proof generation; returns empty slice by default.
    fn signature(&self) -> &[u8] {
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_type_from_u8() {
        assert_eq!(ServiceType::from_u8(0), Some(ServiceType::Tunnel));
        assert_eq!(ServiceType::from_u8(1), Some(ServiceType::Data));
        assert_eq!(ServiceType::from_u8(2), Some(ServiceType::Compute));
        assert_eq!(ServiceType::from_u8(3), Some(ServiceType::Parallel));
        assert_eq!(ServiceType::from_u8(4), Some(ServiceType::Bundle));
        assert_eq!(ServiceType::from_u8(5), None);
    }

    #[test]
    fn test_service_type_repr() {
        assert_eq!(ServiceType::Tunnel as u8, 0);
        assert_eq!(ServiceType::Data as u8, 1);
        assert_eq!(ServiceType::Compute as u8, 2);
        assert_eq!(ServiceType::Parallel as u8, 3);
        assert_eq!(ServiceType::Bundle as u8, 4);
    }

    #[test]
    fn test_epoch_serde() {
        let epoch = Epoch { index: 1, start: 100, end: 200 };
        let json = serde_json::to_string(&epoch).unwrap();
        let parsed: Epoch = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, epoch);
    }
}

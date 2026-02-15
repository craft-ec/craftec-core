use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

const DID_PREFIX: &str = "did:craftec:";

#[derive(Debug, Error)]
pub enum DidError {
    #[error("invalid DID format: {0}")]
    InvalidFormat(String),
    #[error("invalid base58 encoding: {0}")]
    InvalidBase58(#[from] bs58::decode::Error),
    #[error("invalid public key length: expected 32, got {0}")]
    InvalidKeyLength(usize),
}

/// A Craftec decentralized identifier derived from an Ed25519 public key.
///
/// Format: `did:craftec:<base58_ed25519_pubkey>`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Did(String);

impl Did {
    /// Create a DID from a 32-byte Ed25519 public key.
    pub fn from_pubkey(pubkey: &[u8; 32]) -> Self {
        let encoded = bs58::encode(pubkey).into_string();
        Self(format!("{DID_PREFIX}{encoded}"))
    }

    /// Extract the 32-byte public key from this DID.
    pub fn to_pubkey(&self) -> [u8; 32] {
        let encoded = &self.0[DID_PREFIX.len()..];
        let bytes = bs58::decode(encoded).into_vec().expect("DID contains valid base58");
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        key
    }

    /// Return the full DID string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Did {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Did {
    type Err = DidError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.starts_with(DID_PREFIX) {
            return Err(DidError::InvalidFormat(format!(
                "must start with '{DID_PREFIX}'"
            )));
        }

        let encoded = &s[DID_PREFIX.len()..];
        if encoded.is_empty() {
            return Err(DidError::InvalidFormat("missing key portion".into()));
        }

        let bytes = bs58::decode(encoded).into_vec()?;
        if bytes.len() != 32 {
            return Err(DidError::InvalidKeyLength(bytes.len()));
        }

        Ok(Self(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_pubkey() {
        let pubkey = [42u8; 32];
        let did = Did::from_pubkey(&pubkey);
        assert!(did.to_string().starts_with("did:craftec:"));
        assert_eq!(did.to_pubkey(), pubkey);
    }

    #[test]
    fn parse_valid() {
        let pubkey = [1u8; 32];
        let did = Did::from_pubkey(&pubkey);
        let parsed: Did = did.to_string().parse().unwrap();
        assert_eq!(parsed, did);
    }

    #[test]
    fn parse_invalid_prefix() {
        assert!("did:other:abc".parse::<Did>().is_err());
    }

    #[test]
    fn parse_invalid_base58() {
        assert!("did:craftec:0OIl".parse::<Did>().is_err());
    }

    #[test]
    fn parse_wrong_key_length() {
        // Encode only 16 bytes
        let short = bs58::encode(&[0u8; 16]).into_string();
        let s = format!("did:craftec:{short}");
        assert!(s.parse::<Did>().is_err());
    }

    #[test]
    fn parse_empty_key() {
        assert!("did:craftec:".parse::<Did>().is_err());
    }

    #[test]
    fn serde_roundtrip() {
        let did = Did::from_pubkey(&[7u8; 32]);
        let json = serde_json::to_string(&did).unwrap();
        let back: Did = serde_json::from_str(&json).unwrap();
        assert_eq!(back, did);
    }
}

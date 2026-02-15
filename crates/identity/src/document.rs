use serde::{Deserialize, Serialize};

use crate::Did;

/// A verification method within a DID Document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationMethod {
    pub id: String,
    pub method_type: String,
    pub public_key_multibase: String,
}

/// A DID Document derived from a public key. Never stored — always re-derived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DidDocument {
    pub id: Did,
    pub verification_method: Vec<VerificationMethod>,
    pub created: u64,
}

impl DidDocument {
    /// Derive a DID Document from a 32-byte Ed25519 public key.
    pub fn derive(pubkey: &[u8; 32]) -> Self {
        let did = Did::from_pubkey(pubkey);
        let multibase = bs58::encode(pubkey).into_string();

        let vm = VerificationMethod {
            id: format!("{}#key-1", did),
            method_type: "Ed25519VerificationKey2020".to_string(),
            public_key_multibase: format!("z{multibase}"),
        };

        Self {
            id: did,
            verification_method: vec![vm],
            created: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_document() {
        let pubkey = [99u8; 32];
        let doc = DidDocument::derive(&pubkey);
        assert_eq!(doc.id, Did::from_pubkey(&pubkey));
        assert_eq!(doc.verification_method.len(), 1);
        let vm = &doc.verification_method[0];
        assert!(vm.id.ends_with("#key-1"));
        assert_eq!(vm.method_type, "Ed25519VerificationKey2020");
        assert!(vm.public_key_multibase.starts_with('z'));
    }

    #[test]
    fn serde_roundtrip() {
        let doc = DidDocument::derive(&[5u8; 32]);
        let json = serde_json::to_string(&doc).unwrap();
        let back: DidDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(back, doc);
    }
}

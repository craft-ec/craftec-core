use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;

use crate::Did;

/// A Craftec identity combining a keypair with its derived DID.
pub struct Identity {
    pub did: Did,
    pub keypair: SigningKey,
}

impl Identity {
    /// Generate a new random identity.
    pub fn generate() -> Self {
        let keypair = SigningKey::generate(&mut OsRng);
        Self::from_keypair(keypair)
    }

    /// Create an identity from an existing signing key.
    pub fn from_keypair(keypair: SigningKey) -> Self {
        let pubkey = keypair.verifying_key().to_bytes();
        let did = Did::from_pubkey(&pubkey);
        Self { did, keypair }
    }

    /// Create an identity from raw 32-byte ed25519 secret key bytes.
    pub fn from_secret_bytes(secret: &[u8; 32]) -> Self {
        let keypair = SigningKey::from_bytes(secret);
        Self::from_keypair(keypair)
    }

    /// Sign data with this identity's private key.
    pub fn sign(&self, data: &[u8]) -> Signature {
        self.keypair.sign(data)
    }

    /// Verify a signature against a public key.
    pub fn verify(pubkey: &[u8; 32], data: &[u8], signature: &Signature) -> bool {
        let Ok(verifying_key) = VerifyingKey::from_bytes(pubkey) else {
            return false;
        };
        verifying_key.verify(data, signature).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_sign_verify() {
        let id = Identity::generate();
        let data = b"hello craftec";
        let sig = id.sign(data);
        let pubkey = id.did.to_pubkey();
        assert!(Identity::verify(&pubkey, data, &sig));
        assert!(!Identity::verify(&pubkey, b"wrong data", &sig));
    }

    #[test]
    fn from_keypair() {
        let key = SigningKey::generate(&mut OsRng);
        let pubkey = key.verifying_key().to_bytes();
        let id = Identity::from_keypair(key);
        assert_eq!(id.did.to_pubkey(), pubkey);
    }
}

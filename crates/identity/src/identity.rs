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

    /// Return the 32-byte ed25519 public key.
    pub fn pubkey(&self) -> [u8; 32] {
        self.keypair.verifying_key().to_bytes()
    }

    /// Return the 32-byte ed25519 secret key bytes (for keystore persistence).
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.keypair.to_bytes()
    }

    /// Sign data with this identity's private key.
    pub fn sign(&self, data: &[u8]) -> Signature {
        self.keypair.sign(data)
    }

    /// Sign data and return the signature as a 64-byte array.
    pub fn sign_bytes(&self, data: &[u8]) -> [u8; 64] {
        self.sign(data).to_bytes()
    }

    /// Verify a signature against a public key.
    pub fn verify(pubkey: &[u8; 32], data: &[u8], signature: &Signature) -> bool {
        let Ok(verifying_key) = VerifyingKey::from_bytes(pubkey) else {
            return false;
        };
        verifying_key.verify(data, signature).is_ok()
    }

    /// Verify a signature against a public key given as raw 64-byte signature bytes.
    pub fn verify_bytes(pubkey: &[u8; 32], data: &[u8], signature_bytes: &[u8; 64]) -> bool {
        let signature = Signature::from_bytes(signature_bytes);
        Self::verify(pubkey, data, &signature)
    }

    /// Verify a signature using a DID (extracts the public key from the DID).
    pub fn verify_with_did(did: &crate::Did, data: &[u8], signature: &Signature) -> bool {
        let pubkey = did.to_pubkey();
        Self::verify(&pubkey, data, signature)
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

    #[test]
    fn pubkey_and_secret_roundtrip() {
        let id = Identity::generate();
        let pubkey = id.pubkey();
        let secret = id.secret_bytes();
        assert_eq!(pubkey, id.did.to_pubkey());
        let restored = Identity::from_secret_bytes(&secret);
        assert_eq!(restored.pubkey(), pubkey);
        assert_eq!(restored.did, id.did);
    }

    #[test]
    fn sign_bytes_and_verify_bytes() {
        let id = Identity::generate();
        let data = b"test message";
        let sig_bytes = id.sign_bytes(data);
        assert!(Identity::verify_bytes(&id.pubkey(), data, &sig_bytes));
        assert!(!Identity::verify_bytes(&id.pubkey(), b"wrong", &sig_bytes));
    }

    #[test]
    fn verify_with_did() {
        let id = Identity::generate();
        let data = b"verify via did";
        let sig = id.sign(data);
        assert!(Identity::verify_with_did(&id.did, data, &sig));
    }
}

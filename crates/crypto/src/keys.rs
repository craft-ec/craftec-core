use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use thiserror::Error;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

#[derive(Error, Debug)]
pub enum KeyError {
    #[error("Invalid public key length")]
    InvalidPublicKey,
    #[error("Invalid secret key length")]
    InvalidSecretKey,
    #[error("Key derivation failed")]
    DerivationFailed,
}

/// Keypair for signing (Ed25519)
pub struct SigningKeypair {
    pub signing_key: SigningKey,
    pub verifying_key: VerifyingKey,
}

impl Clone for SigningKeypair {
    fn clone(&self) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(&self.signing_key.to_bytes()),
            verifying_key: self.verifying_key,
        }
    }
}

impl SigningKeypair {
    /// Generate a new random signing keypair
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Get the public key as bytes
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.verifying_key.to_bytes()
    }

    /// Get the secret key as bytes
    pub fn secret_key_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Create from raw secret key bytes
    pub fn from_secret_bytes(secret: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(secret);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }
}

/// Keypair for encryption (X25519)
pub struct EncryptionKeypair {
    pub secret: StaticSecret,
    pub public: X25519PublicKey,
}

impl Clone for EncryptionKeypair {
    fn clone(&self) -> Self {
        let secret_bytes = self.secret.as_bytes();
        let secret = StaticSecret::from(*secret_bytes);
        let public = X25519PublicKey::from(&secret);
        Self { secret, public }
    }
}

impl EncryptionKeypair {
    /// Generate a new random encryption keypair
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = X25519PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Get the public key as bytes
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.public.to_bytes()
    }

    /// Get the secret key as bytes
    pub fn secret_key_bytes(&self) -> [u8; 32] {
        *self.secret.as_bytes()
    }

    /// Create from raw secret key bytes
    pub fn from_secret_bytes(secret: &[u8; 32]) -> Self {
        let secret = StaticSecret::from(*secret);
        let public = X25519PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Perform Diffie-Hellman key exchange
    pub fn diffie_hellman(&self, their_public: &[u8; 32]) -> [u8; 32] {
        let their_public = X25519PublicKey::from(*their_public);
        let shared = self.secret.diffie_hellman(&their_public);
        *shared.as_bytes()
    }
}

/// Combined identity containing both signing and encryption keys
pub struct Identity {
    pub signing: SigningKeypair,
    pub encryption: EncryptionKeypair,
}

impl Identity {
    /// Generate a new random identity
    pub fn generate() -> Self {
        Self {
            signing: SigningKeypair::generate(),
            encryption: EncryptionKeypair::generate(),
        }
    }

    /// Get the signing public key as the node's identity
    pub fn pubkey(&self) -> [u8; 32] {
        self.signing.public_key_bytes()
    }
}

/// Generate a one-time keypair for a single request (privacy)
pub fn generate_one_time_keypair() -> SigningKeypair {
    SigningKeypair::generate()
}

/// Hash data using SHA-256
pub fn hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Generate a random 32-byte secret
pub fn generate_random_secret() -> [u8; 32] {
    let mut secret = [0u8; 32];
    rand::RngCore::fill_bytes(&mut OsRng, &mut secret);
    secret
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signing_keypair() {
        let kp = SigningKeypair::generate();
        let pubkey = kp.public_key_bytes();
        let secret = kp.secret_key_bytes();

        let restored = SigningKeypair::from_secret_bytes(&secret);
        assert_eq!(restored.public_key_bytes(), pubkey);
    }

    #[test]
    fn test_encryption_keypair() {
        let kp = EncryptionKeypair::generate();
        let pubkey = kp.public_key_bytes();
        let secret = kp.secret_key_bytes();

        let restored = EncryptionKeypair::from_secret_bytes(&secret);
        assert_eq!(restored.public_key_bytes(), pubkey);
    }

    #[test]
    fn test_diffie_hellman() {
        let alice = EncryptionKeypair::generate();
        let bob = EncryptionKeypair::generate();

        let alice_shared = alice.diffie_hellman(&bob.public_key_bytes());
        let bob_shared = bob.diffie_hellman(&alice.public_key_bytes());

        assert_eq!(alice_shared, bob_shared);
    }

    #[test]
    fn test_identity() {
        let id = Identity::generate();
        let pubkey = id.pubkey();
        assert_eq!(pubkey.len(), 32);
    }

    #[test]
    fn test_hash() {
        let h1 = hash(b"hello");
        let h2 = hash(b"hello");
        assert_eq!(h1, h2);

        let h3 = hash(b"world");
        assert_ne!(h1, h3);
    }
}

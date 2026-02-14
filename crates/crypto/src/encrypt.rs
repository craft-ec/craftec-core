use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::rngs::OsRng;
use rand::RngCore;
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::keys::hash;

#[derive(Error, Debug)]
pub enum EncryptError {
    #[error("Encryption failed")]
    EncryptionFailed,
    #[error("Decryption failed")]
    DecryptionFailed,
    #[error("Invalid key")]
    InvalidKey,
    #[error("Invalid nonce")]
    InvalidNonce,
    #[error("Ciphertext too short")]
    CiphertextTooShort,
}

/// Encrypt data for a recipient using ECDH + ChaCha20-Poly1305
///
/// 1. Perform X25519 Diffie-Hellman to derive shared secret
/// 2. Hash the shared secret to get a symmetric key
/// 3. Encrypt with ChaCha20-Poly1305
pub fn encrypt_for_recipient(
    recipient_pubkey: &[u8; 32],
    sender_secret: &[u8; 32],
    plaintext: &[u8],
) -> Result<Vec<u8>, EncryptError> {
    let sender_secret = StaticSecret::from(*sender_secret);
    let recipient_public = PublicKey::from(*recipient_pubkey);
    let shared_secret = sender_secret.diffie_hellman(&recipient_public);

    let symmetric_key = hash(shared_secret.as_bytes());

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let cipher =
        ChaCha20Poly1305::new_from_slice(&symmetric_key).map_err(|_| EncryptError::InvalidKey)?;

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| EncryptError::EncryptionFailed)?;

    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt data from a sender using ECDH + ChaCha20-Poly1305
pub fn decrypt_from_sender(
    sender_pubkey: &[u8; 32],
    recipient_secret: &[u8; 32],
    ciphertext: &[u8],
) -> Result<Vec<u8>, EncryptError> {
    if ciphertext.len() < 12 {
        return Err(EncryptError::CiphertextTooShort);
    }

    let recipient_secret = StaticSecret::from(*recipient_secret);
    let sender_public = PublicKey::from(*sender_pubkey);
    let shared_secret = recipient_secret.diffie_hellman(&sender_public);

    let symmetric_key = hash(shared_secret.as_bytes());

    let nonce = Nonce::from_slice(&ciphertext[..12]);
    let ciphertext = &ciphertext[12..];

    let cipher =
        ChaCha20Poly1305::new_from_slice(&symmetric_key).map_err(|_| EncryptError::InvalidKey)?;

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| EncryptError::DecryptionFailed)
}

/// Encrypt data with a symmetric key
pub fn encrypt_symmetric(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, EncryptError> {
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|_| EncryptError::InvalidKey)?;

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| EncryptError::EncryptionFailed)?;

    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt data with a symmetric key
pub fn decrypt_symmetric(key: &[u8; 32], ciphertext: &[u8]) -> Result<Vec<u8>, EncryptError> {
    if ciphertext.len() < 12 {
        return Err(EncryptError::CiphertextTooShort);
    }

    let nonce = Nonce::from_slice(&ciphertext[..12]);
    let ciphertext = &ciphertext[12..];

    let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|_| EncryptError::InvalidKey)?;

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| EncryptError::DecryptionFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::EncryptionKeypair;

    #[test]
    fn test_asymmetric_encryption() {
        let sender = EncryptionKeypair::generate();
        let recipient = EncryptionKeypair::generate();

        let plaintext = b"Hello, Craftec!";

        let ciphertext = encrypt_for_recipient(
            &recipient.public_key_bytes(),
            &sender.secret_key_bytes(),
            plaintext,
        )
        .unwrap();

        let decrypted = decrypt_from_sender(
            &sender.public_key_bytes(),
            &recipient.secret_key_bytes(),
            &ciphertext,
        )
        .unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_symmetric_encryption() {
        let key = [42u8; 32];
        let plaintext = b"Secret data";

        let ciphertext = encrypt_symmetric(&key, plaintext).unwrap();
        let decrypted = decrypt_symmetric(&key, &ciphertext).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = [42u8; 32];
        let key2 = [43u8; 32];
        let plaintext = b"Secret data";

        let ciphertext = encrypt_symmetric(&key1, plaintext).unwrap();
        let result = decrypt_symmetric(&key2, &ciphertext);

        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_ciphertext_too_short() {
        let key = [42u8; 32];
        let result = decrypt_symmetric(&key, &[1, 2, 3]);
        assert!(matches!(result, Err(EncryptError::CiphertextTooShort)));
    }

    #[test]
    fn test_encrypt_empty_plaintext() {
        let key = [42u8; 32];
        let ciphertext = encrypt_symmetric(&key, b"").unwrap();
        let decrypted = decrypt_symmetric(&key, &ciphertext).unwrap();
        assert!(decrypted.is_empty());
    }
}

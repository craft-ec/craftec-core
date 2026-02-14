use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};

use crate::keys::SigningKeypair;

/// Sign data with a signing keypair
pub fn sign_data(keypair: &SigningKeypair, data: &[u8]) -> [u8; 64] {
    let signature: Signature = keypair.signing_key.sign(data);
    signature.to_bytes()
}

/// Verify a signature
pub fn verify_signature(pubkey: &[u8; 32], data: &[u8], signature: &[u8; 64]) -> bool {
    let verifying_key = match VerifyingKey::from_bytes(pubkey) {
        Ok(vk) => vk,
        Err(_) => return false,
    };

    let signature = Signature::from_bytes(signature);

    verifying_key.verify(data, &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() {
        let keypair = SigningKeypair::generate();
        let data = b"Hello, Craftec!";

        let signature = sign_data(&keypair, data);
        assert!(verify_signature(
            &keypair.public_key_bytes(),
            data,
            &signature
        ));

        assert!(!verify_signature(
            &keypair.public_key_bytes(),
            b"Wrong data",
            &signature
        ));
    }

    #[test]
    fn test_wrong_pubkey_fails() {
        let keypair1 = SigningKeypair::generate();
        let keypair2 = SigningKeypair::generate();
        let data = b"Test data";

        let signature = sign_data(&keypair1, data);

        assert!(!verify_signature(
            &keypair2.public_key_bytes(),
            data,
            &signature
        ));
    }
}

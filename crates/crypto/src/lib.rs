//! Craftec Crypto
//!
//! Pure cryptographic primitives shared by all Craftec projects.
//! No dependency on any protocol-specific types.

pub mod keys;
pub mod encrypt;
pub mod sign;

pub use keys::{SigningKeypair, EncryptionKeypair, Identity, KeyError, hash};
pub use encrypt::{
    encrypt_for_recipient, decrypt_from_sender,
    encrypt_symmetric, decrypt_symmetric,
    EncryptError,
};
pub use sign::{sign_data, verify_signature};

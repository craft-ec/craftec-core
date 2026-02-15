//! Craftec Identity
//!
//! Decentralized identity primitives: DIDs, DID Documents, and identity management.

mod did;
mod document;
mod identity;
mod reputation;

pub use did::Did;
pub use document::{DidDocument, VerificationMethod};
pub use identity::Identity;
pub use reputation::ReputationScore;

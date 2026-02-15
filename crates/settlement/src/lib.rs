//! Craftec Settlement
//!
//! Client-side types, PDA derivation, instruction builders, and fee helpers
//! for interacting with the Craftec settlement Solana program.
//!
//! This crate does NOT contain the on-chain program itself — it provides
//! the client-side SDK for building transactions.

pub mod accounts;
pub mod fee;
pub mod instruction;
pub mod pda;

pub use accounts::*;
pub use fee::*;
pub use pda::*;

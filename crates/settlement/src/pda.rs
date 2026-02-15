//! PDA (Program Derived Address) derivation helpers.
//!
//! These mirror the on-chain PDA seeds so client code can derive
//! the same addresses the program expects.

use sha2::{Digest, Sha256};

/// Placeholder program ID. Replace with actual deployed program ID.
pub const PROGRAM_ID: [u8; 32] = [0u8; 32];

/// Seed prefixes matching the on-chain program.
pub mod seeds {
    pub const CONFIG: &[u8] = b"config";
    pub const PLAN: &[u8] = b"plan";
    pub const POOL: &[u8] = b"pool";
    pub const CREATOR_POOL: &[u8] = b"creator_pool";
    pub const CLAIM_RECEIPT: &[u8] = b"claim_receipt";
    pub const TREASURY: &[u8] = b"treasury";
    pub const PAYMENT_CHANNEL: &[u8] = b"payment_channel";
}

/// Derive a PDA by hashing seeds together.
///
/// This is a simplified PDA derivation for off-chain use. The actual Solana
/// `find_program_address` involves bump seed searching; this helper produces
/// deterministic addresses from seeds for account lookup and instruction building.
///
/// Returns `(address_bytes, seeds_used)`.
pub fn derive_pda(program_id: &[u8; 32], seeds: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for seed in seeds {
        hasher.update(seed);
    }
    hasher.update(program_id);
    hasher.update(b"ProgramDerivedAddress");
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Derive the global config PDA.
pub fn config_pda(program_id: &[u8; 32]) -> [u8; 32] {
    derive_pda(program_id, &[seeds::CONFIG])
}

/// Derive the protocol fee treasury PDA.
pub fn treasury_pda(program_id: &[u8; 32]) -> [u8; 32] {
    derive_pda(program_id, &[seeds::TREASURY])
}

/// Derive a creator pool PDA from the creator's public key.
pub fn creator_pool_pda(program_id: &[u8; 32], creator_pubkey: &[u8; 32]) -> [u8; 32] {
    derive_pda(program_id, &[seeds::CREATOR_POOL, creator_pubkey])
}

/// Derive a subscription pool PDA from a unique pool pubkey.
pub fn pool_pda(program_id: &[u8; 32], pool_pubkey: &[u8; 32]) -> [u8; 32] {
    derive_pda(program_id, &[seeds::POOL, pool_pubkey])
}

/// Derive a pricing plan PDA.
pub fn plan_pda(program_id: &[u8; 32], service: u8, tier: u8, billing_period: u8) -> [u8; 32] {
    derive_pda(
        program_id,
        &[seeds::PLAN, &[service], &[tier], &[billing_period]],
    )
}

/// Derive a claim receipt PDA (for double-claim prevention).
pub fn claim_receipt_pda(
    program_id: &[u8; 32],
    pool_pubkey: &[u8; 32],
    operator_pubkey: &[u8; 32],
) -> [u8; 32] {
    derive_pda(
        program_id,
        &[seeds::CLAIM_RECEIPT, pool_pubkey, operator_pubkey],
    )
}

/// Derive a payment channel PDA between a user and a storage node.
pub fn payment_channel_pda(
    program_id: &[u8; 32],
    user_pubkey: &[u8; 32],
    node_pubkey: &[u8; 32],
) -> [u8; 32] {
    derive_pda(
        program_id,
        &[seeds::PAYMENT_CHANNEL, user_pubkey, node_pubkey],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creator_pool_pda_deterministic() {
        let program = [1u8; 32];
        let creator = [2u8; 32];
        let pda1 = creator_pool_pda(&program, &creator);
        let pda2 = creator_pool_pda(&program, &creator);
        assert_eq!(pda1, pda2);
    }

    #[test]
    fn test_different_creators_different_pdas() {
        let program = [1u8; 32];
        let creator_a = [2u8; 32];
        let creator_b = [3u8; 32];
        assert_ne!(
            creator_pool_pda(&program, &creator_a),
            creator_pool_pda(&program, &creator_b)
        );
    }

    #[test]
    fn test_payment_channel_pda_deterministic() {
        let program = [1u8; 32];
        let user = [2u8; 32];
        let node = [3u8; 32];
        let pda1 = payment_channel_pda(&program, &user, &node);
        let pda2 = payment_channel_pda(&program, &user, &node);
        assert_eq!(pda1, pda2);
    }

    #[test]
    fn test_payment_channel_pda_asymmetric() {
        let program = [1u8; 32];
        let a = [2u8; 32];
        let b = [3u8; 32];
        // (user=a, node=b) != (user=b, node=a)
        assert_ne!(
            payment_channel_pda(&program, &a, &b),
            payment_channel_pda(&program, &b, &a)
        );
    }

    #[test]
    fn test_claim_receipt_pda() {
        let program = [1u8; 32];
        let pool = [2u8; 32];
        let op = [3u8; 32];
        let pda = claim_receipt_pda(&program, &pool, &op);
        assert_ne!(pda, [0u8; 32]);
    }

    #[test]
    fn test_treasury_pda() {
        let program = [1u8; 32];
        let t = treasury_pda(&program);
        assert_ne!(t, [0u8; 32]);
    }
}

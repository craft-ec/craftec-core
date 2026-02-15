//! Instruction builders for the Craftec settlement Solana program.
//!
//! Each function produces serialized instruction data (discriminator + payload)
//! that can be included in a Solana transaction.

use serde::{Deserialize, Serialize};

use crate::fee::PROTOCOL_FEE_BPS;
use crate::pda;

/// Instruction discriminators (first byte of instruction data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InstructionType {
    InitializeConfig = 0,
    CreatePlan = 1,
    UpdatePlan = 2,
    DeletePlan = 3,
    Subscribe = 4,
    CreateCreatorPool = 5,
    FundCreatorPool = 6,
    FundContent = 7,
    PostDistribution = 8,
    Claim = 9,
    OpenPaymentChannel = 10,
    SettlePaymentChannel = 11,
    ClosePaymentChannel = 12,
}

// -- Instruction data structs --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeConfig {
    pub admin: [u8; 32],
    pub protocol_fee_bps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePlan {
    pub service: u8,
    pub tier: u8,
    pub billing_period: u8,
    pub price_usdc: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCreatorPool {
    pub creator: [u8; 32],
    pub tier: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundCreatorPool {
    pub creator: [u8; 32],
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundContent {
    pub creator: [u8; 32],
    pub content_id: [u8; 32],
    pub amount: u64,
    pub tier: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostDistribution {
    pub pool_pubkey: [u8; 32],
    pub distribution_root: [u8; 32],
    pub total_weight: u64,
    pub groth16_proof: Vec<u8>,
    pub public_inputs: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimPayout {
    pub pool_pubkey: [u8; 32],
    pub operator_pubkey: [u8; 32],
    pub operator_weight: u64,
    pub merkle_proof: Vec<[u8; 32]>,
    pub leaf_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPaymentChannel {
    pub user: [u8; 32],
    pub node: [u8; 32],
    pub amount: u64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlePaymentChannel {
    pub user: [u8; 32],
    pub node: [u8; 32],
    /// Cumulative amount from the latest voucher.
    pub amount: u64,
    /// Voucher nonce (must be > current on-chain nonce).
    pub nonce: u64,
    /// User's signature over `(channel_pda, amount, nonce)`.
    pub voucher_signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosePaymentChannel {
    pub user: [u8; 32],
    pub node: [u8; 32],
}

// -- Instruction builders --

/// Build serialized instruction data: `[discriminator_byte] ++ bincode(payload)`.
fn build_instruction<T: Serialize>(discriminator: InstructionType, data: &T) -> Vec<u8> {
    let payload = bincode::serialize(data).expect("serialization should not fail");
    let mut buf = Vec::with_capacity(1 + payload.len());
    buf.push(discriminator as u8);
    buf.extend_from_slice(&payload);
    buf
}

pub fn initialize_config(admin: [u8; 32]) -> Vec<u8> {
    build_instruction(
        InstructionType::InitializeConfig,
        &InitializeConfig {
            admin,
            protocol_fee_bps: PROTOCOL_FEE_BPS,
        },
    )
}

pub fn create_creator_pool(creator: [u8; 32], tier: u8) -> Vec<u8> {
    build_instruction(
        InstructionType::CreateCreatorPool,
        &CreateCreatorPool { creator, tier },
    )
}

pub fn fund_creator_pool(creator: [u8; 32], amount: u64) -> Vec<u8> {
    build_instruction(
        InstructionType::FundCreatorPool,
        &FundCreatorPool { creator, amount },
    )
}

pub fn fund_content(creator: [u8; 32], content_id: [u8; 32], amount: u64, tier: u8) -> Vec<u8> {
    build_instruction(
        InstructionType::FundContent,
        &FundContent { creator, content_id, amount, tier },
    )
}

pub fn post_distribution(
    pool_pubkey: [u8; 32],
    distribution_root: [u8; 32],
    total_weight: u64,
    groth16_proof: Vec<u8>,
    public_inputs: Vec<u8>,
) -> Vec<u8> {
    build_instruction(
        InstructionType::PostDistribution,
        &PostDistribution { pool_pubkey, distribution_root, total_weight, groth16_proof, public_inputs },
    )
}

pub fn claim_payout(
    pool_pubkey: [u8; 32],
    operator_pubkey: [u8; 32],
    operator_weight: u64,
    merkle_proof: Vec<[u8; 32]>,
    leaf_index: u32,
) -> Vec<u8> {
    build_instruction(
        InstructionType::Claim,
        &ClaimPayout { pool_pubkey, operator_pubkey, operator_weight, merkle_proof, leaf_index },
    )
}

pub fn open_payment_channel(user: [u8; 32], node: [u8; 32], amount: u64, expires_at: i64) -> Vec<u8> {
    build_instruction(
        InstructionType::OpenPaymentChannel,
        &OpenPaymentChannel { user, node, amount, expires_at },
    )
}

pub fn settle_payment_channel(
    user: [u8; 32],
    node: [u8; 32],
    amount: u64,
    nonce: u64,
    voucher_signature: Vec<u8>,
) -> Vec<u8> {
    build_instruction(
        InstructionType::SettlePaymentChannel,
        &SettlePaymentChannel { user, node, amount, nonce, voucher_signature },
    )
}

pub fn close_payment_channel(user: [u8; 32], node: [u8; 32]) -> Vec<u8> {
    build_instruction(
        InstructionType::ClosePaymentChannel,
        &ClosePaymentChannel { user, node },
    )
}

// -- Account list helpers --

/// Account metadata for instruction building.
#[derive(Debug, Clone)]
pub struct AccountMeta {
    pub pubkey: [u8; 32],
    pub is_signer: bool,
    pub is_writable: bool,
}

/// Accounts needed for a `ClaimPayout` instruction.
pub fn claim_payout_accounts(
    program_id: &[u8; 32],
    pool_pubkey: &[u8; 32],
    operator_pubkey: &[u8; 32],
) -> Vec<AccountMeta> {
    let pool_pda = pda::creator_pool_pda(program_id, pool_pubkey);
    let claim_pda = pda::claim_receipt_pda(program_id, pool_pubkey, operator_pubkey);
    let treasury = pda::treasury_pda(program_id);

    vec![
        AccountMeta { pubkey: pool_pda, is_signer: false, is_writable: true },
        AccountMeta { pubkey: claim_pda, is_signer: false, is_writable: true },
        AccountMeta { pubkey: treasury, is_signer: false, is_writable: true },
        AccountMeta { pubkey: *operator_pubkey, is_signer: true, is_writable: true },
    ]
}

/// Accounts needed for a `SettlePaymentChannel` instruction.
pub fn settle_payment_channel_accounts(
    program_id: &[u8; 32],
    user: &[u8; 32],
    node: &[u8; 32],
) -> Vec<AccountMeta> {
    let channel_pda = pda::payment_channel_pda(program_id, user, node);
    let treasury = pda::treasury_pda(program_id);

    vec![
        AccountMeta { pubkey: channel_pda, is_signer: false, is_writable: true },
        AccountMeta { pubkey: treasury, is_signer: false, is_writable: true },
        AccountMeta { pubkey: *node, is_signer: true, is_writable: true },
        AccountMeta { pubkey: *user, is_signer: false, is_writable: true },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_discriminators() {
        assert_eq!(InstructionType::InitializeConfig as u8, 0);
        assert_eq!(InstructionType::ClosePaymentChannel as u8, 12);
    }

    #[test]
    fn test_initialize_config_builder() {
        let data = initialize_config([1u8; 32]);
        assert_eq!(data[0], InstructionType::InitializeConfig as u8);
        assert!(data.len() > 1);
    }

    #[test]
    fn test_create_creator_pool_builder() {
        let data = create_creator_pool([2u8; 32], 2);
        assert_eq!(data[0], InstructionType::CreateCreatorPool as u8);
    }

    #[test]
    fn test_fund_creator_pool_builder() {
        let data = fund_creator_pool([2u8; 32], 1_000_000);
        assert_eq!(data[0], InstructionType::FundCreatorPool as u8);
    }

    #[test]
    fn test_claim_payout_builder() {
        let proof = vec![[1u8; 32], [2u8; 32]];
        let data = claim_payout([1u8; 32], [2u8; 32], 100, proof, 0);
        assert_eq!(data[0], InstructionType::Claim as u8);
    }

    #[test]
    fn test_open_payment_channel_builder() {
        let data = open_payment_channel([1u8; 32], [2u8; 32], 50_000_000, 1703000000);
        assert_eq!(data[0], InstructionType::OpenPaymentChannel as u8);
    }

    #[test]
    fn test_settle_payment_channel_builder() {
        let data = settle_payment_channel([1u8; 32], [2u8; 32], 25_000_000, 5, vec![0u8; 64]);
        assert_eq!(data[0], InstructionType::SettlePaymentChannel as u8);
    }

    #[test]
    fn test_close_payment_channel_builder() {
        let data = close_payment_channel([1u8; 32], [2u8; 32]);
        assert_eq!(data[0], InstructionType::ClosePaymentChannel as u8);
    }

    #[test]
    fn test_claim_payout_roundtrip() {
        let original = ClaimPayout {
            pool_pubkey: [1u8; 32],
            operator_pubkey: [2u8; 32],
            operator_weight: 42,
            merkle_proof: vec![[1u8; 32]],
            leaf_index: 7,
        };
        let encoded = bincode::serialize(&original).unwrap();
        let decoded: ClaimPayout = bincode::deserialize(&encoded).unwrap();
        assert_eq!(original.operator_weight, decoded.operator_weight);
        assert_eq!(original.leaf_index, decoded.leaf_index);
    }

    #[test]
    fn test_settle_payment_channel_roundtrip() {
        let original = SettlePaymentChannel {
            user: [1u8; 32],
            node: [2u8; 32],
            amount: 10_000_000,
            nonce: 99,
            voucher_signature: vec![3u8; 64],
        };
        let encoded = bincode::serialize(&original).unwrap();
        let decoded: SettlePaymentChannel = bincode::deserialize(&encoded).unwrap();
        assert_eq!(original.amount, decoded.amount);
        assert_eq!(original.nonce, decoded.nonce);
    }

    #[test]
    fn test_claim_accounts_list() {
        let program = [1u8; 32];
        let pool = [2u8; 32];
        let operator = [3u8; 32];
        let accounts = claim_payout_accounts(&program, &pool, &operator);
        assert_eq!(accounts.len(), 4);
        assert!(accounts[0].is_writable);
        assert!(accounts[3].is_signer);
    }

    #[test]
    fn test_settle_accounts_list() {
        let program = [1u8; 32];
        let user = [2u8; 32];
        let node = [3u8; 32];
        let accounts = settle_payment_channel_accounts(&program, &user, &node);
        assert_eq!(accounts.len(), 4);
        assert!(accounts[2].is_signer);
    }

    #[test]
    fn test_fund_content_builder() {
        let data = fund_content([1u8; 32], [2u8; 32], 5_000_000, 2);
        assert_eq!(data[0], InstructionType::FundContent as u8);
    }

    #[test]
    fn test_post_distribution_builder() {
        let data = post_distribution([1u8; 32], [2u8; 32], 1000, vec![0u8; 128], vec![0u8; 64]);
        assert_eq!(data[0], InstructionType::PostDistribution as u8);
    }
}

//! Protocol fee constants and calculation helpers.
//!
//! All amounts are in USDC lamports (6 decimals, 1 USDC = 1_000_000).

/// Default protocol fee in basis points (500 BPS = 5%).
pub const PROTOCOL_FEE_BPS: u64 = 500;

/// Maximum protocol fee in basis points (50%).
pub const MAX_PROTOCOL_FEE_BPS: u64 = 5_000;

/// Basis points denominator.
pub const BPS_DENOMINATOR: u64 = 10_000;

/// USDC decimals on Solana.
pub const USDC_DECIMALS: u8 = 6;

/// 1 USDC in lamports.
pub const USDC_ONE: u64 = 1_000_000;

/// Calculate protocol fee for a given amount.
///
/// Returns `(fee, payout)` where `payout = amount - fee`.
///
/// # Panics
/// Panics if `fee_bps > MAX_PROTOCOL_FEE_BPS`.
pub fn calculate_fee(amount: u64, fee_bps: u64) -> (u64, u64) {
    assert!(
        fee_bps <= MAX_PROTOCOL_FEE_BPS,
        "fee_bps {} exceeds max {}",
        fee_bps,
        MAX_PROTOCOL_FEE_BPS
    );
    let fee = amount.saturating_mul(fee_bps) / BPS_DENOMINATOR;
    let payout = amount.saturating_sub(fee);
    (fee, payout)
}

/// Calculate protocol fee using the default rate.
pub fn calculate_default_fee(amount: u64) -> (u64, u64) {
    calculate_fee(amount, PROTOCOL_FEE_BPS)
}

/// Calculate proportional payout from a pool.
///
/// `payout = (operator_weight / total_weight) * pool_balance * (1 - fee_bps/10000)`
///
/// Returns `(fee, payout)`.
pub fn calculate_proportional_payout(
    pool_balance: u64,
    operator_weight: u64,
    total_weight: u64,
    fee_bps: u64,
) -> (u64, u64) {
    if total_weight == 0 || operator_weight == 0 || pool_balance == 0 {
        return (0, 0);
    }
    // Use u128 to avoid overflow on large balances
    let share = (pool_balance as u128)
        .saturating_mul(operator_weight as u128)
        / (total_weight as u128);
    let share = share.min(u64::MAX as u128) as u64;
    calculate_fee(share, fee_bps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_fee() {
        // 100 USDC → 5 USDC fee, 95 USDC payout
        let (fee, payout) = calculate_default_fee(100 * USDC_ONE);
        assert_eq!(fee, 5 * USDC_ONE);
        assert_eq!(payout, 95 * USDC_ONE);
    }

    #[test]
    fn test_zero_amount() {
        let (fee, payout) = calculate_default_fee(0);
        assert_eq!(fee, 0);
        assert_eq!(payout, 0);
    }

    #[test]
    fn test_small_amount() {
        // 1 USDC lamport → fee rounds down to 0
        let (fee, payout) = calculate_default_fee(1);
        assert_eq!(fee, 0);
        assert_eq!(payout, 1);
    }

    #[test]
    fn test_custom_fee() {
        // 1000 BPS = 10%
        let (fee, payout) = calculate_fee(100 * USDC_ONE, 1000);
        assert_eq!(fee, 10 * USDC_ONE);
        assert_eq!(payout, 90 * USDC_ONE);
    }

    #[test]
    #[should_panic(expected = "exceeds max")]
    fn test_fee_too_high() {
        calculate_fee(100, MAX_PROTOCOL_FEE_BPS + 1);
    }

    #[test]
    fn test_proportional_payout() {
        // Pool: 1000 USDC, operator has 25% weight, 5% fee
        let (fee, payout) = calculate_proportional_payout(
            1000 * USDC_ONE,
            25,
            100,
            PROTOCOL_FEE_BPS,
        );
        // share = 250 USDC, fee = 12.5 USDC (rounds down to 12_500_000), payout = 237.5
        assert_eq!(fee, 12_500_000);
        assert_eq!(payout, 237_500_000);
    }

    #[test]
    fn test_proportional_zero_weight() {
        let (fee, payout) = calculate_proportional_payout(1000, 0, 100, PROTOCOL_FEE_BPS);
        assert_eq!(fee, 0);
        assert_eq!(payout, 0);
    }

    #[test]
    fn test_proportional_zero_total() {
        let (fee, payout) = calculate_proportional_payout(1000, 50, 0, PROTOCOL_FEE_BPS);
        assert_eq!(fee, 0);
        assert_eq!(payout, 0);
    }
}

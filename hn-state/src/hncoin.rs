//! HNCOIN monetary constants (ADR-0024, "Decided"; ADR-0023, "Decided:
//! HNCOIN Decimals And Atomic Unit") — decided since ADR-0024/ADR-0023,
//! with no code consumer until ADR-0038's genesis loader, the same
//! "decided, no consumer yet" state [`crate::UNBONDING_PERIOD_BLOCKS`]/
//! [`crate::validator_transition::MINIMUM_VALIDATOR_BOND`] were already
//! in. Values copied directly from ADR-0024's own table, not re-derived
//! — this module is not a second owner of them (ADR-0024's own "One
//! Owning Document" rule).

/// `1 HNCOIN = 10^9 hnit` (ADR-0023, "Decided: HNCOIN Decimals And
/// Atomic Unit").
pub const HNCOIN_DECIMALS: u32 = 9;

/// `10^HNCOIN_DECIMALS`, the number of `hnit` in one HNCOIN.
const HNIT_PER_HNCOIN: u128 = 1_000_000_000;

/// The fixed maximum (and, since there is no post-genesis issuance,
/// permanent) HNCOIN supply (ADR-0024, "Decided": `MAX_SUPPLY =
/// 1,000,000,000 HNCOIN`), in `hnit`.
pub const MAX_SUPPLY: u128 = 1_000_000_000 * HNIT_PER_HNCOIN;

/// `GENESIS_SUPPLY == MAX_SUPPLY` (ADR-0024, "Decided: Fixed Supply") —
/// the entire supply is created exactly once, at genesis.
pub const GENESIS_SUPPLY: u128 = MAX_SUPPLY;

/// The Liquidity & Ecosystem Reserve allocation (ADR-0024, "Decided:
/// Genesis Allocation" — 80% of `GENESIS_SUPPLY`), in `hnit`.
pub const RESERVE_ALLOCATION: u128 = 800_000_000 * HNIT_PER_HNCOIN;

/// The Founder Allocation (ADR-0024, "Decided: Genesis Allocation" —
/// 10% of `GENESIS_SUPPLY`), in `hnit`.
pub const FOUNDER_ALLOCATION: u128 = 100_000_000 * HNIT_PER_HNCOIN;

/// The Community & Airdrop Allocation (ADR-0024, "Decided: Genesis
/// Allocation" — 10% of `GENESIS_SUPPLY`), in `hnit`.
pub const COMMUNITY_ALLOCATION: u128 = 100_000_000 * HNIT_PER_HNCOIN;

#[cfg(test)]
mod tests {
    use super::{
        COMMUNITY_ALLOCATION, FOUNDER_ALLOCATION, GENESIS_SUPPLY, MAX_SUPPLY, RESERVE_ALLOCATION,
    };

    #[test]
    fn genesis_supply_equals_max_supply() {
        assert_eq!(GENESIS_SUPPLY, MAX_SUPPLY);
    }

    #[test]
    fn the_three_allocations_sum_to_genesis_supply() {
        assert_eq!(
            RESERVE_ALLOCATION + FOUNDER_ALLOCATION + COMMUNITY_ALLOCATION,
            GENESIS_SUPPLY
        );
    }

    #[test]
    fn matches_adr_0024s_own_table() {
        assert_eq!(MAX_SUPPLY, 1_000_000_000_000_000_000);
        assert_eq!(RESERVE_ALLOCATION, 800_000_000_000_000_000);
        assert_eq!(FOUNDER_ALLOCATION, 100_000_000_000_000_000);
        assert_eq!(COMMUNITY_ALLOCATION, 100_000_000_000_000_000);
    }
}

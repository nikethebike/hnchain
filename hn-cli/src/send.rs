//! `send`'s own non-networking logic (ADR-0047, "Decided: Building
//! And Signing A Transfer"): resolving its flags into the values
//! [`crate::transfer::build_signed_transfer`] needs, and the
//! confirmation-prompt rules. The actual GET/POST calls, stdin
//! reading, and `--wait` polling live in `main.rs`, where they can
//! talk to a real node and a real terminal; everything here is a pure
//! function so it can be tested without either.

use hn_core::BlockHeight;

use crate::{amount, profile};

/// Resolves `--amount`/`--amount-hnit` into a single `hnit` quantity.
/// `cli::parse` already rejects both being given together; this
/// function's own job is requiring *at least* one and running the
/// actual grammar each flag implies.
pub fn resolve_amount(
    amount_hncoin: Option<&str>,
    amount_hnit: Option<&str>,
) -> Result<u128, String> {
    match (amount_hncoin, amount_hnit) {
        (Some(decimal), None) => {
            amount::parse_decimal_hncoin(decimal).map_err(|error| error.to_string())
        }
        (None, Some(integer)) => amount::parse_hnit(integer).map_err(|error| error.to_string()),
        _ => Err("exactly one of --amount or --amount-hnit is required".to_string()),
    }
}

/// Resolves `--fee-limit`, falling back to the profile's own
/// placeholder default (ADR-0047, "Fee limit and validity window").
pub fn resolve_fee_limit(fee_limit: Option<&str>) -> Result<u128, String> {
    match fee_limit {
        Some(decimal) => amount::parse_decimal_hncoin(decimal).map_err(|error| error.to_string()),
        None => Ok(profile::DEFAULT_FEE_LIMIT_HNIT),
    }
}

/// Resolves the validity window: `None` (never expires) when
/// `--no-expiry` is given, otherwise `latest_height +
/// VALIDITY_WINDOW_BLOCKS` (ADR-0047's own placeholder default),
/// saturating rather than overflowing at `u64::MAX`.
#[must_use]
pub fn resolve_max_height(no_expiry: bool, latest_height: u64) -> Option<BlockHeight> {
    if no_expiry {
        None
    } else {
        Some(BlockHeight::new(
            latest_height.saturating_add(profile::VALIDITY_WINDOW_BLOCKS),
        ))
    }
}

/// Resolves `--nonce`'s own manual override, falling back to the
/// account's committed nonce read from the node.
#[must_use]
pub fn resolve_nonce(explicit: Option<u64>, committed: u64) -> u64 {
    explicit.unwrap_or(committed)
}

/// Whether a non-interactive invocation (stdin is not a terminal)
/// without `--yes` must refuse rather than assume consent (ADR-0047,
/// "Decided: Building And Signing A Transfer", step 6).
#[must_use]
pub fn requires_explicit_yes(is_terminal: bool, yes: bool) -> bool {
    !yes && !is_terminal
}

/// Parses the interactive confirmation prompt's own typed answer.
#[must_use]
pub fn answer_confirms(answer: &str) -> bool {
    matches!(answer.trim(), "y" | "Y" | "yes" | "Yes" | "YES")
}

#[cfg(test)]
mod tests {
    use super::{
        answer_confirms, requires_explicit_yes, resolve_amount, resolve_fee_limit,
        resolve_max_height, resolve_nonce,
    };
    use crate::profile;

    #[test]
    fn resolve_amount_accepts_decimal_hncoin() {
        assert_eq!(resolve_amount(Some("1.5"), None), Ok(1_500_000_000));
    }

    #[test]
    fn resolve_amount_accepts_plain_hnit() {
        assert_eq!(resolve_amount(None, Some("42")), Ok(42));
    }

    #[test]
    fn resolve_amount_rejects_neither_flag() {
        assert!(resolve_amount(None, None).is_err());
    }

    #[test]
    fn resolve_amount_rejects_both_flags() {
        assert!(resolve_amount(Some("1"), Some("1")).is_err());
    }

    #[test]
    fn resolve_amount_propagates_the_grammar_error() {
        assert!(resolve_amount(Some("0"), None).is_err());
        assert!(resolve_amount(None, Some("0")).is_err());
    }

    #[test]
    fn resolve_fee_limit_falls_back_to_the_profiles_own_placeholder() {
        assert_eq!(resolve_fee_limit(None), Ok(profile::DEFAULT_FEE_LIMIT_HNIT));
    }

    #[test]
    fn resolve_fee_limit_accepts_an_override() {
        assert_eq!(resolve_fee_limit(Some("2.0")), Ok(2_000_000_000));
    }

    #[test]
    fn resolve_max_height_is_none_with_no_expiry() {
        assert_eq!(resolve_max_height(true, 1_000), None);
    }

    #[test]
    fn resolve_max_height_adds_the_profiles_own_window() {
        assert_eq!(
            resolve_max_height(false, 1_000).map(hn_core::BlockHeight::get),
            Some(1_000 + profile::VALIDITY_WINDOW_BLOCKS)
        );
    }

    #[test]
    fn resolve_max_height_saturates_instead_of_overflowing() {
        assert_eq!(
            resolve_max_height(false, u64::MAX).map(hn_core::BlockHeight::get),
            Some(u64::MAX)
        );
    }

    #[test]
    fn resolve_nonce_prefers_the_explicit_override() {
        assert_eq!(resolve_nonce(Some(7), 3), 7);
    }

    #[test]
    fn resolve_nonce_falls_back_to_the_committed_value() {
        assert_eq!(resolve_nonce(None, 3), 3);
    }

    #[test]
    fn requires_explicit_yes_only_when_non_interactive_and_unconfirmed() {
        assert!(requires_explicit_yes(false, false));
        assert!(!requires_explicit_yes(false, true));
        assert!(!requires_explicit_yes(true, false));
        assert!(!requires_explicit_yes(true, true));
    }

    #[test]
    fn answer_confirms_accepts_common_spellings() {
        assert!(answer_confirms("y"));
        assert!(answer_confirms("Y\n"));
        assert!(answer_confirms("yes"));
        assert!(answer_confirms("YES"));
    }

    #[test]
    fn answer_confirms_rejects_everything_else() {
        assert!(!answer_confirms(""));
        assert!(!answer_confirms("n"));
        assert!(!answer_confirms("no"));
        assert!(!answer_confirms("sure"));
    }
}

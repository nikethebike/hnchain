//! Strict decimal HNCOIN amount parsing (ADR-0047, "Decided: Amounts
//! — Exact Decimal, 9 Fractional Digits, No Floats").

use std::fmt;

use hn_state::HNCOIN_DECIMALS;

/// `10^HNCOIN_DECIMALS` — the number of `hnit` in one HNCOIN
/// (ADR-0023). Derived from `hn_state::HNCOIN_DECIMALS` rather than a
/// second hardcoded literal, so this module can never silently drift
/// from that ADR's own one owning value.
pub const HNIT_PER_HNCOIN: u128 = 10_u128.pow(HNCOIN_DECIMALS);

/// Why an amount string was rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmountError(pub String);

impl fmt::Display for AmountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::error::Error for AmountError {}

/// Parses `input` as a strict decimal HNCOIN amount (`--amount`,
/// ADR-0047) into `hnit`, using integer arithmetic only: no sign, no
/// exponent, no thousands separators, at most
/// [`HNCOIN_DECIMALS`] fractional digits (more is an error, never
/// rounded), `u128` overflow is an error, and an amount that comes
/// out to exactly zero is an error.
pub fn parse_decimal_hncoin(input: &str) -> Result<u128, AmountError> {
    if input.is_empty() {
        return Err(AmountError("amount must not be empty".to_string()));
    }
    if input.starts_with('+') || input.starts_with('-') {
        return Err(AmountError(format!("amount must not have a sign: {input}")));
    }

    let (integer_part, fractional_part) = match input.split_once('.') {
        Some((integer_part, fractional_part)) => (integer_part, Some(fractional_part)),
        None => (input, None),
    };

    if integer_part.is_empty() || !integer_part.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AmountError(format!(
            "amount must have a plain decimal integer part: {input}"
        )));
    }

    let fractional_digits = match fractional_part {
        Some(fractional_part) => {
            if fractional_part.is_empty() {
                return Err(AmountError(format!(
                    "amount has a decimal point with no digits after it: {input}"
                )));
            }
            if !fractional_part.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(AmountError(format!(
                    "amount must have only digits after the decimal point: {input}"
                )));
            }
            if fractional_part.len() > HNCOIN_DECIMALS as usize {
                return Err(AmountError(format!(
                    "amount has more than {HNCOIN_DECIMALS} fractional digits: {input}"
                )));
            }
            fractional_part
        }
        None => "",
    };

    let integer_value: u128 = integer_part
        .parse()
        .map_err(|_| AmountError(format!("amount's integer part overflows: {input}")))?;
    let padded_fraction = format!(
        "{fractional_digits:0<width$}",
        width = HNCOIN_DECIMALS as usize
    );
    let fraction_value: u128 = padded_fraction
        .parse()
        .map_err(|_| AmountError(format!("amount's fractional part is invalid: {input}")))?;

    let hnit = integer_value
        .checked_mul(HNIT_PER_HNCOIN)
        .and_then(|whole_hnit| whole_hnit.checked_add(fraction_value))
        .ok_or_else(|| AmountError(format!("amount overflows a 128-bit hnit value: {input}")))?;

    if hnit == 0 {
        return Err(AmountError("amount must not be zero".to_string()));
    }
    Ok(hnit)
}

/// Parses `input` as a plain integer `hnit` amount (`--amount-hnit`,
/// ADR-0047, "for scripts"): digits only, no sign, `u128` overflow is
/// an error, zero is an error (the same "never a meaningful transfer"
/// rule [`parse_decimal_hncoin`] applies).
pub fn parse_hnit(input: &str) -> Result<u128, AmountError> {
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AmountError(format!(
            "amount-hnit must be a plain non-negative integer: {input}"
        )));
    }
    let value: u128 = input
        .parse()
        .map_err(|_| AmountError(format!("amount-hnit overflows a 128-bit value: {input}")))?;
    if value == 0 {
        return Err(AmountError("amount-hnit must not be zero".to_string()));
    }
    Ok(value)
}

/// Formats `hnit` in both units at once (ADR-0047: "Every output that
/// shows an amount shows both forms... so nothing depends on the
/// reader knowing the scale"), for example `1.500000000 HNCOIN
/// (1500000000 hnit)`.
#[must_use]
pub fn format_amount(hnit: u128) -> String {
    let whole = hnit / HNIT_PER_HNCOIN;
    let fraction = hnit % HNIT_PER_HNCOIN;
    format!(
        "{whole}.{fraction:0width$} HNCOIN ({hnit} hnit)",
        width = HNCOIN_DECIMALS as usize
    )
}

#[cfg(test)]
mod tests {
    use super::{format_amount, parse_decimal_hncoin, parse_hnit};

    #[test]
    fn accepts_a_whole_number() {
        assert_eq!(parse_decimal_hncoin("1"), Ok(1_000_000_000));
    }

    #[test]
    fn accepts_a_fractional_amount() {
        assert_eq!(parse_decimal_hncoin("1.5"), Ok(1_500_000_000));
    }

    #[test]
    fn accepts_the_smallest_possible_amount() {
        assert_eq!(parse_decimal_hncoin("0.000000001"), Ok(1));
    }

    #[test]
    fn accepts_exactly_nine_fractional_digits() {
        assert_eq!(parse_decimal_hncoin("0.123456789"), Ok(123_456_789));
    }

    #[test]
    fn rejects_more_than_nine_fractional_digits() {
        assert!(parse_decimal_hncoin("0.1234567890").is_err());
    }

    #[test]
    fn rejects_a_trailing_decimal_point_with_no_digits() {
        assert!(parse_decimal_hncoin("1.").is_err());
    }

    #[test]
    fn rejects_a_missing_integer_part() {
        assert!(parse_decimal_hncoin(".5").is_err());
    }

    #[test]
    fn rejects_an_exponent() {
        assert!(parse_decimal_hncoin("1e3").is_err());
    }

    #[test]
    fn rejects_a_sign() {
        assert!(parse_decimal_hncoin("-1").is_err());
        assert!(parse_decimal_hncoin("+1").is_err());
    }

    #[test]
    fn rejects_a_thousands_separator() {
        assert!(parse_decimal_hncoin("1,5").is_err());
    }

    #[test]
    fn rejects_zero() {
        assert!(parse_decimal_hncoin("0").is_err());
        assert!(parse_decimal_hncoin("0.0").is_err());
        assert!(parse_decimal_hncoin("0.000000000").is_err());
    }

    #[test]
    fn rejects_an_empty_string() {
        assert!(parse_decimal_hncoin("").is_err());
    }

    #[test]
    fn rejects_u128_overflow() {
        let huge = "9".repeat(40);
        assert!(parse_decimal_hncoin(&huge).is_err());
    }

    #[test]
    fn parse_hnit_accepts_a_plain_integer() {
        assert_eq!(parse_hnit("1500000000"), Ok(1_500_000_000));
    }

    #[test]
    fn parse_hnit_rejects_zero() {
        assert!(parse_hnit("0").is_err());
    }

    #[test]
    fn parse_hnit_rejects_a_sign_or_decimal_point() {
        assert!(parse_hnit("-1").is_err());
        assert!(parse_hnit("1.5").is_err());
    }

    #[test]
    fn parse_hnit_rejects_u128_overflow() {
        let huge = "9".repeat(40);
        assert!(parse_hnit(&huge).is_err());
    }

    #[test]
    fn format_amount_shows_both_units() {
        assert_eq!(
            format_amount(1_500_000_000),
            "1.500000000 HNCOIN (1500000000 hnit)"
        );
    }

    #[test]
    fn format_amount_pads_small_fractions() {
        assert_eq!(format_amount(1), "0.000000001 HNCOIN (1 hnit)");
    }

    #[test]
    fn decimal_and_hnit_round_trip_through_format() -> Result<(), super::AmountError> {
        let hnit = parse_decimal_hncoin("1.5")?;
        assert_eq!(format_amount(hnit), "1.500000000 HNCOIN (1500000000 hnit)");
        Ok(())
    }
}

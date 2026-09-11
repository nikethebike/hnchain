use hn_crypto::Digest;

use crate::error::StateResult;
use crate::key::state_key_core;

/// `domain_id` for the `validators` domain (ADR-0007, State Domains).
///
/// This domain holds more than validator identity and consensus state:
/// ADR-0007 also routes `staking` and `slashing` protocol-namespace
/// modules here ("per-validator/delegator records" and "per-validator
/// penalty history" respectively, keyed per validator or delegator, not
/// in `system`) — see [`ValidatorSection`] for why that is not yet
/// reflected in a fuller section registry.
pub const DOMAIN_VALIDATORS: u8 = 0x06;

/// The `validators` domain's SectionId registry (ADR-0007, State
/// Domains). Deliberately not closed/exhaustive the way
/// [`crate::AccountSection`] is: only `record` has a decided schema
/// today. `staking` and `slashing` sections will be added once ADR-0006's
/// `stake`/`unstake` transaction schemas and ADR-0015's slashing
/// activation criteria are decided — the same incrementally-populated-
/// section arc `AccountSection` itself went through this session
/// (envelope → nonce → balance/asset → lifecycle, one section added only
/// once each had a real, decided schema behind it), not a registry
/// invented ahead of the data it would hold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ValidatorSection {
    /// [`crate::ValidatorRecordV1`] itself.
    Record = 0x01,
}

impl ValidatorSection {
    /// Returns the registry value for this section.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Derives the state key for one `validators` domain section leaf
/// (ADR-0007, Canonical State Keys: core schema).
///
/// `validator_id` is used as the state key's `object_id` — not a
/// derived address such as `hn_crypto::validator_address_body` — because
/// `validator_id` is defined to stay stable across consensus key
/// rotation (ADR-0012, "Decided: `validator_id` width, not its exact
/// derivation"), while an address derived from `consensus_key` would
/// change on rotation. Keying state by the rotatable address would lose
/// track of a validator's own record the moment it rotated keys, exactly
/// the failure mode `validator_id` exists to prevent. `validator_id`'s
/// own exact derivation function remains open (same ADR-0012 citation);
/// this function treats it as opaque bytes, the same way
/// `account_section_state_key` treats `account_address` as opaque bytes
/// it does not derive itself.
pub fn validator_section_state_key(
    validator_id: &Digest,
    section: ValidatorSection,
) -> StateResult<Digest> {
    state_key_core(DOMAIN_VALIDATORS, section.as_u8(), validator_id, &[])
}

#[cfg(test)]
mod tests {
    use super::{ValidatorSection, validator_section_state_key};
    use crate::error::StateResult;

    const VALIDATOR_ID: [u8; 32] = [0x44; 32];

    #[test]
    fn record_key_matches_independent_oracle() -> StateResult<()> {
        let key = validator_section_state_key(&VALIDATOR_ID, ValidatorSection::Record)?;
        assert_eq!(
            hex(&key),
            "b3d09b69ad4e75dd459450bfd9dd14399b899590a3b67d600276b2e283509e2a"
        );
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

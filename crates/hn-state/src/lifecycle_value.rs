use hn_hncs::{Decoder, write_u8, write_u16};

use crate::error::{StateError, StateResult};

/// `lifecycle_version` for the current `LifecycleValueV1` shape
/// (account-state.md §4.9, "Decided: lifecycle storage representation
/// only").
pub const LIFECYCLE_VERSION_1: u16 = 1;

/// The `state` registry (account-state.md §4.9), closed for this
/// profile. Unlike `account_type` (§3.1), every value is a real,
/// reachable state; none is reserved as invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum LifecycleState {
    /// Account exists but may not yet be fully active.
    Created = 0x00,
    /// Account may participate according to its permissions and
    /// balances.
    Active = 0x01,
    /// Account exists but selected transitions are blocked by protocol
    /// rules.
    Frozen = 0x02,
    /// Account is valid but should no longer receive new capabilities
    /// except migration or archival operations.
    Deprecated = 0x03,
    /// Account is no longer on the hot execution path but remains
    /// provable according to archival rules.
    Archived = 0x04,
    /// Account has completed a protocol-defined removal process.
    Destroyed = 0x05,
}

impl LifecycleState {
    fn from_u8(value: u8) -> StateResult<Self> {
        match value {
            0x00 => Ok(Self::Created),
            0x01 => Ok(Self::Active),
            0x02 => Ok(Self::Frozen),
            0x03 => Ok(Self::Deprecated),
            0x04 => Ok(Self::Archived),
            0x05 => Ok(Self::Destroyed),
            _ => Err(StateError::InvalidLifecycleState { value }),
        }
    }

    const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// The `accounts` domain lifecycle leaf value (account-state.md §4.9,
/// ADR-0007 SectionId `0x07`): the account's current lifecycle state
/// only. Transition rules (required authorization, allowed source/target
/// states, and similar) are not decided by this schema; it stores
/// whatever state a future transition specification determines is
/// current.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleValueV1 {
    /// The account's current lifecycle state.
    pub state: LifecycleState,
}

impl LifecycleValueV1 {
    /// Encodes this value as canonical HNCS bytes (account-state.md
    /// §4.9).
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + 1);
        write_u16(&mut out, LIFECYCLE_VERSION_1);
        write_u8(&mut out, self.state.as_u8());
        out
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`LifecycleValueV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let lifecycle_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if lifecycle_version != LIFECYCLE_VERSION_1 {
            return Err(StateError::UnsupportedLifecycleVersion {
                value: lifecycle_version,
            });
        }

        let state = LifecycleState::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self { state })
    }
}

#[cfg(test)]
mod tests {
    use super::{LifecycleState, LifecycleValueV1, StateError};

    fn sample() -> LifecycleValueV1 {
        LifecycleValueV1 {
            state: LifecycleState::Created,
        }
    }

    #[test]
    fn encodes_matching_independent_oracle() {
        let encoded = sample().encode();
        assert_eq!(hex(&encoded), "010000");
    }

    #[test]
    fn round_trips_through_decode() -> crate::error::StateResult<()> {
        let encoded = sample().encode();
        let decoded = LifecycleValueV1::decode(&encoded)?;
        assert_eq!(decoded, sample());
        Ok(())
    }

    #[test]
    fn rejects_unsupported_lifecycle_version() {
        let mut encoded = sample().encode();
        encoded[0] = 0x02; // lifecycle_version low byte, little-endian
        assert_eq!(
            LifecycleValueV1::decode(&encoded),
            Err(StateError::UnsupportedLifecycleVersion { value: 2 })
        );
    }

    #[test]
    fn rejects_invalid_state() {
        let mut encoded = sample().encode();
        encoded[2] = 0x06; // one past the closed registry's last value
        assert_eq!(
            LifecycleValueV1::decode(&encoded),
            Err(StateError::InvalidLifecycleState { value: 0x06 })
        );
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut encoded = sample().encode();
        encoded.push(0xff);
        assert!(LifecycleValueV1::decode(&encoded).is_err());
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

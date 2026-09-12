use hn_core::AccountNonce;
use hn_hncs::{Decoder, write_u16, write_u64};

use crate::error::{StateError, StateResult};

/// `nonce_version` for the current `NonceValueV1` shape
/// (account-state.md §4.4, "Decided: nonce storage width only").
pub const NONCE_VERSION_1: u16 = 1;

/// The `accounts` domain nonce leaf value (account-state.md §4.4,
/// ADR-0007 SectionId `0x03`): a canonical storage encoding for
/// `hn_core::AccountNonce`. This type decides the stored width and
/// initial value only; the transaction-validation semantics that
/// produce the stored count (replay protection domain, increment
/// timing, ordering, behavior for failed execution) are decided in
/// ADR-0006 (Transaction Format, "Nonce"), not here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NonceValueV1 {
    /// The account's current nonce.
    pub nonce: AccountNonce,
}

impl NonceValueV1 {
    /// Encodes this value as canonical HNCS bytes
    /// (account-state.md §4.4).
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + 8);
        write_u16(&mut out, NONCE_VERSION_1);
        write_u64(&mut out, self.nonce.get());
        out
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`NonceValueV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let nonce_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if nonce_version != NONCE_VERSION_1 {
            return Err(StateError::UnsupportedNonceVersion {
                value: nonce_version,
            });
        }

        let nonce = decoder.read_u64().map_err(StateError::Encoding)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self {
            nonce: AccountNonce::new(nonce),
        })
    }
}

#[cfg(test)]
mod tests {
    use hn_core::AccountNonce;

    use super::{NonceValueV1, StateError};

    fn sample() -> NonceValueV1 {
        NonceValueV1 {
            nonce: AccountNonce::INITIAL,
        }
    }

    #[test]
    fn encodes_matching_independent_oracle() {
        let encoded = sample().encode();
        assert_eq!(hex(&encoded), "01000000000000000000");
    }

    #[test]
    fn round_trips_through_decode() -> crate::error::StateResult<()> {
        let encoded = sample().encode();
        let decoded = NonceValueV1::decode(&encoded)?;
        assert_eq!(decoded, sample());
        Ok(())
    }

    #[test]
    fn rejects_unsupported_nonce_version() {
        let mut encoded = sample().encode();
        encoded[0] = 0x02; // nonce_version low byte, little-endian
        assert_eq!(
            NonceValueV1::decode(&encoded),
            Err(StateError::UnsupportedNonceVersion { value: 2 })
        );
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut encoded = sample().encode();
        encoded.push(0xff);
        assert!(NonceValueV1::decode(&encoded).is_err());
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

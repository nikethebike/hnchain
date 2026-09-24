use hn_core::AccountNonce;
use hn_crypto::Digest;

use crate::account::{AccountSection, account_section_state_key};
use crate::error::StateResult;
use crate::node::{leaf_hash, value_hash};
use crate::nonce_value::NonceValueV1;
use crate::state_store::StateReader;
use crate::tree::Leaf;

/// Fetches `account`'s current nonce from `reader` — absence maps to
/// [`AccountNonce::INITIAL`], `hn_core::AccountNonce`'s own already-
/// decided default (account-state.md §4.4), the same "absence means the
/// section's own default" convention [`crate::fetch_balance`]/
/// [`crate::fetch_identity`] already use.
pub fn fetch_nonce(reader: &impl StateReader, account: &Digest) -> StateResult<AccountNonce> {
    let key = account_section_state_key(account, AccountSection::Nonce)?;
    match reader.get(&key)? {
        Some(bytes) => Ok(NonceValueV1::decode(&bytes)?.nonce),
        None => Ok(AccountNonce::INITIAL),
    }
}

/// Computes the write-set leaf that records `account`'s nonce as
/// `nonce` (ADR-0006, "Nonce"; ADR-0030, "Transaction And Block
/// Application"). Callers apply this unconditionally once a
/// transaction is included, regardless of whether its own payload
/// succeeded — nonce consumption on failure is already decided
/// (account-state.md §4.4/ADR-0006), this is simply its first
/// implementation.
pub fn nonce_leaf(account: &Digest, nonce: AccountNonce) -> StateResult<Leaf> {
    let key = account_section_state_key(account, AccountSection::Nonce)?;
    let value_bytes = NonceValueV1 { nonce }.encode();
    let vh = value_hash(&value_bytes)?;
    Ok((key, leaf_hash(&key, &vh)?))
}

#[cfg(test)]
mod tests {
    use hn_core::AccountNonce;

    use super::{fetch_nonce, nonce_leaf};
    use crate::account::{AccountSection, account_section_state_key};
    use crate::error::{StateError, StateResult};
    use crate::nonce_value::NonceValueV1;
    use crate::state_store::StateReader;

    const ACCOUNT: [u8; 32] = [0x11; 32];

    struct MapReader(std::collections::BTreeMap<[u8; 32], Vec<u8>>);

    impl StateReader for MapReader {
        fn get(&self, state_key: &[u8; 32]) -> StateResult<Option<Vec<u8>>> {
            Ok(self.0.get(state_key).cloned())
        }
    }

    #[test]
    fn fetch_nonce_defaults_to_initial_when_absent() -> StateResult<()> {
        let reader = MapReader(std::collections::BTreeMap::new());
        assert_eq!(fetch_nonce(&reader, &ACCOUNT)?, AccountNonce::INITIAL);
        Ok(())
    }

    #[test]
    fn fetch_nonce_reads_a_stored_value() -> StateResult<()> {
        let key = account_section_state_key(&ACCOUNT, AccountSection::Nonce)?;
        let stored = NonceValueV1 {
            nonce: AccountNonce::new(7),
        };
        let reader = MapReader(std::collections::BTreeMap::from([(key, stored.encode())]));
        assert_eq!(fetch_nonce(&reader, &ACCOUNT)?, AccountNonce::new(7));
        Ok(())
    }

    #[test]
    fn nonce_leaf_targets_the_nonce_section_key() -> StateResult<()> {
        let leaf = nonce_leaf(&ACCOUNT, AccountNonce::new(3))?;
        let expected_key = account_section_state_key(&ACCOUNT, AccountSection::Nonce)?;
        assert_eq!(leaf.0, expected_key);
        Ok(())
    }

    #[test]
    fn fetch_nonce_propagates_a_decode_error() -> StateResult<()> {
        let key = account_section_state_key(&ACCOUNT, AccountSection::Nonce)?;
        // Too few bytes to even hold `nonce_version` -- a framing
        // error, not a version mismatch.
        let reader = MapReader(std::collections::BTreeMap::from([(key, vec![0xff])]));
        assert!(matches!(
            fetch_nonce(&reader, &ACCOUNT),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }
}

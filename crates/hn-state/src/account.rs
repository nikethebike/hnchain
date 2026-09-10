use hn_crypto::Digest;

use crate::{
    error::{StateError, StateResult},
    key::{state_key_core, state_key_extension},
};

/// `domain_id` for the `accounts` domain (ADR-0007, State Domains).
pub const DOMAIN_ACCOUNTS: u8 = 0x01;

/// `domain_id` for the `account_extensions` domain (ADR-0007, Account
/// Extensions Domain).
pub const DOMAIN_ACCOUNT_EXTENSIONS: u8 = 0x02;

/// `extension_id` reserved for the extension registry leaf (ADR-0007,
/// Account Extensions Domain: Registry And Payload Leaves).
pub const EXTENSION_REGISTRY_ID: u16 = 0x0000;

/// The `accounts` domain SectionId registry (ADR-0007, Accounts Domain:
/// Sections). Closed for tree profile `0x0001`: every account section
/// occupies exactly one leaf, and this list is exhaustive for the current
/// profile.
///
/// `extension` is deliberately not a variant here: ADR-0007 addresses it
/// through the separate [`DOMAIN_ACCOUNT_EXTENSIONS`] domain instead
/// (`docs/specs/core/account-state.md` §3, §4.8).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AccountSection {
    /// `envelope_version`, `account_type`, `address`, `section_versions`
    /// (account-state.md §4.1).
    Envelope = 0x00,
    /// Cryptographic identity binding (account-state.md §3.2).
    Identity = 0x01,
    /// Native HNCOIN balance only; a singleton, unlike [`Self::Asset`]
    /// (account-state.md §4.3).
    Balance = 0x02,
    /// Replay protection / transaction ordering (account-state.md §4.4).
    Nonce = 0x03,
    /// Account-level authorization capabilities (account-state.md §4.5).
    Permission = 0x04,
    /// Bounded protocol-level account metadata (account-state.md §4.6).
    Metadata = 0x05,
    /// Per-account non-native (protocol-level, bridged) asset balance
    /// line items, referencing definitions in the `assets` domain; a
    /// variable-cardinality collection, unlike [`Self::Balance`]
    /// (account-state.md §4.7).
    Asset = 0x06,
    /// Account lifecycle state (account-state.md §4.9).
    Lifecycle = 0x07,
}

impl AccountSection {
    /// Returns the registry value for this section.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Derives the state key for one core account section
/// (ADR-0007, Canonical State Keys: core schema; Accounts Domain:
/// Sections).
///
/// `account_address` is the account's own 32-byte `address_body`
/// (ADR-0003 `address_namespace = 0x01`), for example
/// `hn_crypto::account_address_body`'s output — this function treats it
/// as opaque bytes and does not derive it itself.
pub fn account_section_state_key(
    account_address: &Digest,
    section: AccountSection,
) -> StateResult<Digest> {
    state_key_core(DOMAIN_ACCOUNTS, section.as_u8(), account_address, &[])
}

/// Derives the state key for an account's extension registry leaf
/// (ADR-0007, Account Extensions Domain: Registry And Payload Leaves).
///
/// The registry leaf lists which extensions are installed for the
/// account; it does not itself carry any extension's payload.
pub fn account_extension_registry_state_key(account_address: &Digest) -> StateResult<Digest> {
    state_key_extension(
        DOMAIN_ACCOUNT_EXTENSIONS,
        EXTENSION_REGISTRY_ID,
        account_address,
        &[],
    )
}

/// Derives the state key for one installed extension's payload leaf
/// (ADR-0007, Account Extensions Domain: Registry And Payload Leaves).
///
/// `extension_id` must be nonzero: `0x0000` is reserved for the registry
/// leaf ([`account_extension_registry_state_key`]), not a payload.
pub fn account_extension_payload_state_key(
    account_address: &Digest,
    extension_id: u16,
) -> StateResult<Digest> {
    if extension_id == EXTENSION_REGISTRY_ID {
        return Err(StateError::ReservedExtensionId);
    }

    state_key_extension(
        DOMAIN_ACCOUNT_EXTENSIONS,
        extension_id,
        account_address,
        &[],
    )
}

#[cfg(test)]
mod tests {
    use super::{
        AccountSection, StateError, account_extension_payload_state_key,
        account_extension_registry_state_key, account_section_state_key,
    };
    use crate::error::StateResult;

    const ACCOUNT_ADDRESS: [u8; 32] = [0x11; 32];

    #[test]
    fn section_keys_are_pairwise_distinct() -> StateResult<()> {
        let sections = [
            AccountSection::Envelope,
            AccountSection::Identity,
            AccountSection::Balance,
            AccountSection::Nonce,
            AccountSection::Permission,
            AccountSection::Metadata,
            AccountSection::Asset,
            AccountSection::Lifecycle,
        ];

        let mut keys = Vec::new();
        for section in sections {
            keys.push(account_section_state_key(&ACCOUNT_ADDRESS, section)?);
        }

        for (i, a) in keys.iter().enumerate() {
            for b in &keys[i + 1..] {
                assert_ne!(a, b);
            }
        }

        Ok(())
    }

    #[test]
    fn extension_registry_and_payload_keys_differ() -> StateResult<()> {
        let registry = account_extension_registry_state_key(&ACCOUNT_ADDRESS)?;
        let payload = account_extension_payload_state_key(&ACCOUNT_ADDRESS, 1)?;
        assert_ne!(registry, payload);
        Ok(())
    }

    #[test]
    fn rejects_reserved_extension_id_as_payload() {
        assert_eq!(
            account_extension_payload_state_key(&ACCOUNT_ADDRESS, 0),
            Err(StateError::ReservedExtensionId)
        );
    }
}

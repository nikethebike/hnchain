use hn_crypto::{Digest, hash_profile_0x0001};
use hn_hncs::{write_bytes, write_u8, write_u16};

use crate::error::StateResult;

/// Maximum length, in bytes, of a state key's `object_id` field.
///
/// This is an implementation-level resource bound on the HNCS length
/// field, not a consensus value. ADR-0003 (Address Format, Accepted) fixes
/// `address_body` at 32 bytes, uniform across every namespace, for
/// `address_version = 1`. 64 bytes comfortably covers that with headroom
/// for a future post-quantum address body under a later `address_version`;
/// no change needed unless a future `address_version` exceeds it, in which
/// case raise this bound rather than shrinking it, since shrinking would
/// reject previously valid `object_id` values.
pub const OBJECT_ID_MAX_LEN: usize = 64;

/// Maximum length, in bytes, of a state key's `subkey` field. Every leaf
/// class defined by ADR-0007 uses an empty subkey; this bound exists for
/// future domains (for example `contract_storage`) that will use it.
pub const SUBKEY_MAX_LEN: usize = 64;

/// `state_key_version` carried by both the core and extension state key
/// input schemas (ADR-0007).
const STATE_KEY_VERSION: u16 = 1;

/// Derives a core section state key (ADR-0007, "Core section state key"):
///
/// `state_key = HASH_PROFILE_0x0001("hnchain.state.key.v1", HNCS(StateKeyInputV1))`
///
/// `section_id` addresses a section within `domain_id`'s section registry
/// (for `accounts`, see the SectionId registry in ADR-0007). This function
/// does not itself validate that `section_id` is a member of any
/// particular domain's registry; that is a higher-level account-model
/// concern.
pub fn state_key_core(
    domain_id: u8,
    section_id: u8,
    object_id: &[u8],
    subkey: &[u8],
) -> StateResult<Digest> {
    let mut payload = Vec::new();
    write_u16(&mut payload, STATE_KEY_VERSION);
    write_u8(&mut payload, domain_id);
    write_u8(&mut payload, section_id);
    write_bytes(&mut payload, object_id, OBJECT_ID_MAX_LEN)?;
    write_bytes(&mut payload, subkey, SUBKEY_MAX_LEN)?;

    Ok(hash_profile_0x0001("hnchain.state.key.v1", &payload)?)
}

/// Derives an extension state key (ADR-0007, "Extension state key"):
///
/// `state_key = HASH_PROFILE_0x0001("hnchain.state.key.v1", HNCS(StateKeyInputExtensionV1))`
///
/// `extension_id = 0x0000` addresses the extension registry leaf;
/// `0x0001..=0xFFFF` addresses one extension's payload leaf (ADR-0007,
/// "Account Extensions Domain: Registry And Payload Leaves").
pub fn state_key_extension(
    domain_id: u8,
    extension_id: u16,
    object_id: &[u8],
    subkey: &[u8],
) -> StateResult<Digest> {
    let mut payload = Vec::new();
    write_u16(&mut payload, STATE_KEY_VERSION);
    write_u8(&mut payload, domain_id);
    write_u16(&mut payload, extension_id);
    write_bytes(&mut payload, object_id, OBJECT_ID_MAX_LEN)?;
    write_bytes(&mut payload, subkey, SUBKEY_MAX_LEN)?;

    Ok(hash_profile_0x0001("hnchain.state.key.v1", &payload)?)
}

#[cfg(test)]
mod tests {
    use super::{state_key_core, state_key_extension};

    #[test]
    fn core_and_extension_keys_differ_for_same_object() -> crate::error::StateResult<()> {
        let object_id = [0x11_u8; 32];
        let core = state_key_core(0x01, 0x00, &object_id, &[])?;
        let extension = state_key_extension(0x02, 0x0000, &object_id, &[])?;
        assert_ne!(core, extension);
        Ok(())
    }

    #[test]
    fn matches_independent_oracle_for_envelope_account_a() -> crate::error::StateResult<()> {
        let object_id = [0x11_u8; 32];
        let key = state_key_core(0x01, 0x00, &object_id, &[])?;
        assert_eq!(
            hex(&key),
            "714e5c14db96451f88427778fa7d77881db63796bc5a6342340197164b136c87"
        );
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

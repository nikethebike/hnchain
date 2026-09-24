use hn_crypto::{Digest, KeyDescriptor, account_address_body};

use crate::account::{AccountSection, account_section_state_key};
use crate::error::{StateError, StateResult};
use crate::identity_value::IdentityValueV1;
use crate::node::{leaf_hash, value_hash};
use crate::state_store::StateReader;
use crate::tree::Leaf;

/// Fetches and decodes `account`'s current [`IdentityValueV1`] from
/// `reader`, or `None` if nothing is stored at its Identity-section
/// leaf — mirrors [`crate::fetch_validator_record`]'s own shape for the
/// `validators` domain, specialized to the `accounts` domain's Identity
/// section (ADR-0027).
pub fn fetch_identity(
    reader: &impl StateReader,
    account: &Digest,
) -> StateResult<Option<IdentityValueV1>> {
    let key = account_section_state_key(account, AccountSection::Identity)?;
    match reader.get(&key)? {
        Some(bytes) => Ok(Some(IdentityValueV1::decode(&bytes)?)),
        None => Ok(None),
    }
}

/// Resolves the `account_signing` key a transaction from `sender` must
/// verify against, for an account with **no active multisig
/// configuration** (ADR-0026's own `account_signing_multisig` case is a
/// separate path, [`crate::verify_multisig_authorization`]) — the first
/// concrete resolution of ADR-0002's own `active_key(identity, role,
/// height)` concept for the `account_signing` role, per ADR-0027,
/// "Decided: verification/bootstrap procedure."
///
/// `existing_identity` is whatever [`fetch_identity`] already returned
/// for `sender` — this function does no state access itself, the same
/// "caller already resolved state" boundary every other function in
/// this crate draws (mirrors [`crate::active_key`]'s own separation
/// from the signature verification it feeds).
///
/// Enforces ADR-0027's presence rule and bootstrap check:
///
/// - `existing_identity: Some`, `bootstrap_key: Some` —
///   [`StateError::UnexpectedBootstrapKey`]: redundant, not tolerated.
/// - `existing_identity: Some`, `bootstrap_key: None` — resolves to the
///   stored key, the ordinary case.
/// - `existing_identity: None`, `bootstrap_key: None` —
///   [`StateError::MissingBootstrapKey`]: nothing to verify against.
/// - `existing_identity: None`, `bootstrap_key: Some` — the bootstrap
///   case: `bootstrap_key` must derive `sender`'s own address
///   ([`StateError::BootstrapKeyAddressMismatch`] otherwise, the
///   anti-spoofing check), then resolves to `bootstrap_key` itself.
///
/// This function only resolves the key — it does not verify the
/// transaction's signature (an ordinary `SignatureEnvelope::verify`
/// call against the resolved key, the caller's job) and does not write
/// [`IdentityValueV1`] for a successful bootstrap (a separate state-
/// transition step, [`apply_identity_bootstrap`], since resolving a key
/// is a pure read-side operation while writing state is not).
pub fn resolve_account_signing_key(
    existing_identity: Option<&IdentityValueV1>,
    bootstrap_key: Option<&KeyDescriptor>,
    sender: &Digest,
    network_id: u16,
) -> StateResult<KeyDescriptor> {
    match (existing_identity, bootstrap_key) {
        (Some(_), Some(_)) => Err(StateError::UnexpectedBootstrapKey),
        (Some(identity), None) => Ok(identity.key),
        (None, None) => Err(StateError::MissingBootstrapKey),
        (None, Some(bootstrap_key)) => {
            let derived_address = account_address_body(
                network_id,
                bootstrap_key.algorithm_id(),
                &bootstrap_key.public_key_bytes(),
            )?;
            if derived_address != *sender {
                return Err(StateError::BootstrapKeyAddressMismatch);
            }
            Ok(*bootstrap_key)
        }
    }
}

/// Computes the one write-set leaf a successful bootstrap transaction
/// produces (ADR-0027, "Decided: verification/bootstrap procedure,"
/// step 3): `sender`'s Identity-section leaf, set to `bootstrap_key`.
///
/// Callers apply this only after
/// [`resolve_account_signing_key`] has already accepted `bootstrap_key`
/// (address-derivation check passed) and the transaction's signature
/// has verified against it — the same "implicit creation writes several
/// leaves at once" pattern `transfer`'s own implicit-account-creation
/// path already established for Envelope/Nonce/Balance/Asset/Lifecycle;
/// this is Identity's own leaf in that same automatic-side-effect set,
/// not a separate operation a client requests.
pub fn apply_identity_bootstrap(
    sender: Digest,
    bootstrap_key: &KeyDescriptor,
) -> StateResult<Leaf> {
    let value = IdentityValueV1 {
        key: *bootstrap_key,
    };
    identity_value_leaf(&sender, &value)
}

/// Computes the one write-set leaf a `permission_update`
/// [`crate::permission_update_payload::PermissionUpdatePayloadV1::RotateIdentityKey`]
/// operation produces (ADR-0028, "Account-Level Key Rotation"):
/// `sender`'s Identity-section leaf, replaced with `new_key`.
///
/// Structurally identical to [`apply_identity_bootstrap`] — both just
/// write an [`IdentityValueV1`] to the same leaf — but kept as its own,
/// intent-revealing name rather than merged into one generic function:
/// a reader at either call site should immediately know which flow they
/// are in. Preconditions (an existing `IdentityValueV1` to rotate away
/// from, and no active multisig configuration, ADR-0028) are a
/// transaction-validation concern checked before this function runs,
/// the same boundary [`apply_identity_bootstrap`] already draws for its
/// own address-derivation check.
pub fn apply_identity_rotation(sender: Digest, new_key: &KeyDescriptor) -> StateResult<Leaf> {
    let value = IdentityValueV1 { key: *new_key };
    identity_value_leaf(&sender, &value)
}

fn identity_value_leaf(sender: &Digest, value: &IdentityValueV1) -> StateResult<Leaf> {
    let key = account_section_state_key(sender, AccountSection::Identity)?;
    let value_bytes = value.encode()?;
    let vh = value_hash(&value_bytes)?;
    Ok((key, leaf_hash(&key, &vh)?))
}

#[cfg(test)]
mod tests {
    use hn_crypto::{Ed25519KeyPair, KeyRole, account_address_body};

    use super::{apply_identity_bootstrap, resolve_account_signing_key};
    use crate::error::{StateError, StateResult};
    use crate::identity_value::IdentityValueV1;

    const SENDER: [u8; 32] = [0x11; 32];
    const NETWORK_ID: u16 = 1;

    fn keypair(seed: u8) -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [seed; 32])
    }

    #[test]
    fn resolves_the_stored_key_when_identity_already_exists() -> StateResult<()> {
        let key = keypair(0x01).key_descriptor();
        let identity = IdentityValueV1 { key };
        let resolved = resolve_account_signing_key(Some(&identity), None, &SENDER, NETWORK_ID)?;
        assert_eq!(resolved, key);
        Ok(())
    }

    #[test]
    fn rejects_a_bootstrap_key_when_identity_already_exists() {
        let identity = IdentityValueV1 {
            key: keypair(0x01).key_descriptor(),
        };
        let bootstrap_key = keypair(0x02).key_descriptor();
        assert_eq!(
            resolve_account_signing_key(Some(&identity), Some(&bootstrap_key), &SENDER, NETWORK_ID),
            Err(StateError::UnexpectedBootstrapKey)
        );
    }

    #[test]
    fn rejects_a_missing_bootstrap_key_when_no_identity_exists() {
        assert_eq!(
            resolve_account_signing_key(None, None, &SENDER, NETWORK_ID),
            Err(StateError::MissingBootstrapKey)
        );
    }

    #[test]
    fn bootstraps_from_a_key_that_derives_the_sender_address() -> StateResult<()> {
        let keypair = keypair(0x03);
        let descriptor = keypair.key_descriptor();
        let sender = account_address_body(
            NETWORK_ID,
            descriptor.algorithm_id(),
            &descriptor.public_key_bytes(),
        )?;

        let resolved = resolve_account_signing_key(None, Some(&descriptor), &sender, NETWORK_ID)?;
        assert_eq!(resolved, descriptor);
        Ok(())
    }

    #[test]
    fn rejects_a_bootstrap_key_that_does_not_derive_the_sender_address() {
        let descriptor = keypair(0x04).key_descriptor();
        // SENDER is an arbitrary constant, not derived from this key.
        assert_eq!(
            resolve_account_signing_key(None, Some(&descriptor), &SENDER, NETWORK_ID),
            Err(StateError::BootstrapKeyAddressMismatch)
        );
    }

    #[test]
    fn apply_identity_bootstrap_produces_the_identity_leaf() -> StateResult<()> {
        let descriptor = keypair(0x05).key_descriptor();
        let leaf = apply_identity_bootstrap(SENDER, &descriptor)?;

        let expected_key = crate::account::account_section_state_key(
            &SENDER,
            crate::account::AccountSection::Identity,
        )?;
        assert_eq!(leaf.0, expected_key);

        let expected_value = IdentityValueV1 { key: descriptor };
        assert_eq!(
            leaf.1,
            super::identity_value_leaf(&SENDER, &expected_value)?.1
        );
        Ok(())
    }

    #[test]
    fn apply_identity_rotation_produces_the_identity_leaf() -> StateResult<()> {
        let new_key = keypair(0x06).key_descriptor();
        let leaf = super::apply_identity_rotation(SENDER, &new_key)?;

        let expected_key = crate::account::account_section_state_key(
            &SENDER,
            crate::account::AccountSection::Identity,
        )?;
        assert_eq!(leaf.0, expected_key);

        let expected_value = IdentityValueV1 { key: new_key };
        assert_eq!(
            leaf.1,
            super::identity_value_leaf(&SENDER, &expected_value)?.1
        );
        Ok(())
    }
}

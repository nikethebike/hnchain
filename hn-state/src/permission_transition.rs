use hn_crypto::{Digest, SignatureEnvelope};

use crate::account::{AccountSection, account_section_state_key};
use crate::error::{StateError, StateResult};
use crate::identity_transition::apply_identity_rotation;
use crate::node::{leaf_hash, value_hash};
use crate::permission_update_payload::PermissionUpdatePayloadV1;
use crate::permission_value::{MultisigConfigV1, PermissionValueV1};
use crate::receipt::{ReceiptStatus, ReceiptV1};
use crate::state_store::StateReader;
use crate::tree::Leaf;

/// Fetches and decodes `account`'s current [`PermissionValueV1`] from
/// `reader`, or `None` if nothing is stored at its Permission-section
/// leaf — mirrors [`crate::fetch_identity`]'s own shape for the
/// `accounts` domain's Permission section.
pub fn fetch_permission(
    reader: &impl StateReader,
    account: &Digest,
) -> StateResult<Option<PermissionValueV1>> {
    let key = account_section_state_key(account, AccountSection::Permission)?;
    match reader.get(&key)? {
        Some(bytes) => Ok(Some(PermissionValueV1::decode(&bytes)?)),
        None => Ok(None),
    }
}

/// Computes the updated write-set leaves a `permission_update`
/// produces — `sender`'s Permission-section leaf for
/// [`PermissionUpdatePayloadV1::SetAccountSigningMultisig`] (ADR-0026),
/// `sender`'s Identity-section leaf for
/// [`PermissionUpdatePayloadV1::RotateIdentityKey`] (ADR-0028), or
/// *both* — Permission cleared, Identity set to the successor — for
/// [`PermissionUpdatePayloadV1::DeactivateMultisig`] (ADR-0029) —
/// mirroring [`crate::apply_validator_update`]'s own "one entry point
/// per `tx_type`, internal match over operations" shape.
///
/// Returns `Vec<Leaf>`, not a fixed-size array like every other
/// `apply_*` function in this crate (`apply_transfer -> [Leaf; 2]`,
/// `apply_stake -> [Leaf; 1]`, ...): `permission_update`'s three
/// operations are not uniform in how many sections they touch —
/// `SetAccountSigningMultisig`/`RotateIdentityKey` touch exactly one,
/// `DeactivateMultisig` touches two — and a fixed array would force
/// either an artificial padding write or a second entry-point function
/// for just one operation (ADR-0029, "Rejected Options": both
/// considered and rejected as worse than an honest variable count).
///
/// This is the state-transition half only. Which authorization rule a
/// given operation must satisfy is a transaction-validation concern
/// checked before this function runs, not by it — the same boundary
/// [`crate::apply_stake`]/[`crate::apply_transfer`] already draw between
/// "is this transaction authorized" and "what state does applying it
/// produce": `SetAccountSigningMultisig`'s own rule (today's single-key
/// default for a first activation, or [`verify_multisig_authorization`]
/// against the sender's pre-transaction configuration for a
/// reconfiguration), `RotateIdentityKey`'s own precondition (an
/// existing `IdentityValueV1` and no active multisig configuration,
/// ADR-0028), and `DeactivateMultisig`'s own precondition (an active
/// multisig configuration, authorized via
/// [`verify_multisig_authorization`] against it — the same rule a
/// reconfiguration already uses, ADR-0029) are all left to that
/// not-yet-built layer. There is no domain-specific rejection at this
/// layer: every payload variant was already structurally validated by
/// [`crate::permission_update_payload::PermissionUpdatePayloadV1::decode`],
/// so this function cannot fail for a domain reason — only the generic
/// leaf-construction `Hash`/`Encoding` errors every `*_leaf` helper in
/// this crate can already produce. `DeactivateMultisig` clears the
/// Permission leaf unconditionally (an overwrite to `None`, not a
/// read-then-clear) — it does not need `sender`'s pre-transaction
/// configuration to know what to write, only that overwriting it is
/// what this operation means.
pub fn apply_permission_update(
    sender: Digest,
    payload: &PermissionUpdatePayloadV1,
) -> StateResult<Vec<Leaf>> {
    match payload {
        PermissionUpdatePayloadV1::SetAccountSigningMultisig(config) => {
            let value = PermissionValueV1 {
                account_signing_multisig: Some(config.clone()),
            };
            Ok(vec![permission_value_leaf(&sender, &value)?])
        }
        PermissionUpdatePayloadV1::RotateIdentityKey(new_key) => {
            Ok(vec![apply_identity_rotation(sender, new_key)?])
        }
        PermissionUpdatePayloadV1::DeactivateMultisig(successor) => {
            let cleared = PermissionValueV1 {
                account_signing_multisig: None,
            };
            Ok(vec![
                permission_value_leaf(&sender, &cleared)?,
                apply_identity_rotation(sender, successor)?,
            ])
        }
    }
}

/// Applies a `permission_update` and produces its [`ReceiptV1`] in one
/// step, mirroring [`crate::apply_stake_with_receipt`]'s own shape.
///
/// Unlike every other `apply_*_with_receipt` function in this crate,
/// this one has no `Failed`-receipt branch: [`apply_permission_update`]
/// has no legitimate transaction-outcome rejection of its own (see its
/// own documentation) — the real rejection surface for a
/// `permission_update` (insufficient/invalid signatures) lives in
/// [`verify_multisig_authorization`] and today's single-key default,
/// both checked by a transaction-validation pipeline before this
/// function is ever called, not inside it. Returns `Vec<Leaf>` directly
/// rather than `Option<Vec<Leaf>>` for the same reason: there is no
/// case here that produces zero leaves.
pub fn apply_permission_update_with_receipt(
    sender: Digest,
    payload: &PermissionUpdatePayloadV1,
    tx_id: Digest,
) -> StateResult<(Vec<Leaf>, ReceiptV1)> {
    let leaves = apply_permission_update(sender, payload)?;
    Ok((
        leaves,
        ReceiptV1 {
            tx_id,
            status: ReceiptStatus::Success,
        },
    ))
}

/// Verifies that `signatures` authorize `message` under `config`
/// (ADR-0026, "Decided: Multi-signature verification rule") — the
/// `account_signing_multisig`-active case only. A `None` configuration
/// (single-key mode) is not this function's concern: resolving and
/// verifying against the context-derived default key is
/// [`hn_crypto::SignatureEnvelope::verify`] against a directly-resolved
/// [`hn_crypto::KeyDescriptor`], unchanged from before ADR-0026 — see
/// ADR-0002's own still-open `active_key(identity, role, height)`
/// mechanism for how that resolution itself eventually happens.
///
/// Every entry in `signatures` must carry a present `key_reference`
/// ([`StateError::MissingKeyReference`] otherwise) — an entry shaped for
/// single-key mode has no defined meaning once an account is in
/// multisig mode. Among the entries that do, an entry is *counted*
/// toward `config.threshold` only if its `key_reference` is in bounds of
/// `config.authorized_keys` and its signature verifies against that
/// key; an out-of-bounds reference or a failed verification simply does
/// not count, rather than aborting the whole check — matching ADR-0026's
/// own "cap, not exact" framing ("entries beyond threshold... permitted
/// but never required"): a garbage or invalid extra entry among several
/// gathered opportunistically should not defeat an otherwise-sufficient
/// set of valid ones. Duplicate `key_reference` values are naturally
/// deduplicated by this same counting (the same key cannot contribute a
/// second, independent count).
pub fn verify_multisig_authorization(
    config: &MultisigConfigV1,
    signatures: &[SignatureEnvelope],
    message: &[u8],
) -> StateResult<()> {
    let mut valid_indices: Vec<u8> = Vec::new();

    for envelope in signatures {
        let key_reference = envelope
            .key_reference
            .ok_or(StateError::MissingKeyReference)?;

        if valid_indices.contains(&key_reference) {
            continue;
        }

        let Some(descriptor) = config.authorized_keys.get(usize::from(key_reference)) else {
            continue;
        };

        if envelope.verify(descriptor, message).is_ok() {
            valid_indices.push(key_reference);
        }
    }

    if valid_indices.len() >= usize::from(config.threshold) {
        Ok(())
    } else {
        Err(StateError::InsufficientMultisigSignatures {
            required: config.threshold,
            valid: valid_indices.len(),
        })
    }
}

fn permission_value_leaf(sender: &Digest, value: &PermissionValueV1) -> StateResult<Leaf> {
    let key = account_section_state_key(sender, AccountSection::Permission)?;
    let value_bytes = value.encode()?;
    let vh = value_hash(&value_bytes)?;
    Ok((key, leaf_hash(&key, &vh)?))
}

#[cfg(test)]
mod tests {
    use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope};

    use super::{
        StateError, apply_permission_update, apply_permission_update_with_receipt,
        verify_multisig_authorization,
    };
    use crate::account::{AccountSection, account_section_state_key};
    use crate::error::StateResult;
    use crate::permission_update_payload::PermissionUpdatePayloadV1;
    use crate::permission_value::MultisigConfigV1;
    use crate::receipt::ReceiptStatus;

    const SENDER: [u8; 32] = [0x11; 32];
    const TX_ID: [u8; 32] = [0x99; 32];
    const MESSAGE: &[u8] = b"hnchain permission_update test message";

    fn keypair(seed: u8) -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [seed; 32])
    }

    fn two_of_three_config() -> (MultisigConfigV1, [Ed25519KeyPair; 3]) {
        let keypairs = [keypair(0x01), keypair(0x02), keypair(0x03)];
        let mut authorized_keys: Vec<_> = keypairs
            .iter()
            .map(Ed25519KeyPair::key_descriptor)
            .collect();
        authorized_keys.sort_by_key(hn_crypto::KeyDescriptor::public_key_bytes);
        (
            MultisigConfigV1 {
                threshold: 2,
                authorized_keys,
            },
            keypairs,
        )
    }

    /// Builds a `SignatureEnvelope` for `keypair` over `message`,
    /// `key_reference`d against its own position in `config`'s
    /// authorized-keys list. `StateResult`-returning rather than
    /// panicking if `keypair` is not one of `config`'s authorized keys
    /// (a test-setup bug, not a case any test here intends to exercise)
    /// — this workspace denies `unwrap`/`expect`/`panic` even in test
    /// code.
    fn envelope_for(
        config: &MultisigConfigV1,
        keypair: &Ed25519KeyPair,
        message: &[u8],
    ) -> StateResult<SignatureEnvelope> {
        let descriptor = keypair.key_descriptor();
        let key_reference = config
            .authorized_keys
            .iter()
            .position(|candidate| candidate.public_key_bytes() == descriptor.public_key_bytes())
            .ok_or(StateError::MissingKeyReference)? as u8;
        Ok(SignatureEnvelope {
            algorithm_id: descriptor.algorithm_id(),
            key_reference: Some(key_reference),
            signature: keypair.sign(message).to_vec(),
        })
    }

    #[test]
    fn apply_sets_the_permission_leaf() -> StateResult<()> {
        let (config, _keypairs) = two_of_three_config();
        let payload = PermissionUpdatePayloadV1::SetAccountSigningMultisig(config);
        let leaves = apply_permission_update(SENDER, &payload)?;
        assert_eq!(leaves.len(), 1);
        Ok(())
    }

    #[test]
    fn apply_with_receipt_always_yields_success() -> StateResult<()> {
        let (config, _keypairs) = two_of_three_config();
        let payload = PermissionUpdatePayloadV1::SetAccountSigningMultisig(config);
        let (_leaves, receipt) = apply_permission_update_with_receipt(SENDER, &payload, TX_ID)?;
        assert_eq!(receipt.status, ReceiptStatus::Success);
        Ok(())
    }

    #[test]
    fn apply_rotates_the_identity_key() -> StateResult<()> {
        let new_key =
            Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x09; 32]).key_descriptor();
        let payload = PermissionUpdatePayloadV1::RotateIdentityKey(new_key);
        let leaves = apply_permission_update(SENDER, &payload)?;
        assert_eq!(leaves.len(), 1);
        Ok(())
    }

    #[test]
    fn apply_deactivates_multisig_and_sets_the_successor_identity() -> StateResult<()> {
        let successor =
            Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x0b; 32]).key_descriptor();
        let payload = PermissionUpdatePayloadV1::DeactivateMultisig(successor);
        let leaves = apply_permission_update(SENDER, &payload)?;
        assert_eq!(leaves.len(), 2);

        let permission_key = account_section_state_key(&SENDER, AccountSection::Permission)?;
        let identity_key = account_section_state_key(&SENDER, AccountSection::Identity)?;
        let leaf_keys: Vec<_> = leaves.iter().map(|leaf| leaf.0).collect();
        assert!(leaf_keys.contains(&permission_key));
        assert!(leaf_keys.contains(&identity_key));
        Ok(())
    }

    #[test]
    fn verify_succeeds_with_exactly_threshold_valid_signatures() -> StateResult<()> {
        let (config, keypairs) = two_of_three_config();
        let signatures = vec![
            envelope_for(&config, &keypairs[0], MESSAGE)?,
            envelope_for(&config, &keypairs[1], MESSAGE)?,
        ];
        verify_multisig_authorization(&config, &signatures, MESSAGE)
    }

    #[test]
    fn verify_succeeds_with_more_than_threshold_valid_signatures() -> StateResult<()> {
        let (config, keypairs) = two_of_three_config();
        let signatures = vec![
            envelope_for(&config, &keypairs[0], MESSAGE)?,
            envelope_for(&config, &keypairs[1], MESSAGE)?,
            envelope_for(&config, &keypairs[2], MESSAGE)?,
        ];
        verify_multisig_authorization(&config, &signatures, MESSAGE)
    }

    #[test]
    fn verify_rejects_below_threshold_signatures() -> StateResult<()> {
        let (config, keypairs) = two_of_three_config();
        let signatures = vec![envelope_for(&config, &keypairs[0], MESSAGE)?];
        assert_eq!(
            verify_multisig_authorization(&config, &signatures, MESSAGE),
            Err(StateError::InsufficientMultisigSignatures {
                required: 2,
                valid: 1
            })
        );
        Ok(())
    }

    #[test]
    fn verify_does_not_double_count_a_duplicate_key_reference() -> StateResult<()> {
        let (config, keypairs) = two_of_three_config();
        let one_signature = envelope_for(&config, &keypairs[0], MESSAGE)?;
        let signatures = vec![one_signature.clone(), one_signature];
        assert_eq!(
            verify_multisig_authorization(&config, &signatures, MESSAGE),
            Err(StateError::InsufficientMultisigSignatures {
                required: 2,
                valid: 1
            })
        );
        Ok(())
    }

    #[test]
    fn verify_does_not_count_a_signature_over_the_wrong_message() -> StateResult<()> {
        let (config, keypairs) = two_of_three_config();
        let signatures = vec![
            envelope_for(&config, &keypairs[0], b"a different message")?,
            envelope_for(&config, &keypairs[1], MESSAGE)?,
        ];
        assert_eq!(
            verify_multisig_authorization(&config, &signatures, MESSAGE),
            Err(StateError::InsufficientMultisigSignatures {
                required: 2,
                valid: 1
            })
        );
        Ok(())
    }

    #[test]
    fn verify_does_not_count_an_out_of_bounds_key_reference() -> StateResult<()> {
        let (config, keypairs) = two_of_three_config();
        let mut bad = envelope_for(&config, &keypairs[0], MESSAGE)?;
        bad.key_reference = Some(200);
        let signatures = vec![bad, envelope_for(&config, &keypairs[1], MESSAGE)?];
        assert_eq!(
            verify_multisig_authorization(&config, &signatures, MESSAGE),
            Err(StateError::InsufficientMultisigSignatures {
                required: 2,
                valid: 1
            })
        );
        Ok(())
    }

    #[test]
    fn verify_rejects_a_signature_entry_with_no_key_reference() -> StateResult<()> {
        let (config, keypairs) = two_of_three_config();
        let mut single_key_shaped = envelope_for(&config, &keypairs[0], MESSAGE)?;
        single_key_shaped.key_reference = None;
        let signatures = vec![single_key_shaped];
        assert_eq!(
            verify_multisig_authorization(&config, &signatures, MESSAGE),
            Err(StateError::MissingKeyReference)
        );
        Ok(())
    }
}

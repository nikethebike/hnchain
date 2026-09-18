use hn_crypto::{Digest, SignatureEnvelope};

use crate::account::{AccountSection, account_section_state_key};
use crate::error::{StateError, StateResult};
use crate::node::{leaf_hash, value_hash};
use crate::permission_update_payload::PermissionUpdatePayloadV1;
use crate::permission_value::{MultisigConfigV1, PermissionValueV1};
use crate::receipt::{ReceiptStatus, ReceiptV1};
use crate::tree::Leaf;

/// Computes the one updated write-set leaf a `permission_update`
/// produces (ADR-0026, "Decided: `permission_update` payload"):
/// `sender`'s Permission-section leaf, set to
/// `payload.new_account_signing_multisig`.
///
/// This is the state-transition half only. Which authorization rule a
/// given `permission_update` must satisfy (today's single-key default
/// for a first activation, or [`verify_multisig_authorization`] against
/// the sender's pre-transaction configuration for a reconfiguration) is
/// a transaction-validation concern checked before this function runs,
/// not by it — the same boundary [`crate::apply_stake`]/
/// [`crate::apply_transfer`] already draw between "is this transaction
/// authorized" and "what state does applying it produce." There is no
/// domain-specific rejection at this layer: `payload.
/// new_account_signing_multisig` was already structurally validated by
/// [`crate::permission_update_payload::PermissionUpdatePayloadV1::decode`]
/// (threshold bounds, canonical key order, no duplicates), so this
/// function cannot fail for a domain reason — only the generic
/// leaf-construction `Hash`/`Encoding` errors every `*_leaf` helper in
/// this crate can already produce.
pub fn apply_permission_update(
    sender: Digest,
    payload: &PermissionUpdatePayloadV1,
) -> StateResult<[Leaf; 1]> {
    let value = PermissionValueV1 {
        account_signing_multisig: Some(payload.new_account_signing_multisig.clone()),
    };
    Ok([permission_value_leaf(&sender, &value)?])
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
/// function is ever called, not inside it. Returns `[Leaf; 1]` directly
/// rather than `Option<[Leaf; 1]>` for the same reason: there is no case
/// here that produces zero leaves.
pub fn apply_permission_update_with_receipt(
    sender: Digest,
    payload: &PermissionUpdatePayloadV1,
    tx_id: Digest,
) -> StateResult<([Leaf; 1], ReceiptV1)> {
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
        let payload = PermissionUpdatePayloadV1 {
            new_account_signing_multisig: config,
        };
        let [_leaf] = apply_permission_update(SENDER, &payload)?;
        Ok(())
    }

    #[test]
    fn apply_with_receipt_always_yields_success() -> StateResult<()> {
        let (config, _keypairs) = two_of_three_config();
        let payload = PermissionUpdatePayloadV1 {
            new_account_signing_multisig: config,
        };
        let (_leaves, receipt) = apply_permission_update_with_receipt(SENDER, &payload, TX_ID)?;
        assert_eq!(receipt.status, ReceiptStatus::Success);
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

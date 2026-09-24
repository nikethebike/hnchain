use hn_core::BlockHeight;
use hn_crypto::Digest;

use crate::active_set::fetch_validator_record;
use crate::error::{StateError, StateResult};
use crate::identity_transition::apply_identity_bootstrap;
use crate::list_merkle::list_merkle_root;
use crate::nonce_transition::{fetch_nonce, nonce_leaf};
use crate::permission_transition::apply_permission_update_with_receipt;
use crate::permission_update_payload::PermissionUpdatePayloadV1;
use crate::receipt::ReceiptV1;
use crate::state_store::StateReader;
use crate::transaction_envelope::{
    TransactionEnvelope, TransactionPayload, decode_transaction_payload,
};
use crate::transfer::{apply_transfer_with_receipt, fetch_transfer_party};
use crate::tree::Leaf;
use crate::tx_id::tx_id;
use crate::validator_transition::{
    apply_stake_with_receipt, apply_unstake_with_receipt, apply_validator_update_with_receipt,
};

/// One transaction's result from [`apply_transaction`] (ADR-0030,
/// "Transaction And Block Application").
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedTransaction {
    /// This transaction's own ID (ADR-0006, "Transaction ID").
    pub tx_id: Digest,
    /// Every write-set leaf this transaction produces, in order: the
    /// bootstrap `IdentityValueV1` leaf (ADR-0027), if `bootstrap_key`
    /// was present and verification succeeded; then the underlying
    /// `apply_*` operation's own leaves (if any; a `Failed` receipt
    /// writes none of its own); then the unconditional nonce update
    /// ([`crate::nonce_leaf`]), always.
    pub write_set: Vec<Leaf>,
    /// This transaction's receipt.
    pub receipt: ReceiptV1,
}

/// Applies one transaction against `reader` (already-existing state, as
/// of `current_height`) — ADR-0030, "Decided: `apply_transaction`
/// composes what already exists."
///
/// Returns a hard [`StateError`] (not a `Failed` receipt) for anything
/// that means this transaction was never validly includable at all:
/// failed signature/bootstrap/multisig authorization
/// ([`TransactionEnvelope::verify`]), a nonce that does not match
/// `sender`'s current stored value exactly
/// ([`StateError::NonceMismatch`]), a height outside
/// `validity_window` ([`StateError::TransactionOutsideValidityWindow`]),
/// or a payload that fails to decode. A *legitimate* execution failure
/// (insufficient balance, wrong validator status, ...) is not one of
/// these — it becomes an ordinary `ReceiptStatus::Failed` receipt with
/// no leaves of its own, exactly as each underlying `apply_*_with_receipt`
/// already decided; this function does not reinterpret that boundary.
///
/// Once `verify` succeeds, a `bootstrap_key: Some(key)` envelope's new
/// `IdentityValueV1` is written as the automatic side effect
/// `TransactionEnvelope::verify`'s own doc comment names as this
/// function's job ([`crate::apply_identity_bootstrap`]) — ADR-0027,
/// "write once, reuse forever".
///
/// The nonce leaf ([`crate::nonce_leaf`]) is always appended to the
/// write-set, success or failure — ADR-0006's own "nonce still consumed
/// on failure" rule, implemented here for the first time.
///
/// Deducts no fee (ADR-0030, "Decided: no fee deduction in this
/// pass" — the fee rate/floor remains an undecided economic
/// parameter). `governance` and the 3 still-undecided `tx_type`s are
/// not wired — [`decode_transaction_payload`] itself already rejects
/// the latter; `governance` is rejected here too
/// ([`StateError::UndecidedTransactionPayload`]), named explicitly in
/// ADR-0030's own "Explicitly Not Resolved" (needs a chamber-weight
/// query this crate does not have yet).
pub fn apply_transaction(
    envelope: &TransactionEnvelope,
    reader: &impl StateReader,
    current_height: BlockHeight,
) -> StateResult<AppliedTransaction> {
    envelope.verify(reader)?;

    let bootstrap_leaf = match &envelope.bootstrap_key {
        Some(key) => Some(apply_identity_bootstrap(envelope.sender, key)?),
        None => None,
    };

    let stored_nonce = fetch_nonce(reader, &envelope.sender)?;
    if stored_nonce != envelope.nonce {
        return Err(StateError::NonceMismatch {
            expected: stored_nonce.get(),
            found: envelope.nonce.get(),
        });
    }

    if let Some(min_height) = envelope.validity_window.min_height
        && current_height.get() < min_height.get()
    {
        return Err(StateError::TransactionOutsideValidityWindow);
    }
    if let Some(max_height) = envelope.validity_window.max_height
        && current_height.get() > max_height.get()
    {
        return Err(StateError::TransactionOutsideValidityWindow);
    }

    let tx_id_value = tx_id(&envelope.encode()?)?;
    let payload = decode_transaction_payload(envelope.tx_type, &envelope.payload)?;

    let (leaves, receipt) = match payload {
        TransactionPayload::Transfer(payload) => {
            let sender = fetch_transfer_party(reader, envelope.sender)?;
            let recipient = fetch_transfer_party(reader, payload.recipient)?;
            let (leaves, receipt) =
                apply_transfer_with_receipt(&sender, &recipient, &payload, tx_id_value)?;
            (leaves.map(|leaves| leaves.to_vec()), receipt)
        }
        TransactionPayload::Stake(payload) => {
            let record = fetch_validator_record(reader, &envelope.sender)?.ok_or(
                StateError::UnknownValidator {
                    validator_id: envelope.sender,
                },
            )?;
            let (leaves, receipt) = apply_stake_with_receipt(&record, &payload, tx_id_value)?;
            (leaves.map(|leaves| leaves.to_vec()), receipt)
        }
        TransactionPayload::Unstake(payload) => {
            let record = fetch_validator_record(reader, &envelope.sender)?.ok_or(
                StateError::UnknownValidator {
                    validator_id: envelope.sender,
                },
            )?;
            let (leaves, receipt) =
                apply_unstake_with_receipt(&record, &payload, current_height, tx_id_value)?;
            (leaves.map(|leaves| leaves.to_vec()), receipt)
        }
        TransactionPayload::ValidatorUpdate(payload) => {
            let existing = fetch_validator_record(reader, &envelope.sender)?;
            let (leaves, receipt) = apply_validator_update_with_receipt(
                existing.as_ref(),
                envelope.sender,
                &payload,
                tx_id_value,
            )?;
            (leaves.map(|leaves| leaves.to_vec()), receipt)
        }
        TransactionPayload::Governance(_) => {
            return Err(StateError::UndecidedTransactionPayload {
                tx_type: envelope.tx_type.as_u8(),
            });
        }
        TransactionPayload::PermissionUpdate(payload) => {
            let payload: PermissionUpdatePayloadV1 = payload;
            let (leaves, receipt) =
                apply_permission_update_with_receipt(envelope.sender, &payload, tx_id_value)?;
            (Some(leaves), receipt)
        }
    };

    let mut write_set = Vec::new();
    if let Some(bootstrap_leaf) = bootstrap_leaf {
        write_set.push(bootstrap_leaf);
    }
    write_set.extend(leaves.unwrap_or_default());
    let next_nonce = stored_nonce
        .checked_next()
        .map_err(|_| StateError::NonceOverflow)?;
    write_set.push(nonce_leaf(&envelope.sender, next_nonce)?);

    Ok(AppliedTransaction {
        tx_id: tx_id_value,
        write_set,
        receipt,
    })
}

/// A block's transactions, applied (ADR-0030, "Decided: `apply_block`
/// aggregates, does not attempt the full new `state_root`").
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockApplicationResult {
    /// Every transaction's own result, in block order.
    pub applied: Vec<AppliedTransaction>,
    /// `hn-list-merkle-v1` root over every `tx_id`, in block order
    /// (ADR-0008, "Ordered List Commitment").
    pub transactions_root: Digest,
    /// `hn-list-merkle-v1` root over every `ReceiptV1.digest()`, in
    /// block order (ADR-0008, "Ordered List Commitment").
    pub receipts_root: Digest,
}

/// Applies every transaction in `transactions`, in order, against the
/// same pre-block `reader` — see ADR-0030's own "Explicitly Not
/// Resolved" for why this does not yet give a second same-sender
/// transaction in one block a correct, up-to-date view of the first
/// one's effects. Does not compute `BlockHeader.state_root`: doing so
/// needs the complete current leaf set, which nothing in this project
/// maintains yet (no durable backend, ADR-0019).
pub fn apply_block(
    transactions: &[TransactionEnvelope],
    reader: &impl StateReader,
    current_height: BlockHeight,
) -> StateResult<BlockApplicationResult> {
    let mut applied = Vec::with_capacity(transactions.len());
    for envelope in transactions {
        applied.push(apply_transaction(envelope, reader, current_height)?);
    }

    let tx_ids: Vec<Digest> = applied.iter().map(|entry| entry.tx_id).collect();
    let receipt_digests = applied
        .iter()
        .map(|entry| entry.receipt.digest())
        .collect::<StateResult<Vec<Digest>>>()?;

    let transactions_root = list_merkle_root(&tx_ids)?;
    let receipts_root = list_merkle_root(&receipt_digests)?;

    Ok(BlockApplicationResult {
        applied,
        transactions_root,
        receipts_root,
    })
}

#[cfg(test)]
mod tests {
    use hn_core::{AccountNonce, BlockHeight};
    use hn_crypto::{Digest, Ed25519KeyPair, KeyRole, SignatureEnvelope, account_address_body};

    use super::{apply_block, apply_identity_bootstrap, apply_transaction};
    use crate::access_list::AccessListV1;
    use crate::error::{StateError, StateResult};
    use crate::governance_payload::GovernancePayloadV1;
    use crate::list_merkle::list_merkle_root;
    use crate::receipt::{ReceiptStatus, ReceiptV1};
    use crate::state_store::StateReader;
    use crate::transaction_envelope::{TX_VERSION_1, TransactionEnvelope, TxType};
    use crate::transfer_payload::TransferPayloadV1;
    use crate::tx_id::tx_id;
    use crate::validity_window::ValidityWindowV1;

    const NETWORK_ID: u16 = 1;

    struct MapReader(std::collections::BTreeMap<[u8; 32], Vec<u8>>);

    impl StateReader for MapReader {
        fn get(&self, state_key: &[u8; 32]) -> StateResult<Option<Vec<u8>>> {
            Ok(self.0.get(state_key).cloned())
        }
    }

    fn empty_reader() -> MapReader {
        MapReader(std::collections::BTreeMap::new())
    }

    fn keypair(seed: u8) -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [seed; 32])
    }

    fn address_of(keypair: &Ed25519KeyPair) -> StateResult<Digest> {
        let descriptor = keypair.key_descriptor();
        Ok(account_address_body(
            NETWORK_ID,
            descriptor.algorithm_id(),
            &descriptor.public_key_bytes(),
        )?)
    }

    fn no_window() -> ValidityWindowV1 {
        ValidityWindowV1 {
            min_height: None,
            max_height: None,
        }
    }

    fn zero_transfer_payload() -> StateResult<Vec<u8>> {
        TransferPayloadV1 {
            recipient: [0x22; 32],
            asset_id: None,
            amount: 0,
        }
        .encode()
    }

    /// Builds a signed, bootstrap-keyed envelope (`bootstrap_key:
    /// Some(keypair.key_descriptor())`, `sender` derived from it) — the
    /// cheapest way to get a validly-authorizable sender against an
    /// empty [`MapReader`], mirroring
    /// `transaction_envelope::tests::verify::bare_envelope`.
    fn bootstrap_envelope(
        keypair: &Ed25519KeyPair,
        nonce: u64,
        tx_type: TxType,
        payload: Vec<u8>,
        validity_window: ValidityWindowV1,
    ) -> StateResult<TransactionEnvelope> {
        let sender = address_of(keypair)?;
        let mut envelope = TransactionEnvelope {
            tx_version: TX_VERSION_1,
            chain_id: 1,
            network_id: NETWORK_ID,
            tx_type,
            sender,
            bootstrap_key: Some(keypair.key_descriptor()),
            nonce: AccountNonce::new(nonce),
            fee_limit: 100,
            validity_window,
            access_list: AccessListV1 {
                reads: vec![],
                writes: vec![],
            },
            payload,
            signatures: vec![],
        };
        let digest = envelope.signing_payload().signing_digest()?;
        envelope.signatures = vec![SignatureEnvelope {
            algorithm_id: keypair.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: keypair.sign(&digest).to_vec(),
        }];
        Ok(envelope)
    }

    #[test]
    fn bootstrap_transfer_writes_identity_leaf_first_and_advances_nonce() -> StateResult<()> {
        let keypair = keypair(0x01);
        let envelope = bootstrap_envelope(
            &keypair,
            0,
            TxType::Transfer,
            zero_transfer_payload()?,
            no_window(),
        )?;
        let reader = empty_reader();

        let applied = apply_transaction(&envelope, &reader, BlockHeight::GENESIS)?;

        let expected_bootstrap_leaf =
            apply_identity_bootstrap(envelope.sender, &keypair.key_descriptor())?;
        // bootstrap leaf, sender balance leaf, recipient balance leaf, nonce leaf.
        assert_eq!(applied.write_set.len(), 4);
        assert_eq!(applied.write_set[0], expected_bootstrap_leaf);
        assert_eq!(applied.receipt.status, ReceiptStatus::Success);
        assert_eq!(applied.tx_id, tx_id(&envelope.encode()?)?);
        Ok(())
    }

    #[test]
    fn rejects_a_nonce_that_does_not_match_the_stored_value() -> StateResult<()> {
        let keypair = keypair(0x02);
        let envelope = bootstrap_envelope(
            &keypair,
            1,
            TxType::Transfer,
            zero_transfer_payload()?,
            no_window(),
        )?;
        let reader = empty_reader();

        assert_eq!(
            apply_transaction(&envelope, &reader, BlockHeight::GENESIS),
            Err(StateError::NonceMismatch {
                expected: 0,
                found: 1,
            })
        );
        Ok(())
    }

    #[test]
    fn rejects_a_height_below_the_validity_window() -> StateResult<()> {
        let keypair = keypair(0x03);
        let window = ValidityWindowV1 {
            min_height: Some(BlockHeight::new(100)),
            max_height: None,
        };
        let envelope = bootstrap_envelope(
            &keypair,
            0,
            TxType::Transfer,
            zero_transfer_payload()?,
            window,
        )?;
        let reader = empty_reader();

        assert_eq!(
            apply_transaction(&envelope, &reader, BlockHeight::new(1)),
            Err(StateError::TransactionOutsideValidityWindow)
        );
        Ok(())
    }

    #[test]
    fn rejects_governance_as_undecided() -> StateResult<()> {
        let keypair = keypair(0x04);
        let payload = GovernancePayloadV1::Propose {
            title: "test".to_string(),
            content_hash: [0x55; 32],
        }
        .encode()?;
        let envelope = bootstrap_envelope(&keypair, 0, TxType::Governance, payload, no_window())?;
        let reader = empty_reader();

        assert_eq!(
            apply_transaction(&envelope, &reader, BlockHeight::GENESIS),
            Err(StateError::UndecidedTransactionPayload {
                tx_type: TxType::Governance.as_u8()
            })
        );
        Ok(())
    }

    #[test]
    fn apply_block_aggregates_every_transaction_in_order() -> StateResult<()> {
        let first_keypair = keypair(0x05);
        let second_keypair = keypair(0x06);
        let first = bootstrap_envelope(
            &first_keypair,
            0,
            TxType::Transfer,
            zero_transfer_payload()?,
            no_window(),
        )?;
        let second = bootstrap_envelope(
            &second_keypair,
            0,
            TxType::Transfer,
            zero_transfer_payload()?,
            no_window(),
        )?;
        let reader = empty_reader();

        let result = apply_block(
            &[first.clone(), second.clone()],
            &reader,
            BlockHeight::GENESIS,
        )?;

        let expected_tx_ids = vec![tx_id(&first.encode()?)?, tx_id(&second.encode()?)?];
        let expected_receipts = vec![
            ReceiptV1 {
                tx_id: expected_tx_ids[0],
                status: ReceiptStatus::Success,
            }
            .digest()?,
            ReceiptV1 {
                tx_id: expected_tx_ids[1],
                status: ReceiptStatus::Success,
            }
            .digest()?,
        ];

        assert_eq!(result.applied.len(), 2);
        assert_eq!(
            result.transactions_root,
            list_merkle_root(&expected_tx_ids)?
        );
        assert_eq!(result.receipts_root, list_merkle_root(&expected_receipts)?);
        Ok(())
    }
}

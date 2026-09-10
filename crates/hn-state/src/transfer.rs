use hn_crypto::Digest;

use crate::{
    account::{AccountSection, account_section_state_key},
    asset_value::AssetValueV1,
    balance_value::BalanceValueV1,
    error::{StateError, StateResult},
    node::{leaf_hash, value_hash},
    receipt::{ReceiptStatus, ReceiptV1},
    transfer_payload::TransferPayloadV1,
    tree::Leaf,
};

/// One account's current balance-domain state, as needed to apply a
/// `transfer` against it (ADR-0006, "Payload", `transfer`). Every other
/// section of the account is unaffected by a transfer and is not part
/// of this type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferParty {
    /// The account's `address_body`.
    pub address: Digest,
    /// The account's current native HNCOIN balance.
    pub balance: BalanceValueV1,
    /// The account's current non-native asset holdings.
    pub assets: AssetValueV1,
}

/// Computes the two updated write-set leaves a `transfer` produces
/// (ADR-0006, "Payload", `transfer`): the sender's and recipient's
/// applicable section — Balance if `payload.asset_id` is absent, Asset
/// if present. Every other leaf in either account's state is unchanged;
/// callers combine these two leaves with the rest of an existing write
/// set (or with a new account's other initial leaves, for implicit
/// creation — ADR-0006, "Decided: implicit account creation").
///
/// Checks the validation precondition that the sender's applicable
/// balance is at least `payload.amount`
/// ([`StateError::InsufficientBalance`] on failure), and that crediting
/// the recipient does not overflow `u128`
/// ([`StateError::BalanceOverflow`] — practically unreachable given
/// realistic amounts, but not silently wrapped).
pub fn apply_transfer(
    sender: &TransferParty,
    recipient: &TransferParty,
    payload: &TransferPayloadV1,
) -> StateResult<[Leaf; 2]> {
    match payload.asset_id {
        None => apply_balance_transfer(sender, recipient, payload.amount),
        Some(asset_id) => apply_asset_transfer(sender, recipient, asset_id, payload.amount),
    }
}

/// Applies a `transfer` and produces its [`ReceiptV1`] (ADR-0006,
/// "Receipts") in one step, so callers building a real write set never
/// have to reimplement the mapping from [`apply_transfer`]'s result to
/// a receipt themselves. `tx_id` is the caller-computed transaction ID
/// (ADR-0006, "Transaction ID") this receipt belongs to.
///
/// The expected validation-precondition failures
/// ([`StateError::InsufficientBalance`], [`StateError::BalanceOverflow`])
/// are legitimate transaction outcomes, not bugs: they become a
/// `ReceiptStatus::Failed` receipt with no leaves to write, matching the
/// already-decided Nonce/Fees rule that a failed execution still
/// applies (only the payload's own state effects revert — nonce and
/// fee leaves, outside this function's scope, still update elsewhere).
/// Any other error — for example [`StateError::Encoding`] failing on a
/// value this function just constructed — is a real internal error and
/// is propagated, never folded into a receipt.
pub fn apply_transfer_with_receipt(
    sender: &TransferParty,
    recipient: &TransferParty,
    payload: &TransferPayloadV1,
    tx_id: Digest,
) -> StateResult<(Option<[Leaf; 2]>, ReceiptV1)> {
    match apply_transfer(sender, recipient, payload) {
        Ok(leaves) => Ok((
            Some(leaves),
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Success,
            },
        )),
        Err(StateError::InsufficientBalance | StateError::BalanceOverflow) => Ok((
            None,
            ReceiptV1 {
                tx_id,
                status: ReceiptStatus::Failed,
            },
        )),
        Err(other) => Err(other),
    }
}

fn apply_balance_transfer(
    sender: &TransferParty,
    recipient: &TransferParty,
    amount: u128,
) -> StateResult<[Leaf; 2]> {
    let new_sender_balance = sender
        .balance
        .native_balance
        .checked_sub(amount)
        .ok_or(StateError::InsufficientBalance)?;
    let new_recipient_balance = recipient
        .balance
        .native_balance
        .checked_add(amount)
        .ok_or(StateError::BalanceOverflow)?;

    Ok([
        balance_leaf(&sender.address, new_sender_balance)?,
        balance_leaf(&recipient.address, new_recipient_balance)?,
    ])
}

fn balance_leaf(address: &Digest, native_balance: u128) -> StateResult<Leaf> {
    let key = account_section_state_key(address, AccountSection::Balance)?;
    let value_bytes = BalanceValueV1 { native_balance }.encode();
    let vh = value_hash(&value_bytes)?;
    Ok((key, leaf_hash(&key, &vh)?))
}

fn apply_asset_transfer(
    sender: &TransferParty,
    recipient: &TransferParty,
    asset_id: u16,
    amount: u128,
) -> StateResult<[Leaf; 2]> {
    let sender_current = holding_amount(&sender.assets, asset_id);
    let new_sender_amount = sender_current
        .checked_sub(amount)
        .ok_or(StateError::InsufficientBalance)?;

    let recipient_current = holding_amount(&recipient.assets, asset_id);
    let new_recipient_amount = recipient_current
        .checked_add(amount)
        .ok_or(StateError::BalanceOverflow)?;

    let sender_holdings = set_holding(&sender.assets, asset_id, new_sender_amount);
    let recipient_holdings = set_holding(&recipient.assets, asset_id, new_recipient_amount);

    Ok([
        asset_leaf(&sender.address, sender_holdings)?,
        asset_leaf(&recipient.address, recipient_holdings)?,
    ])
}

fn holding_amount(assets: &AssetValueV1, asset_id: u16) -> u128 {
    assets
        .holdings
        .iter()
        .find(|(id, _)| *id == asset_id)
        .map_or(0, |(_, amount)| *amount)
}

/// Replaces `asset_id`'s holding with `new_amount`, removing the entry
/// entirely when `new_amount` is zero — absence from `holdings` means a
/// zero balance (account-state.md §4.7), not an explicit zero-amount
/// entry.
fn set_holding(assets: &AssetValueV1, asset_id: u16, new_amount: u128) -> Vec<(u16, u128)> {
    let mut holdings: Vec<(u16, u128)> = assets
        .holdings
        .iter()
        .filter(|(id, _)| *id != asset_id)
        .copied()
        .collect();
    if new_amount != 0 {
        holdings.push((asset_id, new_amount));
    }
    holdings
}

fn asset_leaf(address: &Digest, holdings: Vec<(u16, u128)>) -> StateResult<Leaf> {
    let key = account_section_state_key(address, AccountSection::Asset)?;
    let value_bytes = AssetValueV1 { holdings }.encode()?;
    let vh = value_hash(&value_bytes)?;
    Ok((key, leaf_hash(&key, &vh)?))
}

#[cfg(test)]
mod tests {
    use super::{TransferParty, apply_transfer, apply_transfer_with_receipt};
    use crate::{
        AssetValueV1, BalanceValueV1, ReceiptStatus, StateError, TransferPayloadV1,
        error::StateResult,
    };

    const SENDER_ADDRESS: [u8; 32] = [0x11; 32];
    const RECIPIENT_ADDRESS: [u8; 32] = [0x22; 32];

    fn party(address: [u8; 32], native_balance: u128, holdings: Vec<(u16, u128)>) -> TransferParty {
        TransferParty {
            address,
            balance: BalanceValueV1 { native_balance },
            assets: AssetValueV1 { holdings },
        }
    }

    #[test]
    fn debits_sender_and_credits_recipient_native_balance() -> StateResult<()> {
        let sender = party(SENDER_ADDRESS, 1_000, vec![]);
        let recipient = party(RECIPIENT_ADDRESS, 100, vec![]);
        let payload = TransferPayloadV1 {
            recipient: RECIPIENT_ADDRESS,
            asset_id: None,
            amount: 300,
        };

        let [sender_leaf, recipient_leaf] = apply_transfer(&sender, &recipient, &payload)?;

        let expected_sender =
            crate::account_section_state_key(&SENDER_ADDRESS, crate::AccountSection::Balance)?;
        let expected_recipient =
            crate::account_section_state_key(&RECIPIENT_ADDRESS, crate::AccountSection::Balance)?;
        assert_eq!(sender_leaf.0, expected_sender);
        assert_eq!(recipient_leaf.0, expected_recipient);
        assert_ne!(sender_leaf.1, recipient_leaf.1);
        Ok(())
    }

    #[test]
    fn rejects_insufficient_native_balance() {
        let sender = party(SENDER_ADDRESS, 10, vec![]);
        let recipient = party(RECIPIENT_ADDRESS, 0, vec![]);
        let payload = TransferPayloadV1 {
            recipient: RECIPIENT_ADDRESS,
            asset_id: None,
            amount: 11,
        };

        assert_eq!(
            apply_transfer(&sender, &recipient, &payload),
            Err(StateError::InsufficientBalance)
        );
    }

    const TX_ID: [u8; 32] = [0x99; 32];

    #[test]
    fn successful_transfer_yields_leaves_and_success_receipt() -> StateResult<()> {
        let sender = party(SENDER_ADDRESS, 1_000, vec![]);
        let recipient = party(RECIPIENT_ADDRESS, 100, vec![]);
        let payload = TransferPayloadV1 {
            recipient: RECIPIENT_ADDRESS,
            asset_id: None,
            amount: 300,
        };

        let (leaves, receipt) = apply_transfer_with_receipt(&sender, &recipient, &payload, TX_ID)?;

        assert!(leaves.is_some());
        assert_eq!(receipt.tx_id, TX_ID);
        assert_eq!(receipt.status, ReceiptStatus::Success);
        Ok(())
    }

    #[test]
    fn failed_transfer_yields_no_leaves_and_failed_receipt() -> StateResult<()> {
        let sender = party(SENDER_ADDRESS, 10, vec![]);
        let recipient = party(RECIPIENT_ADDRESS, 0, vec![]);
        let payload = TransferPayloadV1 {
            recipient: RECIPIENT_ADDRESS,
            asset_id: None,
            amount: 11,
        };

        let (leaves, receipt) = apply_transfer_with_receipt(&sender, &recipient, &payload, TX_ID)?;

        assert!(leaves.is_none());
        assert_eq!(receipt.tx_id, TX_ID);
        assert_eq!(receipt.status, ReceiptStatus::Failed);
        Ok(())
    }

    #[test]
    fn asset_transfer_creates_new_holding_and_zeroes_out_old_one() -> StateResult<()> {
        let sender = party(SENDER_ADDRESS, 0, vec![(7, 250)]);
        let recipient = party(RECIPIENT_ADDRESS, 0, vec![]);
        let payload = TransferPayloadV1 {
            recipient: RECIPIENT_ADDRESS,
            asset_id: Some(7),
            amount: 250,
        };

        let [sender_leaf, recipient_leaf] = apply_transfer(&sender, &recipient, &payload)?;

        // Sender's holding drops to zero, so its Asset leaf must equal an
        // account with an empty holdings map (account-state.md §4.7:
        // absence means zero, not an explicit zero entry).
        let empty_asset_value = AssetValueV1 { holdings: vec![] }.encode()?;
        let empty_vh = crate::value_hash(&empty_asset_value)?;
        let sender_key =
            crate::account_section_state_key(&SENDER_ADDRESS, crate::AccountSection::Asset)?;
        let expected_sender_leaf = crate::leaf_hash(&sender_key, &empty_vh)?;
        assert_eq!(sender_leaf.1, expected_sender_leaf);

        let recipient_asset_value = AssetValueV1 {
            holdings: vec![(7, 250)],
        }
        .encode()?;
        let recipient_vh = crate::value_hash(&recipient_asset_value)?;
        let recipient_key =
            crate::account_section_state_key(&RECIPIENT_ADDRESS, crate::AccountSection::Asset)?;
        let expected_recipient_leaf = crate::leaf_hash(&recipient_key, &recipient_vh)?;
        assert_eq!(recipient_leaf.1, expected_recipient_leaf);

        Ok(())
    }

    #[test]
    fn rejects_insufficient_asset_holding() {
        let sender = party(SENDER_ADDRESS, 0, vec![(7, 10)]);
        let recipient = party(RECIPIENT_ADDRESS, 0, vec![]);
        let payload = TransferPayloadV1 {
            recipient: RECIPIENT_ADDRESS,
            asset_id: Some(7),
            amount: 11,
        };

        assert_eq!(
            apply_transfer(&sender, &recipient, &payload),
            Err(StateError::InsufficientBalance)
        );
    }
}

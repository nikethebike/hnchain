use hn_crypto::Digest;

use crate::{
    account::{AccountSection, account_section_state_key},
    asset_value::AssetValueV1,
    balance_value::BalanceValueV1,
    error::{StateError, StateResult},
    receipt::{ReceiptStatus, ReceiptV1},
    state_store::{StateReader, Write},
    transfer_payload::TransferPayloadV1,
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

/// Fetches and decodes `account`'s current [`BalanceValueV1`] from
/// `reader` — absence maps to `native_balance = 0`, the same "absence
/// means the section's own already-decided default" convention
/// [`fetch_asset`]/[`crate::fetch_identity`] already use, not a
/// separate design choice.
pub fn fetch_balance(reader: &impl StateReader, account: &Digest) -> StateResult<BalanceValueV1> {
    let key = account_section_state_key(account, AccountSection::Balance)?;
    match reader.get(&key)? {
        Some(bytes) => BalanceValueV1::decode(&bytes),
        None => Ok(BalanceValueV1 { native_balance: 0 }),
    }
}

/// Fetches and decodes `account`'s current [`AssetValueV1`] from
/// `reader` — absence maps to empty `holdings`, matching
/// [`AssetValueV1`]'s own already-documented "absence means zero"
/// invariant.
pub fn fetch_asset(reader: &impl StateReader, account: &Digest) -> StateResult<AssetValueV1> {
    let key = account_section_state_key(account, AccountSection::Asset)?;
    match reader.get(&key)? {
        Some(bytes) => AssetValueV1::decode(&bytes),
        None => Ok(AssetValueV1 {
            holdings: Vec::new(),
        }),
    }
}

/// Fetches `account`'s current [`TransferParty`] from `reader` — the
/// combination [`fetch_balance`]/[`fetch_asset`] `apply_transfer`
/// already expects, for either side of a transfer. Absence of either
/// section maps to its own zero default, so a never-before-seen
/// recipient address is a valid, ordinary input here (implicit
/// creation, ADR-0006, "Decided: implicit account creation") — this
/// function does not itself write anything to establish that account;
/// it only reads whatever is or is not already there.
pub fn fetch_transfer_party(
    reader: &impl StateReader,
    account: Digest,
) -> StateResult<TransferParty> {
    Ok(TransferParty {
        address: account,
        balance: fetch_balance(reader, &account)?,
        assets: fetch_asset(reader, &account)?,
    })
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
) -> StateResult<[Write; 2]> {
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
) -> StateResult<(Option<[Write; 2]>, ReceiptV1)> {
    match apply_transfer(sender, recipient, payload) {
        Ok(writes) => Ok((
            Some(writes),
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
) -> StateResult<[Write; 2]> {
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
        balance_write(&sender.address, new_sender_balance)?,
        balance_write(&recipient.address, new_recipient_balance)?,
    ])
}

pub(crate) fn balance_write(address: &Digest, native_balance: u128) -> StateResult<Write> {
    let key = account_section_state_key(address, AccountSection::Balance)?;
    let value = BalanceValueV1 { native_balance }.encode();
    Ok(Write {
        state_key: key,
        value,
    })
}

fn apply_asset_transfer(
    sender: &TransferParty,
    recipient: &TransferParty,
    asset_id: u16,
    amount: u128,
) -> StateResult<[Write; 2]> {
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
        asset_write(&sender.address, sender_holdings)?,
        asset_write(&recipient.address, recipient_holdings)?,
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

fn asset_write(address: &Digest, holdings: Vec<(u16, u128)>) -> StateResult<Write> {
    let key = account_section_state_key(address, AccountSection::Asset)?;
    let value = AssetValueV1 { holdings }.encode()?;
    Ok(Write {
        state_key: key,
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        TransferParty, apply_transfer, apply_transfer_with_receipt, fetch_asset, fetch_balance,
        fetch_transfer_party,
    };
    use crate::{
        AssetValueV1, BalanceValueV1, ReceiptStatus, StateError, StateReader, TransferPayloadV1,
        error::StateResult,
    };

    const SENDER_ADDRESS: [u8; 32] = [0x11; 32];
    const RECIPIENT_ADDRESS: [u8; 32] = [0x22; 32];

    struct MapReader(std::collections::BTreeMap<[u8; 32], Vec<u8>>);

    impl StateReader for MapReader {
        fn get(&self, state_key: &[u8; 32]) -> StateResult<Option<Vec<u8>>> {
            Ok(self.0.get(state_key).cloned())
        }
    }

    #[test]
    fn fetch_balance_defaults_to_zero_when_absent() -> StateResult<()> {
        let reader = MapReader(std::collections::BTreeMap::new());
        assert_eq!(
            fetch_balance(&reader, &SENDER_ADDRESS)?,
            BalanceValueV1 { native_balance: 0 }
        );
        Ok(())
    }

    #[test]
    fn fetch_balance_reads_a_stored_value() -> StateResult<()> {
        let key =
            crate::account_section_state_key(&SENDER_ADDRESS, crate::AccountSection::Balance)?;
        let stored = BalanceValueV1 {
            native_balance: 500,
        };
        let reader = MapReader(std::collections::BTreeMap::from([(key, stored.encode())]));
        assert_eq!(fetch_balance(&reader, &SENDER_ADDRESS)?, stored);
        Ok(())
    }

    #[test]
    fn fetch_asset_defaults_to_empty_holdings_when_absent() -> StateResult<()> {
        let reader = MapReader(std::collections::BTreeMap::new());
        assert_eq!(
            fetch_asset(&reader, &SENDER_ADDRESS)?,
            AssetValueV1 { holdings: vec![] }
        );
        Ok(())
    }

    #[test]
    fn fetch_asset_reads_a_stored_value() -> StateResult<()> {
        let key = crate::account_section_state_key(&SENDER_ADDRESS, crate::AccountSection::Asset)?;
        let stored = AssetValueV1 {
            holdings: vec![(3, 42)],
        };
        let reader = MapReader(std::collections::BTreeMap::from([(key, stored.encode()?)]));
        assert_eq!(fetch_asset(&reader, &SENDER_ADDRESS)?, stored);
        Ok(())
    }

    #[test]
    fn fetch_transfer_party_combines_balance_and_asset() -> StateResult<()> {
        let balance_key =
            crate::account_section_state_key(&SENDER_ADDRESS, crate::AccountSection::Balance)?;
        let asset_key =
            crate::account_section_state_key(&SENDER_ADDRESS, crate::AccountSection::Asset)?;
        let balance = BalanceValueV1 { native_balance: 10 };
        let assets = AssetValueV1 {
            holdings: vec![(1, 2)],
        };
        let reader = MapReader(std::collections::BTreeMap::from([
            (balance_key, balance.encode()),
            (asset_key, assets.encode()?),
        ]));

        let party = fetch_transfer_party(&reader, SENDER_ADDRESS)?;
        assert_eq!(
            party,
            TransferParty {
                address: SENDER_ADDRESS,
                balance,
                assets,
            }
        );
        Ok(())
    }

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

        let [sender_write, recipient_write] = apply_transfer(&sender, &recipient, &payload)?;

        let expected_sender =
            crate::account_section_state_key(&SENDER_ADDRESS, crate::AccountSection::Balance)?;
        let expected_recipient =
            crate::account_section_state_key(&RECIPIENT_ADDRESS, crate::AccountSection::Balance)?;
        assert_eq!(sender_write.state_key, expected_sender);
        assert_eq!(recipient_write.state_key, expected_recipient);
        assert_ne!(sender_write.value, recipient_write.value);
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

        let (writes, receipt) = apply_transfer_with_receipt(&sender, &recipient, &payload, TX_ID)?;

        assert!(writes.is_some());
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

        let (writes, receipt) = apply_transfer_with_receipt(&sender, &recipient, &payload, TX_ID)?;

        assert!(writes.is_none());
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

        let [sender_write, recipient_write] = apply_transfer(&sender, &recipient, &payload)?;

        // Sender's holding drops to zero, so its Asset write must equal
        // an account with an empty holdings map (account-state.md §4.7:
        // absence means zero, not an explicit zero entry).
        let empty_asset_value = AssetValueV1 { holdings: vec![] }.encode()?;
        let sender_key =
            crate::account_section_state_key(&SENDER_ADDRESS, crate::AccountSection::Asset)?;
        assert_eq!(sender_write.state_key, sender_key);
        assert_eq!(sender_write.value, empty_asset_value);

        let recipient_asset_value = AssetValueV1 {
            holdings: vec![(7, 250)],
        }
        .encode()?;
        let recipient_key =
            crate::account_section_state_key(&RECIPIENT_ADDRESS, crate::AccountSection::Asset)?;
        assert_eq!(recipient_write.state_key, recipient_key);
        assert_eq!(recipient_write.value, recipient_asset_value);

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

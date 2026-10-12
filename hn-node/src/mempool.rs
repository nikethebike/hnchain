use std::collections::HashMap;
use std::fmt;

use hn_core::BlockHeight;
use hn_crypto::Digest;
use hn_state::{StateError, StateReader, TransactionEnvelope, ValidatorRecordV1, apply_block};

/// Capacity of a [`Mempool`] (ADR-0045, "Decided: Mempool Admission") —
/// a local, non-consensus constant: a devnet value chosen here, not a
/// protocol parameter any other node needs to agree with. No fee
/// market exists yet to prioritize eviction by (ADR-0045's own
/// "Rejected Options: Fee Market / Fee Priority Now"), so eviction
/// policy is simply "reject new transactions once full" — see
/// [`Mempool::admit`].
pub const MEMPOOL_MAX_TXS: usize = 10_000;

/// Why [`Mempool::admit`] rejected a transaction (ADR-0045, "Decided:
/// Mempool Admission") — every variant names exactly one of that
/// decision's six ordered steps, in the order they are checked, plus
/// the two housekeeping rejections ([`Self::AlreadyPresent`],
/// [`Self::Full`]) that are not part of the six-step pipeline itself.
/// Never fatal to whatever process holds a [`Mempool`] — ADR-0045's own
/// "Decided: Transaction Gossip": "Admission failure is a dropped,
/// logged message, never a fatal error."
#[derive(Debug)]
pub enum MempoolError {
    /// Step 1: the raw bytes exceeded `hn_state::MAX_TRANSACTION_SIZE` or did
    /// not decode as a canonical `TransactionEnvelope` at all.
    Decode(StateError),
    /// Step 2: `chain_id`/`network_id` does not match this node's own.
    WrongChainOrNetwork,
    /// Step 3: `TransactionEnvelope::verify` rejected it (bad
    /// signature, bad or missing bootstrap key, ...).
    Unverified(StateError),
    /// Step 4: `validity_window` does not cover `last_finalized_height + 1`
    /// — the height this transaction would actually be considered for
    /// next, not the last one already finalized.
    OutsideValidityWindow,
    /// Step 5: a dry-run `apply_block` against committed state failed
    /// — this transaction would abort block application and must never
    /// be selected into one (ADR-0045, "Decided: Block Building
    /// (Proposer)").
    ExecutionFailed(StateError),
    /// Step 6: this exact `tx_id` is already admitted.
    AlreadyPresent,
    /// At [`MEMPOOL_MAX_TXS`] capacity; rejected rather than evicting
    /// anything else, since no fee market exists yet to decide what a
    /// new transaction would need to outbid.
    Full,
}

impl fmt::Display for MempoolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(error) => write!(formatter, "could not decode transaction: {error}"),
            Self::WrongChainOrNetwork => {
                write!(formatter, "chain_id/network_id does not match this node")
            }
            Self::Unverified(error) => write!(formatter, "signature verification failed: {error}"),
            Self::OutsideValidityWindow => {
                write!(formatter, "validity window does not cover the next height")
            }
            Self::ExecutionFailed(error) => {
                write!(
                    formatter,
                    "transaction would abort block application: {error}"
                )
            }
            Self::AlreadyPresent => write!(formatter, "transaction already admitted"),
            Self::Full => write!(formatter, "mempool is at capacity"),
        }
    }
}

impl std::error::Error for MempoolError {}

/// A node's own local transaction pool (ADR-0045, "Decided: Mempool Is
/// Local Policy, Not Consensus" — contents, ordering, and eviction are
/// this node's own choice; block validity never depends on any node's
/// mempool matching another's).
///
/// Tracks arrival order (`order`) separately from the lookup table
/// (`entries`): block building (a later pass) selects "per sender
/// ascending by nonce, across senders in mempool arrival order"
/// (ADR-0045, "Decided: Block Building (Proposer)") — a `HashMap` alone
/// has no ordering to offer that, and a `Vec` alone has no `O(1)`
/// duplicate check. Keeping both, kept in sync by every mutating
/// method here, is simpler than a single ordered-map type this
/// workspace does not otherwise depend on.
#[derive(Debug, Default)]
pub struct Mempool {
    order: Vec<Digest>,
    entries: HashMap<Digest, TransactionEnvelope>,
}

impl Mempool {
    /// An empty mempool, bounded at [`MEMPOOL_MAX_TXS`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of currently-admitted transactions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether this mempool currently holds no transactions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Whether `tx_id` is currently admitted.
    #[must_use]
    pub fn contains(&self, tx_id: &Digest) -> bool {
        self.entries.contains_key(tx_id)
    }

    /// The admitted transaction stored under `tx_id`, if any —
    /// ADR-0045's own "Decided: Transaction Gossip" needs this to
    /// answer a peer's `TransactionRequest` with the real envelope it
    /// already admitted.
    #[must_use]
    pub fn get(&self, tx_id: &Digest) -> Option<&TransactionEnvelope> {
        self.entries.get(tx_id)
    }

    /// Runs `raw_bytes` through ADR-0045's own six-step admission
    /// pipeline, in order, and — if every step passes — stores the
    /// decoded transaction and returns its `tx_id`.
    ///
    /// `last_finalized_height` is this node's own last *finalized*
    /// height, not the height a transaction is actually being checked
    /// against: steps 4 and 5 both evaluate it for
    /// `last_finalized_height + 1`, the next height a transaction could
    /// actually be included at (ADR-0045's own "contains
    /// `current_height + 1`" wording). `validator_candidates` is
    /// passed straight through to the step-5 dry run, the same
    /// "caller already resolved it" boundary `apply_block` itself
    /// already draws.
    pub fn admit(
        &mut self,
        raw_bytes: &[u8],
        reader: &impl StateReader,
        last_finalized_height: BlockHeight,
        chain_id: u8,
        network_id: u16,
        validator_candidates: &[ValidatorRecordV1],
    ) -> Result<Digest, MempoolError> {
        if self.order.len() >= MEMPOOL_MAX_TXS {
            return Err(MempoolError::Full);
        }

        // Step 1: size bound + canonical decode.
        // `TransactionEnvelope::decode` itself rejects anything over
        // `MAX_TRANSACTION_SIZE` before attempting to decode it -- the
        // "cheap check before expensive decode" ordering ADR-0006 already
        // fixed, not a separate check this function repeats.
        let envelope = TransactionEnvelope::decode(raw_bytes).map_err(MempoolError::Decode)?;

        // Step 2: chain/network binding.
        if envelope.chain_id != chain_id || envelope.network_id != network_id {
            return Err(MempoolError::WrongChainOrNetwork);
        }

        // Step 3: signature (and bootstrap, if present) verification.
        envelope.verify(reader).map_err(MempoolError::Unverified)?;

        // Step 4: validity window, evaluated against the next height.
        let next_height = BlockHeight::new(last_finalized_height.get().saturating_add(1));
        if let Some(min_height) = envelope.validity_window.min_height
            && next_height.get() < min_height.get()
        {
            return Err(MempoolError::OutsideValidityWindow);
        }
        if let Some(max_height) = envelope.validity_window.max_height
            && next_height.get() > max_height.get()
        {
            return Err(MempoolError::OutsideValidityWindow);
        }

        // Step 5: a dry run that never persists anything -- exactly the
        // "simulate without persisting" use `apply_block`'s own doc
        // comment already reserves for mempool validation.
        apply_block(
            std::slice::from_ref(&envelope),
            reader,
            next_height,
            validator_candidates,
        )
        .map_err(MempoolError::ExecutionFailed)?;

        // Step 6: dedup by tx_id.
        let tx_id = hn_state::tx_id(&envelope.encode().map_err(MempoolError::Decode)?)
            .map_err(MempoolError::Decode)?;
        if self.entries.contains_key(&tx_id) {
            return Err(MempoolError::AlreadyPresent);
        }

        self.order.push(tx_id);
        self.entries.insert(tx_id, envelope);
        Ok(tx_id)
    }

    /// Removes every one of `tx_ids` that is currently admitted — ADR-0045's
    /// own "Decided: Commit": "After commit, included transactions are
    /// removed from the mempool."
    pub fn remove_included(&mut self, tx_ids: impl IntoIterator<Item = Digest>) {
        for tx_id in tx_ids {
            if self.entries.remove(&tx_id).is_some() {
                self.order.retain(|entry| entry != &tx_id);
            }
        }
    }

    /// Removes every admitted transaction whose own `validity_window`
    /// no longer covers `current_height` — ADR-0045's own "Decided:
    /// Mempool Admission": entries are "removed... when their validity
    /// window expires." Does not re-run admission (chain/network,
    /// signature, dry-run execution) — those were already checked once
    /// at admission time and nothing about them changes over time; only
    /// the height-bound expiry condition does.
    pub fn evict_expired(&mut self, current_height: BlockHeight) {
        let expired: Vec<Digest> = self
            .entries
            .iter()
            .filter_map(|(tx_id, envelope)| {
                let expired = envelope
                    .validity_window
                    .max_height
                    .is_some_and(|max_height| current_height.get() > max_height.get());
                expired.then_some(*tx_id)
            })
            .collect();
        self.remove_included(expired);
    }
}

#[cfg(test)]
mod tests {
    use hn_core::AccountNonce;
    use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope, account_address_body};
    use hn_state::{
        AccessListV1, BalanceValueV1, TX_VERSION_1, TransferPayloadV1, TxType, ValidityWindowV1,
        account_section_state_key,
    };
    use hn_storage::InMemoryStateStore;

    use super::*;

    const CHAIN_ID: u8 = 1;
    const NETWORK_ID: u16 = 1;
    const RECIPIENT: [u8; 32] = [0x22; 32];

    fn funded_store(
        sender: [u8; 32],
        native_balance: u128,
    ) -> hn_state::StateResult<InMemoryStateStore> {
        use hn_state::{AccountSection, StateWriter};
        let mut store = InMemoryStateStore::new();
        let key = account_section_state_key(&sender, AccountSection::Balance)?;
        store.set(key, BalanceValueV1 { native_balance }.encode())?;
        Ok(store)
    }

    fn signed_envelope(
        keypair: &Ed25519KeyPair,
        sender: [u8; 32],
        nonce: u64,
        validity_window: ValidityWindowV1,
    ) -> hn_state::StateResult<TransactionEnvelope> {
        let payload = TransferPayloadV1 {
            recipient: RECIPIENT,
            asset_id: None,
            amount: 10,
        }
        .encode()?;
        let mut envelope = TransactionEnvelope {
            tx_version: TX_VERSION_1,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            tx_type: TxType::Transfer,
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

    fn open_window() -> ValidityWindowV1 {
        ValidityWindowV1 {
            min_height: None,
            max_height: None,
        }
    }

    fn sender_fixture(seed: u8) -> hn_state::StateResult<([u8; 32], Ed25519KeyPair)> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [seed; 32]);
        let sender = account_address_body(
            NETWORK_ID,
            keypair.key_descriptor().algorithm_id(),
            &keypair.key_descriptor().public_key_bytes(),
        )?;
        Ok((sender, keypair))
    }

    #[test]
    fn admits_a_valid_transaction() -> hn_state::StateResult<()> {
        let (sender, keypair) = sender_fixture(0x01)?;
        let store = funded_store(sender, 1_000)?;
        let envelope = signed_envelope(&keypair, sender, 0, open_window())?;
        let expected_tx_id = hn_state::tx_id(&envelope.encode()?)?;

        let mut mempool = Mempool::new();
        let tx_id = mempool
            .admit(
                &envelope.encode()?,
                &store,
                BlockHeight::GENESIS,
                CHAIN_ID,
                NETWORK_ID,
                &[],
            )
            .map_err(|error| StateError::Storage(error.to_string()))?;

        assert_eq!(tx_id, expected_tx_id);
        assert_eq!(mempool.len(), 1);
        assert!(mempool.contains(&tx_id));
        Ok(())
    }

    #[test]
    fn rejects_oversized_bytes_before_decoding() {
        let mut mempool = Mempool::new();
        let oversized = vec![0_u8; hn_state::MAX_TRANSACTION_SIZE + 1];
        let store = InMemoryStateStore::new();
        let result = mempool.admit(
            &oversized,
            &store,
            BlockHeight::GENESIS,
            CHAIN_ID,
            NETWORK_ID,
            &[],
        );
        assert!(matches!(result, Err(MempoolError::Decode(_))));
    }

    #[test]
    fn rejects_a_wrong_chain_id() -> hn_state::StateResult<()> {
        let (sender, keypair) = sender_fixture(0x02)?;
        let store = funded_store(sender, 1_000)?;
        let envelope = signed_envelope(&keypair, sender, 0, open_window())?;

        let mut mempool = Mempool::new();
        let result = mempool.admit(
            &envelope.encode()?,
            &store,
            BlockHeight::GENESIS,
            CHAIN_ID + 1,
            NETWORK_ID,
            &[],
        );
        assert!(matches!(result, Err(MempoolError::WrongChainOrNetwork)));
        Ok(())
    }

    #[test]
    fn rejects_an_invalid_signature() -> hn_state::StateResult<()> {
        let (sender, keypair) = sender_fixture(0x03)?;
        let store = funded_store(sender, 1_000)?;
        let mut envelope = signed_envelope(&keypair, sender, 0, open_window())?;
        envelope.signatures[0].signature[0] ^= 0xff;

        let mut mempool = Mempool::new();
        let result = mempool.admit(
            &envelope.encode()?,
            &store,
            BlockHeight::GENESIS,
            CHAIN_ID,
            NETWORK_ID,
            &[],
        );
        assert!(matches!(result, Err(MempoolError::Unverified(_))));
        Ok(())
    }

    #[test]
    fn rejects_a_transaction_outside_its_validity_window() -> hn_state::StateResult<()> {
        let (sender, keypair) = sender_fixture(0x04)?;
        let store = funded_store(sender, 1_000)?;
        let window = ValidityWindowV1 {
            min_height: None,
            max_height: Some(BlockHeight::GENESIS),
        };
        let envelope = signed_envelope(&keypair, sender, 0, window)?;

        let mut mempool = Mempool::new();
        // last_finalized_height = GENESIS means the next height is
        // GENESIS + 1, already past this transaction's own max_height.
        let result = mempool.admit(
            &envelope.encode()?,
            &store,
            BlockHeight::GENESIS,
            CHAIN_ID,
            NETWORK_ID,
            &[],
        );
        assert!(matches!(result, Err(MempoolError::OutsideValidityWindow)));
        Ok(())
    }

    #[test]
    fn rejects_a_transaction_that_would_abort_application() -> hn_state::StateResult<()> {
        let (sender, keypair) = sender_fixture(0x05)?;
        // Funded with less than the transfer's own amount (10) -- the
        // dry run must fail with InsufficientBalance... except that is
        // a *legitimate* Failed-receipt outcome, not an apply_block
        // error. Use a wrong nonce instead, which apply_transaction
        // treats as a hard error (NonceMismatch), to exercise step 5's
        // own rejection path.
        let store = funded_store(sender, 1_000)?;
        let envelope = signed_envelope(&keypair, sender, 1, open_window())?;

        let mut mempool = Mempool::new();
        let result = mempool.admit(
            &envelope.encode()?,
            &store,
            BlockHeight::GENESIS,
            CHAIN_ID,
            NETWORK_ID,
            &[],
        );
        assert!(matches!(result, Err(MempoolError::ExecutionFailed(_))));
        Ok(())
    }

    #[test]
    fn rejects_a_duplicate_tx_id() -> hn_state::StateResult<()> {
        let (sender, keypair) = sender_fixture(0x06)?;
        let store = funded_store(sender, 1_000)?;
        let envelope = signed_envelope(&keypair, sender, 0, open_window())?;

        let mut mempool = Mempool::new();
        mempool
            .admit(
                &envelope.encode()?,
                &store,
                BlockHeight::GENESIS,
                CHAIN_ID,
                NETWORK_ID,
                &[],
            )
            .map_err(|error| StateError::Storage(error.to_string()))?;

        let result = mempool.admit(
            &envelope.encode()?,
            &store,
            BlockHeight::GENESIS,
            CHAIN_ID,
            NETWORK_ID,
            &[],
        );
        assert!(matches!(result, Err(MempoolError::AlreadyPresent)));
        assert_eq!(mempool.len(), 1);
        Ok(())
    }

    #[test]
    fn rejects_once_at_capacity() -> hn_state::StateResult<()> {
        let mut mempool = Mempool::new();
        // Force capacity without admitting MEMPOOL_MAX_TXS real
        // transactions: directly exercise the guard by shrinking what
        // "full" means is not possible (the constant is fixed), so
        // instead fill `order` to capacity directly via repeated valid
        // admissions from distinct senders is impractically slow for a
        // unit test -- assert the guard fires by constructing a
        // mempool whose `order` already has MEMPOOL_MAX_TXS entries.
        mempool.order = vec![[0_u8; 32]; MEMPOOL_MAX_TXS];
        let (sender, keypair) = sender_fixture(0x07)?;
        let store = funded_store(sender, 1_000)?;
        let envelope = signed_envelope(&keypair, sender, 0, open_window())?;
        let result = mempool.admit(
            &envelope.encode()?,
            &store,
            BlockHeight::GENESIS,
            CHAIN_ID,
            NETWORK_ID,
            &[],
        );
        assert!(matches!(result, Err(MempoolError::Full)));
        Ok(())
    }

    #[test]
    fn remove_included_drops_only_the_named_entries() -> hn_state::StateResult<()> {
        let (sender_a, keypair_a) = sender_fixture(0x08)?;
        let (sender_b, keypair_b) = sender_fixture(0x09)?;
        let mut store = funded_store(sender_a, 1_000)?;
        {
            use hn_state::{AccountSection, StateWriter};
            let key = account_section_state_key(&sender_b, AccountSection::Balance)?;
            store.set(
                key,
                BalanceValueV1 {
                    native_balance: 1_000,
                }
                .encode(),
            )?;
        }
        let envelope_a = signed_envelope(&keypair_a, sender_a, 0, open_window())?;
        let envelope_b = signed_envelope(&keypair_b, sender_b, 0, open_window())?;

        let mut mempool = Mempool::new();
        let tx_id_a = mempool
            .admit(
                &envelope_a.encode()?,
                &store,
                BlockHeight::GENESIS,
                CHAIN_ID,
                NETWORK_ID,
                &[],
            )
            .map_err(|error| StateError::Storage(error.to_string()))?;
        let tx_id_b = mempool
            .admit(
                &envelope_b.encode()?,
                &store,
                BlockHeight::GENESIS,
                CHAIN_ID,
                NETWORK_ID,
                &[],
            )
            .map_err(|error| StateError::Storage(error.to_string()))?;

        mempool.remove_included([tx_id_a]);
        assert_eq!(mempool.len(), 1);
        assert!(!mempool.contains(&tx_id_a));
        assert!(mempool.contains(&tx_id_b));
        Ok(())
    }

    #[test]
    fn evict_expired_removes_only_transactions_past_their_own_max_height()
    -> hn_state::StateResult<()> {
        let (sender_a, keypair_a) = sender_fixture(0x0a)?;
        let (sender_b, keypair_b) = sender_fixture(0x0b)?;
        let mut store = funded_store(sender_a, 1_000)?;
        {
            use hn_state::{AccountSection, StateWriter};
            let key = account_section_state_key(&sender_b, AccountSection::Balance)?;
            store.set(
                key,
                BalanceValueV1 {
                    native_balance: 1_000,
                }
                .encode(),
            )?;
        }
        let expiring_window = ValidityWindowV1 {
            min_height: None,
            max_height: Some(BlockHeight::new(5)),
        };
        let envelope_a = signed_envelope(&keypair_a, sender_a, 0, expiring_window)?;
        let envelope_b = signed_envelope(&keypair_b, sender_b, 0, open_window())?;

        let mut mempool = Mempool::new();
        let tx_id_a = mempool
            .admit(
                &envelope_a.encode()?,
                &store,
                BlockHeight::GENESIS,
                CHAIN_ID,
                NETWORK_ID,
                &[],
            )
            .map_err(|error| StateError::Storage(error.to_string()))?;
        let tx_id_b = mempool
            .admit(
                &envelope_b.encode()?,
                &store,
                BlockHeight::GENESIS,
                CHAIN_ID,
                NETWORK_ID,
                &[],
            )
            .map_err(|error| StateError::Storage(error.to_string()))?;

        mempool.evict_expired(BlockHeight::new(6));
        assert_eq!(mempool.len(), 1);
        assert!(!mempool.contains(&tx_id_a));
        assert!(mempool.contains(&tx_id_b));
        Ok(())
    }
}

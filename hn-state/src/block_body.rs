use hn_crypto::Digest;
use hn_hncs::{Decoder, validate_count, write_bytes, write_list, write_u16, write_u32};

use crate::error::{StateError, StateResult};
use crate::extra_data::{MAX_EXTRA_DATA_LEN, extra_data_hash};
use crate::list_merkle::{list_empty_root, list_merkle_root};
use crate::receipt::ReceiptV1;
use crate::transaction_envelope::{MAX_TRANSACTION_SIZE, TransactionEnvelope};
use crate::tx_id::tx_id;

/// `body_version` for the current [`BlockBody`] shape (ADR-0008,
/// "Versioned Block Envelope").
pub const BODY_VERSION_1: u16 = 1;

/// Maximum number of entries in `BlockBody.transactions`/`.receipts`
/// (ADR-0008, "Decided: Block Size And Transaction Count Limits").
/// Value copied from that decision, not re-derived — this module is
/// not a second owner of it.
pub const MAX_TRANSACTIONS_PER_BLOCK: usize = 10_000;

/// Maximum length, in bytes, of one embedded `ReceiptV1` blob.
/// `ReceiptV1::encode` is currently fixed-width (35 bytes: `u16` +
/// `[u8; 32]` + `u8`); this bound is deliberately generous headroom
/// over that, not the exact current width, so a future `ReceiptV1`
/// growing (`fee_charged`/`resource_usage`/`emitted_event_references`,
/// all still open, ADR-0006 "Receipts") does not need this wire format
/// to change to fit — an implementation resource bound, same class of
/// decision as `MAX_TRANSACTION_SIZE`.
pub const MAX_RECEIPT_BLOB_LEN: usize = 512;

/// Maximum number of entries in `BlockBody.evidence`. An implementation
/// resource bound, not derived — evidence *count* is still an open
/// decision (ADR-0008, "Block Size And Transaction Count Limits" /
/// `docs/specs/core/block-format.md` §14); this only needs to be some
/// finite value for `evidence` to be a well-formed bounded list at all.
pub const MAX_EVIDENCE_PER_BLOCK: usize = 1024;

/// Maximum length, in bytes, of one embedded evidence blob. Same class
/// of decision as [`MAX_EVIDENCE_PER_BLOCK`] — no `ConsensusEvidence`
/// schema is decided yet (ADR-0015), so `evidence` entries are opaque,
/// already-canonical bytes here, matching
/// [`crate::evidence_digest`]'s own "does not define a concrete
/// `ConsensusEvidence` schema" boundary.
pub const MAX_EVIDENCE_BLOB_LEN: usize = 65_536;

/// The canonical block body (ADR-0008, "Decision" — `BlockBody`).
///
/// `transactions`/`receipts` are hand-rolled `u32 count || elements`
/// sequences, each element itself a length-prefixed blob — not
/// `hn_hncs::write_list`/`read_list`: `TransactionEnvelope::decode`/
/// `ReceiptV1::decode` can each fail with a domain-specific
/// [`StateError`], which those generic helpers' `HncsResult`-typed
/// element closures cannot express — the same justification
/// `hn_state::QuorumCertificate.aggregate_proof` and
/// `hn_network::ConsensusProposalMessageV1.transactions` already
/// established for the identical situation.
///
/// `evidence` uses `write_list`/`read_list` directly: each entry is
/// opaque bytes (no `ConsensusEvidence` schema decided yet, ADR-0015),
/// so decoding one can only ever fail with a plain HNCS framing error,
/// which those generic helpers handle natively.
///
/// `extra_data` is bounded opaque bytes (ADR-0008, "Decided: Extra
/// Data Format") — no separate version field of its own;
/// `body_version` already covers it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockBody {
    /// Structure version for this body shape.
    pub body_version: u16,
    /// This block's transactions, in canonical order.
    pub transactions: Vec<TransactionEnvelope>,
    /// This block's receipts, in the same order as `transactions`.
    pub receipts: Vec<ReceiptV1>,
    /// Included Byzantine evidence, each entry already-canonical bytes
    /// of a not-yet-decided `ConsensusEvidence` shape (ADR-0015).
    pub evidence: Vec<Vec<u8>>,
    /// Bounded, opaque, explicitly-specified-only extra data
    /// (ADR-0008, "Decided: Extra Data Format").
    pub extra_data: Vec<u8>,
}

impl BlockBody {
    /// Encodes this body as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, self.body_version);
        write_transactions(&mut out, &self.transactions)?;
        write_receipts(&mut out, &self.receipts)?;
        write_list(
            &mut out,
            &self.evidence,
            MAX_EVIDENCE_PER_BLOCK,
            |out, blob| write_bytes(out, blob, MAX_EVIDENCE_BLOB_LEN),
        )
        .map_err(StateError::Encoding)?;
        write_bytes(&mut out, &self.extra_data, MAX_EXTRA_DATA_LEN)
            .map_err(StateError::Encoding)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`BlockBody::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let body_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if body_version != BODY_VERSION_1 {
            return Err(StateError::UnsupportedBlockBodyVersion {
                value: body_version,
            });
        }
        let transactions = read_transactions(&mut decoder)?;
        let receipts = read_receipts(&mut decoder)?;
        let evidence = decoder
            .read_list(MAX_EVIDENCE_PER_BLOCK, |decoder| {
                decoder
                    .read_bytes(MAX_EVIDENCE_BLOB_LEN)
                    .map(<[u8]>::to_vec)
            })
            .map_err(StateError::Encoding)?;
        let extra_data = decoder
            .read_bytes(MAX_EXTRA_DATA_LEN)
            .map_err(StateError::Encoding)?
            .to_vec();

        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self {
            body_version,
            transactions,
            receipts,
            evidence,
            extra_data,
        })
    }

    /// `hn-list-merkle-v1` commitment over each transaction's `tx_id`,
    /// in block order (ADR-0008, "Transactions Root").
    pub fn transactions_root(&self) -> StateResult<Digest> {
        let leaves: Vec<Digest> = self
            .transactions
            .iter()
            .map(|tx| tx_id(&tx.encode()?))
            .collect::<StateResult<_>>()?;
        list_merkle_root(&leaves)
    }

    /// `hn-list-merkle-v1` commitment over each receipt's digest, in
    /// the same order as `transactions_root` (ADR-0008, "Receipts
    /// Root").
    pub fn receipts_root(&self) -> StateResult<Digest> {
        let leaves: Vec<Digest> = self
            .receipts
            .iter()
            .map(ReceiptV1::digest)
            .collect::<StateResult<_>>()?;
        list_merkle_root(&leaves)
    }

    /// `hn-list-merkle-v1` commitment over included evidence's own
    /// `evidence_digest`, sorted by ascending digest (ADR-0008,
    /// "Evidence Root"; ADR-0015). Empty when `evidence` is empty
    /// (`list_empty_root`) — genesis's own case, since nothing has a
    /// real use for this field yet.
    pub fn evidence_root(&self) -> StateResult<Digest> {
        if self.evidence.is_empty() {
            return list_empty_root();
        }
        let mut leaves: Vec<Digest> = self
            .evidence
            .iter()
            .map(|blob| crate::evidence_digest::evidence_digest(blob))
            .collect::<StateResult<_>>()?;
        leaves.sort_unstable();
        list_merkle_root(&leaves)
    }

    /// This body's own `extra_data_hash` (ADR-0008, "Decided: Extra
    /// Data Format").
    pub fn extra_data_hash(&self) -> StateResult<Digest> {
        extra_data_hash(&self.extra_data)
    }
}

fn write_transactions(out: &mut Vec<u8>, transactions: &[TransactionEnvelope]) -> StateResult<()> {
    validate_count(transactions.len(), MAX_TRANSACTIONS_PER_BLOCK).map_err(StateError::Encoding)?;
    let count = u32::try_from(transactions.len()).map_err(|_| {
        StateError::Encoding(hn_hncs::HncsError::LengthFieldOverflow {
            length: transactions.len(),
        })
    })?;
    write_u32(out, count);
    for tx in transactions {
        let bytes = tx.encode()?;
        write_bytes(out, &bytes, MAX_TRANSACTION_SIZE).map_err(StateError::Encoding)?;
    }
    Ok(())
}

fn read_transactions(decoder: &mut Decoder<'_>) -> StateResult<Vec<TransactionEnvelope>> {
    let count = decoder.read_u32().map_err(StateError::Encoding)? as usize;
    validate_count(count, MAX_TRANSACTIONS_PER_BLOCK).map_err(StateError::Encoding)?;
    let mut transactions = Vec::with_capacity(count);
    for _ in 0..count {
        let bytes = decoder
            .read_bytes(MAX_TRANSACTION_SIZE)
            .map_err(StateError::Encoding)?;
        transactions.push(TransactionEnvelope::decode(bytes)?);
    }
    Ok(transactions)
}

fn write_receipts(out: &mut Vec<u8>, receipts: &[ReceiptV1]) -> StateResult<()> {
    validate_count(receipts.len(), MAX_TRANSACTIONS_PER_BLOCK).map_err(StateError::Encoding)?;
    let count = u32::try_from(receipts.len()).map_err(|_| {
        StateError::Encoding(hn_hncs::HncsError::LengthFieldOverflow {
            length: receipts.len(),
        })
    })?;
    write_u32(out, count);
    for receipt in receipts {
        write_bytes(out, &receipt.encode(), MAX_RECEIPT_BLOB_LEN).map_err(StateError::Encoding)?;
    }
    Ok(())
}

fn read_receipts(decoder: &mut Decoder<'_>) -> StateResult<Vec<ReceiptV1>> {
    let count = decoder.read_u32().map_err(StateError::Encoding)? as usize;
    validate_count(count, MAX_TRANSACTIONS_PER_BLOCK).map_err(StateError::Encoding)?;
    let mut receipts = Vec::with_capacity(count);
    for _ in 0..count {
        let bytes = decoder
            .read_bytes(MAX_RECEIPT_BLOB_LEN)
            .map_err(StateError::Encoding)?;
        receipts.push(ReceiptV1::decode(bytes)?);
    }
    Ok(receipts)
}

#[cfg(test)]
mod tests {
    use hn_core::AccountNonce;
    use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope};

    use super::{BODY_VERSION_1, BlockBody};
    use crate::access_list::AccessListV1;
    use crate::error::{StateError, StateResult};
    use crate::receipt::{ReceiptStatus, ReceiptV1};
    use crate::transaction_envelope::{TX_VERSION_1, TransactionEnvelope, TxType};
    use crate::validity_window::ValidityWindowV1;

    fn sample_tx(nonce: u64) -> StateResult<TransactionEnvelope> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x11; 32]);
        let mut envelope = TransactionEnvelope {
            tx_version: TX_VERSION_1,
            chain_id: 1,
            network_id: 1,
            tx_type: TxType::Transfer,
            sender: [0x22; 32],
            bootstrap_key: Some(keypair.key_descriptor()),
            nonce: AccountNonce::new(nonce),
            fee_limit: 100,
            validity_window: ValidityWindowV1 {
                min_height: None,
                max_height: None,
            },
            access_list: AccessListV1 {
                reads: vec![],
                writes: vec![],
            },
            payload: crate::transfer_payload::TransferPayloadV1 {
                recipient: [0x33; 32],
                asset_id: None,
                amount: 1,
            }
            .encode()?,
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

    fn sample_body() -> StateResult<BlockBody> {
        let tx = sample_tx(0)?;
        let receipt = ReceiptV1 {
            tx_id: crate::tx_id::tx_id(&tx.encode()?)?,
            status: ReceiptStatus::Success,
        };
        Ok(BlockBody {
            body_version: BODY_VERSION_1,
            transactions: vec![tx],
            receipts: vec![receipt],
            evidence: vec![b"evidence-a".to_vec(), b"evidence-b".to_vec()],
            extra_data: b"extra".to_vec(),
        })
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        let body = sample_body()?;
        let decoded = BlockBody::decode(&body.encode()?)?;
        assert_eq!(decoded, body);
        Ok(())
    }

    #[test]
    fn round_trips_an_empty_body() -> StateResult<()> {
        let body = BlockBody {
            body_version: BODY_VERSION_1,
            transactions: vec![],
            receipts: vec![],
            evidence: vec![],
            extra_data: vec![],
        };
        let decoded = BlockBody::decode(&body.encode()?)?;
        assert_eq!(decoded, body);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_body_version() -> StateResult<()> {
        let mut encoded = sample_body()?.encode()?;
        encoded[0] = 0x02;
        assert_eq!(
            BlockBody::decode(&encoded),
            Err(StateError::UnsupportedBlockBodyVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn transactions_root_matches_hand_computed_value() -> StateResult<()> {
        let body = sample_body()?;
        let tx_id = crate::tx_id::tx_id(&body.transactions[0].encode()?)?;
        assert_eq!(body.transactions_root()?, tx_id);
        Ok(())
    }

    #[test]
    fn receipts_root_matches_hand_computed_value() -> StateResult<()> {
        let body = sample_body()?;
        assert_eq!(body.receipts_root()?, body.receipts[0].digest()?);
        Ok(())
    }

    #[test]
    fn empty_body_roots_are_the_list_empty_root() -> StateResult<()> {
        let body = BlockBody {
            body_version: BODY_VERSION_1,
            transactions: vec![],
            receipts: vec![],
            evidence: vec![],
            extra_data: vec![],
        };
        let empty = crate::list_merkle::list_empty_root()?;
        assert_eq!(body.transactions_root()?, empty);
        assert_eq!(body.receipts_root()?, empty);
        assert_eq!(body.evidence_root()?, empty);
        Ok(())
    }

    #[test]
    fn extra_data_hash_matches_the_standalone_function() -> StateResult<()> {
        let body = sample_body()?;
        assert_eq!(
            body.extra_data_hash()?,
            crate::extra_data::extra_data_hash(&body.extra_data)?
        );
        Ok(())
    }
}

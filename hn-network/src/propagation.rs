use hn_crypto::Digest;
use hn_hncs::{Decoder, validate_count, write_bool, write_fixed_bytes, write_u32};
use hn_state::{QuorumCertificate, TransactionEnvelope};

use crate::error::NetworkResult;

/// Maximum number of `TransactionEnvelope` entries in one
/// `BlockResponseV1`/`ConsensusProposalMessageV1` — matches ADR-0008's
/// own already-decided `MAX_TRANSACTIONS_PER_BLOCK` (10,000), not yet
/// implemented as a Rust constant anywhere in `hn-state` (no
/// block-processing pipeline consumes it yet), reused here as the
/// value rather than duplicated as a fresh, independent choice.
pub const MAX_TRANSACTIONS_PER_MESSAGE: usize = 10_000;

/// Maximum length, in bytes, of one embedded `TransactionEnvelope`
/// blob — matches `hn_state::MAX_TRANSACTION_SIZE` exactly (reused
/// directly, not duplicated).
pub const MAX_TRANSACTION_BLOB_LEN: usize = hn_state::MAX_TRANSACTION_SIZE;

/// Maximum length, in bytes, of one embedded `QuorumCertificate` blob.
/// An implementation resource bound with headroom over a realistic
/// certificate's own worst case (`MAX_SIGNER_COMMITMENT_LEN` +
/// `MAX_QUORUM_SIGNATURES` individual signatures) — same class of
/// decision as `MAX_TRANSACTION_BLOB_LEN`, picked fresh since no
/// existing `hn_state` constant already covers a whole encoded `QuorumCertificate`.
pub const MAX_QC_BLOB_LEN: usize = 1_048_576;

/// "I have this transaction" (ADR-0018, "Gossip Announcements").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionAnnounceV1 {
    /// The announced transaction's own `tx_id`.
    pub tx_id: Digest,
}

impl TransactionAnnounceV1 {
    /// Encodes this value as canonical HNCS bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write_fixed_bytes(&mut out, &self.tx_id);
        out
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`TransactionAnnounceV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let tx_id = decoder.read_fixed_bytes::<32>()?;
        decoder.finish()?;
        Ok(Self { tx_id })
    }
}

/// "Send me this transaction."
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionRequestV1 {
    /// The requested transaction's own `tx_id`.
    pub tx_id: Digest,
}

impl TransactionRequestV1 {
    /// Encodes this value as canonical HNCS bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write_fixed_bytes(&mut out, &self.tx_id);
        out
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`TransactionRequestV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let tx_id = decoder.read_fixed_bytes::<32>()?;
        decoder.finish()?;
        Ok(Self { tx_id })
    }
}

/// The requested transaction's full canonical bytes — one transaction
/// per response in this pass (batching is a real future optimization,
/// not attempted here).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionResponseV1 {
    /// The requested transaction.
    pub envelope: TransactionEnvelope,
}

impl TransactionResponseV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> NetworkResult<Vec<u8>> {
        Ok(self.envelope.encode()?)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`TransactionResponseV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        Ok(Self {
            envelope: TransactionEnvelope::decode(bytes)?,
        })
    }
}

/// "I have this block" (`block_hash` only).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockAnnounceV1 {
    /// The announced block's own hash. Not derived from a real
    /// `BlockHeader` (none exists anywhere in this codebase yet) —
    /// whatever the caller's own block-identification scheme produces.
    pub block_hash: Digest,
}

impl BlockAnnounceV1 {
    /// Encodes this value as canonical HNCS bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write_fixed_bytes(&mut out, &self.block_hash);
        out
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`BlockAnnounceV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let block_hash = decoder.read_fixed_bytes::<32>()?;
        decoder.finish()?;
        Ok(Self { block_hash })
    }
}

/// "Send me this block."
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockRequestV1 {
    /// The requested block's hash.
    pub block_hash: Digest,
}

impl BlockRequestV1 {
    /// Encodes this value as canonical HNCS bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write_fixed_bytes(&mut out, &self.block_hash);
        out
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`BlockRequestV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let block_hash = decoder.read_fixed_bytes::<32>()?;
        decoder.finish()?;
        Ok(Self { block_hash })
    }
}

/// The requested block's transaction list (ADR-0036, "Decided:
/// Block/Transaction/Vote Propagation") — **not a real block**: no
/// header, no `state_root`, no justification. Deliberately the exact
/// same shape `hn_consensus::ConsensusEngine::handle_proposal` already
/// consumes (`block_hash` + `Vec<TransactionEnvelope>`), so a future
/// wiring pass can hand one straight to it unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockResponseV1 {
    /// The block's hash.
    pub block_hash: Digest,
    /// The block's transactions, in order.
    pub transactions: Vec<TransactionEnvelope>,
}

impl BlockResponseV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> NetworkResult<Vec<u8>> {
        let mut out = Vec::new();
        write_fixed_bytes(&mut out, &self.block_hash);
        write_transaction_list(&mut out, &self.transactions)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`BlockResponseV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let block_hash = decoder.read_fixed_bytes::<32>()?;
        let transactions = read_transaction_list(&mut decoder)?;
        decoder.finish()?;
        Ok(Self {
            block_hash,
            transactions,
        })
    }
}

/// A consensus round proposal (ADR-0036, "Decided:
/// Block/Transaction/Vote Propagation") — deliberately the exact same
/// shape `hn_consensus::ConsensusEngine::handle_proposal`'s own three
/// parameters already take.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsensusProposalMessageV1 {
    /// The proposed block's hash.
    pub block_hash: Digest,
    /// The proposed block's transactions, in order.
    pub transactions: Vec<TransactionEnvelope>,
    /// A `prevote` quorum certificate justifying re-proposing a value a
    /// receiving validator may be locked on something else for (see
    /// `hn_consensus::ConsensusState`'s own "Locking rule").
    pub justification: Option<QuorumCertificate>,
}

impl ConsensusProposalMessageV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> NetworkResult<Vec<u8>> {
        let mut out = Vec::new();
        write_fixed_bytes(&mut out, &self.block_hash);
        write_transaction_list(&mut out, &self.transactions)?;
        write_optional_qc(&mut out, self.justification.as_ref())?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`ConsensusProposalMessageV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let block_hash = decoder.read_fixed_bytes::<32>()?;
        let transactions = read_transaction_list(&mut decoder)?;
        let justification = read_optional_qc(&mut decoder)?;
        decoder.finish()?;
        Ok(Self {
            block_hash,
            transactions,
            justification,
        })
    }
}

/// Hand-rolled `u32 count || elements` encoding, each element itself a
/// length-prefixed blob — not `hn_hncs::write_list`/`read_list`:
/// `TransactionEnvelope::decode` can fail with a domain-specific
/// `hn_state::StateError`, which those generic helpers'
/// `HncsResult`-typed closures cannot express, the same justification
/// `hn_state::QuorumCertificate.aggregate_proof` already established
/// for the identical situation.
fn write_transaction_list(
    out: &mut Vec<u8>,
    transactions: &[TransactionEnvelope],
) -> NetworkResult<()> {
    validate_count(transactions.len(), MAX_TRANSACTIONS_PER_MESSAGE)?;
    write_u32(out, transactions.len() as u32);
    for envelope in transactions {
        let bytes = envelope.encode()?;
        hn_hncs::write_bytes(out, &bytes, MAX_TRANSACTION_BLOB_LEN)?;
    }
    Ok(())
}

fn read_transaction_list(decoder: &mut Decoder<'_>) -> NetworkResult<Vec<TransactionEnvelope>> {
    let count = decoder.read_u32()? as usize;
    validate_count(count, MAX_TRANSACTIONS_PER_MESSAGE)?;
    let mut transactions = Vec::with_capacity(count);
    for _ in 0..count {
        let bytes = decoder.read_bytes(MAX_TRANSACTION_BLOB_LEN)?;
        transactions.push(TransactionEnvelope::decode(bytes)?);
    }
    Ok(transactions)
}

fn write_optional_qc(out: &mut Vec<u8>, qc: Option<&QuorumCertificate>) -> NetworkResult<()> {
    write_bool(out, qc.is_some());
    if let Some(qc) = qc {
        let bytes = qc.encode()?;
        hn_hncs::write_bytes(out, &bytes, MAX_QC_BLOB_LEN)?;
    }
    Ok(())
}

fn read_optional_qc(decoder: &mut Decoder<'_>) -> NetworkResult<Option<QuorumCertificate>> {
    if decoder.read_bool()? {
        let bytes = decoder.read_bytes(MAX_QC_BLOB_LEN)?;
        Ok(Some(QuorumCertificate::decode(bytes)?))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use hn_core::AccountNonce;
    use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope};
    use hn_state::{
        AccessListV1, QuorumCertificate, TX_VERSION_1, TransactionEnvelope, TransferPayloadV1,
        TxType, ValidityWindowV1, VoteTargetType, VoteType,
    };

    use super::{
        BlockAnnounceV1, BlockRequestV1, BlockResponseV1, ConsensusProposalMessageV1,
        TransactionAnnounceV1, TransactionRequestV1, TransactionResponseV1,
    };
    use crate::error::NetworkResult;

    const TX_ID: [u8; 32] = [0x11; 32];
    const BLOCK_HASH: [u8; 32] = [0x22; 32];

    fn sample_envelope(nonce: u64) -> NetworkResult<TransactionEnvelope> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x33; 32]);
        let payload = TransferPayloadV1 {
            recipient: [0x44; 32],
            asset_id: None,
            amount: 1_000,
        }
        .encode()?;
        let mut envelope = TransactionEnvelope {
            tx_version: TX_VERSION_1,
            chain_id: 1,
            network_id: 1,
            tx_type: TxType::Transfer,
            sender: [0x55; 32],
            bootstrap_key: None,
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

    fn sample_qc(round: u64) -> QuorumCertificate {
        QuorumCertificate {
            certificate_type: VoteType::Prevote,
            chain_id: 1,
            network_id: 1,
            epoch: hn_core::Epoch::new(0),
            height: hn_core::BlockHeight::new(1),
            round: hn_core::Round::new(round),
            validator_set_commitment: [0x66; 32],
            target_type: VoteTargetType::Block,
            target_hash: BLOCK_HASH,
            total_voting_power: 100,
            signed_voting_power: 100,
            signer_commitment: vec![0b0000_0001],
            aggregate_proof: vec![SignatureEnvelope {
                algorithm_id: 1,
                key_reference: None,
                signature: vec![0xaa; 64],
            }],
        }
    }

    #[test]
    fn transaction_announce_round_trips_through_decode() -> NetworkResult<()> {
        let announce = TransactionAnnounceV1 { tx_id: TX_ID };
        let decoded = TransactionAnnounceV1::decode(&announce.encode())?;
        assert_eq!(decoded, announce);
        Ok(())
    }

    #[test]
    fn transaction_request_round_trips_through_decode() -> NetworkResult<()> {
        let request = TransactionRequestV1 { tx_id: TX_ID };
        let decoded = TransactionRequestV1::decode(&request.encode())?;
        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn transaction_response_round_trips_a_real_envelope() -> NetworkResult<()> {
        let response = TransactionResponseV1 {
            envelope: sample_envelope(0)?,
        };
        let decoded = TransactionResponseV1::decode(&response.encode()?)?;
        assert_eq!(decoded, response);
        Ok(())
    }

    #[test]
    fn block_announce_round_trips_through_decode() -> NetworkResult<()> {
        let announce = BlockAnnounceV1 {
            block_hash: BLOCK_HASH,
        };
        let decoded = BlockAnnounceV1::decode(&announce.encode())?;
        assert_eq!(decoded, announce);
        Ok(())
    }

    #[test]
    fn block_request_round_trips_through_decode() -> NetworkResult<()> {
        let request = BlockRequestV1 {
            block_hash: BLOCK_HASH,
        };
        let decoded = BlockRequestV1::decode(&request.encode())?;
        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn block_response_round_trips_several_real_transactions() -> NetworkResult<()> {
        let response = BlockResponseV1 {
            block_hash: BLOCK_HASH,
            transactions: vec![sample_envelope(0)?, sample_envelope(1)?],
        };
        let decoded = BlockResponseV1::decode(&response.encode()?)?;
        assert_eq!(decoded, response);
        Ok(())
    }

    #[test]
    fn block_response_round_trips_with_no_transactions() -> NetworkResult<()> {
        let response = BlockResponseV1 {
            block_hash: BLOCK_HASH,
            transactions: vec![],
        };
        let decoded = BlockResponseV1::decode(&response.encode()?)?;
        assert_eq!(decoded, response);
        Ok(())
    }

    #[test]
    fn consensus_proposal_round_trips_without_justification() -> NetworkResult<()> {
        let message = ConsensusProposalMessageV1 {
            block_hash: BLOCK_HASH,
            transactions: vec![sample_envelope(0)?],
            justification: None,
        };
        let decoded = ConsensusProposalMessageV1::decode(&message.encode()?)?;
        assert_eq!(decoded, message);
        Ok(())
    }

    #[test]
    fn consensus_proposal_round_trips_with_justification() -> NetworkResult<()> {
        let message = ConsensusProposalMessageV1 {
            block_hash: BLOCK_HASH,
            transactions: vec![sample_envelope(0)?],
            justification: Some(sample_qc(0)),
        };
        let decoded = ConsensusProposalMessageV1::decode(&message.encode()?)?;
        assert_eq!(decoded, message);
        Ok(())
    }
}

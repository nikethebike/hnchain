use hn_core::BlockHeight;
use hn_crypto::{Digest, SignatureEnvelope};
use hn_hncs::{Decoder, validate_count, write_bytes, write_fixed_bytes, write_u32, write_u64};
use hn_state::{BlockEnvelope, FinalizedBlock, TransactionEnvelope};

use crate::error::NetworkResult;

/// Maximum number of `TransactionEnvelope` entries in one
/// `BlockResponseV1` — matches `hn_state::MAX_TRANSACTIONS_PER_BLOCK`
/// (ADR-0008) exactly, reused as the value rather than duplicated as a
/// fresh, independent choice. `ConsensusProposalMessageV1` no longer
/// has a transactions field of its own (ADR-0042): its proposed
/// block's transactions live inside `hn_state::BlockBody`, bounded by
/// that same constant directly.
pub const MAX_TRANSACTIONS_PER_MESSAGE: usize = 10_000;

/// Maximum length, in bytes, of one embedded `TransactionEnvelope`
/// blob — matches `hn_state::MAX_TRANSACTION_SIZE` exactly (reused
/// directly, not duplicated).
pub const MAX_TRANSACTION_BLOB_LEN: usize = hn_state::MAX_TRANSACTION_SIZE;

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

/// Maximum length, in bytes, of one embedded `BlockEnvelope` blob.
/// Generous headroom over the sum of `hn_state`'s own
/// `MAX_BLOCK_HEADER_BLOB_LEN`/`MAX_BLOCK_BODY_BLOB_LEN`/
/// `MAX_JUSTIFICATION_BLOB_LEN` (the envelope's own three internal
/// bounds) — a wire-frame safety cap, not a second, independently
/// chosen policy value.
pub const MAX_BLOCK_ENVELOPE_BLOB_LEN: usize = 33_554_432;

/// A consensus round proposal (ADR-0036, "Decided:
/// Block/Transaction/Vote Propagation"; ADR-0039, "Decided: Proposals
/// Self-Describe Their Height/Round"; ADR-0042, "Decided:
/// `BlockEnvelope`, Not A Bare `block_hash` Claim") — the whole
/// `hn_state::BlockEnvelope` a receiver needs to independently
/// recompute `block_hash` and cross-check `header` against `body`,
/// plus `proposer_signature`: a `SignatureEnvelope` over `envelope.
/// block_hash()` from the registered consensus key of whichever
/// validator `envelope.header.proposer` names. `height`/`round` are no
/// longer separate top-level fields (ADR-0039's own reason for adding
/// them — self-describing height/round for a receiver's own stale-
/// round filter — is already satisfied by `envelope.header.height`/
/// `.round` now that the full header travels on the wire; a redundant
/// top-level copy would just be a second value that could disagree
/// with the header's own).
///
/// `hn_consensus::ConsensusEvent::Proposal` itself still carries no
/// height/round (it always targets whichever round the local
/// `ConsensusState` is already attempting) and no signature (it is
/// fed already-verified `block_hash`/`transactions`/`justification` by
/// its caller) — this message type is `hn-node`'s own wire boundary
/// around that trusted-input layer, not a change to the layer itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsensusProposalMessageV1 {
    /// The proposed block, in full — a receiver recomputes
    /// `block_hash` from `envelope.header` rather than trusting any
    /// separately-carried claimed value (ADR-0042).
    pub envelope: BlockEnvelope,
    /// The proposer's own signature over `envelope.block_hash()`,
    /// verified by a receiver against `envelope.header.proposer`'s
    /// registered consensus key (ADR-0042, "Decided: Proposer
    /// Authentication").
    pub proposer_signature: SignatureEnvelope,
}

impl ConsensusProposalMessageV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> NetworkResult<Vec<u8>> {
        let mut out = Vec::new();
        write_bytes(
            &mut out,
            &self.envelope.encode()?,
            MAX_BLOCK_ENVELOPE_BLOB_LEN,
        )?;
        self.proposer_signature.encode_into(&mut out)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`ConsensusProposalMessageV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let envelope_bytes = decoder.read_bytes(MAX_BLOCK_ENVELOPE_BLOB_LEN)?;
        let envelope = BlockEnvelope::decode(envelope_bytes)?;
        let proposer_signature = SignatureEnvelope::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(Self {
            envelope,
            proposer_signature,
        })
    }
}

/// Maximum number of `FinalizedBlock` entries in one
/// `SyncBlockRangeResponseV1` (ADR-0044, "Decided: Wire Protocol") — a
/// large gap is fetched across several bounded requests in sequence,
/// never one unbounded message.
pub const MAX_SYNC_BLOCKS_PER_RESPONSE: u32 = 64;

/// Maximum length, in bytes, of one embedded `FinalizedBlock` blob —
/// generous headroom over the sum of `hn_state`'s own
/// `MAX_FINALIZED_BLOCK_ENVELOPE_BLOB_LEN`/
/// `MAX_FINALIZED_BLOCK_QC_BLOB_LEN` (`FinalizedBlock::encode`'s own
/// two internal bounds) — a wire-frame safety cap, not a second,
/// independently chosen policy value.
pub const MAX_FINALIZED_BLOCK_BLOB_LEN: usize = 21_000_000;

/// "Send me finalized blocks starting after `from_height`, up to
/// `max_count` of them" (ADR-0044, "Decided: Wire Protocol") — a
/// receiver that knows only that it is behind, not which hashes it is
/// missing, the opposite shape of `BlockRequestV1`'s own by-hash
/// lookup for a receiver that already knows what it wants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncBlockRangeRequestV1 {
    /// Send blocks starting immediately *after* this height.
    pub from_height: BlockHeight,
    /// Never send more than this many blocks in the response.
    pub max_count: u32,
}

impl SyncBlockRangeRequestV1 {
    /// Encodes this value as canonical HNCS bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write_u64(&mut out, self.from_height.get());
        write_u32(&mut out, self.max_count);
        out
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`SyncBlockRangeRequestV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let from_height = BlockHeight::new(decoder.read_u64()?);
        let max_count = decoder.read_u32()?;
        decoder.finish()?;
        Ok(Self {
            from_height,
            max_count,
        })
    }
}

/// A bounded, height-ordered run of [`FinalizedBlock`]s, starting
/// immediately after the request's own `from_height` (ADR-0044,
/// "Decided: Wire Protocol") — a prefix of what was asked for if the
/// responder does not have (or will not send) the full range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncBlockRangeResponseV1 {
    /// The returned blocks, in ascending height order.
    pub blocks: Vec<FinalizedBlock>,
}

impl SyncBlockRangeResponseV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> NetworkResult<Vec<u8>> {
        let mut out = Vec::new();
        validate_count(self.blocks.len(), MAX_SYNC_BLOCKS_PER_RESPONSE as usize)?;
        write_u32(&mut out, self.blocks.len() as u32);
        for block in &self.blocks {
            let bytes = block.encode()?;
            write_bytes(&mut out, &bytes, MAX_FINALIZED_BLOCK_BLOB_LEN)?;
        }
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`SyncBlockRangeResponseV1::encode`].
    pub fn decode(bytes: &[u8]) -> NetworkResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let count = decoder.read_u32()? as usize;
        validate_count(count, MAX_SYNC_BLOCKS_PER_RESPONSE as usize)?;
        let mut blocks = Vec::with_capacity(count);
        for _ in 0..count {
            let block_bytes = decoder.read_bytes(MAX_FINALIZED_BLOCK_BLOB_LEN)?;
            blocks.push(FinalizedBlock::decode(block_bytes)?);
        }
        decoder.finish()?;
        Ok(Self { blocks })
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
        write_bytes(out, &bytes, MAX_TRANSACTION_BLOB_LEN)?;
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

#[cfg(test)]
mod tests {
    use hn_core::{AccountNonce, BlockHeight, Epoch, ProtocolEpoch, Round, UnixTimeMillis};
    use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope};
    use hn_state::{
        AccessListV1, BLOCK_VERSION_1, BODY_VERSION_1, BlockBody, BlockEnvelope, BlockHeader,
        HEADER_VERSION_1, QuorumCertificate, TX_VERSION_1, TransactionEnvelope, TransferPayloadV1,
        TxType, ValidityWindowV1, VoteTargetType, VoteType, list_empty_root,
    };

    use super::{
        BlockAnnounceV1, BlockRequestV1, BlockResponseV1, ConsensusProposalMessageV1,
        MAX_SYNC_BLOCKS_PER_RESPONSE, SyncBlockRangeRequestV1, SyncBlockRangeResponseV1,
        TransactionAnnounceV1, TransactionRequestV1, TransactionResponseV1,
    };
    use crate::error::NetworkResult;
    use hn_state::{FINALIZED_BLOCK_VERSION_1, FinalizedBlock};

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
            epoch: Epoch::new(0),
            height: BlockHeight::new(1),
            round: Round::new(round),
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

    fn sample_block_envelope(
        transactions: Vec<TransactionEnvelope>,
        justification: Option<QuorumCertificate>,
    ) -> NetworkResult<BlockEnvelope> {
        let body = BlockBody {
            body_version: BODY_VERSION_1,
            transactions,
            receipts: vec![],
            evidence: vec![],
            extra_data: vec![],
        };
        let header = BlockHeader {
            header_version: HEADER_VERSION_1,
            chain_id: 1,
            network_id: 1,
            height: BlockHeight::new(7),
            round: Round::new(2),
            epoch: Epoch::new(0),
            protocol_epoch: ProtocolEpoch::GENESIS,
            parent_block_hash: [0_u8; 32],
            proposer: [0x01_u8; 32],
            timestamp: UnixTimeMillis::from_millis(1),
            transactions_root: body.transactions_root()?,
            state_root: [0x02_u8; 32],
            receipts_root: body.receipts_root()?,
            events_root: list_empty_root()?,
            consensus_root: [0x03_u8; 32],
            evidence_root: body.evidence_root()?,
            protocol_parameters_hash: [0x04_u8; 32],
            extra_data_hash: body.extra_data_hash()?,
        };
        Ok(BlockEnvelope {
            block_version: BLOCK_VERSION_1,
            header,
            body,
            justification,
        })
    }

    fn sign_block_hash(envelope: &BlockEnvelope) -> NetworkResult<SignatureEnvelope> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x07; 32]);
        let digest = envelope.block_hash()?;
        Ok(SignatureEnvelope {
            algorithm_id: keypair.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: keypair.sign(&digest).to_vec(),
        })
    }

    #[test]
    fn consensus_proposal_round_trips_without_justification() -> NetworkResult<()> {
        let envelope = sample_block_envelope(vec![sample_envelope(0)?], None)?;
        let proposer_signature = sign_block_hash(&envelope)?;
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let decoded = ConsensusProposalMessageV1::decode(&message.encode()?)?;
        assert_eq!(decoded, message);
        Ok(())
    }

    #[test]
    fn consensus_proposal_round_trips_with_justification() -> NetworkResult<()> {
        let envelope = sample_block_envelope(vec![sample_envelope(0)?], Some(sample_qc(0)))?;
        let proposer_signature = sign_block_hash(&envelope)?;
        let message = ConsensusProposalMessageV1 {
            envelope,
            proposer_signature,
        };
        let decoded = ConsensusProposalMessageV1::decode(&message.encode()?)?;
        assert_eq!(decoded, message);
        Ok(())
    }

    fn sample_finalized_block() -> NetworkResult<FinalizedBlock> {
        Ok(FinalizedBlock {
            finalized_block_version: FINALIZED_BLOCK_VERSION_1,
            envelope: sample_block_envelope(vec![], None)?,
            commit_qc: sample_qc(0),
        })
    }

    #[test]
    fn sync_block_range_request_round_trips_through_decode() -> NetworkResult<()> {
        let request = SyncBlockRangeRequestV1 {
            from_height: BlockHeight::new(5),
            max_count: 10,
        };
        let decoded = SyncBlockRangeRequestV1::decode(&request.encode())?;
        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn sync_block_range_response_round_trips_several_blocks() -> NetworkResult<()> {
        let response = SyncBlockRangeResponseV1 {
            blocks: vec![sample_finalized_block()?, sample_finalized_block()?],
        };
        let decoded = SyncBlockRangeResponseV1::decode(&response.encode()?)?;
        assert_eq!(decoded, response);
        Ok(())
    }

    #[test]
    fn sync_block_range_response_round_trips_with_no_blocks() -> NetworkResult<()> {
        let response = SyncBlockRangeResponseV1 { blocks: vec![] };
        let decoded = SyncBlockRangeResponseV1::decode(&response.encode()?)?;
        assert_eq!(decoded, response);
        Ok(())
    }

    #[test]
    fn sync_block_range_response_encode_rejects_more_than_the_per_response_limit()
    -> NetworkResult<()> {
        let mut blocks = Vec::new();
        for _ in 0..=MAX_SYNC_BLOCKS_PER_RESPONSE {
            blocks.push(sample_finalized_block()?);
        }
        let response = SyncBlockRangeResponseV1 { blocks };
        assert!(response.encode().is_err());
        Ok(())
    }

    #[test]
    fn sync_block_range_response_decode_rejects_a_claimed_count_over_the_limit() {
        let mut bytes = Vec::new();
        hn_hncs::write_u32(&mut bytes, MAX_SYNC_BLOCKS_PER_RESPONSE + 1);
        assert!(SyncBlockRangeResponseV1::decode(&bytes).is_err());
    }
}

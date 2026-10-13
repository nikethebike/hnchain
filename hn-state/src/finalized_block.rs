use hn_hncs::{Decoder, write_bytes, write_u16};

use crate::block_envelope::BlockEnvelope;
use crate::error::{StateError, StateResult};
use crate::vote::QuorumCertificate;

/// `finalized_block_version` for the current [`FinalizedBlock`] shape
/// (ADR-0044, "Decided: Durable Finalized-Block Storage").
pub const FINALIZED_BLOCK_VERSION_1: u16 = 1;

/// Maximum length, in bytes, of one embedded [`BlockEnvelope`] blob —
/// generous headroom over `BlockEnvelope`'s own already-bounded worst
/// case (`MAX_BLOCK_HEADER_BLOB_LEN` + `MAX_BLOCK_BODY_BLOB_LEN` +
/// `MAX_JUSTIFICATION_BLOB_LEN`, plus its own small fixed overhead),
/// the same class of decision as `MAX_BLOCK_BODY_BLOB_LEN` itself.
pub const MAX_FINALIZED_BLOCK_ENVELOPE_BLOB_LEN: usize = 20_000_000;

/// Maximum length, in bytes, of one embedded `commit_qc` blob — same
/// value as `hn_state::MAX_JUSTIFICATION_BLOB_LEN`: `commit_qc` is a
/// `QuorumCertificate` over the same active set a `justification` QC
/// would be, so it has the same real size characteristics.
pub const MAX_FINALIZED_BLOCK_QC_BLOB_LEN: usize = 1_048_576;

/// A durably-persisted finalized block (ADR-0044, "Decided: Durable
/// Finalized-Block Storage") — the pair a syncing peer needs to accept
/// history it was not present for: the block itself, and the real
/// `precommit` quorum certificate that proves `2f+1` of the active set
/// actually finalized it. Without `commit_qc`, a proposer's signature
/// alone would prove only that the proposer proposed something, never
/// that anyone else agreed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalizedBlock {
    /// Structure version for this shape.
    pub finalized_block_version: u16,
    /// This block's own canonical envelope.
    pub envelope: BlockEnvelope,
    /// The real `precommit` quorum certificate that finalized this
    /// block (ADR-0013's own "one precommit QC is sufficient" rule).
    pub commit_qc: QuorumCertificate,
}

impl FinalizedBlock {
    /// Encodes this value as canonical HNCS bytes. `envelope`/
    /// `commit_qc` each already have their own self-contained
    /// `encode`/`decode` (each ending in its own `Decoder::finish`), so
    /// neither can share this value's own decoder directly — each is
    /// embedded as one opaque, length-prefixed blob instead, the same
    /// pattern `BlockEnvelope::encode` itself already uses for
    /// `header`/`body`/`justification`.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, self.finalized_block_version);
        write_bytes(
            &mut out,
            &self.envelope.encode()?,
            MAX_FINALIZED_BLOCK_ENVELOPE_BLOB_LEN,
        )
        .map_err(StateError::Encoding)?;
        write_bytes(
            &mut out,
            &self.commit_qc.encode()?,
            MAX_FINALIZED_BLOCK_QC_BLOB_LEN,
        )
        .map_err(StateError::Encoding)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`FinalizedBlock::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let finalized_block_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if finalized_block_version != FINALIZED_BLOCK_VERSION_1 {
            return Err(StateError::UnsupportedFinalizedBlockVersion {
                value: finalized_block_version,
            });
        }
        let envelope_bytes = decoder
            .read_bytes(MAX_FINALIZED_BLOCK_ENVELOPE_BLOB_LEN)
            .map_err(StateError::Encoding)?;
        let envelope = BlockEnvelope::decode(envelope_bytes)?;
        let qc_bytes = decoder
            .read_bytes(MAX_FINALIZED_BLOCK_QC_BLOB_LEN)
            .map_err(StateError::Encoding)?;
        let commit_qc = QuorumCertificate::decode(qc_bytes)?;
        decoder.finish().map_err(StateError::Encoding)?;
        Ok(Self {
            finalized_block_version,
            envelope,
            commit_qc,
        })
    }
}

#[cfg(test)]
mod tests {
    use hn_core::{BlockHeight, Epoch, ProtocolEpoch, Round, UnixTimeMillis};
    use hn_crypto::SignatureEnvelope;

    use super::{FINALIZED_BLOCK_VERSION_1, FinalizedBlock};
    use crate::block_body::{BODY_VERSION_1, BlockBody};
    use crate::block_envelope::{BLOCK_VERSION_1, BlockEnvelope};
    use crate::block_header::{BlockHeader, HEADER_VERSION_1};
    use crate::error::{StateError, StateResult};
    use crate::list_merkle::list_empty_root;
    use crate::vote::{QuorumCertificate, VoteTargetType, VoteType};

    fn sample_body() -> BlockBody {
        BlockBody {
            body_version: BODY_VERSION_1,
            transactions: Vec::new(),
            receipts: Vec::new(),
            evidence: Vec::new(),
            extra_data: Vec::new(),
        }
    }

    fn sample_header(body: &BlockBody) -> StateResult<BlockHeader> {
        Ok(BlockHeader {
            header_version: HEADER_VERSION_1,
            chain_id: 1,
            network_id: 1,
            height: BlockHeight::new(1),
            round: Round::new(0),
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
        })
    }

    fn sample_envelope() -> StateResult<BlockEnvelope> {
        let body = sample_body();
        let header = sample_header(&body)?;
        Ok(BlockEnvelope {
            block_version: BLOCK_VERSION_1,
            header,
            body,
            justification: None,
        })
    }

    fn sample_qc(block_hash: [u8; 32]) -> QuorumCertificate {
        QuorumCertificate {
            certificate_type: VoteType::Precommit,
            chain_id: 1,
            network_id: 1,
            epoch: Epoch::new(0),
            height: BlockHeight::new(1),
            round: Round::new(0),
            validator_set_commitment: [0x11_u8; 32],
            target_type: VoteTargetType::Block,
            target_hash: block_hash,
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

    fn sample_finalized_block() -> StateResult<FinalizedBlock> {
        let envelope = sample_envelope()?;
        let block_hash = envelope.block_hash()?;
        Ok(FinalizedBlock {
            finalized_block_version: FINALIZED_BLOCK_VERSION_1,
            envelope,
            commit_qc: sample_qc(block_hash),
        })
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        let finalized_block = sample_finalized_block()?;
        let decoded = FinalizedBlock::decode(&finalized_block.encode()?)?;
        assert_eq!(decoded, finalized_block);
        Ok(())
    }

    #[test]
    fn rejects_an_unsupported_version() -> StateResult<()> {
        let finalized_block = sample_finalized_block()?;
        let mut bytes = Vec::new();
        hn_hncs::write_u16(&mut bytes, 2);
        bytes.extend(finalized_block.envelope.encode()?);
        bytes.extend(finalized_block.commit_qc.encode()?);
        assert_eq!(
            FinalizedBlock::decode(&bytes),
            Err(StateError::UnsupportedFinalizedBlockVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_bytes() {
        assert!(matches!(
            FinalizedBlock::decode(&[]),
            Err(StateError::Encoding(_))
        ));
    }

    #[test]
    fn rejects_truncated_bytes() -> StateResult<()> {
        let finalized_block = sample_finalized_block()?;
        let bytes = finalized_block.encode()?;
        assert!(matches!(
            FinalizedBlock::decode(&bytes[..bytes.len() - 1]),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }

    #[test]
    fn rejects_trailing_garbage_bytes() -> StateResult<()> {
        let finalized_block = sample_finalized_block()?;
        let mut bytes = finalized_block.encode()?;
        bytes.push(0xff);
        assert!(matches!(
            FinalizedBlock::decode(&bytes),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }

    #[test]
    fn rejects_pure_garbage_bytes() {
        assert!(matches!(
            FinalizedBlock::decode(&[0xff; 64]),
            Err(StateError::Encoding(_) | StateError::UnsupportedFinalizedBlockVersion { .. })
        ));
    }
}

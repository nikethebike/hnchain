use hn_crypto::Digest;
use hn_hncs::{Decoder, write_bool, write_bytes, write_u16};

use crate::block_body::BlockBody;
use crate::block_header::BlockHeader;
use crate::error::{StateError, StateResult};
use crate::list_merkle::list_empty_root;
use crate::vote::QuorumCertificate;

/// `block_version` for the current [`BlockEnvelope`] shape (ADR-0008,
/// "Decided: initial versions" — independent of `header_version`/
/// `body_version`, ADR-0022's own "Nested Structure Versions" rule).
pub const BLOCK_VERSION_1: u16 = 1;

/// Maximum length, in bytes, of one embedded `BlockHeader` blob.
/// `BlockHeader::encode` is currently fixed-width (well under 400
/// bytes: no variable-length field exists in it at all); this bound is
/// deliberately generous headroom over that, the same class of
/// decision as `hn_state::MAX_RECEIPT_BLOB_LEN`.
pub const MAX_BLOCK_HEADER_BLOB_LEN: usize = 1024;

/// Maximum length, in bytes, of one embedded `BlockBody` blob. An
/// implementation resource bound, not a policy-grade one —
/// `BlockBody`'s own `MAX_TRANSACTIONS_PER_BLOCK`/per-transaction
/// `MAX_TRANSACTION_SIZE` already bound its real worst case far above
/// this; this is a wire-frame safety cap, same relationship
/// `MAX_EVIDENCE_PER_BLOCK`/`MAX_EVIDENCE_BLOB_LEN` already have to
/// `docs/specs/core/block-format.md`'s own still-open "Decided: Block
/// Size And Transaction Count Limits" policy value.
pub const MAX_BLOCK_BODY_BLOB_LEN: usize = 16_777_216;

/// Maximum length, in bytes, of one embedded `justification`
/// `QuorumCertificate` blob. Same value and same class of decision as
/// `hn_network::MAX_QC_BLOB_LEN` (a QC's real size is bounded by active
/// set size, not block content).
pub const MAX_JUSTIFICATION_BLOB_LEN: usize = 1_048_576;

/// The canonical block envelope (ADR-0008, "Decision" — `BlockEnvelope`:
/// `block_version`/`header`/`body`/`justification`) — assembled here for
/// the first time as a concrete type, closing ADR-0008's own long-open
/// "Explicitly not attempted here: `BlockEnvelope`" item. Does not
/// decide anything new about `BlockHeader`/`BlockBody` themselves, only
/// wraps the two already-decided types together with the one remaining
/// conceptual field (`justification`) ADR-0008's own sketch names.
///
/// `justification` lives here, not as a sibling field on a network
/// message, matching ADR-0008's own conceptual shape exactly — a
/// `prevote` quorum certificate justifying this envelope's own value
/// under `hn_consensus::ConsensusState`'s "Locking rule," when present.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockEnvelope {
    /// Structure version for this envelope shape.
    pub block_version: u16,
    /// This block's canonical header.
    pub header: BlockHeader,
    /// This block's canonical body.
    pub body: BlockBody,
    /// A `prevote` quorum certificate justifying this envelope's own
    /// value, when present (`hn_consensus::ConsensusState`'s own
    /// "Locking rule").
    pub justification: Option<QuorumCertificate>,
}

impl BlockEnvelope {
    /// Encodes this envelope as canonical HNCS bytes. `header`/`body`/
    /// `justification` each already have their own self-contained
    /// `encode`/`decode` (each ending in its own `Decoder::finish`), so
    /// none of the three can share this envelope's own decoder directly
    /// — each is embedded as one opaque, length-prefixed blob instead,
    /// the same pattern `hn_network`'s own (now-superseded) embedding of
    /// a bare `justification` QC already established.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, self.block_version);
        write_bytes(&mut out, &self.header.encode()?, MAX_BLOCK_HEADER_BLOB_LEN)
            .map_err(StateError::Encoding)?;
        write_bytes(&mut out, &self.body.encode()?, MAX_BLOCK_BODY_BLOB_LEN)
            .map_err(StateError::Encoding)?;
        write_bool(&mut out, self.justification.is_some());
        if let Some(qc) = &self.justification {
            write_bytes(&mut out, &qc.encode()?, MAX_JUSTIFICATION_BLOB_LEN)
                .map_err(StateError::Encoding)?;
        }
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`BlockEnvelope::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let block_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if block_version != BLOCK_VERSION_1 {
            return Err(StateError::UnsupportedBlockEnvelopeVersion {
                value: block_version,
            });
        }
        let header_bytes = decoder
            .read_bytes(MAX_BLOCK_HEADER_BLOB_LEN)
            .map_err(StateError::Encoding)?;
        let header = BlockHeader::decode(header_bytes)?;
        let body_bytes = decoder
            .read_bytes(MAX_BLOCK_BODY_BLOB_LEN)
            .map_err(StateError::Encoding)?;
        let body = BlockBody::decode(body_bytes)?;
        let justification = if decoder.read_bool().map_err(StateError::Encoding)? {
            let qc_bytes = decoder
                .read_bytes(MAX_JUSTIFICATION_BLOB_LEN)
                .map_err(StateError::Encoding)?;
            Some(QuorumCertificate::decode(qc_bytes)?)
        } else {
            None
        };
        decoder.finish().map_err(StateError::Encoding)?;
        Ok(Self {
            block_version,
            header,
            body,
            justification,
        })
    }

    /// This envelope's own `block_hash` — delegates to
    /// [`BlockHeader::block_hash`].
    pub fn block_hash(&self) -> StateResult<Digest> {
        self.header.block_hash()
    }

    /// Verifies that every header field derivable from `body` content
    /// actually matches what `body` contains (ADR-0042, "Receiver-Side
    /// Header Verification") — a pure, local, no-external-state-needed
    /// check: it never resolves whether the header's *other* fields
    /// (`parent_block_hash`, `proposer`, `state_root`,
    /// `consensus_root`, `protocol_parameters_hash`, `timestamp`) are
    /// correct, since those need context (the active validator set, this
    /// node's own chain position, devnet-wide constants) this type does
    /// not have — that cross-checking is `hn-node`'s own job, same
    /// "assembles already-verified input, does not re-derive it"
    /// boundary this crate already draws elsewhere.
    pub fn verify_body_matches_header(&self) -> StateResult<()> {
        if self.header.transactions_root != self.body.transactions_root()? {
            return Err(StateError::BlockBodyHeaderMismatch {
                field: "transactions_root",
            });
        }
        if self.header.receipts_root != self.body.receipts_root()? {
            return Err(StateError::BlockBodyHeaderMismatch {
                field: "receipts_root",
            });
        }
        if self.header.evidence_root != self.body.evidence_root()? {
            return Err(StateError::BlockBodyHeaderMismatch {
                field: "evidence_root",
            });
        }
        if self.header.extra_data_hash != self.body.extra_data_hash()? {
            return Err(StateError::BlockBodyHeaderMismatch {
                field: "extra_data_hash",
            });
        }
        // `BlockBody` has no `events` field at all yet (HNVM-gated,
        // ADR-0008) -- `events_root` has exactly one valid value until
        // real events exist, same always-empty invariant every current
        // block already relies on.
        if self.header.events_root != list_empty_root()? {
            return Err(StateError::BlockBodyHeaderMismatch {
                field: "events_root",
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hn_core::{BlockHeight, Epoch, ProtocolEpoch, Round, UnixTimeMillis};

    use super::{BLOCK_VERSION_1, BlockEnvelope};
    use crate::block_body::{BODY_VERSION_1, BlockBody};
    use crate::block_header::{BlockHeader, HEADER_VERSION_1};
    use crate::error::{StateError, StateResult};
    use crate::list_merkle::list_empty_root;

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

    fn sample_body() -> BlockBody {
        BlockBody {
            body_version: BODY_VERSION_1,
            transactions: Vec::new(),
            receipts: Vec::new(),
            evidence: Vec::new(),
            extra_data: Vec::new(),
        }
    }

    #[test]
    fn round_trips_through_decode_with_no_justification() -> StateResult<()> {
        let body = sample_body();
        let header = sample_header(&body)?;
        let envelope = BlockEnvelope {
            block_version: BLOCK_VERSION_1,
            header,
            body,
            justification: None,
        };
        let decoded = BlockEnvelope::decode(&envelope.encode()?)?;
        assert_eq!(decoded, envelope);
        Ok(())
    }

    #[test]
    fn rejects_an_unsupported_block_version() -> StateResult<()> {
        let body = sample_body();
        let header = sample_header(&body)?;
        let mut bytes = Vec::new();
        hn_hncs::write_u16(&mut bytes, 2);
        bytes.extend(header.encode()?);
        bytes.extend(body.encode()?);
        hn_hncs::write_bool(&mut bytes, false);
        assert_eq!(
            BlockEnvelope::decode(&bytes),
            Err(StateError::UnsupportedBlockEnvelopeVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn block_hash_matches_the_headers_own() -> StateResult<()> {
        let body = sample_body();
        let header = sample_header(&body)?;
        let expected = header.block_hash()?;
        let envelope = BlockEnvelope {
            block_version: BLOCK_VERSION_1,
            header,
            body,
            justification: None,
        };
        assert_eq!(envelope.block_hash()?, expected);
        Ok(())
    }

    #[test]
    fn verify_body_matches_header_accepts_a_consistent_envelope() -> StateResult<()> {
        let body = sample_body();
        let header = sample_header(&body)?;
        let envelope = BlockEnvelope {
            block_version: BLOCK_VERSION_1,
            header,
            body,
            justification: None,
        };
        envelope.verify_body_matches_header()
    }

    #[test]
    fn verify_body_matches_header_rejects_a_tampered_transactions_root() -> StateResult<()> {
        let body = sample_body();
        let mut header = sample_header(&body)?;
        header.transactions_root = [0xff_u8; 32];
        let envelope = BlockEnvelope {
            block_version: BLOCK_VERSION_1,
            header,
            body,
            justification: None,
        };
        assert_eq!(
            envelope.verify_body_matches_header(),
            Err(StateError::BlockBodyHeaderMismatch {
                field: "transactions_root"
            })
        );
        Ok(())
    }
}

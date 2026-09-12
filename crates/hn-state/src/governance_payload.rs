use hn_crypto::Digest;
use hn_hncs::{Decoder, write_fixed_bytes, write_string, write_u8, write_u16};

use crate::error::{StateError, StateResult};

/// `payload_version` for the current `GovernancePayloadV1` shape
/// (ADR-0025, "Decided: Transaction Payload Shape").
pub const GOVERNANCE_PAYLOAD_VERSION_1: u16 = 1;

/// Maximum length, in bytes, of a `Propose` payload's `title`
/// (ADR-0025, "Decided: Proposal Content"). An implementation resource
/// bound picked with headroom, the same class of decision as
/// `MAX_ACCESS_LIST_ENTRIES` — not derived, chosen to match the
/// genesis message's own bound (`docs/specs/core/genesis.md` §4) in
/// spirit: a short, durable identifying string, not a place to inline
/// a proposal's full argument (that lives off-chain, referenced by
/// `content_hash`).
pub const GOVERNANCE_PROPOSAL_TITLE_MAX_LEN: usize = 256;

/// The `operation` registry (ADR-0025, "Decided: Transaction Payload
/// Shape"), closed for this payload version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GovernanceOperation {
    /// Creates a new proposal. Carries `title`/`content_hash`.
    Propose = 0x01,
    /// Casts a vote on an existing proposal. Carries
    /// `proposal_id`/`choice`.
    Vote = 0x02,
}

impl GovernanceOperation {
    fn from_u8(value: u8) -> StateResult<Self> {
        match value {
            0x01 => Ok(Self::Propose),
            0x02 => Ok(Self::Vote),
            _ => Err(StateError::InvalidGovernanceOperation { value }),
        }
    }

    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// The `choice` registry for a `Vote` payload (ADR-0025, "Decided:
/// Transaction Payload Shape"), closed for this payload version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum VoteChoice {
    /// Votes in favor of the proposal.
    For = 0x01,
    /// Votes against the proposal.
    Against = 0x02,
    /// Explicitly declines to take a side. Counts toward a chamber's
    /// quorum but not toward its `for`/`against` majority ratio
    /// (ADR-0025, "Decided: Chamber Pass Rule").
    Abstain = 0x03,
}

impl VoteChoice {
    pub(crate) fn from_u8(value: u8) -> StateResult<Self> {
        match value {
            0x01 => Ok(Self::For),
            0x02 => Ok(Self::Against),
            0x03 => Ok(Self::Abstain),
            _ => Err(StateError::InvalidVoteChoice { value }),
        }
    }

    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// `governance` (`tx_type = 0x07`) payload (ADR-0025, "Decided:
/// Transaction Payload Shape").
///
/// A Rust `enum`, not a flat struct with `Option` fields the way
/// `ValidatorUpdatePayloadV1` (ADR-0006) is — deliberately different
/// from that established precedent, not an inconsistency: `Propose`
/// and `Vote` carry two entirely disjoint field sets (unlike
/// `ValidatorUpdatePayloadV1`'s five operations, which share exactly
/// one optional field), so an `enum` makes an invalid combination
/// (for example a `Propose` with no `title`) unrepresentable at the
/// type level rather than needing a decode-time presence-mismatch
/// check. The **wire encoding** still matches ADR-0025's own literal
/// requirement — a plain discriminant (`operation`) followed
/// unconditionally by that operation's own fields, never a boxed
/// variant — `encode`/`decode` below produce and consume exactly that
/// layout; only this type's in-memory Rust shape differs from
/// `ValidatorUpdatePayloadV1`'s.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GovernancePayloadV1 {
    /// Creates a new proposal.
    Propose {
        /// A short, bounded, durable identifying title — not the
        /// proposal's full argument (see `content_hash`).
        title: String,
        /// A document commitment (ADR-0025, "Decided: Proposal
        /// Content") to the full proposal text, which lives off-chain.
        content_hash: Digest,
    },
    /// Casts a vote on an existing proposal.
    Vote {
        /// The proposal being voted on.
        proposal_id: Digest,
        /// The sender's choice.
        choice: VoteChoice,
    },
}

impl GovernancePayloadV1 {
    /// This payload's operation.
    #[must_use]
    pub const fn operation(&self) -> GovernanceOperation {
        match self {
            Self::Propose { .. } => GovernanceOperation::Propose,
            Self::Vote { .. } => GovernanceOperation::Vote,
        }
    }

    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, GOVERNANCE_PAYLOAD_VERSION_1);
        write_u8(&mut out, self.operation().as_u8());
        match self {
            Self::Propose {
                title,
                content_hash,
            } => {
                write_string(&mut out, title, GOVERNANCE_PROPOSAL_TITLE_MAX_LEN)
                    .map_err(StateError::Encoding)?;
                write_fixed_bytes(&mut out, content_hash);
            }
            Self::Vote {
                proposal_id,
                choice,
            } => {
                write_fixed_bytes(&mut out, proposal_id);
                write_u8(&mut out, choice.as_u8());
            }
        }
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`GovernancePayloadV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let payload_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if payload_version != GOVERNANCE_PAYLOAD_VERSION_1 {
            return Err(StateError::UnsupportedGovernancePayloadVersion {
                value: payload_version,
            });
        }

        let operation =
            GovernanceOperation::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        let payload = match operation {
            GovernanceOperation::Propose => {
                let title = decoder
                    .read_string(GOVERNANCE_PROPOSAL_TITLE_MAX_LEN)
                    .map_err(StateError::Encoding)?
                    .to_string();
                let content_hash = decoder
                    .read_fixed_bytes::<32>()
                    .map_err(StateError::Encoding)?;
                Self::Propose {
                    title,
                    content_hash,
                }
            }
            GovernanceOperation::Vote => {
                let proposal_id = decoder
                    .read_fixed_bytes::<32>()
                    .map_err(StateError::Encoding)?;
                let choice = VoteChoice::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
                Self::Vote {
                    proposal_id,
                    choice,
                }
            }
        };

        decoder.finish().map_err(StateError::Encoding)?;

        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::{GovernanceOperation, GovernancePayloadV1, StateError, VoteChoice};
    use crate::error::StateResult;

    const CONTENT_HASH: [u8; 32] = [0xaa; 32];
    const PROPOSAL_ID: [u8; 32] = [0xbb; 32];

    fn propose() -> GovernancePayloadV1 {
        GovernancePayloadV1::Propose {
            title: "Raise MAX_ACTIVE_SET_SIZE".to_string(),
            content_hash: CONTENT_HASH,
        }
    }

    fn vote() -> GovernancePayloadV1 {
        GovernancePayloadV1::Vote {
            proposal_id: PROPOSAL_ID,
            choice: VoteChoice::For,
        }
    }

    #[test]
    fn operation_reflects_variant() {
        assert_eq!(propose().operation(), GovernanceOperation::Propose);
        assert_eq!(vote().operation(), GovernanceOperation::Vote);
    }

    #[test]
    fn encodes_propose_matching_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&propose().encode()?),
            "010001190000005261697365204d41585f4143544956455f5345545f53495a45aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        Ok(())
    }

    #[test]
    fn encodes_vote_matching_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&vote().encode()?),
            "010002bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb01"
        );
        Ok(())
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        for payload in [propose(), vote()] {
            let decoded = GovernancePayloadV1::decode(&payload.encode()?)?;
            assert_eq!(decoded, payload);
        }
        Ok(())
    }

    #[test]
    fn rejects_unsupported_payload_version() -> StateResult<()> {
        let mut encoded = vote().encode()?;
        encoded[0] = 0x02; // payload_version low byte, little-endian
        assert_eq!(
            GovernancePayloadV1::decode(&encoded),
            Err(StateError::UnsupportedGovernancePayloadVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_operation() -> StateResult<()> {
        let mut encoded = vote().encode()?;
        encoded[2] = 0x09; // operation byte
        assert_eq!(
            GovernancePayloadV1::decode(&encoded),
            Err(StateError::InvalidGovernanceOperation { value: 0x09 })
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_vote_choice() -> StateResult<()> {
        let mut encoded = vote().encode()?;
        let last = encoded.len() - 1;
        encoded[last] = 0x09;
        assert_eq!(
            GovernancePayloadV1::decode(&encoded),
            Err(StateError::InvalidVoteChoice { value: 0x09 })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> StateResult<()> {
        let mut encoded = vote().encode()?;
        encoded.push(0x00);
        assert!(matches!(
            GovernancePayloadV1::decode(&encoded),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }

    #[test]
    fn rejects_oversized_title() {
        let payload = GovernancePayloadV1::Propose {
            title: "x".repeat(super::GOVERNANCE_PROPOSAL_TITLE_MAX_LEN + 1),
            content_hash: CONTENT_HASH,
        };
        assert!(matches!(payload.encode(), Err(StateError::Encoding(_))));
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

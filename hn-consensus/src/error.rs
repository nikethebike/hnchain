use std::fmt;

use hn_crypto::Digest;
use hn_state::StateError;

use crate::step::ConsensusStep;

/// Errors [`crate::ConsensusState::apply`]/[`crate::ConsensusEngine`]
/// can produce (ADR-0034, "Consensus State Machine Skeleton"; ADR-0035,
/// "Wiring The Consensus Engine To `hn-state`").
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConsensusError {
    /// `event` does not have a defined transition from `step` — the
    /// state machine's transition table (ADR-0034, "Decision") is a
    /// deliberately closed, total function: every unlisted (step,
    /// event) pair is rejected here rather than silently ignored.
    UnexpectedEvent {
        /// The step `event` was received in.
        step: ConsensusStep,
    },
    /// `height.checked_next()` overflowed advancing from `Finalize` to
    /// `NewHeight`.
    HeightOverflow,
    /// `round.checked_next()` overflowed advancing from `Precommit` to
    /// `Timeout`.
    RoundOverflow,
    /// [`hn_state::ConsensusVote::verify`] rejected a vote — bad
    /// signature or unknown signer.
    VoteInvalid(StateError),
    /// A vote's signer is not currently eligible
    /// ([`hn_state::is_eligible_signer`] returned `false`): not a
    /// member of the epoch-frozen active set, or jailed since.
    IneligibleSigner {
        /// The ineligible signer's `validator_id`.
        validator_id: Digest,
    },
    /// [`hn_state::QuorumCertificate::verify_signatures`] rejected a
    /// certificate.
    QuorumCertificateInvalid(StateError),
    /// [`crate::ConsensusEngine::commit_finalized_block`] was called
    /// while the engine's own state was not at
    /// [`crate::ConsensusStep::Finalize`] — committing needs the
    /// height that just finalized, which is only well-defined at that
    /// step.
    HeightNotFinalized {
        /// The step the engine was actually at.
        step: ConsensusStep,
    },
    /// [`crate::ConsensusEngine::commit_finalized_block`] was called
    /// for a `block_hash` this engine never cached a proposal's
    /// transactions for (via
    /// [`crate::ConsensusEngine::handle_proposal`]).
    UnknownProposedBlock {
        /// The block hash with no cached transaction list.
        block_hash: Digest,
    },
    /// A referenced validator has no stored record at all. Defensive:
    /// in [`crate::ConsensusEngine::verify_vote`], this can only fire
    /// if the record existed when `ConsensusVote::verify` resolved the
    /// signer's key moments earlier but is gone by the time this
    /// method's own lookup runs against the same reader — state that
    /// cannot actually change mid-call, kept as a typed error rather
    /// than unwrapped away per this workspace's no-panic discipline,
    /// not a path real callers should expect to hit.
    UnknownValidator {
        /// The missing validator's id.
        validator_id: Digest,
    },
    /// A real `hn-state` state-transition/storage operation failed —
    /// [`hn_state::fetch_validator_record`] or
    /// [`hn_state::apply_and_commit_block`].
    StateApplication(StateError),
}

impl fmt::Display for ConsensusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEvent { step } => {
                write!(formatter, "unexpected event for consensus step {step:?}")
            }
            Self::HeightOverflow => write!(formatter, "block height overflow"),
            Self::RoundOverflow => write!(formatter, "round overflow"),
            Self::VoteInvalid(error) => write!(formatter, "invalid consensus vote: {error}"),
            Self::IneligibleSigner { validator_id } => {
                write!(formatter, "ineligible signer: {validator_id:x?}")
            }
            Self::QuorumCertificateInvalid(error) => {
                write!(formatter, "invalid quorum certificate: {error}")
            }
            Self::HeightNotFinalized { step } => {
                write!(
                    formatter,
                    "height not finalized, engine is at step {step:?}"
                )
            }
            Self::UnknownProposedBlock { block_hash } => {
                write!(formatter, "no cached proposal for block {block_hash:x?}")
            }
            Self::UnknownValidator { validator_id } => {
                write!(formatter, "unknown validator {validator_id:x?}")
            }
            Self::StateApplication(error) => write!(formatter, "state application failed: {error}"),
        }
    }
}

impl std::error::Error for ConsensusError {}

/// Shorthand for `Result<T, ConsensusError>`.
pub type ConsensusResult<T> = Result<T, ConsensusError>;

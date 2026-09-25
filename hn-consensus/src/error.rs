use std::fmt;

use crate::step::ConsensusStep;

/// Errors [`crate::ConsensusState::apply`] can produce (ADR-0034,
/// "Consensus State Machine Skeleton").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
}

impl fmt::Display for ConsensusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEvent { step } => {
                write!(formatter, "unexpected event for consensus step {step:?}")
            }
            Self::HeightOverflow => write!(formatter, "block height overflow"),
            Self::RoundOverflow => write!(formatter, "round overflow"),
        }
    }
}

impl std::error::Error for ConsensusError {}

/// Shorthand for `Result<T, ConsensusError>`.
pub type ConsensusResult<T> = Result<T, ConsensusError>;

/// A round's position within the Tendermint-style state machine
/// (ADR-0009, "Timeout And View Change"; ADR-0034, "Consensus State
/// Machine Skeleton") — `NewHeight -> Propose -> Prevote -> Precommit
/// -> Finalize/Timeout`. `Finalize`/`Timeout` are real, directly
/// observable resting values here, not just names for an edge: a test
/// can assert `state.step == ConsensusStep::Finalize` immediately after
/// a qualifying `precommit` quorum, before anything decides what
/// happens next.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsensusStep {
    /// Freshly entered a height; not yet attempting a round.
    NewHeight,
    /// Waiting for a valid proposal (or its timeout) at the current
    /// round.
    Propose,
    /// Waiting for a `prevote` quorum (or its timeout) at the current
    /// round.
    Prevote,
    /// Waiting for a `precommit` quorum (or its timeout) at the current
    /// round.
    Precommit,
    /// A `precommit` quorum formed for a real block — this height is
    /// final.
    Finalize,
    /// No qualifying `precommit` quorum formed before timeout — this
    /// round failed; the next round (same height) is about to begin.
    Timeout,
}

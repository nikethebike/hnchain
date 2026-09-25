use hn_crypto::Digest;
use hn_state::QuorumCertificate;

/// What a `prevote`/`precommit` targets (ADR-0034) — the local mirror
/// of [`hn_state::VoteTargetType`]/`target_hash`'s pairing, used where
/// this crate names a target itself rather than reading one off an
/// already-built [`QuorumCertificate`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsensusTarget {
    /// A real proposed block.
    Block(Digest),
    /// No progress this round (ADR-0009, "Timeout And View Change").
    Nil,
}

/// An input to [`crate::ConsensusState::apply`] (ADR-0034, "Consensus
/// State Machine Skeleton").
///
/// Every `QuorumCertificate`-carrying variant is trusted as-is: `apply`
/// does not verify its signatures, recompute its quorum threshold, or
/// check validator eligibility — the caller does all of that before
/// constructing the event, the same "this layer trusts already-resolved
/// input" boundary [`hn_state::ConsensusVote::verify`]/
/// [`hn_state::QuorumCertificate::verify_signatures`] already draw one
/// layer down. This crate holds no signing key and performs no network
/// I/O; it decides only what state comes next and what the local
/// validator should do about it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConsensusEvent {
    /// Explicit request to leave `NewHeight`/`Timeout` and begin (or
    /// resume) attempting a round at `Propose` — not a network message,
    /// just the driving caller's own "ready to proceed" signal.
    BeginRound,
    /// A valid proposal was received for the current round.
    Proposal {
        /// The proposed block's hash.
        block_hash: Digest,
        /// A `prevote` quorum certificate for `block_hash`, if the
        /// proposer included one to justify re-proposing a value this
        /// validator may be locked on something else for (ADR-0034,
        /// "Locking rule").
        justification: Option<QuorumCertificate>,
    },
    /// The propose-stage timeout elapsed with no valid proposal
    /// observed.
    ProposeTimeout,
    /// A `prevote` quorum formed for some target this round.
    PrevoteQuorum(QuorumCertificate),
    /// The prevote-stage timeout elapsed with no qualifying quorum
    /// observed.
    PrevoteTimeout,
    /// A `precommit` quorum formed for some target this round.
    PrecommitQuorum(QuorumCertificate),
    /// The precommit-stage timeout elapsed with no qualifying quorum
    /// observed.
    PrecommitTimeout,
    /// Explicit request to leave `Finalize` and begin the next height.
    BeginNewHeight,
}

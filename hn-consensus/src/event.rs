use hn_core::{BlockHeight, Round};
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

/// An input to [`crate::ConsensusState::apply`] (ADR-0048, "Decided:
/// Events Added Or Changed").
///
/// Every `QuorumCertificate`-carrying variant is trusted as-is: `apply`
/// does not verify its signatures, recompute its quorum threshold, or
/// check validator eligibility — the caller does all of that before
/// constructing the event, the same "this layer trusts already-resolved
/// input" boundary [`hn_state::ConsensusVote::verify`]/
/// [`hn_state::QuorumCertificate::verify_signatures`] already draw one
/// layer down (ADR-0048, "Decided: Boundary"). This crate holds no
/// signing key and performs no network I/O; it decides only what state
/// comes next and what the local validator should do about it.
///
/// `Proposal`/`RoundSkip`/`*Timeout` each carry an explicit `height`
/// even though ADR-0048's own event table elides it: "Decided:
/// Boundary" requires the machine to check "each event's height equals
/// `self.height`" for every event, and a `QuorumCertificate`-carrying
/// event already has that height built in (`qc.height`) — these three
/// variants need their own field for the same check to apply to them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConsensusEvent {
    /// Explicit request to leave `NewHeight`/`Timeout` and begin (or
    /// resume) attempting a round at `Propose` — not a network message,
    /// just the driving caller's own "ready to proceed" signal. Carries
    /// no height/round: it is never stale, since the caller only ever
    /// sends it for whatever height/round this state machine is
    /// already at.
    BeginRound,
    /// A proposal for `round` of `height`.
    Proposal {
        /// The height this proposal is for.
        height: BlockHeight,
        /// The round this proposal is for.
        round: Round,
        /// The proposed block's hash.
        block_hash: Digest,
        /// The round at which the proposer itself saw a `prevote`
        /// polka for `block_hash`, if it is re-proposing its own
        /// `valid_value` rather than a fresh block (ADR-0048,
        /// "Decided: Proposer Duties").
        valid_round: Option<Round>,
        /// The `prevote` quorum certificate for `block_hash` at
        /// `valid_round`, present iff `valid_round` is (ADR-0048, R3).
        justification: Option<QuorumCertificate>,
        /// `true` only if the caller ran `verify_proposal` (including
        /// ADR-0045 execution) against `block_hash` and it succeeded.
        /// The machine takes this on trust (ADR-0048, "Decided:
        /// Boundary") — a pinned `hn-node` test must prove every
        /// proposal path sets it from the real verification result.
        validated: bool,
    },
    /// The propose-stage timeout elapsed with no valid proposal
    /// observed, for `round` of `height`.
    ProposeTimeout {
        /// The height this timeout is for.
        height: BlockHeight,
        /// The round this timeout is for.
        round: Round,
    },
    /// A `prevote` quorum formed for some target, at any round of the
    /// current height (ADR-0048: the machine itself decides whether a
    /// non-current round has any effect — see R6 — rather than the
    /// caller dropping it beforehand).
    PrevoteQuorum(QuorumCertificate),
    /// The prevote-stage timeout elapsed with no qualifying quorum
    /// observed, for `round` of `height`.
    PrevoteTimeout {
        /// The height this timeout is for.
        height: BlockHeight,
        /// The round this timeout is for.
        round: Round,
    },
    /// A `precommit` quorum formed for some target, at any round of
    /// the current height (ADR-0048, R8: "decide at any round").
    PrecommitQuorum(QuorumCertificate),
    /// The precommit-stage timeout elapsed with no qualifying quorum
    /// observed, for `round` of `height`.
    PrecommitTimeout {
        /// The height this timeout is for.
        height: BlockHeight,
        /// The round this timeout is for.
        round: Round,
    },
    /// Evidence that more than one third of voting power is already at
    /// `round`, a round later than this state machine's own current
    /// one, for `height` (ADR-0048, R10: "Round skipping"). The caller
    /// has already done the tally (any proposal, prevote or precommit,
    /// from distinct validators, counted once per validator per round)
    /// — this crate has no concept of validator identity or voting
    /// power.
    RoundSkip {
        /// The height this evidence is for.
        height: BlockHeight,
        /// The round more than a third of voting power is already at.
        round: Round,
    },
    /// A previously returned [`crate::ConsensusAction::NeedBlock`]'s
    /// block has now been fetched and validated (ADR-0044 sync),
    /// retrying the finalization [`ConsensusEvent::PrecommitQuorum`]
    /// could not complete at the time.
    BlockFetched {
        /// The block hash that is now available and validated.
        block_hash: Digest,
    },
    /// Explicit request to leave `Finalize` and begin the next height.
    BeginNewHeight,
}

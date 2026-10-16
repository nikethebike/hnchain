//! ADR-0048's own "Verification Requirements" item 5: an independent
//! second implementation of R1-R11 (written from the ADR's transition-
//! rule text alone, by an agent session explicitly forbidden from
//! reading `hn-consensus/src/state.rs`, `engine.rs`, or anything under
//! `hn-consensus/tests/`), diff-tested against the real
//! [`hn_consensus::ConsensusState`] on identical random schedules.
//!
//! `mod independent` below is that second implementation, pasted in
//! verbatim except for one deliberate adjustment (see its own comment
//! at `validated_blocks`): the delivered file tracked "does this
//! validator hold a validated proposal for v" with a single
//! round-keyed map, which cannot also answer "was v ever validated,
//! at any round" the way R8 needs — a BlockFetched for a hash that
//! does not match any pending NeedBlock was therefore silently
//! forgotten rather than remembered for a later PrecommitQuorum. The
//! real implementation already splits this into two fields
//! (`validated_block_hashes` + `round_validated_proposal`) for exactly
//! this reason; the fix below gives the independent copy the same
//! split rather than papering over the gap in this test's own
//! comparison logic.
//!
//! This diff run also found a genuine production bug: `apply_scoped`
//! in `hn-consensus/src/state.rs` treated a `Proposal`/`ProposeTimeout`/
//! `PrevoteTimeout`/`PrecommitTimeout` delivered at the wrong step as
//! `ConsensusError::UnexpectedEvent` (a caller-bug assertion), while
//! this independent implementation — written from the same prose —
//! treated it as a stale, harmless `NoEffect`. ADR-0048's own "Decided:
//! Certificate Routing In `hn-node`" explicitly wants the *machine*,
//! not the caller, to own round/step scoping once the caller-side
//! filter is loosened to drop only on wrong height (planned for phase
//! 4) — under that filter, a genuinely late `Proposal` (slow network)
//! or a timer that fired just before a `RoundSkip` moved this
//! validator on would reach `apply` with a stale step and crash the
//! call with an error instead of being quietly ignored, exactly like
//! the vote-quorum events already are. Fixed in `state.rs` to match;
//! see that file's own `apply_scoped` for the corrected arms.

mod independent {
    //! Independent reimplementation of a Tendermint-style consensus core state
    //! machine, written directly from a prose specification without consulting
    //! any other implementation. Pure in-memory state-transition logic: no I/O,
    //! no crypto, no `unsafe`, no dependencies beyond `std`.
    //!
    //! Judgment calls made where the spec left room (also called out inline
    //! near the relevant code):
    //!   * `ConsensusState` gained two fields beyond the ones the spec lists
    //!     verbatim (`known_proposals`, `pending_finalization`). The spec's
    //!     prose explicitly requires "remembering" a pending finalization
    //!     certificate (R8) and implicitly requires remembering which
    //!     (round, block_hash) proposals this validator has validated, since
    //!     R6/R8 both key off "this validator holds a validated proposal for
    //!     v [at round r]" and nothing else in the given struct can answer
    //!     that question.
    //!   * Events that originate from the network/timers (timeouts, quorum
    //!     certificates, RoundSkip, BlockFetched, Proposal) are treated as
    //!     routinely racy: if they don't match a rule for the current
    //!     (step, round), that's "no effect", not an error -- the same
    //!     treatment the spec itself gives explicitly to several of these
    //!     cases (stale PrecommitTimeout/PrecommitQuorum, stale RoundSkip,
    //!     non-current-round PrevoteQuorum). Events that are the *driving
    //!     caller's own explicit lifecycle signal* (BeginRound, BeginNewHeight)
    //!     are NOT given this leniency: calling them from the wrong step is a
    //!     caller bug and surfaces as `ConsensusError::NoTransition`.
    //!   * R3's justification-validity check originally added
    //!     `certificate_type == VoteType::Prevote` as defense-in-depth beyond
    //!     the spec's own literal checklist. **Reverted** (see this test
    //!     module's own doc comment): ADR-0048's actual R3 precondition list
    //!     is "present, targets v, `justification.round == vr_p`, `vr_p < r`"
    //!     — no certificate-type check — and the real implementation
    //!     doesn't have one either, so this was an unwarranted extra
    //!     restriction, not a gap in the real one.
    //!   * Retroactive R6 triggering (a validated proposal arriving *after* the
    //!     matching polka was already observed) is explicitly left unbuffered,
    //!     per the spec's own documented simplification option. A `Proposal`
    //!     event is still recorded into `known_proposals` regardless of the
    //!     validator's current step/round, so the far more common ordering
    //!     (proposal before polka) works correctly; only the fully-reversed
    //!     ordering is left unhandled.
    //!
    //! **Post-delivery fix (not part of the original independent work, see
    //! this test module's own doc comment for why):** added
    //! `validated_blocks: BTreeSet<Digest>`, populated by any validated
    //! `Proposal` (any round) and by `BlockFetched` unconditionally, used by
    //! R8 in place of scanning `known_proposals`'s values.

    #![allow(
        clippy::all,
        clippy::pedantic,
        clippy::nursery,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        dead_code,
        missing_docs
    )]
    // This module is a frozen oracle for diff-testing the real
    // implementation, not idiomatic project code -- it is deliberately
    // left unrestyled to this workspace's own clippy preferences.

    use std::collections::{BTreeSet, HashMap};

    // ---------------------------------------------------------------------
    // Primitive types
    // ---------------------------------------------------------------------

    /// Marker error for primitive arithmetic overflow; `ConsensusError::Overflow`
    /// is what callers of `apply` actually see.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Overflow;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct BlockHeight(u64);

    impl BlockHeight {
        pub fn new(h: u64) -> Self {
            BlockHeight(h)
        }

        pub fn get(self) -> u64 {
            self.0
        }

        pub fn checked_next(self) -> Result<Self, Overflow> {
            self.0.checked_add(1).map(BlockHeight).ok_or(Overflow)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Round(u64);

    impl Round {
        pub const FIRST: Round = Round(0);

        pub fn new(r: u64) -> Self {
            Round(r)
        }

        pub fn get(self) -> u64 {
            self.0
        }

        pub fn checked_next(self) -> Result<Self, Overflow> {
            self.0.checked_add(1).map(Round).ok_or(Overflow)
        }
    }

    /// `None` behaves as "no constraint" / less than any round -- per the
    /// spec's own warning not to trust `Option`'s derived `Ord` here. Used for
    /// the R3 unlock check `lr <= vr_p`.
    fn locked_round_permits(locked_round: Option<Round>, vr_p: Round) -> bool {
        match locked_round {
            None => true,
            Some(lr) => lr <= vr_p,
        }
    }

    pub type Digest = [u8; 32];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VoteTargetType {
        Block,
        Nil,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VoteType {
        Prevote,
        Precommit,
    }

    /// Opaque epoch marker; never inspected by this state machine.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Epoch(pub u64);

    /// An already-verified quorum certificate. Signatures, eligibility, and
    /// quorum-threshold arithmetic have already been checked by the caller
    /// before this is ever constructed -- this state machine trusts it
    /// completely and never re-derives any of that.
    #[derive(Debug, Clone)]
    pub struct QuorumCertificate {
        pub certificate_type: VoteType,
        pub chain_id: u8,
        pub network_id: u16,
        pub epoch: Epoch,
        pub height: BlockHeight,
        pub round: Round,
        pub validator_set_commitment: Digest,
        pub target_type: VoteTargetType,
        pub target_hash: Digest,
        pub total_voting_power: u128,
        pub signed_voting_power: u128,
        pub signer_commitment: Vec<u8>,
        pub aggregate_proof: Vec<u8>,
    }

    // ---------------------------------------------------------------------
    // Consensus state
    // ---------------------------------------------------------------------

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ConsensusStep {
        NewHeight,
        Propose,
        Prevote,
        Precommit,
        Finalize,
        Timeout,
    }

    #[derive(Debug, Clone)]
    pub struct ConsensusState {
        pub height: BlockHeight,
        pub round: Round,
        pub step: ConsensusStep,

        // What this validator is bound to. Set only by locking (R6).
        pub locked_value: Option<Digest>,
        pub locked_round: Option<Round>,

        // Latest value this validator has seen a polka for in this height.
        pub valid_value: Option<Digest>,
        pub valid_round: Option<Round>,
        pub valid_qc: Option<QuorumCertificate>,

        /// Validated proposals seen this height, keyed by the round they were
        /// proposed for -- used by R6's "for round r specifically" check.
        known_proposals: HashMap<Round, Digest>,

        /// Every block hash ever validated this height, at any round --
        /// used by R8's "any round" check. See this test module's own doc
        /// comment for why this is a separate field from `known_proposals`.
        validated_blocks: BTreeSet<Digest>,

        /// A precommit quorum for a block this validator could not finalize
        /// immediately because it lacked a matching validated proposal (R8's
        /// "NeedBlock" case). Cleared on entering a new height.
        pending_finalization: Option<(Digest, QuorumCertificate)>,
    }

    impl ConsensusState {
        pub fn new(height: BlockHeight) -> Self {
            ConsensusState {
                height,
                round: Round::FIRST,
                step: ConsensusStep::NewHeight,
                locked_value: None,
                locked_round: None,
                valid_value: None,
                valid_round: None,
                valid_qc: None,
                known_proposals: HashMap::new(),
                validated_blocks: BTreeSet::new(),
                pending_finalization: None,
            }
        }
    }

    // ---------------------------------------------------------------------
    // Events / actions / errors
    // ---------------------------------------------------------------------

    #[derive(Debug, Clone)]
    pub enum ConsensusEvent {
        /// Caller's own "ready to proceed" signal: leave NewHeight/Timeout and
        /// begin attempting a round at Propose. Carries no height/round -- the
        /// caller only ever sends it for whatever height/round this state
        /// machine is already at, so it can never be stale.
        BeginRound,
        Proposal {
            round: Round,
            block_hash: Digest,
            valid_round: Option<Round>,
            justification: Option<QuorumCertificate>,
            validated: bool,
        },
        ProposeTimeout {
            round: Round,
        },
        PrevoteQuorum(QuorumCertificate),
        PrevoteTimeout {
            round: Round,
        },
        PrecommitQuorum(QuorumCertificate),
        PrecommitTimeout {
            round: Round,
        },
        RoundSkip {
            round: Round,
        },
        BlockFetched {
            block_hash: Digest,
        },
        BeginNewHeight,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VoteTarget {
        Block(Digest),
        Nil,
    }

    #[derive(Debug, Clone)]
    pub enum ConsensusAction {
        /// Nothing to vote on right now, but this is simply the expected
        /// outcome of this step/event (as opposed to `NoEffect`).
        NoVote,
        Prevote(VoteTarget),
        Precommit(VoteTarget),
        Finalized {
            block_hash: Digest,
            certificate: QuorumCertificate,
        },
        /// A block is known to be final, but this validator has no matching
        /// validated proposal for it yet -- go fetch it.
        NeedBlock {
            block_hash: Digest,
            certificate: QuorumCertificate,
        },
        RoundFailed {
            new_round: Round,
        },
        NewHeight {
            height: BlockHeight,
        },
        /// Distinct from `NoVote`: this event was well-formed but, given the
        /// current state, had no effect at all (e.g. a stale or redundant
        /// vote/quorum/timeout).
        NoEffect,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ConsensusError {
        HeightMismatch {
            expected: BlockHeight,
            actual: BlockHeight,
        },
        NoTransition {
            step: ConsensusStep,
        },
        Overflow,
    }

    impl From<Overflow> for ConsensusError {
        fn from(_: Overflow) -> Self {
            ConsensusError::Overflow
        }
    }

    // ---------------------------------------------------------------------
    // Transition function
    // ---------------------------------------------------------------------

    impl ConsensusState {
        pub fn apply(&mut self, event: ConsensusEvent) -> Result<ConsensusAction, ConsensusError> {
            match event {
                ConsensusEvent::BeginRound => self.on_begin_round(),
                ConsensusEvent::Proposal {
                    round,
                    block_hash,
                    valid_round,
                    justification,
                    validated,
                } => self.on_proposal(round, block_hash, valid_round, justification, validated),
                ConsensusEvent::ProposeTimeout { round } => self.on_propose_timeout(round),
                ConsensusEvent::PrevoteQuorum(qc) => self.on_prevote_quorum(qc),
                ConsensusEvent::PrevoteTimeout { round } => self.on_prevote_timeout(round),
                ConsensusEvent::PrecommitQuorum(qc) => self.on_precommit_quorum(qc),
                ConsensusEvent::PrecommitTimeout { round } => self.on_precommit_timeout(round),
                ConsensusEvent::RoundSkip { round } => self.on_round_skip(round),
                ConsensusEvent::BlockFetched { block_hash } => self.on_block_fetched(block_hash),
                ConsensusEvent::BeginNewHeight => self.on_begin_new_height(),
            }
        }

        // R1.
        fn on_begin_round(&mut self) -> Result<ConsensusAction, ConsensusError> {
            match self.step {
                ConsensusStep::NewHeight | ConsensusStep::Timeout => {
                    self.step = ConsensusStep::Propose;
                    Ok(ConsensusAction::NoVote)
                }
                other => Err(ConsensusError::NoTransition { step: other }),
            }
        }

        // R2 (valid_round = None) and R3 (valid_round = Some(vr_p)).
        fn on_proposal(
            &mut self,
            round: Round,
            block_hash: Digest,
            valid_round: Option<Round>,
            justification: Option<QuorumCertificate>,
            validated: bool,
        ) -> Result<ConsensusAction, ConsensusError> {
            // Record regardless of our current step/round: R6/R8 only care
            // whether we *ever* validated a proposal matching a value (R8) or
            // a (round, value) pair (R6), not whether it arrived while we were
            // actively waiting on Propose. See module doc for the retroactive-
            // triggering simplification this does NOT attempt.
            if validated {
                self.known_proposals.insert(round, block_hash);
                self.validated_blocks.insert(block_hash);
            }

            if self.step != ConsensusStep::Propose || round != self.round {
                return Ok(ConsensusAction::NoEffect);
            }

            let vote = match valid_round {
                None => {
                    // R2
                    if validated
                        && (self.locked_round.is_none() || self.locked_value == Some(block_hash))
                    {
                        VoteTarget::Block(block_hash)
                    } else {
                        VoteTarget::Nil
                    }
                }
                Some(vr_p) => {
                    // R3
                    // No `certificate_type` check: ADR-0048's own R3
                    // precondition list doesn't require one, and neither
                    // does the real implementation -- see module doc.
                    let justification_ok = justification.as_ref().map_or(false, |qc| {
                        qc.target_type == VoteTargetType::Block
                            && qc.target_hash == block_hash
                            && qc.round == vr_p
                    });
                    let preconditions_ok = justification_ok && vr_p < round;

                    if preconditions_ok
                        && validated
                        && (locked_round_permits(self.locked_round, vr_p)
                            || self.locked_value == Some(block_hash))
                    {
                        VoteTarget::Block(block_hash)
                    } else {
                        VoteTarget::Nil
                    }
                }
            };

            self.step = ConsensusStep::Prevote;
            Ok(ConsensusAction::Prevote(vote))
        }

        // R4.
        fn on_propose_timeout(&mut self, round: Round) -> Result<ConsensusAction, ConsensusError> {
            if self.step != ConsensusStep::Propose || round != self.round {
                return Ok(ConsensusAction::NoEffect);
            }
            self.step = ConsensusStep::Prevote;
            Ok(ConsensusAction::Prevote(VoteTarget::Nil))
        }

        // R5 (nil polka) and R6 (block polka).
        fn on_prevote_quorum(
            &mut self,
            qc: QuorumCertificate,
        ) -> Result<ConsensusAction, ConsensusError> {
            if qc.height != self.height {
                return Err(ConsensusError::HeightMismatch {
                    expected: self.height,
                    actual: qc.height,
                });
            }
            if qc.round != self.round {
                // "A prevote quorum for a round that is NOT the current round:
                // has no effect, regardless of step." Applies to nil and block
                // targets alike.
                return Ok(ConsensusAction::NoEffect);
            }

            match qc.target_type {
                VoteTargetType::Nil => {
                    // R5
                    if self.step != ConsensusStep::Prevote {
                        return Ok(ConsensusAction::NoEffect);
                    }
                    self.step = ConsensusStep::Precommit;
                    Ok(ConsensusAction::Precommit(VoteTarget::Nil))
                }
                VoteTargetType::Block => {
                    // R6
                    let value = qc.target_hash;
                    let holds_matching_proposal =
                        self.known_proposals.get(&self.round) == Some(&value);
                    if !holds_matching_proposal {
                        return Ok(ConsensusAction::NoEffect);
                    }

                    match self.step {
                        ConsensusStep::Prevote => {
                            self.valid_value = Some(value);
                            self.valid_round = Some(self.round);
                            self.valid_qc = Some(qc);
                            self.locked_value = Some(value);
                            self.locked_round = Some(self.round);
                            self.step = ConsensusStep::Precommit;
                            Ok(ConsensusAction::Precommit(VoteTarget::Block(value)))
                        }
                        ConsensusStep::Precommit
                        | ConsensusStep::Finalize
                        | ConsensusStep::Timeout => {
                            let unchanged = self.valid_value == Some(value)
                                && self.valid_round == Some(self.round);
                            self.valid_value = Some(value);
                            self.valid_round = Some(self.round);
                            self.valid_qc = Some(qc);
                            if unchanged {
                                Ok(ConsensusAction::NoEffect)
                            } else {
                                Ok(ConsensusAction::NoVote)
                            }
                        }
                        ConsensusStep::NewHeight | ConsensusStep::Propose => {
                            Ok(ConsensusAction::NoEffect)
                        }
                    }
                }
            }
        }

        // R7.
        fn on_prevote_timeout(&mut self, round: Round) -> Result<ConsensusAction, ConsensusError> {
            if self.step != ConsensusStep::Prevote || round != self.round {
                return Ok(ConsensusAction::NoEffect);
            }
            self.step = ConsensusStep::Precommit;
            Ok(ConsensusAction::Precommit(VoteTarget::Nil))
        }

        // R8 (decide at any round / NeedBlock) and the nil-at-current-round
        // half of R8/R9.
        fn on_precommit_quorum(
            &mut self,
            qc: QuorumCertificate,
        ) -> Result<ConsensusAction, ConsensusError> {
            if qc.height != self.height {
                return Err(ConsensusError::HeightMismatch {
                    expected: self.height,
                    actual: qc.height,
                });
            }

            match qc.target_type {
                VoteTargetType::Nil => {
                    if qc.round != self.round || self.step != ConsensusStep::Precommit {
                        return Ok(ConsensusAction::NoEffect);
                    }
                    let new_round = self.round.checked_next()?;
                    self.round = new_round;
                    self.step = ConsensusStep::Timeout;
                    Ok(ConsensusAction::RoundFailed { new_round })
                }
                VoteTargetType::Block => {
                    let value = qc.target_hash;
                    let holds_proposal = self.validated_blocks.contains(&value);
                    if holds_proposal {
                        self.step = ConsensusStep::Finalize;
                        self.pending_finalization = None;
                        Ok(ConsensusAction::Finalized {
                            block_hash: value,
                            certificate: qc,
                        })
                    } else {
                        self.pending_finalization = Some((value, qc.clone()));
                        Ok(ConsensusAction::NeedBlock {
                            block_hash: value,
                            certificate: qc,
                        })
                    }
                }
            }
        }

        // R9 (timeout path only -- the nil-quorum path is in on_precommit_quorum).
        fn on_precommit_timeout(
            &mut self,
            round: Round,
        ) -> Result<ConsensusAction, ConsensusError> {
            if self.step != ConsensusStep::Precommit || round != self.round {
                return Ok(ConsensusAction::NoEffect);
            }
            let new_round = self.round.checked_next()?;
            self.round = new_round;
            self.step = ConsensusStep::Timeout;
            Ok(ConsensusAction::RoundFailed { new_round })
        }

        // R10.
        fn on_round_skip(&mut self, round: Round) -> Result<ConsensusAction, ConsensusError> {
            if round <= self.round {
                return Ok(ConsensusAction::NoEffect);
            }
            self.round = round;
            self.step = ConsensusStep::Timeout;
            Ok(ConsensusAction::RoundFailed { new_round: round })
        }

        // Second half of R8: a block this validator was waiting on has arrived.
        //
        // Fixed (see module doc): remembers *every* fetched block as
        // validated, not only one that matches a currently-pending
        // certificate, mirroring the real implementation's
        // `validated_block_hashes`.
        fn on_block_fetched(
            &mut self,
            block_hash: Digest,
        ) -> Result<ConsensusAction, ConsensusError> {
            self.validated_blocks.insert(block_hash);
            match self.pending_finalization.take() {
                Some((hash, qc)) if hash == block_hash => {
                    self.step = ConsensusStep::Finalize;
                    Ok(ConsensusAction::Finalized {
                        block_hash: hash,
                        certificate: qc,
                    })
                }
                other => {
                    self.pending_finalization = other;
                    Ok(ConsensusAction::NoEffect)
                }
            }
        }

        // R11.
        fn on_begin_new_height(&mut self) -> Result<ConsensusAction, ConsensusError> {
            if self.step != ConsensusStep::Finalize {
                return Err(ConsensusError::NoTransition { step: self.step });
            }
            let new_height = self.height.checked_next()?;
            self.height = new_height;
            self.round = Round::FIRST;
            self.step = ConsensusStep::NewHeight;
            self.locked_value = None;
            self.locked_round = None;
            self.valid_value = None;
            self.valid_round = None;
            self.valid_qc = None;
            self.known_proposals.clear();
            self.validated_blocks.clear();
            self.pending_finalization = None;
            Ok(ConsensusAction::NewHeight { height: new_height })
        }
    }
}

// -------------------------------------------------------------------------
// Diff-test harness
// -------------------------------------------------------------------------

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type Digest = [u8; 32];

const BLOCK_A: Digest = [0xAA; 32];
const BLOCK_B: Digest = [0xBB; 32];

struct Xorshift64(u64);

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x2545_f491_4f6c_dd1d)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AbstractTarget {
    Block(Digest),
    Nil,
}

#[derive(Debug, Clone)]
struct JustificationSpec {
    round: u64,
    target: AbstractTarget,
    is_prevote: bool,
}

#[derive(Debug, Clone)]
enum AbstractEvent {
    BeginRound,
    BeginNewHeight,
    Proposal {
        round: u64,
        block_hash: Digest,
        valid_round: Option<u64>,
        justification: Option<JustificationSpec>,
        validated: bool,
    },
    ProposeTimeout {
        round: u64,
    },
    PrevoteQuorum {
        round: u64,
        target: AbstractTarget,
    },
    PrevoteTimeout {
        round: u64,
    },
    PrecommitQuorum {
        round: u64,
        target: AbstractTarget,
    },
    PrecommitTimeout {
        round: u64,
    },
    RoundSkip {
        round: u64,
    },
    BlockFetched {
        block_hash: Digest,
    },
}

fn random_target(rng: &mut Xorshift64) -> AbstractTarget {
    match rng.next_below(3) {
        0 => AbstractTarget::Nil,
        1 => AbstractTarget::Block(BLOCK_A),
        _ => AbstractTarget::Block(BLOCK_B),
    }
}

fn random_block(rng: &mut Xorshift64) -> Digest {
    if rng.next_below(2) == 0 {
        BLOCK_A
    } else {
        BLOCK_B
    }
}

fn random_event(rng: &mut Xorshift64) -> AbstractEvent {
    let round = rng.next_below(4);
    match rng.next_below(10) {
        0 => AbstractEvent::BeginRound,
        1 => AbstractEvent::BeginNewHeight,
        2 => {
            let block_hash = random_block(rng);
            let validated = rng.next_below(2) == 0;
            let valid_round = if rng.next_below(2) == 0 {
                None
            } else {
                Some(rng.next_below(4))
            };
            let justification = valid_round.map(|vr| {
                let mismatched_round = rng.next_below(4) == 0;
                let mismatched_target = rng.next_below(4) == 0;
                JustificationSpec {
                    round: if mismatched_round {
                        vr.wrapping_add(1)
                    } else {
                        vr
                    },
                    target: if mismatched_target {
                        AbstractTarget::Block(if block_hash == BLOCK_A {
                            BLOCK_B
                        } else {
                            BLOCK_A
                        })
                    } else {
                        AbstractTarget::Block(block_hash)
                    },
                    is_prevote: rng.next_below(5) != 0,
                }
            });
            // Occasionally omit the justification even though valid_round is
            // Some, to exercise the "missing justification" invalid path.
            let justification = if valid_round.is_some() && rng.next_below(6) == 0 {
                None
            } else {
                justification
            };
            AbstractEvent::Proposal {
                round,
                block_hash,
                valid_round,
                justification,
                validated,
            }
        }
        3 => AbstractEvent::ProposeTimeout { round },
        4 => AbstractEvent::PrevoteQuorum {
            round,
            target: random_target(rng),
        },
        5 => AbstractEvent::PrevoteTimeout { round },
        6 => AbstractEvent::PrecommitQuorum {
            round,
            target: random_target(rng),
        },
        7 => AbstractEvent::PrecommitTimeout { round },
        8 => AbstractEvent::RoundSkip { round },
        _ => AbstractEvent::BlockFetched {
            block_hash: random_block(rng),
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
enum NormalizedAction {
    NoVote,
    Prevote(AbstractTarget),
    Precommit(AbstractTarget),
    Finalized(Digest),
    NeedBlock(Digest),
    RoundAdvanced(u64),
    NewHeight(u64),
    NoEffect,
}

fn real_vote_qc(
    height: hn_core::BlockHeight,
    round: u64,
    target: AbstractTarget,
    certificate_type: hn_state::VoteType,
) -> hn_state::QuorumCertificate {
    let (target_type, target_hash) = match target {
        AbstractTarget::Block(hash) => (hn_state::VoteTargetType::Block, hash),
        AbstractTarget::Nil => (hn_state::VoteTargetType::Nil, [0u8; 32]),
    };
    hn_state::QuorumCertificate {
        certificate_type,
        chain_id: 1,
        network_id: 1,
        epoch: hn_core::Epoch::new(0),
        height,
        round: hn_core::Round::new(round),
        validator_set_commitment: [0u8; 32],
        target_type,
        target_hash,
        total_voting_power: 100,
        signed_voting_power: 70,
        signer_commitment: Vec::new(),
        aggregate_proof: Vec::new(),
    }
}

fn real_justification(
    height: hn_core::BlockHeight,
    spec: &JustificationSpec,
) -> hn_state::QuorumCertificate {
    let certificate_type = if spec.is_prevote {
        hn_state::VoteType::Prevote
    } else {
        hn_state::VoteType::Precommit
    };
    real_vote_qc(height, spec.round, spec.target, certificate_type)
}

fn apply_real(
    state: &mut hn_consensus::ConsensusState,
    height: hn_core::BlockHeight,
    event: &AbstractEvent,
) -> Result<hn_consensus::ConsensusAction, hn_consensus::ConsensusError> {
    use hn_consensus::ConsensusEvent as E;
    use hn_core::Round;

    let real_event = match event {
        AbstractEvent::BeginRound => E::BeginRound,
        AbstractEvent::BeginNewHeight => E::BeginNewHeight,
        AbstractEvent::Proposal {
            round,
            block_hash,
            valid_round,
            justification,
            validated,
        } => E::Proposal {
            height,
            round: Round::new(*round),
            block_hash: *block_hash,
            valid_round: valid_round.map(Round::new),
            justification: justification
                .as_ref()
                .map(|spec| real_justification(height, spec)),
            validated: *validated,
        },
        AbstractEvent::ProposeTimeout { round } => E::ProposeTimeout {
            height,
            round: Round::new(*round),
        },
        AbstractEvent::PrevoteQuorum { round, target } => E::PrevoteQuorum(real_vote_qc(
            height,
            *round,
            *target,
            hn_state::VoteType::Prevote,
        )),
        AbstractEvent::PrevoteTimeout { round } => E::PrevoteTimeout {
            height,
            round: Round::new(*round),
        },
        AbstractEvent::PrecommitQuorum { round, target } => E::PrecommitQuorum(real_vote_qc(
            height,
            *round,
            *target,
            hn_state::VoteType::Precommit,
        )),
        AbstractEvent::PrecommitTimeout { round } => E::PrecommitTimeout {
            height,
            round: Round::new(*round),
        },
        AbstractEvent::RoundSkip { round } => E::RoundSkip {
            height,
            round: Round::new(*round),
        },
        AbstractEvent::BlockFetched { block_hash } => E::BlockFetched {
            block_hash: *block_hash,
        },
    };
    state.apply(real_event)
}

fn independent_justification(
    height: u64,
    spec: &JustificationSpec,
) -> independent::QuorumCertificate {
    let target = match spec.target {
        AbstractTarget::Block(hash) => (independent::VoteTargetType::Block, hash),
        AbstractTarget::Nil => (independent::VoteTargetType::Nil, [0u8; 32]),
    };
    independent::QuorumCertificate {
        certificate_type: if spec.is_prevote {
            independent::VoteType::Prevote
        } else {
            independent::VoteType::Precommit
        },
        chain_id: 1,
        network_id: 1,
        epoch: independent::Epoch(0),
        height: independent::BlockHeight::new(height),
        round: independent::Round::new(spec.round),
        validator_set_commitment: [0u8; 32],
        target_type: target.0,
        target_hash: target.1,
        total_voting_power: 100,
        signed_voting_power: 70,
        signer_commitment: Vec::new(),
        aggregate_proof: Vec::new(),
    }
}

fn independent_vote_qc(
    height: u64,
    round: u64,
    target: AbstractTarget,
    certificate_type: independent::VoteType,
) -> independent::QuorumCertificate {
    let (target_type, target_hash) = match target {
        AbstractTarget::Block(hash) => (independent::VoteTargetType::Block, hash),
        AbstractTarget::Nil => (independent::VoteTargetType::Nil, [0u8; 32]),
    };
    independent::QuorumCertificate {
        certificate_type,
        chain_id: 1,
        network_id: 1,
        epoch: independent::Epoch(0),
        height: independent::BlockHeight::new(height),
        round: independent::Round::new(round),
        validator_set_commitment: [0u8; 32],
        target_type,
        target_hash,
        total_voting_power: 100,
        signed_voting_power: 70,
        signer_commitment: Vec::new(),
        aggregate_proof: Vec::new(),
    }
}

/// `height` is the height *both* machines are already at when `event` is
/// applied -- the independent implementation's own events don't carry a
/// height field at all (see this file's module doc), but its
/// `QuorumCertificate`s still do, and must match `state.height` or
/// `on_prevote_quorum`/`on_precommit_quorum` reject them as a
/// `HeightMismatch`, exactly like the real implementation's own
/// `WrongHeight`. Passing a stale fixed height here (this harness's own
/// earlier bug, not a real divergence) made the real side correctly reject
/// a post-`BeginNewHeight` event the independent side wrongly accepted.
fn apply_independent(
    state: &mut independent::ConsensusState,
    height: u64,
    event: &AbstractEvent,
) -> Result<independent::ConsensusAction, independent::ConsensusError> {
    use independent::{ConsensusEvent as E, Round};

    let independent_event =
        match event {
            AbstractEvent::BeginRound => E::BeginRound,
            AbstractEvent::BeginNewHeight => E::BeginNewHeight,
            AbstractEvent::Proposal {
                round,
                block_hash,
                valid_round,
                justification,
                validated,
            } => E::Proposal {
                round: Round::new(*round),
                block_hash: *block_hash,
                valid_round: valid_round.map(Round::new),
                justification: justification
                    .as_ref()
                    .map(|spec| independent_justification(height, spec)),
                validated: *validated,
            },
            AbstractEvent::ProposeTimeout { round } => E::ProposeTimeout {
                round: Round::new(*round),
            },
            AbstractEvent::PrevoteQuorum { round, target } => E::PrevoteQuorum(
                independent_vote_qc(height, *round, *target, independent::VoteType::Prevote),
            ),
            AbstractEvent::PrevoteTimeout { round } => E::PrevoteTimeout {
                round: Round::new(*round),
            },
            AbstractEvent::PrecommitQuorum { round, target } => E::PrecommitQuorum(
                independent_vote_qc(height, *round, *target, independent::VoteType::Precommit),
            ),
            AbstractEvent::PrecommitTimeout { round } => E::PrecommitTimeout {
                round: Round::new(*round),
            },
            AbstractEvent::RoundSkip { round } => E::RoundSkip {
                round: Round::new(*round),
            },
            AbstractEvent::BlockFetched { block_hash } => E::BlockFetched {
                block_hash: *block_hash,
            },
        };
    state.apply(independent_event)
}

fn normalize_real(action: &hn_consensus::ConsensusAction) -> NormalizedAction {
    use hn_consensus::{ConsensusAction as A, ConsensusTarget as T};
    match action {
        A::None => NormalizedAction::NoVote,
        A::Prevote(T::Block(h)) => NormalizedAction::Prevote(AbstractTarget::Block(*h)),
        A::Prevote(T::Nil) => NormalizedAction::Prevote(AbstractTarget::Nil),
        A::Precommit(T::Block(h)) => NormalizedAction::Precommit(AbstractTarget::Block(*h)),
        A::Precommit(T::Nil) => NormalizedAction::Precommit(AbstractTarget::Nil),
        A::Finalized { block_hash, .. } => NormalizedAction::Finalized(*block_hash),
        A::NeedBlock { block_hash, .. } => NormalizedAction::NeedBlock(*block_hash),
        A::RoundAdvanced { round } => NormalizedAction::RoundAdvanced(round.get()),
        A::NewHeight { height } => NormalizedAction::NewHeight(height.get()),
        A::Ignored { .. } => NormalizedAction::NoEffect,
    }
}

fn normalize_independent(action: &independent::ConsensusAction) -> NormalizedAction {
    use independent::{ConsensusAction as A, VoteTarget as T};
    match action {
        A::NoVote => NormalizedAction::NoVote,
        A::Prevote(T::Block(h)) => NormalizedAction::Prevote(AbstractTarget::Block(*h)),
        A::Prevote(T::Nil) => NormalizedAction::Prevote(AbstractTarget::Nil),
        A::Precommit(T::Block(h)) => NormalizedAction::Precommit(AbstractTarget::Block(*h)),
        A::Precommit(T::Nil) => NormalizedAction::Precommit(AbstractTarget::Nil),
        A::Finalized { block_hash, .. } => NormalizedAction::Finalized(*block_hash),
        A::NeedBlock { block_hash, .. } => NormalizedAction::NeedBlock(*block_hash),
        A::RoundFailed { new_round } => NormalizedAction::RoundAdvanced(new_round.get()),
        A::NewHeight { height } => NormalizedAction::NewHeight(height.get()),
        A::NoEffect => NormalizedAction::NoEffect,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum NormalizedStep {
    NewHeight,
    Propose,
    Prevote,
    Precommit,
    Finalize,
    Timeout,
}

fn normalize_step_real(step: hn_consensus::ConsensusStep) -> NormalizedStep {
    use hn_consensus::ConsensusStep as S;
    match step {
        S::NewHeight => NormalizedStep::NewHeight,
        S::Propose => NormalizedStep::Propose,
        S::Prevote => NormalizedStep::Prevote,
        S::Precommit => NormalizedStep::Precommit,
        S::Finalize => NormalizedStep::Finalize,
        S::Timeout => NormalizedStep::Timeout,
    }
}

fn normalize_step_independent(step: independent::ConsensusStep) -> NormalizedStep {
    use independent::ConsensusStep as S;
    match step {
        S::NewHeight => NormalizedStep::NewHeight,
        S::Propose => NormalizedStep::Propose,
        S::Prevote => NormalizedStep::Prevote,
        S::Precommit => NormalizedStep::Precommit,
        S::Finalize => NormalizedStep::Finalize,
        S::Timeout => NormalizedStep::Timeout,
    }
}

fn states_match(real: &hn_consensus::ConsensusState, other: &independent::ConsensusState) -> bool {
    real.height.get() == other.height.get()
        && real.round.get() == other.round.get()
        && normalize_step_real(real.step) == normalize_step_independent(other.step)
        && real.locked_value == other.locked_value
        && real.locked_round.map(hn_core::Round::get)
            == other.locked_round.map(independent::Round::get)
        && real.valid_value == other.valid_value
        && real.valid_round.map(hn_core::Round::get)
            == other.valid_round.map(independent::Round::get)
        && real.valid_qc.is_some() == other.valid_qc.is_some()
}

const SEED_COUNT: u64 = 20_000;
const STEPS_PER_SCHEDULE: usize = 25;

fn run_one_schedule(seed: u64) -> TestResult {
    let mut rng = Xorshift64::new(seed);
    let mut real_state = hn_consensus::ConsensusState::new_height(hn_core::BlockHeight::new(1));
    let mut independent_state = independent::ConsensusState::new(independent::BlockHeight::new(1));

    for step_index in 0..STEPS_PER_SCHEDULE {
        // Both machines are kept in lockstep by the mismatch checks below,
        // so reading the live height off either one before building this
        // iteration's event is equivalent; `real_state` is picked
        // arbitrarily.
        let height = hn_core::BlockHeight::new(real_state.height.get());
        let event = random_event(&mut rng);
        let real_result = apply_real(&mut real_state, height, &event);
        let independent_result = apply_independent(&mut independent_state, height.get(), &event);

        match (real_result, independent_result) {
            (Ok(real_action), Ok(independent_action)) => {
                let real_normalized = normalize_real(&real_action);
                let independent_normalized = normalize_independent(&independent_action);
                if real_normalized != independent_normalized {
                    return Err(format!(
                        "seed {seed} step {step_index}: action mismatch for {event:?}: \
                         real={real_normalized:?} independent={independent_normalized:?}"
                    )
                    .into());
                }
                if !states_match(&real_state, &independent_state) {
                    return Err(format!(
                        "seed {seed} step {step_index}: state mismatch for {event:?}: \
                         real={real_state:?} independent={independent_state:?}"
                    )
                    .into());
                }
            }
            (Err(_), Err(_)) => {
                // Both rejected the event -- agreement, even though the two
                // error types differ (the spec mandates identical
                // accept/reject behavior and identical resulting state when
                // accepted, not an identical error taxonomy).
            }
            (other_real, other_independent) => {
                return Err(format!(
                    "seed {seed} step {step_index}: accept/reject mismatch for {event:?}: \
                     real={other_real:?} independent={other_independent:?}"
                )
                .into());
            }
        }
    }
    Ok(())
}

#[test]
fn independent_implementation_matches_the_real_one_on_random_schedules() -> TestResult {
    for seed in 0..SEED_COUNT {
        run_one_schedule(seed)?;
    }
    Ok(())
}

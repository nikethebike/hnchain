use std::collections::{BTreeMap, BTreeSet};

use hn_core::{BlockHeight, Round};
use hn_crypto::Digest;
use hn_state::{QuorumCertificate, VoteTargetType};

use crate::action::ConsensusAction;
use crate::error::{ConsensusError, ConsensusResult};
use crate::event::{ConsensusEvent, ConsensusTarget};
use crate::step::ConsensusStep;

/// One validator's local view of the Tendermint-style round state
/// machine at a given height (ADR-0048, "Decided: ConsensusState v2").
///
/// Replaces ADR-0034's merged `locked_block`/`highest_qc` with
/// Tendermint's own separate `(lockedValue, lockedRound)`/`(validValue,
/// validRound)` pairs, plus `valid_qc` (the polka itself, kept so a
/// proposer can re-attach it as `justification` when re-proposing —
/// ADR-0048, "Decided: Proposer Duties"). All five reset only on
/// entering a new height (R11); they survive round changes (R9, R10)
/// within one height untouched.
///
/// Three further fields are *not* part of the ADR's own struct
/// listing but are required by R6/R8's own prose, which must work
/// regardless of which of {proposal, polka, precommit quorum} this
/// validator receives first:
///
/// - `validated_block_hashes`: every block hash this validator has
///   validated a proposal for, at any round of the current height
///   (R8: "a validated proposal for v (**any round**)").
/// - `validated_proposals`: every `(round, block_hash)` pair this
///   validator has validated a proposal for this height (R6: "a
///   validated proposal for v for round r"), keyed by the round the
///   proposal was *for* — not reset on a round change, unlike an
///   earlier version of this field that tracked only the current
///   round and forgot a proposal that arrived before this validator's
///   own round caught up to it (found via ADR-0048's own Verification
///   Requirement 5, the independent-implementation diff test).
/// - `pending_finalization`: a precommit quorum this validator cannot
///   yet act on because it has no validated proposal for the block it
///   targets (R8's "need block" path). Cleared once
///   [`ConsensusEvent::BlockFetched`] matches it.
///
/// **Implementation choice, not dictated by the ADR text:** R6's own
/// "it records the polka (so a late proposal can still trigger R6)"
/// describes the *net effect* a caller must achieve, not a
/// requirement that this crate buffer an unmatched polka internally.
/// `apply` returns exactly one [`ConsensusAction`] per event; rather
/// than smuggle a second, synthetic action out of the `Proposal`
/// handler when a late-validated proposal unblocks an
/// already-received polka, this state machine requires the *caller*
/// to re-submit the same [`ConsensusEvent::PrevoteQuorum`] it already
/// holds (from its own vote pool) once it has validated the matching
/// proposal. The caller already has everything needed to do this
/// (the certificate it received, and the proposal it just validated)
/// without this crate retaining it. `validated_proposals` exists so
/// that re-submission — or the normal in-order delivery, or a
/// proposal that arrived early — all find a match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsensusState {
    /// The height this state machine is attempting to finalize.
    pub height: BlockHeight,
    /// The current round attempt within `height`.
    pub round: Round,
    /// Where in the round this state machine currently is.
    pub step: ConsensusStep,
    /// The value this validator is bound to protecting, if any. Set
    /// only by locking (R6, step `Prevote` exactly).
    pub locked_value: Option<Digest>,
    /// The round at which `locked_value` was locked. `Some` iff
    /// `locked_value` is.
    pub locked_round: Option<Round>,
    /// The most recent value this validator has seen a `prevote`
    /// polka for in this height, locked or not. Set by any valid
    /// polka seen in step >= `Prevote` (R6).
    pub valid_value: Option<Digest>,
    /// The round at which `valid_value`'s own polka formed. `Some`
    /// iff `valid_value` is.
    pub valid_round: Option<Round>,
    /// The polka itself that justified `valid_value`/`valid_round` —
    /// kept so a proposer re-proposing it can attach it as
    /// `justification` (R1). `Some` iff `valid_value` is.
    pub valid_qc: Option<QuorumCertificate>,

    validated_block_hashes: BTreeSet<Digest>,
    validated_proposals: BTreeMap<Round, Digest>,
    pending_finalization: Option<(Digest, QuorumCertificate)>,
}

impl ConsensusState {
    /// Starts a fresh height: round [`Round::FIRST`], step `NewHeight`,
    /// no lock, no valid value — none of them survive across heights,
    /// only within one (R11).
    #[must_use]
    pub fn new_height(height: BlockHeight) -> Self {
        Self {
            height,
            round: Round::FIRST,
            step: ConsensusStep::NewHeight,
            locked_value: None,
            locked_round: None,
            valid_value: None,
            valid_round: None,
            valid_qc: None,
            validated_block_hashes: BTreeSet::new(),
            validated_proposals: BTreeMap::new(),
            pending_finalization: None,
        }
    }

    /// Applies `event`, mutating `self` to the resulting state and
    /// returning what the local validator should do about it
    /// (ADR-0048, "Decided: Transition Rules" — R1 through R11 in
    /// full; R1 itself is the caller's own re-proposal behavior on
    /// entering `Propose`, not a transition this function owns).
    ///
    /// Trusts `event` completely: no signature verification, quorum
    /// recomputation, or validator-eligibility check happens here —
    /// that is the caller's job before constructing `event`
    /// (ADR-0048, "Decided: Boundary"). The machine does check that
    /// each event's own height matches `self.height`
    /// ([`ConsensusError::WrongHeight`]) and does its own round/step
    /// scoping (R6 current round only; R8, R10 any round of the
    /// current height) — narrower than ADR-0034's "apply trusts its
    /// event completely," which left all of that to the caller.
    pub fn apply(&mut self, event: ConsensusEvent) -> ConsensusResult<ConsensusAction> {
        let result = self.apply_inner(event);
        debug_assert!(
            self.invariants_hold(),
            "ConsensusState invariant violated: {self:?}"
        );
        result
    }

    fn apply_inner(&mut self, event: ConsensusEvent) -> ConsensusResult<ConsensusAction> {
        match event {
            ConsensusEvent::PrecommitQuorum(qc) if qc.target_type == VoteTargetType::Block => {
                self.decide_or_need_block(qc)
            }
            ConsensusEvent::PrecommitQuorum(qc) => self.handle_nil_precommit_quorum(qc),
            ConsensusEvent::BlockFetched { block_hash } => {
                self.retry_pending_finalization(block_hash)
            }
            ConsensusEvent::RoundSkip { height, round } => self.handle_round_skip(height, round),
            ConsensusEvent::PrevoteQuorum(qc) if qc.target_type == VoteTargetType::Block => {
                self.handle_block_polka(qc)
            }
            ConsensusEvent::PrevoteQuorum(qc) => self.handle_nil_prevote_quorum(qc),
            ConsensusEvent::Proposal {
                height,
                round,
                block_hash,
                valid_round,
                justification,
                validated,
            } => self.handle_proposal(
                height,
                round,
                block_hash,
                valid_round,
                justification,
                validated,
            ),
            other => self.apply_scoped(other),
        }
    }

    /// R2/R3's own bookkeeping-plus-vote. Routed directly from
    /// `apply_inner`, like every other event that must record
    /// something regardless of `self.step`/`self.round`, rather than
    /// through `apply_scoped`'s step-scoped table: R8's "a validated
    /// proposal for v, at any round" is pure historical memory, not
    /// conditioned on this proposal having arrived in order, so
    /// `validated_block_hashes` must grow on *every* `validated: true`
    /// proposal — including one that is otherwise stale (wrong round,
    /// or this validator has already left `Propose`) — or a validator
    /// that received a late-but-genuinely-validated proposal would
    /// wrongly `NeedBlock` a later `PrecommitQuorum` it already has the
    /// block for. Found via ADR-0048's own Verification Requirement 5
    /// (the independent-implementation diff test): only the vote-
    /// casting half (R2/R3 proper) stays scoped to `Propose` at the
    /// current round, same as before.
    fn handle_proposal(
        &mut self,
        height: BlockHeight,
        round: Round,
        block_hash: Digest,
        valid_round: Option<Round>,
        justification: Option<QuorumCertificate>,
        validated: bool,
    ) -> ConsensusResult<ConsensusAction> {
        self.check_height(height)?;
        if validated {
            self.validated_block_hashes.insert(block_hash);
            self.validated_proposals.insert(round, block_hash);
        }
        if self.step != ConsensusStep::Propose {
            return Ok(ConsensusAction::Ignored {
                reason: "proposal received while not in step Propose",
            });
        }
        if round != self.round {
            return Ok(ConsensusAction::Ignored {
                reason: "proposal for a round other than the current one",
            });
        }
        let target = self.decide_prevote_target(
            block_hash,
            round,
            valid_round,
            justification.as_ref(),
            validated,
        );
        self.step = ConsensusStep::Prevote;
        Ok(ConsensusAction::Prevote(target))
    }

    /// `BeginRound`/`ProposeTimeout`/`PrevoteTimeout`/
    /// `PrecommitTimeout`/`BeginNewHeight` — every event whose own
    /// handling is scoped to exactly `(self.step, self.round)` (R1's
    /// own `BeginRound` half, R4, R7, R9's timeout half, R11).
    /// `Proposal` (R2/R3) is handled separately, see
    /// [`Self::handle_proposal`].
    ///
    /// Only `BeginRound`/`BeginNewHeight` keep ADR-0034's original
    /// "closed, total function" treatment of an unlisted `(step,
    /// event)` pair as [`ConsensusError::UnexpectedEvent`] — the
    /// caller only ever sends either one when it already knows this
    /// machine is at the step that accepts it, so receiving one
    /// anywhere else is a caller bug, not a race. The three
    /// `*Timeout` variants get their own wrong-step arms further down
    /// returning [`ConsensusAction::Ignored`] instead: found via
    /// ADR-0048's own Verification Requirement 5 (an independent
    /// second implementation, diff-tested against this one) that
    /// treating a timer whose own round already moved on as a hard
    /// error — rather than the same `Ignored` every
    /// `QuorumCertificate`-carrying event already gets — contradicts
    /// "Decided: Certificate Routing In `hn-node`"'s own plan to drop
    /// the caller-side filter to height alone and let the machine own
    /// round/step scoping.
    fn apply_scoped(&mut self, event: ConsensusEvent) -> ConsensusResult<ConsensusAction> {
        match (self.step, event) {
            (ConsensusStep::NewHeight, ConsensusEvent::BeginRound)
            | (ConsensusStep::Timeout, ConsensusEvent::BeginRound) => {
                self.step = ConsensusStep::Propose;
                Ok(ConsensusAction::None)
            }

            (ConsensusStep::Propose, ConsensusEvent::ProposeTimeout { height, round }) => {
                self.check_height(height)?;
                if round != self.round {
                    return Ok(ConsensusAction::Ignored {
                        reason: "timeout for a round other than the current one",
                    });
                }
                self.step = ConsensusStep::Prevote;
                Ok(ConsensusAction::Prevote(ConsensusTarget::Nil))
            }

            (ConsensusStep::Prevote, ConsensusEvent::PrevoteTimeout { height, round }) => {
                self.check_height(height)?;
                if round != self.round {
                    return Ok(ConsensusAction::Ignored {
                        reason: "timeout for a round other than the current one",
                    });
                }
                self.step = ConsensusStep::Precommit;
                Ok(ConsensusAction::Precommit(ConsensusTarget::Nil))
            }

            (ConsensusStep::Precommit, ConsensusEvent::PrecommitTimeout { height, round }) => {
                self.check_height(height)?;
                if round != self.round {
                    return Ok(ConsensusAction::Ignored {
                        reason: "timeout for a round other than the current one",
                    });
                }
                self.advance_round()
            }

            (ConsensusStep::Finalize, ConsensusEvent::BeginNewHeight) => {
                let next_height = self
                    .height
                    .checked_next()
                    .map_err(|_| ConsensusError::HeightOverflow)?;
                *self = Self::new_height(next_height);
                Ok(ConsensusAction::NewHeight {
                    height: next_height,
                })
            }

            // A `*Timeout` delivered at a step other than the one it is
            // scoped to is a timer whose own round already moved on
            // (ADR-0048, "Decided: Certificate Routing In `hn-node`": once
            // the caller-side filter drops only on wrong height, the
            // *machine* owns round/step scoping) — not a caller bug, unlike
            // `BeginRound`/`BeginNewHeight` below, which the caller only
            // ever sends when it already knows this machine is at the
            // right step. `Ignored`, not `UnexpectedEvent`, matching the
            // treatment every `QuorumCertificate`-carrying event already
            // gets.
            (_, ConsensusEvent::ProposeTimeout { height, .. }) => {
                self.check_height(height)?;
                Ok(ConsensusAction::Ignored {
                    reason: "propose timeout received while not in step Propose",
                })
            }
            (_, ConsensusEvent::PrevoteTimeout { height, .. }) => {
                self.check_height(height)?;
                Ok(ConsensusAction::Ignored {
                    reason: "prevote timeout received while not in step Prevote",
                })
            }
            (_, ConsensusEvent::PrecommitTimeout { height, .. }) => {
                self.check_height(height)?;
                Ok(ConsensusAction::Ignored {
                    reason: "precommit timeout received while not in step Precommit",
                })
            }

            (step, _) => Err(ConsensusError::UnexpectedEvent { step }),
        }
    }

    /// R9's timeout/nil-quorum half and R10's round-skip half share
    /// this: `round = round + 1` (or, for round skip, `round =
    /// round'`), `step = Timeout`, `locked_*`/`valid_*` untouched
    /// (the entire reason locking exists). `validated_proposals` is
    /// keyed by round and never needs clearing here — see that
    /// field's own doc comment.
    fn advance_round(&mut self) -> ConsensusResult<ConsensusAction> {
        let next_round = self
            .round
            .checked_next()
            .map_err(|_| ConsensusError::RoundOverflow)?;
        self.round = next_round;
        self.step = ConsensusStep::Timeout;
        Ok(ConsensusAction::RoundAdvanced { round: next_round })
    }

    /// R10. `round` must be strictly ahead of `self.round` — the
    /// caller is the one that tallies ">1/3 of voting power already
    /// at `round`" (this crate has no concept of validator identity
    /// or voting power), so a non-forward `round` here is simply
    /// stale, not an error.
    fn handle_round_skip(
        &mut self,
        height: BlockHeight,
        round: Round,
    ) -> ConsensusResult<ConsensusAction> {
        self.check_height(height)?;
        if round.get() <= self.round.get() {
            return Ok(ConsensusAction::Ignored {
                reason: "round skip target is not ahead of the current round",
            });
        }
        self.round = round;
        self.step = ConsensusStep::Timeout;
        Ok(ConsensusAction::RoundAdvanced { round })
    }

    /// R5's nil half: any `prevote` quorum for nil, at the current
    /// round, while in step `Prevote` — a quorum for a round this
    /// machine has already moved past is stale, not an error
    /// (ADR-0048, "Decided: Certificate Routing In `hn-node`":
    /// `Ignored` is distinct from a dropped/rejected event).
    fn handle_nil_prevote_quorum(
        &mut self,
        qc: QuorumCertificate,
    ) -> ConsensusResult<ConsensusAction> {
        self.check_height(qc.height)?;
        if qc.round != self.round || self.step != ConsensusStep::Prevote {
            return Ok(ConsensusAction::Ignored {
                reason: "nil prevote quorum outside the current prevote step",
            });
        }
        self.step = ConsensusStep::Precommit;
        Ok(ConsensusAction::Precommit(ConsensusTarget::Nil))
    }

    /// R9's nil-quorum half: a `precommit` quorum for nil at the
    /// current round advances the round exactly like its timeout
    /// twin. Stale (wrong round or step) is `Ignored`, not an error.
    fn handle_nil_precommit_quorum(
        &mut self,
        qc: QuorumCertificate,
    ) -> ConsensusResult<ConsensusAction> {
        self.check_height(qc.height)?;
        if qc.round != self.round || self.step != ConsensusStep::Precommit {
            return Ok(ConsensusAction::Ignored {
                reason: "nil precommit quorum outside the current precommit step",
            });
        }
        self.advance_round()
    }

    /// R6. A block-targeting `prevote` polka at the current round,
    /// while step is `Prevote` or later. Stale (wrong round, or seen
    /// before this round ever reached `Prevote`) is `Ignored`. If
    /// this validator holds no validated proposal yet for the
    /// polka's own target, it is also `Ignored` — see this struct's
    /// own documentation for why this crate does not buffer the
    /// polka itself, unlike the ADR's own prose.
    fn handle_block_polka(&mut self, qc: QuorumCertificate) -> ConsensusResult<ConsensusAction> {
        self.check_height(qc.height)?;
        if qc.round != self.round {
            return Ok(ConsensusAction::Ignored {
                reason: "prevote polka for a round other than the current one",
            });
        }
        if !Self::is_at_or_past_prevote(self.step) {
            return Ok(ConsensusAction::Ignored {
                reason: "prevote polka received before this round reached prevote",
            });
        }
        if self.validated_proposals.get(&self.round) == Some(&qc.target_hash) {
            Ok(self.apply_polka(qc))
        } else {
            Ok(ConsensusAction::Ignored {
                reason: "no validated proposal yet for this polka's own target; \
                         the caller must re-submit this certificate once it \
                         validates a matching proposal",
            })
        }
    }

    const fn is_at_or_past_prevote(step: ConsensusStep) -> bool {
        matches!(
            step,
            ConsensusStep::Prevote
                | ConsensusStep::Precommit
                | ConsensusStep::Finalize
                | ConsensusStep::Timeout
        )
    }

    /// R6's own effect, shared by [`Self::handle_block_polka`]: always
    /// updates `valid_value`/`valid_round`/`valid_qc`; locks and
    /// precommits only if `step` is `Prevote` exactly (the first time
    /// this round's own polka is observed at that step); otherwise
    /// only the valid-update happens, with no vote to cast.
    fn apply_polka(&mut self, qc: QuorumCertificate) -> ConsensusAction {
        let already_recorded =
            self.valid_value == Some(qc.target_hash) && self.valid_round == Some(qc.round);
        self.valid_value = Some(qc.target_hash);
        self.valid_round = Some(qc.round);
        self.valid_qc = Some(qc.clone());

        if self.step == ConsensusStep::Prevote {
            self.locked_value = Some(qc.target_hash);
            self.locked_round = Some(qc.round);
            self.step = ConsensusStep::Precommit;
            ConsensusAction::Precommit(ConsensusTarget::Block(qc.target_hash))
        } else if already_recorded {
            ConsensusAction::Ignored {
                reason: "valid value already recorded for this round",
            }
        } else {
            ConsensusAction::None
        }
    }

    /// R8. A block-targeting `precommit` quorum, at any round of the
    /// current height, decided independently of `self.step`/
    /// `self.round` — this is the one event this state machine acts
    /// on no matter where it currently is in its own round, since a
    /// precommit quorum is final proof regardless.
    fn decide_or_need_block(&mut self, qc: QuorumCertificate) -> ConsensusResult<ConsensusAction> {
        self.check_height(qc.height)?;
        let block_hash = qc.target_hash;
        if self.validated_block_hashes.contains(&block_hash) {
            self.step = ConsensusStep::Finalize;
            // Clears any stale `NeedBlock` left over from an earlier
            // `PrecommitQuorum` this validator could not yet act on —
            // found via ADR-0048's own Verification Requirement 5 (the
            // independent-implementation diff test): without this, an
            // unrelated later `BlockFetched` for that earlier pending
            // hash would still match and replay a second, redundant
            // `Finalized` action after this height is already done.
            self.pending_finalization = None;
            Ok(ConsensusAction::Finalized {
                block_hash,
                commit_qc: qc,
            })
        } else {
            self.pending_finalization = Some((block_hash, qc.clone()));
            Ok(ConsensusAction::NeedBlock {
                block_hash,
                commit_qc: qc,
            })
        }
    }

    /// Completes a [`Self::decide_or_need_block`] that returned
    /// `NeedBlock`, once the caller has fetched and validated the
    /// block (ADR-0044 sync) and reports it back.
    fn retry_pending_finalization(
        &mut self,
        block_hash: Digest,
    ) -> ConsensusResult<ConsensusAction> {
        self.validated_block_hashes.insert(block_hash);
        match self.pending_finalization.take() {
            Some((hash, qc)) if hash == block_hash => {
                self.step = ConsensusStep::Finalize;
                Ok(ConsensusAction::Finalized {
                    block_hash: hash,
                    commit_qc: qc,
                })
            }
            other => {
                self.pending_finalization = other;
                Ok(ConsensusAction::Ignored {
                    reason: "no pending finalization matches this block",
                })
            }
        }
    }

    fn check_height(&self, height: BlockHeight) -> ConsensusResult<()> {
        if height == self.height {
            Ok(())
        } else {
            Err(ConsensusError::WrongHeight {
                expected: self.height,
                actual: height,
            })
        }
    }

    /// `Propose` step's own locking rule (ADR-0048, R2 and R3):
    ///
    /// - No polka justification (R2): prevote `block_hash` if
    ///   `validated` and this validator is either unlocked or already
    ///   locked on exactly `block_hash`; otherwise prevote nil.
    /// - A polka justification (R3): first checks the justification
    ///   itself is a block-targeting polka for `block_hash`, at
    ///   exactly `valid_round`, with `valid_round` strictly before
    ///   `round` — any failure here means nil regardless of locking.
    ///   If it holds, prevote `block_hash` when `validated` and
    ///   (unlocked, or `locked_round <= valid_round`, or already
    ///   locked on `block_hash`); otherwise nil.
    fn decide_prevote_target(
        &self,
        block_hash: Digest,
        round: Round,
        valid_round: Option<Round>,
        justification: Option<&QuorumCertificate>,
        validated: bool,
    ) -> ConsensusTarget {
        match valid_round {
            None => {
                if validated && self.lock_allows(block_hash) {
                    ConsensusTarget::Block(block_hash)
                } else {
                    ConsensusTarget::Nil
                }
            }
            Some(vr_p) => {
                let justification_valid = justification.is_some_and(|qc| {
                    qc.target_type == VoteTargetType::Block
                        && qc.target_hash == block_hash
                        && qc.round == vr_p
                        && vr_p.get() < round.get()
                });
                if !justification_valid {
                    return ConsensusTarget::Nil;
                }
                let unlocks = self.locked_round.is_none()
                    || self.locked_round.is_some_and(|lr| lr.get() <= vr_p.get())
                    || self.locked_value == Some(block_hash);
                if validated && unlocks {
                    ConsensusTarget::Block(block_hash)
                } else {
                    ConsensusTarget::Nil
                }
            }
        }
    }

    fn lock_allows(&self, block_hash: Digest) -> bool {
        self.locked_value.is_none() || self.locked_value == Some(block_hash)
    }

    /// ADR-0048, "Decided: ConsensusState v2" invariants — checked by
    /// `debug_assert` after every `apply`, and independently by
    /// property tests (the simulator, a later pass).
    fn invariants_hold(&self) -> bool {
        if self.locked_value.is_some() != self.locked_round.is_some() {
            return false;
        }
        if self.valid_value.is_some() != self.valid_round.is_some() {
            return false;
        }
        if self.valid_value.is_some() != self.valid_qc.is_some() {
            return false;
        }
        if let (Some(locked_round), Some(valid_round)) = (self.locked_round, self.valid_round)
            && locked_round.get() > valid_round.get()
        {
            return false;
        }
        if let (Some(valid_value), Some(valid_round), Some(valid_qc)) =
            (self.valid_value, self.valid_round, &self.valid_qc)
        {
            if valid_qc.target_type != VoteTargetType::Block {
                return false;
            }
            if valid_qc.target_hash != valid_value {
                return false;
            }
            if valid_qc.round != valid_round {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use hn_core::{BlockHeight, Epoch, Round};
    use hn_state::{VoteTargetType, VoteType};

    use super::{ConsensusState, ConsensusStep, ConsensusTarget, QuorumCertificate};
    use crate::action::ConsensusAction;
    use crate::error::ConsensusError;
    use crate::event::ConsensusEvent;

    const HEIGHT: u64 = 1;
    const BLOCK_A: [u8; 32] = [0xaa; 32];
    const BLOCK_B: [u8; 32] = [0xbb; 32];

    fn height() -> BlockHeight {
        BlockHeight::new(HEIGHT)
    }

    fn qc(
        certificate_type: VoteType,
        target_type: VoteTargetType,
        target_hash: [u8; 32],
        round: u64,
    ) -> QuorumCertificate {
        QuorumCertificate {
            certificate_type,
            chain_id: 1,
            network_id: 1,
            epoch: Epoch::new(0),
            height: height(),
            round: Round::new(round),
            validator_set_commitment: [0x11; 32],
            target_type,
            target_hash,
            total_voting_power: 300,
            signed_voting_power: 300,
            signer_commitment: vec![],
            aggregate_proof: vec![],
        }
    }

    fn block_qc(certificate_type: VoteType, block: [u8; 32], round: u64) -> QuorumCertificate {
        qc(certificate_type, VoteTargetType::Block, block, round)
    }

    fn nil_qc(certificate_type: VoteType, round: u64) -> QuorumCertificate {
        qc(certificate_type, VoteTargetType::Nil, [0; 32], round)
    }

    fn fresh() -> ConsensusState {
        ConsensusState::new_height(height())
    }

    fn proposal(round: u64, block_hash: [u8; 32], validated: bool) -> ConsensusEvent {
        ConsensusEvent::Proposal {
            height: height(),
            round: Round::new(round),
            block_hash,
            valid_round: None,
            justification: None,
            validated,
        }
    }

    fn proposal_with_justification(
        round: u64,
        block_hash: [u8; 32],
        valid_round: u64,
        justification: QuorumCertificate,
        validated: bool,
    ) -> ConsensusEvent {
        ConsensusEvent::Proposal {
            height: height(),
            round: Round::new(round),
            block_hash,
            valid_round: Some(Round::new(valid_round)),
            justification: Some(justification),
            validated,
        }
    }

    // ---- new_height / R11 ----

    #[test]
    fn new_height_starts_fresh() {
        let state = fresh();
        assert_eq!(state.height, height());
        assert_eq!(state.round, Round::FIRST);
        assert_eq!(state.step, ConsensusStep::NewHeight);
        assert_eq!(state.locked_value, None);
        assert_eq!(state.locked_round, None);
        assert_eq!(state.valid_value, None);
        assert_eq!(state.valid_round, None);
        assert_eq!(state.valid_qc, None);
    }

    #[test]
    fn r11_new_height_resets_lock_and_valid_state() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Finalize;
        state.locked_value = Some(BLOCK_A);
        state.locked_round = Some(Round::new(2));
        state.valid_value = Some(BLOCK_A);
        state.valid_round = Some(Round::new(2));
        state.valid_qc = Some(block_qc(VoteType::Prevote, BLOCK_A, 2));
        state.round = Round::new(3);

        let action = state.apply(ConsensusEvent::BeginNewHeight)?;
        assert_eq!(
            action,
            ConsensusAction::NewHeight {
                height: BlockHeight::new(HEIGHT + 1)
            }
        );
        assert_eq!(state.height, BlockHeight::new(HEIGHT + 1));
        assert_eq!(state.round, Round::FIRST);
        assert_eq!(state.step, ConsensusStep::NewHeight);
        assert_eq!(state.locked_value, None);
        assert_eq!(state.locked_round, None);
        assert_eq!(state.valid_value, None);
        assert_eq!(state.valid_round, None);
        assert_eq!(state.valid_qc, None);
        Ok(())
    }

    // ---- R2: proposal without a polka ----

    #[test]
    fn r2_unlocked_validator_prevotes_a_validated_proposal() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;

        let action = state.apply(proposal(0, BLOCK_A, true))?;
        assert_eq!(
            action,
            ConsensusAction::Prevote(ConsensusTarget::Block(BLOCK_A))
        );
        assert_eq!(state.step, ConsensusStep::Prevote);
        Ok(())
    }

    #[test]
    fn r2_unvalidated_proposal_prevotes_nil_even_when_unlocked() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;

        let action = state.apply(proposal(0, BLOCK_A, false))?;
        assert_eq!(action, ConsensusAction::Prevote(ConsensusTarget::Nil));
        Ok(())
    }

    #[test]
    fn r2_locked_validator_prevotes_the_same_value_again() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.round = Round::new(1);
        state.locked_value = Some(BLOCK_A);
        state.locked_round = Some(Round::new(0));
        state.valid_value = Some(BLOCK_A);
        state.valid_round = Some(Round::new(0));
        state.valid_qc = Some(block_qc(VoteType::Prevote, BLOCK_A, 0));

        let action = state.apply(proposal(1, BLOCK_A, true))?;
        assert_eq!(
            action,
            ConsensusAction::Prevote(ConsensusTarget::Block(BLOCK_A))
        );
        Ok(())
    }

    /// Named negative case from ADR-0048's own Verification
    /// Requirements: "lock protects against a different value."
    #[test]
    fn r2_lock_protects_against_a_different_unjustified_value() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.round = Round::new(1);
        state.locked_value = Some(BLOCK_A);
        state.locked_round = Some(Round::new(0));
        state.valid_value = Some(BLOCK_A);
        state.valid_round = Some(Round::new(0));
        state.valid_qc = Some(block_qc(VoteType::Prevote, BLOCK_A, 0));

        let action = state.apply(proposal(1, BLOCK_B, true))?;
        assert_eq!(action, ConsensusAction::Prevote(ConsensusTarget::Nil));
        // The lock itself is untouched by a rejected re-proposal.
        assert_eq!(state.locked_value, Some(BLOCK_A));
        Ok(())
    }

    // ---- R3: proposal with a polka ----

    #[test]
    fn r3_newer_justifying_polka_unlocks_the_vote_but_not_the_lock() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.round = Round::new(2);
        state.locked_value = Some(BLOCK_A);
        state.locked_round = Some(Round::new(0));
        state.valid_value = Some(BLOCK_A);
        state.valid_round = Some(Round::new(0));
        state.valid_qc = Some(block_qc(VoteType::Prevote, BLOCK_A, 0));

        let justification = block_qc(VoteType::Prevote, BLOCK_B, 1);
        let action = state.apply(proposal_with_justification(
            2,
            BLOCK_B,
            1,
            justification,
            true,
        ))?;
        assert_eq!(
            action,
            ConsensusAction::Prevote(ConsensusTarget::Block(BLOCK_B))
        );
        // Named negative case: the vote unlocked, but `locked_value`
        // itself only ever changes via R6 actually locking -- it did
        // not change just because this round's own vote was cast for
        // a different value.
        assert_eq!(state.locked_value, Some(BLOCK_A));
        assert_eq!(state.locked_round, Some(Round::new(0)));
        Ok(())
    }

    #[test]
    fn r3_stale_justifying_polka_does_not_unlock() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.round = Round::new(2);
        state.locked_value = Some(BLOCK_A);
        state.locked_round = Some(Round::new(1));
        state.valid_value = Some(BLOCK_A);
        state.valid_round = Some(Round::new(1));
        state.valid_qc = Some(block_qc(VoteType::Prevote, BLOCK_A, 1));

        // Justification is for an earlier round than the current lock.
        let justification = block_qc(VoteType::Prevote, BLOCK_B, 0);
        let action = state.apply(proposal_with_justification(
            2,
            BLOCK_B,
            0,
            justification,
            true,
        ))?;
        assert_eq!(action, ConsensusAction::Prevote(ConsensusTarget::Nil));
        Ok(())
    }

    #[test]
    fn r3_justification_targeting_a_different_block_is_rejected() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.round = Round::new(1);

        // Justification targets BLOCK_A, but the proposal is for BLOCK_B.
        let justification = block_qc(VoteType::Prevote, BLOCK_A, 0);
        let action = state.apply(proposal_with_justification(
            1,
            BLOCK_B,
            0,
            justification,
            true,
        ))?;
        assert_eq!(action, ConsensusAction::Prevote(ConsensusTarget::Nil));
        Ok(())
    }

    #[test]
    fn r3_justification_round_not_before_the_proposed_round_is_rejected()
    -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.round = Round::new(1);

        // valid_round claims 1, same as the round being proposed --
        // must be strictly earlier.
        let justification = block_qc(VoteType::Prevote, BLOCK_A, 1);
        let action = state.apply(proposal_with_justification(
            1,
            BLOCK_A,
            1,
            justification,
            true,
        ))?;
        assert_eq!(action, ConsensusAction::Prevote(ConsensusTarget::Nil));
        Ok(())
    }

    #[test]
    fn r3_unlocked_validator_prevotes_a_validated_reproposal() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.round = Round::new(1);

        let justification = block_qc(VoteType::Prevote, BLOCK_A, 0);
        let action = state.apply(proposal_with_justification(
            1,
            BLOCK_A,
            0,
            justification,
            true,
        ))?;
        assert_eq!(
            action,
            ConsensusAction::Prevote(ConsensusTarget::Block(BLOCK_A))
        );
        Ok(())
    }

    // ---- R4 ----

    #[test]
    fn r4_propose_timeout_casts_a_nil_prevote() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;

        let action = state.apply(ConsensusEvent::ProposeTimeout {
            height: height(),
            round: Round::FIRST,
        })?;
        assert_eq!(action, ConsensusAction::Prevote(ConsensusTarget::Nil));
        assert_eq!(state.step, ConsensusStep::Prevote);
        Ok(())
    }

    // ---- R5 ----

    #[test]
    fn r5_nil_prevote_quorum_casts_a_nil_precommit() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Prevote;

        let action = state.apply(ConsensusEvent::PrevoteQuorum(nil_qc(VoteType::Prevote, 0)))?;
        assert_eq!(action, ConsensusAction::Precommit(ConsensusTarget::Nil));
        assert_eq!(state.locked_value, None);
        assert_eq!(state.valid_value, None);
        Ok(())
    }

    // ---- R6 ----

    #[test]
    fn r6_polka_with_a_validated_proposal_locks_and_precommits() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.apply(proposal(0, BLOCK_A, true))?;
        assert_eq!(state.step, ConsensusStep::Prevote);

        let action = state.apply(ConsensusEvent::PrevoteQuorum(block_qc(
            VoteType::Prevote,
            BLOCK_A,
            0,
        )))?;
        assert_eq!(
            action,
            ConsensusAction::Precommit(ConsensusTarget::Block(BLOCK_A))
        );
        assert_eq!(state.step, ConsensusStep::Precommit);
        assert_eq!(state.locked_value, Some(BLOCK_A));
        assert_eq!(state.locked_round, Some(Round::new(0)));
        assert_eq!(state.valid_value, Some(BLOCK_A));
        assert_eq!(state.valid_round, Some(Round::new(0)));
        Ok(())
    }

    /// Named negative case: "polka without a validated proposal does
    /// not lock."
    #[test]
    fn r6_polka_without_a_validated_proposal_does_not_lock() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Prevote;
        // No Proposal event was ever applied this round -- no
        // validated proposal is on record.

        let action = state.apply(ConsensusEvent::PrevoteQuorum(block_qc(
            VoteType::Prevote,
            BLOCK_A,
            0,
        )))?;
        assert_eq!(
            action,
            ConsensusAction::Ignored {
                reason: "no validated proposal yet for this polka's own target; \
                         the caller must re-submit this certificate once it \
                         validates a matching proposal",
            }
        );
        assert_eq!(state.locked_value, None);
        assert_eq!(state.valid_value, None);
        assert_eq!(state.step, ConsensusStep::Prevote);
        Ok(())
    }

    /// Named negative case: "valid update after step moved on" --
    /// once step has already advanced past `Prevote`, a late-but-
    /// matching polka still updates `valid_value`/`valid_round`, but
    /// never locks and never casts a vote.
    #[test]
    fn r6_valid_update_after_step_moved_on_does_not_lock_or_vote() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.apply(proposal(0, BLOCK_A, true))?;
        // Prevote timeout fires before the polka arrives: step moves
        // on to Precommit with a nil vote already cast.
        state.apply(ConsensusEvent::PrevoteTimeout {
            height: height(),
            round: Round::FIRST,
        })?;
        assert_eq!(state.step, ConsensusStep::Precommit);
        assert_eq!(state.locked_value, None);

        let action = state.apply(ConsensusEvent::PrevoteQuorum(block_qc(
            VoteType::Prevote,
            BLOCK_A,
            0,
        )))?;
        assert_eq!(action, ConsensusAction::None);
        assert_eq!(state.step, ConsensusStep::Precommit);
        assert_eq!(state.locked_value, None);
        assert_eq!(state.valid_value, Some(BLOCK_A));
        assert_eq!(state.valid_round, Some(Round::new(0)));
        Ok(())
    }

    #[test]
    fn r6_repeating_the_same_valid_update_is_ignored() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.apply(proposal(0, BLOCK_A, true))?;
        state.apply(ConsensusEvent::PrevoteTimeout {
            height: height(),
            round: Round::FIRST,
        })?;
        state.apply(ConsensusEvent::PrevoteQuorum(block_qc(
            VoteType::Prevote,
            BLOCK_A,
            0,
        )))?;

        let action = state.apply(ConsensusEvent::PrevoteQuorum(block_qc(
            VoteType::Prevote,
            BLOCK_A,
            0,
        )))?;
        assert_eq!(
            action,
            ConsensusAction::Ignored {
                reason: "valid value already recorded for this round"
            }
        );
        Ok(())
    }

    #[test]
    fn r6_polka_for_a_non_current_round_is_ignored() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Prevote;
        state.round = Round::new(2);

        let action = state.apply(ConsensusEvent::PrevoteQuorum(block_qc(
            VoteType::Prevote,
            BLOCK_A,
            1,
        )))?;
        assert_eq!(
            action,
            ConsensusAction::Ignored {
                reason: "prevote polka for a round other than the current one"
            }
        );
        Ok(())
    }

    // ---- R7 ----

    #[test]
    fn r7_prevote_timeout_casts_a_nil_precommit() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Prevote;

        let action = state.apply(ConsensusEvent::PrevoteTimeout {
            height: height(),
            round: Round::FIRST,
        })?;
        assert_eq!(action, ConsensusAction::Precommit(ConsensusTarget::Nil));
        assert_eq!(state.step, ConsensusStep::Precommit);
        Ok(())
    }

    // ---- R8 ----

    #[test]
    fn r8_precommit_quorum_with_a_validated_proposal_finalizes_immediately()
    -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.apply(proposal(0, BLOCK_A, true))?;

        let commit_qc = block_qc(VoteType::Precommit, BLOCK_A, 0);
        let action = state.apply(ConsensusEvent::PrecommitQuorum(commit_qc.clone()))?;
        assert_eq!(
            action,
            ConsensusAction::Finalized {
                block_hash: BLOCK_A,
                commit_qc
            }
        );
        assert_eq!(state.step, ConsensusStep::Finalize);
        Ok(())
    }

    #[test]
    fn r8_precommit_quorum_at_an_earlier_round_finalizes_a_late_validator()
    -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        // Validate the proposal at round 0, but round 0 times out
        // all the way to round 3 locally without this validator ever
        // hearing a precommit quorum for it in real time.
        state.apply(proposal(0, BLOCK_A, true))?;
        state.apply(ConsensusEvent::PrevoteTimeout {
            height: height(),
            round: Round::new(0),
        })?;
        state.apply(ConsensusEvent::PrecommitTimeout {
            height: height(),
            round: Round::new(0),
        })?;
        state.apply(ConsensusEvent::BeginRound)?;
        state.apply(ConsensusEvent::ProposeTimeout {
            height: height(),
            round: Round::new(1),
        })?;
        state.apply(ConsensusEvent::PrevoteTimeout {
            height: height(),
            round: Round::new(1),
        })?;
        state.apply(ConsensusEvent::PrecommitTimeout {
            height: height(),
            round: Round::new(1),
        })?;
        state.apply(ConsensusEvent::BeginRound)?;
        state.apply(ConsensusEvent::ProposeTimeout {
            height: height(),
            round: Round::new(2),
        })?;
        state.apply(ConsensusEvent::PrevoteTimeout {
            height: height(),
            round: Round::new(2),
        })?;
        state.apply(ConsensusEvent::PrecommitTimeout {
            height: height(),
            round: Round::new(2),
        })?;
        state.apply(ConsensusEvent::BeginRound)?;
        assert_eq!(state.round, Round::new(3));
        assert_eq!(state.step, ConsensusStep::Propose);

        // The rest of the network finalized round 0's own value while
        // this validator kept timing out locally (e.g. a network
        // partition that has since healed).
        let commit_qc = block_qc(VoteType::Precommit, BLOCK_A, 0);
        let action = state.apply(ConsensusEvent::PrecommitQuorum(commit_qc.clone()))?;
        assert_eq!(
            action,
            ConsensusAction::Finalized {
                block_hash: BLOCK_A,
                commit_qc
            }
        );
        assert_eq!(state.step, ConsensusStep::Finalize);
        Ok(())
    }

    #[test]
    fn r8_precommit_quorum_without_the_proposal_asks_for_the_block() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;

        let commit_qc = block_qc(VoteType::Precommit, BLOCK_A, 0);
        let action = state.apply(ConsensusEvent::PrecommitQuorum(commit_qc.clone()))?;
        assert_eq!(
            action,
            ConsensusAction::NeedBlock {
                block_hash: BLOCK_A,
                commit_qc: commit_qc.clone()
            }
        );
        // Not finalized yet -- still waiting on the block.
        assert_eq!(state.step, ConsensusStep::Propose);

        let action = state.apply(ConsensusEvent::BlockFetched {
            block_hash: BLOCK_A,
        })?;
        assert_eq!(
            action,
            ConsensusAction::Finalized {
                block_hash: BLOCK_A,
                commit_qc
            }
        );
        assert_eq!(state.step, ConsensusStep::Finalize);
        Ok(())
    }

    #[test]
    fn r8_block_fetched_for_an_unrelated_block_is_ignored() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        let commit_qc = block_qc(VoteType::Precommit, BLOCK_A, 0);
        state.apply(ConsensusEvent::PrecommitQuorum(commit_qc))?;

        let action = state.apply(ConsensusEvent::BlockFetched {
            block_hash: BLOCK_B,
        })?;
        assert_eq!(
            action,
            ConsensusAction::Ignored {
                reason: "no pending finalization matches this block"
            }
        );
        assert_eq!(state.step, ConsensusStep::Propose);
        Ok(())
    }

    // ---- R9 ----

    #[test]
    fn r9_nil_precommit_quorum_advances_the_round_without_clearing_the_lock()
    -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Precommit;
        state.locked_value = Some(BLOCK_A);
        state.locked_round = Some(Round::FIRST);
        state.valid_value = Some(BLOCK_A);
        state.valid_round = Some(Round::FIRST);
        state.valid_qc = Some(block_qc(VoteType::Prevote, BLOCK_A, 0));

        let action = state.apply(ConsensusEvent::PrecommitQuorum(nil_qc(
            VoteType::Precommit,
            0,
        )))?;
        assert_eq!(
            action,
            ConsensusAction::RoundAdvanced {
                round: Round::new(1)
            }
        );
        assert_eq!(state.step, ConsensusStep::Timeout);
        assert_eq!(state.round, Round::new(1));
        // The whole point of locking: it survives a failed round.
        assert_eq!(state.locked_value, Some(BLOCK_A));
        assert_eq!(state.valid_value, Some(BLOCK_A));

        let action = state.apply(ConsensusEvent::BeginRound)?;
        assert_eq!(action, ConsensusAction::None);
        assert_eq!(state.step, ConsensusStep::Propose);
        assert_eq!(state.round, Round::new(1));
        Ok(())
    }

    #[test]
    fn r9_precommit_timeout_advances_the_round() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Precommit;

        let action = state.apply(ConsensusEvent::PrecommitTimeout {
            height: height(),
            round: Round::FIRST,
        })?;
        assert_eq!(
            action,
            ConsensusAction::RoundAdvanced {
                round: Round::new(1)
            }
        );
        assert_eq!(state.step, ConsensusStep::Timeout);
        Ok(())
    }

    // ---- R10 ----

    #[test]
    fn r10_round_skip_jumps_forward() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;

        let action = state.apply(ConsensusEvent::RoundSkip {
            height: height(),
            round: Round::new(5),
        })?;
        assert_eq!(
            action,
            ConsensusAction::RoundAdvanced {
                round: Round::new(5)
            }
        );
        assert_eq!(state.round, Round::new(5));
        assert_eq!(state.step, ConsensusStep::Timeout);
        Ok(())
    }

    #[test]
    fn r10_round_skip_does_not_release_a_lock() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.locked_value = Some(BLOCK_A);
        state.locked_round = Some(Round::FIRST);

        state.apply(ConsensusEvent::RoundSkip {
            height: height(),
            round: Round::new(5),
        })?;
        assert_eq!(state.locked_value, Some(BLOCK_A));
        Ok(())
    }

    /// The machine's own half of "round skip needs more than one
    /// third": a target round that is not strictly ahead of the
    /// current one is simply stale. The actual ">1/3 of voting
    /// power" tally is the caller's own job (this crate has no
    /// concept of validator identity or voting power) and belongs to
    /// the simulator, not here.
    #[test]
    fn r10_round_skip_to_a_non_forward_round_is_ignored() -> Result<(), ConsensusError> {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        state.round = Round::new(2);

        let action = state.apply(ConsensusEvent::RoundSkip {
            height: height(),
            round: Round::new(2),
        })?;
        assert_eq!(
            action,
            ConsensusAction::Ignored {
                reason: "round skip target is not ahead of the current round"
            }
        );
        assert_eq!(state.round, Round::new(2));
        Ok(())
    }

    // ---- Boundary / WrongHeight ----

    #[test]
    fn wrong_height_proposal_is_rejected() {
        let mut state = fresh();
        state.step = ConsensusStep::Propose;
        let wrong_height = BlockHeight::new(HEIGHT + 1);
        let action = state.apply(ConsensusEvent::Proposal {
            height: wrong_height,
            round: Round::FIRST,
            block_hash: BLOCK_A,
            valid_round: None,
            justification: None,
            validated: true,
        });
        assert_eq!(
            action,
            Err(ConsensusError::WrongHeight {
                expected: height(),
                actual: wrong_height
            })
        );
    }

    #[test]
    fn wrong_height_precommit_quorum_is_rejected() {
        let mut state = fresh();
        let mut qc_wrong_height = block_qc(VoteType::Precommit, BLOCK_A, 0);
        qc_wrong_height.height = BlockHeight::new(HEIGHT + 1);
        let action = state.apply(ConsensusEvent::PrecommitQuorum(qc_wrong_height));
        assert_eq!(
            action,
            Err(ConsensusError::WrongHeight {
                expected: height(),
                actual: BlockHeight::new(HEIGHT + 1)
            })
        );
    }

    // ---- Unchanged from ADR-0034 ----

    #[test]
    fn full_happy_path_finalizes_and_advances_to_the_next_height() -> Result<(), ConsensusError> {
        let mut state = fresh();

        assert_eq!(
            state.apply(ConsensusEvent::BeginRound)?,
            ConsensusAction::None
        );
        assert_eq!(state.step, ConsensusStep::Propose);

        let action = state.apply(proposal(0, BLOCK_A, true))?;
        assert_eq!(
            action,
            ConsensusAction::Prevote(ConsensusTarget::Block(BLOCK_A))
        );
        assert_eq!(state.step, ConsensusStep::Prevote);

        let prevote_qc = block_qc(VoteType::Prevote, BLOCK_A, 0);
        let action = state.apply(ConsensusEvent::PrevoteQuorum(prevote_qc.clone()))?;
        assert_eq!(
            action,
            ConsensusAction::Precommit(ConsensusTarget::Block(BLOCK_A))
        );
        assert_eq!(state.step, ConsensusStep::Precommit);
        assert_eq!(state.locked_value, Some(BLOCK_A));
        assert_eq!(state.valid_qc, Some(prevote_qc));

        let precommit_qc = block_qc(VoteType::Precommit, BLOCK_A, 0);
        let action = state.apply(ConsensusEvent::PrecommitQuorum(precommit_qc.clone()))?;
        assert_eq!(
            action,
            ConsensusAction::Finalized {
                block_hash: BLOCK_A,
                commit_qc: precommit_qc,
            }
        );
        assert_eq!(state.step, ConsensusStep::Finalize);

        let action = state.apply(ConsensusEvent::BeginNewHeight)?;
        assert_eq!(
            action,
            ConsensusAction::NewHeight {
                height: BlockHeight::new(HEIGHT + 1)
            }
        );
        assert_eq!(state.height, BlockHeight::new(HEIGHT + 1));
        assert_eq!(state.round, Round::FIRST);
        assert_eq!(state.step, ConsensusStep::NewHeight);
        assert_eq!(state.locked_value, None);
        assert_eq!(state.valid_value, None);

        Ok(())
    }

    #[test]
    fn rejects_an_event_with_no_transition_for_the_current_step() {
        let mut state = fresh();
        // Still `NewHeight`: `BeginRound`/`BeginNewHeight` are the only
        // events this scoping still treats as a caller bug when the step
        // doesn't match (`Proposal`/`*Timeout` get `Ignored` instead, see
        // the test below).
        assert_eq!(
            state.apply(ConsensusEvent::BeginNewHeight),
            Err(ConsensusError::UnexpectedEvent {
                step: ConsensusStep::NewHeight
            })
        );
    }

    #[test]
    fn ignores_a_proposal_or_timeout_delivered_at_the_wrong_step() {
        let mut state = fresh();
        // Still `NewHeight`: a proposal and every `*Timeout` arriving here
        // are stale network/timer messages, not a caller bug (ADR-0048,
        // "Decided: Certificate Routing In `hn-node`" — found via the
        // independent-implementation diff test, ADR-0048's own
        // Verification Requirement 5).
        assert_eq!(
            state.apply(proposal(0, BLOCK_A, true)),
            Ok(ConsensusAction::Ignored {
                reason: "proposal received while not in step Propose",
            })
        );
        assert_eq!(
            state.apply(ConsensusEvent::ProposeTimeout {
                height: height(),
                round: Round::FIRST,
            }),
            Ok(ConsensusAction::Ignored {
                reason: "propose timeout received while not in step Propose",
            })
        );
        assert_eq!(
            state.apply(ConsensusEvent::PrevoteTimeout {
                height: height(),
                round: Round::FIRST,
            }),
            Ok(ConsensusAction::Ignored {
                reason: "prevote timeout received while not in step Prevote",
            })
        );
        assert_eq!(
            state.apply(ConsensusEvent::PrecommitTimeout {
                height: height(),
                round: Round::FIRST,
            }),
            Ok(ConsensusAction::Ignored {
                reason: "precommit timeout received while not in step Precommit",
            })
        );
        // None of these stale deliveries actually moved the state machine.
        assert_eq!(state.step, ConsensusStep::NewHeight);
    }

    #[test]
    fn rejects_a_height_overflow_entering_a_new_height() {
        let mut state = ConsensusState::new_height(BlockHeight::new(u64::MAX));
        state.step = ConsensusStep::Finalize;
        assert_eq!(
            state.apply(ConsensusEvent::BeginNewHeight),
            Err(ConsensusError::HeightOverflow)
        );
    }

    #[test]
    fn rejects_a_round_overflow_advancing_the_round() {
        let mut state = fresh();
        state.round = Round::new(u64::MAX);
        state.step = ConsensusStep::Precommit;
        assert_eq!(
            state.apply(ConsensusEvent::PrecommitTimeout {
                height: height(),
                round: Round::new(u64::MAX)
            }),
            Err(ConsensusError::RoundOverflow)
        );
    }
}

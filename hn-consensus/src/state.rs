use hn_core::{BlockHeight, Round};
use hn_crypto::Digest;
use hn_state::{QuorumCertificate, VoteTargetType};

use crate::action::ConsensusAction;
use crate::error::{ConsensusError, ConsensusResult};
use crate::event::{ConsensusEvent, ConsensusTarget};
use crate::step::ConsensusStep;

/// One validator's local view of the Tendermint-style round state
/// machine at a given height (ADR-0009, "Timeout And View Change";
/// ADR-0034, "Consensus State Machine Skeleton").
///
/// `locked_block`/`highest_qc` persist across rounds within the same
/// height (reset only on entering a new height) — this is what makes
/// locking meaningful: a validator that has locked on a block in an
/// earlier round of this height keeps protecting it through later
/// rounds until a qualifying newer `prevote` quorum justifies
/// unlocking (see [`ConsensusState::apply`]'s own "Locking rule").
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsensusState {
    /// The height this state machine is attempting to finalize.
    pub height: BlockHeight,
    /// The current round attempt within `height`.
    pub round: Round,
    /// Where in the round this state machine currently is.
    pub step: ConsensusStep,
    /// The block this validator is locked on, if any (ADR-0034,
    /// "Locking rule").
    pub locked_block: Option<Digest>,
    /// The highest `prevote` quorum certificate observed for a block at
    /// this height, if any — updated together with `locked_block`, and
    /// the evidence checked when deciding whether a newly-proposed
    /// block may unlock it (ADR-0034, "Locking rule": a deliberate
    /// merge of Tendermint's textbook `lockedRound`/`validRound` pair
    /// into one field).
    pub highest_qc: Option<QuorumCertificate>,
}

impl ConsensusState {
    /// Starts a fresh height: round `Round::FIRST`, step `NewHeight`,
    /// no lock — locking never survives across heights, only within
    /// one (ADR-0034, "Locking rule").
    #[must_use]
    pub fn new_height(height: BlockHeight) -> Self {
        Self {
            height,
            round: Round::FIRST,
            step: ConsensusStep::NewHeight,
            locked_block: None,
            highest_qc: None,
        }
    }

    /// Applies `event`, mutating `self` to the resulting state and
    /// returning what the local validator should do about it (ADR-0034,
    /// "Decision" — the transition table in full).
    ///
    /// A deliberately closed, total function: every `(step, event)`
    /// pair not in the transition table is
    /// `Err(ConsensusError::UnexpectedEvent)`, never a silent no-op —
    /// see ADR-0034's own "Rejected Options" for why. Trusts `event`
    /// completely: no signature verification, quorum recomputation, or
    /// validator-eligibility check happens here — that is the caller's
    /// job before constructing `event` (ADR-0034, "Boundary").
    pub fn apply(&mut self, event: ConsensusEvent) -> ConsensusResult<ConsensusAction> {
        match (self.step, event) {
            (ConsensusStep::NewHeight, ConsensusEvent::BeginRound) => {
                self.step = ConsensusStep::Propose;
                Ok(ConsensusAction::None)
            }

            (
                ConsensusStep::Propose,
                ConsensusEvent::Proposal {
                    block_hash,
                    justification,
                },
            ) => {
                let target = self.decide_prevote_target(block_hash, justification.as_ref());
                self.step = ConsensusStep::Prevote;
                Ok(ConsensusAction::Prevote(target))
            }
            (ConsensusStep::Propose, ConsensusEvent::ProposeTimeout) => {
                self.step = ConsensusStep::Prevote;
                Ok(ConsensusAction::Prevote(ConsensusTarget::Nil))
            }

            (ConsensusStep::Prevote, ConsensusEvent::PrevoteQuorum(qc)) => {
                let target = match qc.target_type {
                    VoteTargetType::Block => {
                        let block_hash = qc.target_hash;
                        self.locked_block = Some(block_hash);
                        self.highest_qc = Some(qc);
                        ConsensusTarget::Block(block_hash)
                    }
                    VoteTargetType::Nil => ConsensusTarget::Nil,
                };
                self.step = ConsensusStep::Precommit;
                Ok(ConsensusAction::Precommit(target))
            }
            (ConsensusStep::Prevote, ConsensusEvent::PrevoteTimeout) => {
                self.step = ConsensusStep::Precommit;
                Ok(ConsensusAction::Precommit(ConsensusTarget::Nil))
            }

            (ConsensusStep::Precommit, ConsensusEvent::PrecommitQuorum(qc))
                if qc.target_type == VoteTargetType::Block =>
            {
                self.step = ConsensusStep::Finalize;
                let block_hash = qc.target_hash;
                Ok(ConsensusAction::Finalized {
                    block_hash,
                    commit_qc: qc,
                })
            }
            (ConsensusStep::Precommit, ConsensusEvent::PrecommitQuorum(_)) => self.advance_round(),
            (ConsensusStep::Precommit, ConsensusEvent::PrecommitTimeout) => self.advance_round(),

            (ConsensusStep::Timeout, ConsensusEvent::BeginRound) => {
                self.step = ConsensusStep::Propose;
                Ok(ConsensusAction::None)
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

            (step, _) => Err(ConsensusError::UnexpectedEvent { step }),
        }
    }

    /// `Precommit -> Timeout`: no qualifying block quorum formed this
    /// round (a nil `precommit` quorum, or the precommit timeout fired
    /// with no quorum at all) — `locked_block`/`highest_qc` are
    /// untouched, the entire reason locking exists (ADR-0034, "Decision"
    /// table).
    fn advance_round(&mut self) -> ConsensusResult<ConsensusAction> {
        let next_round = self
            .round
            .checked_next()
            .map_err(|_| ConsensusError::RoundOverflow)?;
        self.round = next_round;
        self.step = ConsensusStep::Timeout;
        Ok(ConsensusAction::RoundAdvanced { round: next_round })
    }

    /// `Propose` step's locking rule (ADR-0034, "Locking rule"):
    ///
    /// - Not locked on anything: prevote the proposal.
    /// - Locked on exactly `block_hash`: prevote it again.
    /// - Locked on something else: prevote it only if `justification`
    ///   is a `prevote` quorum certificate for `block_hash` at a round
    ///   no earlier than `highest_qc`'s own — otherwise protect the
    ///   lock and prevote nil.
    fn decide_prevote_target(
        &self,
        block_hash: Digest,
        justification: Option<&QuorumCertificate>,
    ) -> ConsensusTarget {
        match self.locked_block {
            None => ConsensusTarget::Block(block_hash),
            Some(locked) if locked == block_hash => ConsensusTarget::Block(block_hash),
            Some(_) => {
                let unlocks = justification.is_some_and(|qc| {
                    qc.target_type == VoteTargetType::Block
                        && qc.target_hash == block_hash
                        && self
                            .highest_qc
                            .as_ref()
                            .is_none_or(|highest| qc.round.get() >= highest.round.get())
                });
                if unlocks {
                    ConsensusTarget::Block(block_hash)
                } else {
                    ConsensusTarget::Nil
                }
            }
        }
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

    const BLOCK_A: [u8; 32] = [0xaa; 32];
    const BLOCK_B: [u8; 32] = [0xbb; 32];

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
            height: BlockHeight::new(1),
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

    #[test]
    fn new_height_starts_fresh() {
        let state = ConsensusState::new_height(BlockHeight::new(1));
        assert_eq!(state.height, BlockHeight::new(1));
        assert_eq!(state.round, Round::FIRST);
        assert_eq!(state.step, ConsensusStep::NewHeight);
        assert_eq!(state.locked_block, None);
        assert_eq!(state.highest_qc, None);
    }

    #[test]
    fn full_happy_path_finalizes_and_advances_to_the_next_height() -> Result<(), ConsensusError> {
        let mut state = ConsensusState::new_height(BlockHeight::new(1));

        assert_eq!(
            state.apply(ConsensusEvent::BeginRound)?,
            ConsensusAction::None
        );
        assert_eq!(state.step, ConsensusStep::Propose);

        let action = state.apply(ConsensusEvent::Proposal {
            block_hash: BLOCK_A,
            justification: None,
        })?;
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
        assert_eq!(state.locked_block, Some(BLOCK_A));
        assert_eq!(state.highest_qc, Some(prevote_qc));

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
                height: BlockHeight::new(2)
            }
        );
        assert_eq!(state.height, BlockHeight::new(2));
        assert_eq!(state.round, Round::FIRST);
        assert_eq!(state.step, ConsensusStep::NewHeight);
        assert_eq!(state.locked_block, None);
        assert_eq!(state.highest_qc, None);

        Ok(())
    }

    #[test]
    fn propose_timeout_casts_a_nil_prevote() -> Result<(), ConsensusError> {
        let mut state = ConsensusState::new_height(BlockHeight::new(1));
        state.step = ConsensusStep::Propose;

        let action = state.apply(ConsensusEvent::ProposeTimeout)?;
        assert_eq!(action, ConsensusAction::Prevote(ConsensusTarget::Nil));
        assert_eq!(state.step, ConsensusStep::Prevote);
        Ok(())
    }

    #[test]
    fn prevote_timeout_casts_a_nil_precommit() -> Result<(), ConsensusError> {
        let mut state = ConsensusState::new_height(BlockHeight::new(1));
        state.step = ConsensusStep::Prevote;

        let action = state.apply(ConsensusEvent::PrevoteTimeout)?;
        assert_eq!(action, ConsensusAction::Precommit(ConsensusTarget::Nil));
        assert_eq!(state.step, ConsensusStep::Precommit);
        Ok(())
    }

    #[test]
    fn nil_prevote_quorum_does_not_lock() -> Result<(), ConsensusError> {
        let mut state = ConsensusState::new_height(BlockHeight::new(1));
        state.step = ConsensusStep::Prevote;

        let action = state.apply(ConsensusEvent::PrevoteQuorum(nil_qc(VoteType::Prevote, 0)))?;
        assert_eq!(action, ConsensusAction::Precommit(ConsensusTarget::Nil));
        assert_eq!(state.locked_block, None);
        assert_eq!(state.highest_qc, None);
        Ok(())
    }

    #[test]
    fn nil_precommit_quorum_advances_the_round_without_clearing_the_lock()
    -> Result<(), ConsensusError> {
        let mut state = ConsensusState::new_height(BlockHeight::new(1));
        state.step = ConsensusStep::Precommit;
        state.locked_block = Some(BLOCK_A);
        state.highest_qc = Some(block_qc(VoteType::Prevote, BLOCK_A, 0));

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
        assert_eq!(state.locked_block, Some(BLOCK_A));

        let action = state.apply(ConsensusEvent::BeginRound)?;
        assert_eq!(action, ConsensusAction::None);
        assert_eq!(state.step, ConsensusStep::Propose);
        assert_eq!(state.round, Round::new(1));
        Ok(())
    }

    #[test]
    fn precommit_timeout_advances_the_round() -> Result<(), ConsensusError> {
        let mut state = ConsensusState::new_height(BlockHeight::new(1));
        state.step = ConsensusStep::Precommit;

        let action = state.apply(ConsensusEvent::PrecommitTimeout)?;
        assert_eq!(
            action,
            ConsensusAction::RoundAdvanced {
                round: Round::new(1)
            }
        );
        assert_eq!(state.step, ConsensusStep::Timeout);
        Ok(())
    }

    #[test]
    fn a_locked_validator_prevotes_nil_for_a_different_unjustified_proposal()
    -> Result<(), ConsensusError> {
        let mut state = ConsensusState::new_height(BlockHeight::new(1));
        state.step = ConsensusStep::Propose;
        state.round = Round::new(1);
        state.locked_block = Some(BLOCK_A);
        state.highest_qc = Some(block_qc(VoteType::Prevote, BLOCK_A, 0));

        let action = state.apply(ConsensusEvent::Proposal {
            block_hash: BLOCK_B,
            justification: None,
        })?;
        assert_eq!(action, ConsensusAction::Prevote(ConsensusTarget::Nil));
        // The lock itself is untouched by a rejected re-proposal.
        assert_eq!(state.locked_block, Some(BLOCK_A));
        Ok(())
    }

    #[test]
    fn a_locked_validator_unlocks_given_a_newer_justifying_quorum() -> Result<(), ConsensusError> {
        let mut state = ConsensusState::new_height(BlockHeight::new(1));
        state.step = ConsensusStep::Propose;
        state.round = Round::new(1);
        state.locked_block = Some(BLOCK_A);
        state.highest_qc = Some(block_qc(VoteType::Prevote, BLOCK_A, 0));

        let justification = block_qc(VoteType::Prevote, BLOCK_B, 1);
        let action = state.apply(ConsensusEvent::Proposal {
            block_hash: BLOCK_B,
            justification: Some(justification),
        })?;
        assert_eq!(
            action,
            ConsensusAction::Prevote(ConsensusTarget::Block(BLOCK_B))
        );
        Ok(())
    }

    #[test]
    fn a_locked_validator_ignores_a_stale_justifying_quorum() -> Result<(), ConsensusError> {
        let mut state = ConsensusState::new_height(BlockHeight::new(1));
        state.step = ConsensusStep::Propose;
        state.round = Round::new(2);
        state.locked_block = Some(BLOCK_A);
        state.highest_qc = Some(block_qc(VoteType::Prevote, BLOCK_A, 1));

        // Justification is for an earlier round than the current lock.
        let justification = block_qc(VoteType::Prevote, BLOCK_B, 0);
        let action = state.apply(ConsensusEvent::Proposal {
            block_hash: BLOCK_B,
            justification: Some(justification),
        })?;
        assert_eq!(action, ConsensusAction::Prevote(ConsensusTarget::Nil));
        Ok(())
    }

    #[test]
    fn an_unlocked_validator_prevotes_whatever_is_proposed() -> Result<(), ConsensusError> {
        let mut state = ConsensusState::new_height(BlockHeight::new(1));
        state.step = ConsensusStep::Propose;

        let action = state.apply(ConsensusEvent::Proposal {
            block_hash: BLOCK_B,
            justification: None,
        })?;
        assert_eq!(
            action,
            ConsensusAction::Prevote(ConsensusTarget::Block(BLOCK_B))
        );
        Ok(())
    }

    #[test]
    fn rejects_an_event_with_no_transition_for_the_current_step() {
        let mut state = ConsensusState::new_height(BlockHeight::new(1));
        // Still `NewHeight`: a prevote quorum has no defined transition here.
        assert_eq!(
            state.apply(ConsensusEvent::PrevoteQuorum(nil_qc(VoteType::Prevote, 0))),
            Err(ConsensusError::UnexpectedEvent {
                step: ConsensusStep::NewHeight
            })
        );
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
        let mut state = ConsensusState::new_height(BlockHeight::new(1));
        state.round = Round::new(u64::MAX);
        state.step = ConsensusStep::Precommit;
        assert_eq!(
            state.apply(ConsensusEvent::PrecommitTimeout),
            Err(ConsensusError::RoundOverflow)
        );
    }
}

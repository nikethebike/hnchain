//! Deterministic adversarial simulator (ADR-0048, "Verification
//! Requirements", items 2 and 3): several real [`ConsensusState`]
//! instances, driven by a harness that controls what each one
//! receives each round and can script Byzantine behavior -- in
//! process, no sockets, no real signatures.
//!
//! [`ConsensusState::apply`] trusts a [`QuorumCertificate`]'s own
//! signatures as already verified (ADR-0048, "Decided: Boundary") —
//! this simulator is testing the state machine's own transition
//! logic, not the wire protocol, so it builds certificates directly
//! rather than signing anything real. `hn-node`'s own integration
//! tests already cover the real-signature, real-network path.
//!
//! **Simplification, stated plainly:** rather than a fully general
//! async event queue (arbitrary per-message delay/reorder/
//! duplication), this harness is round-synchronous: every validator
//! processes the same round's own propose/prevote/precommit stages
//! together, and the harness controls, per stage, exactly which
//! validators receive which content (so a partition, a dropped
//! message, or an equivocating proposer sending different peers
//! different values are all still real, just expressed as "who
//! receives what" rather than as a reordered byte stream). Message
//! duplication is not separately modeled: `apply` already treats a
//! repeated identical event as idempotent (ADR-0048's own `Ignored`
//! path), so duplicating a message in this model would not exercise
//! anything the single-delivery case does not already.
//!
//! Only *honest* validators' own [`ConsensusState`] is meaningful:
//! a Byzantine one is not bound by the rules at all, so this harness
//! never drives its state through `apply` for a round where its own
//! [`Behavior`] overrides what it sends — there would be nothing
//! real to assert about it.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use hn_consensus::{ConsensusAction, ConsensusEvent, ConsensusState, ConsensusTarget};
use hn_core::{BlockHeight, Epoch, Round};
use hn_state::{QuorumCertificate, VoteTargetType, VoteType};

pub type Digest = [u8; 32];
pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub const CHAIN_ID: u8 = 1;
pub const NETWORK_ID: u16 = 1;
pub const VALIDATOR_SET_COMMITMENT: Digest = [0x11; 32];

/// A validator's own deviation from the honest rules (ADR-0048,
/// Verification Requirement 2's own Byzantine menu).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Behavior {
    /// Follows the real rules through its own `ConsensusState`.
    Honest,
    /// Never sends a vote, and (if the round's own proposer) never
    /// sends a proposal either.
    Silent,
    /// As the round's own proposer only: sends `first` to every
    /// validator at or before `split_index` (by position in the
    /// network's own ascending-`id` validator list) and `second` to
    /// everyone after it. Votes honestly through its own real state
    /// in every round, including ones it does not propose.
    EquivocatingProposer {
        first: Digest,
        second: Digest,
        split_index: usize,
    },
    /// Always votes for `target` (a block no one may have proposed,
    /// or nil), at every stage, regardless of this round's own real
    /// proposal.
    VoteForAnything(ConsensusTarget),
}

/// One simulated validator. The network's own validator list must
/// stay in ascending `id` order — [`Network::new`] sorts it.
pub struct SimValidator {
    pub id: Digest,
    pub voting_power: u128,
    pub behavior: Behavior,
    pub state: ConsensusState,
}

/// How one round actually concluded.
#[derive(Debug, Eq, PartialEq)]
pub enum RoundOutcome {
    /// Every honest validator that reached a decision finalized the
    /// same `block_hash` — [`Network::run_round`] itself returns a
    /// hard `Err` the moment two different ones disagree, rather than
    /// ever reporting two different `Finalized` outcomes.
    Finalized { block_hash: Digest },
    /// No qualifying quorum formed, or one formed for nil; every
    /// honest validator advanced to `next_round`.
    RoundFailed { next_round: Round },
}

/// Per-round delivery control: which validators (by `id`) do not
/// receive this round's own proposal at all (a dropped message or a
/// partition) — they still receive prevote/precommit quorum events
/// normally once those form.
#[derive(Clone, Debug, Default)]
pub struct Delivery {
    pub proposal_dropped_for: BTreeSet<Digest>,
    /// Validators whose own real precommit vote this stage is lost in
    /// the network: their own `state` still processes the real
    /// prevote quorum and locks normally (so `valid_value`/
    /// `locked_value` update exactly as a real dropped-precommit-
    /// message validator's would), but their vote is excluded from
    /// this round's own precommit tally, so a precommit quorum that
    /// would otherwise have formed may not (ADR-0048, Finding 3.1's
    /// own "a polka formed, but the precommit quorum did not").
    pub precommit_dropped_for: BTreeSet<Digest>,
}

fn has_quorum(signed_power: u128, total_power: u128) -> bool {
    signed_power.saturating_mul(3) > total_power.saturating_mul(2)
}

/// The deterministic stand-in block hash a fresh (non-re-proposing)
/// proposer builds for `(height, round)` -- exposed so tests can
/// predict what an ordinary round proposes without hard-coding a
/// value the harness itself never actually produces.
#[must_use]
pub fn deterministic_block_hash(height: BlockHeight, round: Round) -> Digest {
    let mut hash = [0_u8; 32];
    hash[0..8].copy_from_slice(&height.get().to_le_bytes());
    hash[8..16].copy_from_slice(&round.get().to_le_bytes());
    hash[16] = 0xB7;
    hash
}

/// A real `hn-consensus` network of validators, all at the same
/// height, driven round by round.
pub struct Network {
    pub height: BlockHeight,
    pub validators: Vec<SimValidator>,
}

impl Network {
    /// Builds a fresh network at `height` from `(id, voting_power,
    /// behavior)` triples, sorted into ascending-`id` order.
    #[must_use]
    pub fn new(height: BlockHeight, mut members: Vec<(Digest, u128, Behavior)>) -> Self {
        members.sort_by_key(|(id, ..)| *id);
        let validators = members
            .into_iter()
            .map(|(id, voting_power, behavior)| SimValidator {
                id,
                voting_power,
                behavior,
                state: ConsensusState::new_height(height),
            })
            .collect();
        Self { height, validators }
    }

    #[must_use]
    pub fn total_voting_power(&self) -> u128 {
        self.validators.iter().map(|v| v.voting_power).sum()
    }

    fn ordered_ids(&self) -> Vec<Digest> {
        self.validators.iter().map(|v| v.id).collect()
    }

    fn proposer_for(&self, round: Round) -> TestResult<Digest> {
        hn_consensus::round_proposer(&self.ordered_ids(), self.height, round)
            .ok_or_else(|| "empty validator set has no proposer".into())
    }

    fn base_qc(&self, certificate_type: VoteType, round: Round) -> QuorumCertificate {
        QuorumCertificate {
            certificate_type,
            chain_id: CHAIN_ID,
            network_id: NETWORK_ID,
            epoch: Epoch::new(0),
            height: self.height,
            round,
            validator_set_commitment: VALIDATOR_SET_COMMITMENT,
            target_type: VoteTargetType::Nil,
            target_hash: [0; 32],
            total_voting_power: self.total_voting_power(),
            signed_voting_power: 0,
            signer_commitment: Vec::new(),
            aggregate_proof: Vec::new(),
        }
    }

    /// Aggregates `votes` (one target per validator that actually
    /// cast one this stage) into a single [`QuorumCertificate`], iff
    /// one target's own signed voting power crosses quorum. A nil
    /// quorum is preferred if multiple targets somehow both crossed
    /// it in the same tally (should not happen under quorum
    /// intersection with fewer than a third Byzantine, but this
    /// function does not itself assume that holds).
    fn aggregate(
        &self,
        certificate_type: VoteType,
        round: Round,
        votes: &[(Digest, u128, ConsensusTarget)],
    ) -> Option<QuorumCertificate> {
        let total_power = self.total_voting_power();
        let mut block_power: BTreeMap<Digest, u128> = BTreeMap::new();
        let mut nil_power: u128 = 0;
        for (_, power, target) in votes {
            match target {
                ConsensusTarget::Block(hash) => {
                    *block_power.entry(*hash).or_insert(0) += power;
                }
                ConsensusTarget::Nil => nil_power += power,
            }
        }
        if has_quorum(nil_power, total_power) {
            let mut qc = self.base_qc(certificate_type, round);
            qc.target_type = VoteTargetType::Nil;
            qc.signed_voting_power = nil_power;
            return Some(qc);
        }
        for (hash, power) in block_power {
            if has_quorum(power, total_power) {
                let mut qc = self.base_qc(certificate_type, round);
                qc.target_type = VoteTargetType::Block;
                qc.target_hash = hash;
                qc.signed_voting_power = power;
                return Some(qc);
            }
        }
        None
    }

    /// Runs one full round: propose, prevote, precommit. Advances
    /// every honest validator's own `state` as a side effect. A
    /// stage that never reaches quorum gets every honest validator's
    /// own timeout instead, deterministically (this simulator has no
    /// real clock -- a round that cannot reach quorum always times
    /// out rather than hanging).
    pub fn run_round(&mut self, round: Round, delivery: &Delivery) -> TestResult<RoundOutcome> {
        let height = self.height;
        let validator_ids = self.ordered_ids();
        let proposer_id = self.proposer_for(round)?;
        let proposer_index = self
            .validators
            .iter()
            .position(|v| v.id == proposer_id)
            .ok_or("proposer not found in this network")?;
        let proposer_behavior = self.validators[proposer_index].behavior.clone();
        let proposer_valid = {
            let state = &self.validators[proposer_index].state;
            (state.valid_value, state.valid_round, state.valid_qc.clone())
        };
        let fresh_block = deterministic_block_hash(height, round);

        // Transition every validator whose own state is actually
        // driven (Honest, and an EquivocatingProposer's — it votes
        // honestly through its own real state on every round,
        // including ones it does not propose) out of `NewHeight`/
        // `Timeout` into `Propose` for this round. `Silent` and
        // `VoteForAnything` never touch their own state at all: they
        // override what they send unconditionally, so there is
        // nothing real to keep in sync.
        for index in 0..self.validators.len() {
            let drives_real_state = !matches!(
                self.validators[index].behavior,
                Behavior::Silent | Behavior::VoteForAnything(_)
            );
            if drives_real_state {
                self.validators[index]
                    .state
                    .apply(ConsensusEvent::BeginRound)?;
            }
        }

        // --- Propose + Prevote ---
        let mut prevotes: Vec<(Digest, u128, ConsensusTarget)> = Vec::new();
        for index in 0..self.validators.len() {
            let id = self.validators[index].id;
            let power = self.validators[index].voting_power;
            let behavior = self.validators[index].behavior.clone();

            if behavior == Behavior::Silent {
                continue;
            }
            if let Behavior::VoteForAnything(target) = behavior {
                prevotes.push((id, power, target));
                continue;
            }

            let action = if delivery.proposal_dropped_for.contains(&id) {
                self.validators[index]
                    .state
                    .apply(ConsensusEvent::ProposeTimeout { height, round })?
            } else {
                let (block_hash, valid_round, justification) = match &proposer_behavior {
                    Behavior::EquivocatingProposer {
                        first,
                        second,
                        split_index,
                    } => {
                        let recipient_index = validator_ids
                            .iter()
                            .position(|v| *v == id)
                            .unwrap_or(usize::MAX);
                        let value = if recipient_index <= *split_index {
                            *first
                        } else {
                            *second
                        };
                        (value, None, None)
                    }
                    _ => match proposer_valid.0 {
                        Some(value) => (value, proposer_valid.1, proposer_valid.2.clone()),
                        None => (fresh_block, None, None),
                    },
                };
                self.validators[index]
                    .state
                    .apply(ConsensusEvent::Proposal {
                        height,
                        round,
                        block_hash,
                        valid_round,
                        justification,
                        validated: true,
                    })?
            };

            if let ConsensusAction::Prevote(target) = action {
                prevotes.push((id, power, target));
            }
        }

        let prevote_qc = self.aggregate(VoteType::Prevote, round, &prevotes);

        // --- Precommit ---
        let mut precommits: Vec<(Digest, u128, ConsensusTarget)> = Vec::new();
        for index in 0..self.validators.len() {
            let id = self.validators[index].id;
            let power = self.validators[index].voting_power;
            let behavior = self.validators[index].behavior.clone();

            if behavior == Behavior::Silent {
                continue;
            }
            if let Behavior::VoteForAnything(target) = behavior {
                precommits.push((id, power, target));
                continue;
            }

            let action = match &prevote_qc {
                Some(qc) => self.validators[index]
                    .state
                    .apply(ConsensusEvent::PrevoteQuorum(qc.clone()))?,
                None => self.validators[index]
                    .state
                    .apply(ConsensusEvent::PrevoteTimeout { height, round })?,
            };
            // This validator's own state still locks/votes normally
            // either way (see Delivery::precommit_dropped_for's own
            // doc) -- only whether the vote reaches the tally below
            // depends on delivery.
            if let ConsensusAction::Precommit(target) = action
                && !delivery.precommit_dropped_for.contains(&id)
            {
                precommits.push((id, power, target));
            }
        }

        let precommit_qc = self.aggregate(VoteType::Precommit, round, &precommits);

        // --- Decide ---
        let mut finalized: Option<Digest> = None;
        let mut need_block: Option<(Digest, QuorumCertificate)> = None;
        for index in 0..self.validators.len() {
            if self.validators[index].behavior != Behavior::Honest {
                // A Byzantine validator's own "finalization" proves
                // nothing about the real safety property.
                continue;
            }
            let action = match &precommit_qc {
                Some(qc) => self.validators[index]
                    .state
                    .apply(ConsensusEvent::PrecommitQuorum(qc.clone()))?,
                None => self.validators[index]
                    .state
                    .apply(ConsensusEvent::PrecommitTimeout { height, round })?,
            };
            match action {
                ConsensusAction::Finalized { block_hash, .. } => match finalized {
                    Some(existing) if existing != block_hash => {
                        return Err(format!(
                            "SAFETY VIOLATION: height {} finalized both {existing:x?} and {block_hash:x?}",
                            height.get()
                        )
                        .into());
                    }
                    _ => finalized = Some(block_hash),
                },
                ConsensusAction::NeedBlock {
                    block_hash,
                    commit_qc,
                } => {
                    need_block = Some((block_hash, commit_qc));
                }
                _ => {}
            }
        }

        if let Some(block_hash) = finalized {
            return Ok(RoundOutcome::Finalized { block_hash });
        }

        if let Some((needed_hash, _)) = need_block {
            // Resolves immediately: this simulator has no network
            // layer to delay an ADR-0044 sync through.
            for index in 0..self.validators.len() {
                if self.validators[index].behavior != Behavior::Honest {
                    continue;
                }
                let action = self.validators[index]
                    .state
                    .apply(ConsensusEvent::BlockFetched {
                        block_hash: needed_hash,
                    })?;
                if let ConsensusAction::Finalized { block_hash, .. } = action {
                    match finalized {
                        Some(existing) if existing != block_hash => {
                            return Err(format!(
                                "SAFETY VIOLATION: height {} finalized both {existing:x?} and {block_hash:x?}",
                                height.get()
                            )
                            .into());
                        }
                        _ => finalized = Some(block_hash),
                    }
                }
            }
            if let Some(block_hash) = finalized {
                return Ok(RoundOutcome::Finalized { block_hash });
            }
        }

        let next_round = round.checked_next().map_err(|error| format!("{error:?}"))?;
        Ok(RoundOutcome::RoundFailed { next_round })
    }

    /// Drives [`Network::run_round`] starting at `start_round` until
    /// a height finalizes or `max_rounds` is exhausted (a test-level
    /// safety bound, never a protocol one), returning the finalized
    /// block hash.
    pub fn run_height(
        &mut self,
        start_round: Round,
        max_rounds: u64,
        delivery_for_round: impl Fn(Round) -> Delivery,
    ) -> TestResult<Digest> {
        let mut round = start_round;
        for _ in 0..max_rounds {
            let delivery = delivery_for_round(round);
            match self.run_round(round, &delivery)? {
                RoundOutcome::Finalized { block_hash } => return Ok(block_hash),
                RoundOutcome::RoundFailed { next_round } => round = next_round,
            }
        }
        Err(format!("height did not finalize within {max_rounds} rounds").into())
    }
}

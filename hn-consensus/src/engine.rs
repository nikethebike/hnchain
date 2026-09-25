use std::collections::HashMap;

use hn_core::BlockHeight;
use hn_crypto::Digest;
use hn_state::{
    BlockApplicationResult, ConsensusVote, QuorumCertificate, StateCommitter, StateReader,
    TransactionEnvelope, ValidatorRecordV1, VoteType, apply_and_commit_block,
    fetch_validator_record, is_eligible_signer,
};

use crate::action::ConsensusAction;
use crate::error::{ConsensusError, ConsensusResult};
use crate::event::ConsensusEvent;
use crate::state::ConsensusState;
use crate::step::ConsensusStep;

/// Wraps [`ConsensusState`]'s pure transitions with real calls into
/// `hn-state` (ADR-0035, "Wiring The Consensus Engine To `hn-state`"):
/// [`ConsensusVote::verify`]/[`QuorumCertificate::verify_signatures`]
/// before trusting a vote or certificate, [`hn_state::active_set`]'s
/// output (passed in by the caller — see each method's own
/// documentation for why this crate does not call it itself) to decide
/// signer eligibility, and [`apply_and_commit_block`] to actually
/// persist a finalized block's transactions.
///
/// `proposed_blocks` is a transient, in-memory cache of transaction
/// lists this engine has seen proposed, keyed by block hash — **not**
/// ADR-0019's `BlockStore` (no durability, no header, no receipts
/// retention policy). It exists only so
/// [`ConsensusEngine::commit_finalized_block`] has something to hand
/// [`apply_and_commit_block`] once a block actually finalizes; entries
/// for every other block this height are discarded once that happens
/// (ADR-0035, "Explicitly Not Resolved" — real block/header/receipts
/// storage is a distinct, larger, not-yet-decided piece of work this
/// cache deliberately does not attempt to be).
#[derive(Debug)]
pub struct ConsensusEngine {
    state: ConsensusState,
    proposed_blocks: HashMap<Digest, Vec<TransactionEnvelope>>,
}

impl ConsensusEngine {
    /// Starts a fresh engine at `height` (see
    /// [`ConsensusState::new_height`]).
    #[must_use]
    pub fn new_height(height: BlockHeight) -> Self {
        Self {
            state: ConsensusState::new_height(height),
            proposed_blocks: HashMap::new(),
        }
    }

    /// This engine's current state machine snapshot.
    #[must_use]
    pub fn state(&self) -> &ConsensusState {
        &self.state
    }

    /// `NewHeight`/`Timeout` -> `Propose`. See
    /// [`ConsensusEvent::BeginRound`].
    pub fn begin_round(&mut self) -> ConsensusResult<ConsensusAction> {
        self.state.apply(ConsensusEvent::BeginRound)
    }

    /// Records `transactions` under `block_hash` (so
    /// [`ConsensusEngine::commit_finalized_block`] can find them if
    /// this block goes on to finalize) and feeds a `Proposal` event
    /// into the state machine. Does not itself validate `transactions`
    /// against `block_hash` or check proposer eligibility (ADR-0011) —
    /// both are the caller's job before calling this, the same
    /// "caller already resolved it" boundary every method here draws.
    pub fn handle_proposal(
        &mut self,
        block_hash: Digest,
        transactions: Vec<TransactionEnvelope>,
        justification: Option<QuorumCertificate>,
    ) -> ConsensusResult<ConsensusAction> {
        self.proposed_blocks.insert(block_hash, transactions);
        self.state.apply(ConsensusEvent::Proposal {
            block_hash,
            justification,
        })
    }

    /// `Propose` -> `Prevote`, casting a nil prevote. See
    /// [`ConsensusEvent::ProposeTimeout`].
    pub fn propose_timeout(&mut self) -> ConsensusResult<ConsensusAction> {
        self.state.apply(ConsensusEvent::ProposeTimeout)
    }

    /// Verifies `qc`'s signatures ([`QuorumCertificate::verify_signatures`])
    /// against `ordered_active_set` before trusting it, then feeds it
    /// into the state machine as a `PrevoteQuorum`/`PrecommitQuorum`
    /// event, dispatched by `qc.certificate_type`.
    ///
    /// `ordered_active_set` is exactly [`hn_state::active_set`]'s own
    /// output for the relevant epoch, mapped to each record's
    /// `validator_id` — this engine does not call `active_set` itself:
    /// that function takes an already-fetched candidate slice (this
    /// crate cannot enumerate storage, mirroring every other
    /// `hn-state` boundary this session already established, e.g.
    /// `chamber_weights`), and it is computed once per epoch, not once
    /// per certificate, so recomputing it inside this per-event method
    /// would be the wrong place for it regardless.
    pub fn handle_quorum_certificate(
        &mut self,
        qc: QuorumCertificate,
        reader: &impl StateReader,
        ordered_active_set: &[Digest],
    ) -> ConsensusResult<ConsensusAction> {
        qc.verify_signatures(reader, ordered_active_set)
            .map_err(ConsensusError::QuorumCertificateInvalid)?;

        let event = match qc.certificate_type {
            VoteType::Prevote => ConsensusEvent::PrevoteQuorum(qc),
            VoteType::Precommit => ConsensusEvent::PrecommitQuorum(qc),
        };
        self.state.apply(event)
    }

    /// `Prevote` -> `Precommit`, casting a nil precommit. See
    /// [`ConsensusEvent::PrevoteTimeout`].
    pub fn prevote_timeout(&mut self) -> ConsensusResult<ConsensusAction> {
        self.state.apply(ConsensusEvent::PrevoteTimeout)
    }

    /// `Precommit` -> `Timeout`, advancing the round. See
    /// [`ConsensusEvent::PrecommitTimeout`].
    pub fn precommit_timeout(&mut self) -> ConsensusResult<ConsensusAction> {
        self.state.apply(ConsensusEvent::PrecommitTimeout)
    }

    /// Verifies one individual vote — [`ConsensusVote::verify`] (the
    /// cryptographic signature) plus signer eligibility
    /// ([`hn_state::is_eligible_signer`], combining `ordered_active_set`
    /// membership with the signer's live status via
    /// [`fetch_validator_record`]). Does **not** aggregate `vote` into
    /// a [`QuorumCertificate`] — no vote-pool/aggregation algorithm
    /// exists in this codebase yet (ADR-0035, "Explicitly Not
    /// Resolved"); this method only tells the caller whether one vote
    /// is legitimate, the input a future aggregator would consume.
    ///
    /// `ordered_active_set` is the same [`hn_state::active_set`] output
    /// [`ConsensusEngine::handle_quorum_certificate`] takes, for the
    /// same reason.
    pub fn verify_vote(
        &self,
        vote: &ConsensusVote,
        reader: &impl StateReader,
        ordered_active_set: &[Digest],
    ) -> ConsensusResult<()> {
        vote.verify(reader).map_err(ConsensusError::VoteInvalid)?;

        let validator_id = vote.payload.validator_id;
        let in_epoch_active_set = ordered_active_set.contains(&validator_id);
        let record = fetch_validator_record(reader, &validator_id)
            .map_err(ConsensusError::StateApplication)?
            .ok_or(ConsensusError::UnknownValidator { validator_id })?;

        if is_eligible_signer(in_epoch_active_set, record.status) {
            Ok(())
        } else {
            Err(ConsensusError::IneligibleSigner { validator_id })
        }
    }

    /// Once a `PrecommitQuorum` event has produced
    /// [`ConsensusAction::Finalized`] (via
    /// [`ConsensusEngine::handle_quorum_certificate`]) and this
    /// engine's own step is [`ConsensusStep::Finalize`]
    /// ([`ConsensusError::HeightNotFinalized`] otherwise — committing
    /// needs the height that just finalized, only well-defined at that
    /// step), looks up `block_hash`'s cached transactions
    /// ([`ConsensusError::UnknownProposedBlock`] if this engine never
    /// saw that proposal) and actually applies and commits them via
    /// [`apply_and_commit_block`] — the first real connection from this
    /// crate's own state machine into a durable backend.
    ///
    /// Clears every other cached proposal for this height afterward:
    /// only one block can ever finalize per height, so the rest were
    /// competing, now-irrelevant round attempts.
    ///
    /// `validator_candidates` is passed straight through to
    /// [`apply_and_commit_block`] — see its own documentation for why
    /// this crate cannot supply it itself.
    pub fn commit_finalized_block<S: StateReader + StateCommitter>(
        &mut self,
        block_hash: Digest,
        store: &mut S,
        validator_candidates: &[ValidatorRecordV1],
    ) -> ConsensusResult<BlockApplicationResult> {
        if self.state.step != ConsensusStep::Finalize {
            return Err(ConsensusError::HeightNotFinalized {
                step: self.state.step,
            });
        }
        let transactions = self
            .proposed_blocks
            .remove(&block_hash)
            .ok_or(ConsensusError::UnknownProposedBlock { block_hash })?;

        let result = apply_and_commit_block(
            &transactions,
            store,
            self.state.height,
            validator_candidates,
        )
        .map_err(ConsensusError::StateApplication)?;

        self.proposed_blocks.clear();
        Ok(result)
    }

    /// `Finalize` -> `NewHeight` (height + 1). See
    /// [`ConsensusEvent::BeginNewHeight`].
    pub fn begin_new_height(&mut self) -> ConsensusResult<ConsensusAction> {
        self.state.apply(ConsensusEvent::BeginNewHeight)
    }
}

#[cfg(test)]
mod tests {
    use hn_core::{BlockHeight, Epoch, Round};
    use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope};
    use hn_state::{
        ConsensusVote, StateCommitter, StateReader, StateResult, ValidatorSection, ValidatorStatus,
        VoteSigningPayloadV1, VoteTargetType, VoteType, Write, validator_section_state_key,
    };

    use super::{ConsensusEngine, QuorumCertificate, ValidatorRecordV1};
    use crate::action::ConsensusAction;
    use crate::error::ConsensusError;
    use crate::event::ConsensusTarget;
    use crate::step::ConsensusStep;

    const BLOCK_A: [u8; 32] = [0xaa; 32];
    const BLOCK_B: [u8; 32] = [0xbb; 32];
    const VSC: [u8; 32] = [0x11; 32];

    struct MapStore(std::collections::BTreeMap<[u8; 32], Vec<u8>>);

    impl StateReader for MapStore {
        fn get(&self, state_key: &[u8; 32]) -> StateResult<Option<Vec<u8>>> {
            Ok(self.0.get(state_key).cloned())
        }
    }

    impl StateCommitter for MapStore {
        fn commit(&mut self, writes: &[Write]) -> StateResult<()> {
            for write in writes {
                self.0.insert(write.state_key, write.value.clone());
            }
            Ok(())
        }
    }

    fn keypair(seed: u8) -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [seed; 32])
    }

    /// One validator, holding all voting power — the cheapest way to
    /// get a real, signature-checkable quorum.
    fn single_validator_store(
        validator_id: [u8; 32],
        keypair: &Ed25519KeyPair,
        status: ValidatorStatus,
    ) -> StateResult<MapStore> {
        let record = ValidatorRecordV1 {
            validator_id,
            consensus_key: keypair.key_descriptor(),
            bonded_stake: 100,
            voting_power: 100,
            status,
            pending_unbonding: None,
        };
        let key = validator_section_state_key(&validator_id, ValidatorSection::Record)?;
        Ok(MapStore(std::collections::BTreeMap::from([(
            key,
            record.encode()?,
        )])))
    }

    fn signed_qc(
        certificate_type: VoteType,
        validator_id: [u8; 32],
        keypair: &Ed25519KeyPair,
        target_type: VoteTargetType,
        target_hash: [u8; 32],
        round: u64,
    ) -> StateResult<QuorumCertificate> {
        let payload = VoteSigningPayloadV1 {
            vote_type: certificate_type,
            chain_id: 1,
            network_id: 1,
            epoch: Epoch::new(0),
            height: BlockHeight::new(1),
            round: Round::new(round),
            validator_set_commitment: VSC,
            validator_id,
            target_type,
            target_hash,
            vote_metadata: Vec::new(),
        };
        let digest = payload.signing_digest()?;
        let signature = SignatureEnvelope {
            algorithm_id: keypair.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: keypair.sign(&digest).to_vec(),
        };

        Ok(QuorumCertificate {
            certificate_type,
            chain_id: 1,
            network_id: 1,
            epoch: Epoch::new(0),
            height: BlockHeight::new(1),
            round: Round::new(round),
            validator_set_commitment: VSC,
            target_type,
            target_hash,
            total_voting_power: 100,
            signed_voting_power: 100,
            signer_commitment: vec![0b0000_0001],
            aggregate_proof: vec![signature],
        })
    }

    #[test]
    fn begin_round_transitions_to_propose() -> Result<(), ConsensusError> {
        let mut engine = ConsensusEngine::new_height(BlockHeight::new(1));
        assert_eq!(engine.begin_round()?, ConsensusAction::None);
        assert_eq!(engine.state().step, ConsensusStep::Propose);
        Ok(())
    }

    #[test]
    fn handle_proposal_transitions_to_prevote_and_caches_transactions() -> Result<(), ConsensusError>
    {
        let mut engine = ConsensusEngine::new_height(BlockHeight::new(1));
        engine.begin_round()?;

        let action = engine.handle_proposal(BLOCK_A, vec![], None)?;
        assert_eq!(
            action,
            ConsensusAction::Prevote(ConsensusTarget::Block(BLOCK_A))
        );
        assert_eq!(engine.state().step, ConsensusStep::Prevote);
        assert!(engine.proposed_blocks.contains_key(&BLOCK_A));
        Ok(())
    }

    #[test]
    fn handle_quorum_certificate_dispatches_prevote_and_precommit_correctly()
    -> Result<(), ConsensusError> {
        let keypair = keypair(0x01);
        let validator_id = [0x01; 32];
        let store = single_validator_store(validator_id, &keypair, ValidatorStatus::Active)
            .map_err(ConsensusError::StateApplication)?;
        let ordered_active_set = vec![validator_id];

        let mut engine = ConsensusEngine::new_height(BlockHeight::new(1));
        engine.begin_round()?;
        engine.handle_proposal(BLOCK_A, vec![], None)?;

        let prevote_qc = signed_qc(
            VoteType::Prevote,
            validator_id,
            &keypair,
            VoteTargetType::Block,
            BLOCK_A,
            0,
        )
        .map_err(ConsensusError::StateApplication)?;
        let action = engine.handle_quorum_certificate(prevote_qc, &store, &ordered_active_set)?;
        assert_eq!(
            action,
            ConsensusAction::Precommit(ConsensusTarget::Block(BLOCK_A))
        );
        assert_eq!(engine.state().locked_block, Some(BLOCK_A));

        let precommit_qc = signed_qc(
            VoteType::Precommit,
            validator_id,
            &keypair,
            VoteTargetType::Block,
            BLOCK_A,
            0,
        )
        .map_err(ConsensusError::StateApplication)?;
        let action =
            engine.handle_quorum_certificate(precommit_qc.clone(), &store, &ordered_active_set)?;
        assert_eq!(
            action,
            ConsensusAction::Finalized {
                block_hash: BLOCK_A,
                commit_qc: precommit_qc,
            }
        );
        assert_eq!(engine.state().step, ConsensusStep::Finalize);
        Ok(())
    }

    #[test]
    fn handle_quorum_certificate_rejects_invalid_signatures() -> Result<(), ConsensusError> {
        let stored_keypair = keypair(0x02);
        let signing_keypair = keypair(0x03);
        let validator_id = [0x02; 32];
        let store = single_validator_store(validator_id, &stored_keypair, ValidatorStatus::Active)
            .map_err(ConsensusError::StateApplication)?;
        let ordered_active_set = vec![validator_id];

        let mut engine = ConsensusEngine::new_height(BlockHeight::new(1));
        engine.begin_round()?;
        engine.handle_proposal(BLOCK_A, vec![], None)?;

        // Signed by the wrong key -- the store has `stored_keypair`'s
        // key on file for `validator_id`, not `signing_keypair`'s.
        let bad_qc = signed_qc(
            VoteType::Prevote,
            validator_id,
            &signing_keypair,
            VoteTargetType::Block,
            BLOCK_A,
            0,
        )
        .map_err(ConsensusError::StateApplication)?;

        assert!(matches!(
            engine.handle_quorum_certificate(bad_qc, &store, &ordered_active_set),
            Err(ConsensusError::QuorumCertificateInvalid(_))
        ));
        Ok(())
    }

    #[test]
    fn commit_finalized_block_rejects_when_not_at_finalize_step() {
        let mut engine = ConsensusEngine::new_height(BlockHeight::new(1));
        let mut store = MapStore(std::collections::BTreeMap::new());
        assert_eq!(
            engine.commit_finalized_block(BLOCK_A, &mut store, &[]),
            Err(ConsensusError::HeightNotFinalized {
                step: ConsensusStep::NewHeight
            })
        );
    }

    #[test]
    fn commit_finalized_block_rejects_an_unknown_block_hash() -> Result<(), ConsensusError> {
        let keypair = keypair(0x04);
        let validator_id = [0x04; 32];
        let store = single_validator_store(validator_id, &keypair, ValidatorStatus::Active)
            .map_err(ConsensusError::StateApplication)?;
        let ordered_active_set = vec![validator_id];

        let mut engine = ConsensusEngine::new_height(BlockHeight::new(1));
        engine.begin_round()?;
        engine.handle_proposal(BLOCK_A, vec![], None)?;
        let prevote_qc = signed_qc(
            VoteType::Prevote,
            validator_id,
            &keypair,
            VoteTargetType::Block,
            BLOCK_A,
            0,
        )
        .map_err(ConsensusError::StateApplication)?;
        engine.handle_quorum_certificate(prevote_qc, &store, &ordered_active_set)?;
        let precommit_qc = signed_qc(
            VoteType::Precommit,
            validator_id,
            &keypair,
            VoteTargetType::Block,
            BLOCK_A,
            0,
        )
        .map_err(ConsensusError::StateApplication)?;
        engine.handle_quorum_certificate(precommit_qc, &store, &ordered_active_set)?;
        assert_eq!(engine.state().step, ConsensusStep::Finalize);

        let mut store = MapStore(std::collections::BTreeMap::new());
        assert_eq!(
            engine.commit_finalized_block(BLOCK_B, &mut store, &[]),
            Err(ConsensusError::UnknownProposedBlock {
                block_hash: BLOCK_B
            })
        );
        Ok(())
    }

    fn signed_vote(validator_id: [u8; 32], keypair: &Ed25519KeyPair) -> StateResult<ConsensusVote> {
        let payload = VoteSigningPayloadV1 {
            vote_type: VoteType::Prevote,
            chain_id: 1,
            network_id: 1,
            epoch: Epoch::new(0),
            height: BlockHeight::new(1),
            round: Round::new(0),
            validator_set_commitment: VSC,
            validator_id,
            target_type: VoteTargetType::Block,
            target_hash: BLOCK_A,
            vote_metadata: Vec::new(),
        };
        let digest = payload.signing_digest()?;
        Ok(ConsensusVote {
            payload,
            signature: SignatureEnvelope {
                algorithm_id: keypair.key_descriptor().algorithm_id(),
                key_reference: None,
                signature: keypair.sign(&digest).to_vec(),
            },
        })
    }

    #[test]
    fn verify_vote_accepts_a_real_eligible_signer() -> Result<(), ConsensusError> {
        let keypair = keypair(0x05);
        let validator_id = [0x05; 32];
        let store = single_validator_store(validator_id, &keypair, ValidatorStatus::Active)
            .map_err(ConsensusError::StateApplication)?;
        let vote = signed_vote(validator_id, &keypair).map_err(ConsensusError::StateApplication)?;

        let engine = ConsensusEngine::new_height(BlockHeight::new(1));
        engine.verify_vote(&vote, &store, &[validator_id])
    }

    #[test]
    fn verify_vote_rejects_a_signer_not_in_the_epoch_active_set() -> Result<(), ConsensusError> {
        let keypair = keypair(0x06);
        let validator_id = [0x06; 32];
        let store = single_validator_store(validator_id, &keypair, ValidatorStatus::Active)
            .map_err(ConsensusError::StateApplication)?;
        let vote = signed_vote(validator_id, &keypair).map_err(ConsensusError::StateApplication)?;

        let engine = ConsensusEngine::new_height(BlockHeight::new(1));
        assert_eq!(
            engine.verify_vote(&vote, &store, &[]),
            Err(ConsensusError::IneligibleSigner { validator_id })
        );
        Ok(())
    }

    #[test]
    fn verify_vote_rejects_a_jailed_signer() -> Result<(), ConsensusError> {
        let keypair = keypair(0x07);
        let validator_id = [0x07; 32];
        let store = single_validator_store(validator_id, &keypair, ValidatorStatus::Jailed)
            .map_err(ConsensusError::StateApplication)?;
        let vote = signed_vote(validator_id, &keypair).map_err(ConsensusError::StateApplication)?;

        let engine = ConsensusEngine::new_height(BlockHeight::new(1));
        assert_eq!(
            engine.verify_vote(&vote, &store, &[validator_id]),
            Err(ConsensusError::IneligibleSigner { validator_id })
        );
        Ok(())
    }

    /// An unknown validator is rejected by `vote.verify()` itself
    /// (which resolves the signer's key via `active_key`, a thin
    /// wrapper over `fetch_validator_record`) before `verify_vote`'s
    /// own separate `fetch_validator_record` call is ever reached —
    /// found while writing this test, not assumed: the first version
    /// asserted `ConsensusError::UnknownValidator` here and failed,
    /// since that variant can only fire for a validator that exists at
    /// `verify()` time but has somehow vanished by the time
    /// `verify_vote`'s own lookup runs a moment later against the same
    /// `reader` — defensive code for state that cannot actually change
    /// mid-call, kept rather than unwrapped away per this workspace's
    /// no-panic discipline, not a reachable path from here.
    #[test]
    fn verify_vote_rejects_an_unknown_validator() -> Result<(), ConsensusError> {
        let keypair = keypair(0x08);
        let validator_id = [0x08; 32];
        let store = MapStore(std::collections::BTreeMap::new());
        let vote = signed_vote(validator_id, &keypair).map_err(ConsensusError::StateApplication)?;

        let engine = ConsensusEngine::new_height(BlockHeight::new(1));
        assert!(matches!(
            engine.verify_vote(&vote, &store, &[validator_id]),
            Err(ConsensusError::VoteInvalid(
                hn_state::StateError::UnknownValidator { .. }
            ))
        ));
        Ok(())
    }
}

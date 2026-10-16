use std::collections::{BTreeMap, HashMap};

use hn_crypto::{Digest, SignatureEnvelope};
use hn_state::{ConsensusVote, QuorumCertificate, VoteTargetType, VoteType};

/// Identifies one `(height, round, vote_type)` voting attempt — several
/// competing targets (a real block, or nil) may each be independently
/// collecting votes within it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct RoundKey {
    height: u64,
    round: u64,
    vote_type_tag: u8,
}

/// Identifies one target within a [`RoundKey`]. `target_type`/`VoteType`
/// (`hn_state`) derive neither `Hash` nor `Ord`, so this module maps
/// each to a local `u8` tag rather than depending on their private
/// `as_u8` encodings.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct TargetKey {
    target_type_tag: u8,
    target_hash: Digest,
}

fn vote_type_tag(vote_type: VoteType) -> u8 {
    match vote_type {
        VoteType::Prevote => 1,
        VoteType::Precommit => 2,
    }
}

fn target_type_tag(target_type: VoteTargetType) -> u8 {
    match target_type {
        VoteTargetType::Block => 1,
        VoteTargetType::Nil => 2,
    }
}

/// `signed_voting_power * 3 > total_voting_power * 2` — the threshold
/// formula [`QuorumCertificate`]'s own type documentation already fixes
/// for this profile, not a new rule invented here. `saturating_mul`
/// only guards against a theoretical `u128` overflow at voting-power
/// magnitudes this codebase has no realistic path to reach; it is not a
/// claim that such magnitudes are expected.
fn has_quorum(signed_voting_power: u128, total_voting_power: u128) -> bool {
    signed_voting_power.saturating_mul(3) > total_voting_power.saturating_mul(2)
}

/// Proof that a validator signed two different targets at the same
/// `(height, round, vote_type)` (ADR-0048, "Decided: Equivocation
/// Evidence"). Scoped to votes only — a proposer sending two
/// different proposals for the same `(height, round)` is a separate
/// equivocation class (ADR-0048, "Decided: Proposer Duties"), tracked
/// by whatever receives proposals, not by this pool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EquivocationEvidence {
    /// The first vote this pool saw at the key both votes share.
    pub vote_a: ConsensusVote,
    /// The later, conflicting vote from the same signer.
    pub vote_b: ConsensusVote,
}

#[derive(Debug, Default)]
struct Tally {
    /// `bit_position -> signature`, one entry per distinct signer.
    /// `BTreeMap` so iteration is already in ascending bit-position
    /// order — exactly [`QuorumCertificate::aggregate_proof`]'s own
    /// required order.
    signatures: BTreeMap<usize, SignatureEnvelope>,
    signed_voting_power: u128,
    /// Set once this target has already produced a
    /// [`QuorumCertificate`] — later votes for it still record (for
    /// completeness) but never emit a second certificate for the same
    /// target.
    certified: bool,
}

/// Collects individual, already-verified [`ConsensusVote`]s toward a
/// [`QuorumCertificate`] (ADR-0037, "Decided: Vote Aggregation") — the
/// piece `hn-consensus`'s own crate documentation named as "unbuilt"
/// since ADR-0035. Holds no signing key, verifies nothing itself
/// (`ConsensusEngine::record_vote` calls `verify_vote` first): this type
/// only counts already-trusted votes and knows when they add up to a
/// quorum.
///
/// Stale entries from heights/rounds already left behind are not
/// pruned — an accepted simplification for this pass, not a leak this
/// type's own callers are expected to work around; see ADR-0037's own
/// "Explicitly Not Resolved."
///
/// `first_vote` is ADR-0048's own "Decided: Equivocation Evidence":
/// per `(height, round, vote_type, bit_position)`, the first verified
/// vote seen, regardless of target. A later vote at the same key for
/// the *same* target is idempotent (falls through to the normal
/// tally below); for a *different* target it is diverted into
/// [`EquivocationEvidence`] instead of ever being tallied — safe for
/// quorum arithmetic either way, since a Byzantine validator gains
/// nothing by equivocating (it is never double-counted), and an
/// honest one never does.
#[derive(Debug, Default)]
pub struct VotePool {
    rounds: HashMap<RoundKey, HashMap<TargetKey, Tally>>,
    first_vote: HashMap<RoundKey, HashMap<usize, (TargetKey, ConsensusVote)>>,
    equivocations: Vec<EquivocationEvidence>,
}

impl VotePool {
    /// An empty pool.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every [`EquivocationEvidence`] recorded so far, oldest first.
    #[must_use]
    pub fn equivocations(&self) -> &[EquivocationEvidence] {
        &self.equivocations
    }

    /// Records `vote`'s signature at `bit_position` (its signer's index
    /// in the caller's `ordered_active_set`, ascending `validator_id`
    /// order — the same order [`QuorumCertificate::verify_signatures`]
    /// already assumes), worth `voting_power` out of
    /// `total_voting_power` among `active_set_len` total signer slots.
    ///
    /// Idempotent for a repeated `bit_position` against the same target:
    /// a duplicate vote from the same signer does not double-count its
    /// voting power. A `bit_position` that has already voted for a
    /// *different* target at this same `(height, round, vote_type)` is
    /// recorded as [`EquivocationEvidence`] instead — not tallied at
    /// all, so it can never itself produce a certificate. Returns the
    /// newly-formed [`QuorumCertificate`] the first time this
    /// `(height, round, vote_type, target)`'s signed voting power
    /// crosses the quorum threshold — `None` on every other call,
    /// including every later call for a target that already crossed
    /// it.
    pub fn insert(
        &mut self,
        vote: &ConsensusVote,
        bit_position: usize,
        voting_power: u128,
        total_voting_power: u128,
        active_set_len: usize,
    ) -> Option<QuorumCertificate> {
        let payload = &vote.payload;
        let round_key = RoundKey {
            height: payload.height.get(),
            round: payload.round.get(),
            vote_type_tag: vote_type_tag(payload.vote_type),
        };
        let target_key = TargetKey {
            target_type_tag: target_type_tag(payload.target_type),
            target_hash: payload.target_hash,
        };

        let first_votes = self.first_vote.entry(round_key).or_default();
        if let Some((existing_target, existing_vote)) = first_votes.get(&bit_position) {
            if *existing_target != target_key {
                self.equivocations.push(EquivocationEvidence {
                    vote_a: existing_vote.clone(),
                    vote_b: vote.clone(),
                });
                return None;
            }
        } else {
            first_votes.insert(bit_position, (target_key, vote.clone()));
        }

        let tally = self
            .rounds
            .entry(round_key)
            .or_default()
            .entry(target_key)
            .or_default();

        if tally
            .signatures
            .insert(bit_position, vote.signature.clone())
            .is_none()
        {
            tally.signed_voting_power = tally.signed_voting_power.saturating_add(voting_power);
        }

        if tally.certified || !has_quorum(tally.signed_voting_power, total_voting_power) {
            return None;
        }
        tally.certified = true;

        let signer_commitment_len = active_set_len.div_ceil(8);
        let mut signer_commitment = vec![0_u8; signer_commitment_len];
        for &position in tally.signatures.keys() {
            signer_commitment[position / 8] |= 1 << (position % 8);
        }
        let aggregate_proof = tally.signatures.values().cloned().collect();

        Some(QuorumCertificate {
            certificate_type: payload.vote_type,
            chain_id: payload.chain_id,
            network_id: payload.network_id,
            epoch: payload.epoch,
            height: payload.height,
            round: payload.round,
            validator_set_commitment: payload.validator_set_commitment,
            target_type: payload.target_type,
            target_hash: payload.target_hash,
            total_voting_power,
            signed_voting_power: tally.signed_voting_power,
            signer_commitment,
            aggregate_proof,
        })
    }
}

#[cfg(test)]
mod tests {
    use hn_core::{BlockHeight, Epoch, Round};
    use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope};
    use hn_state::VoteSigningPayloadV1;

    use super::{ConsensusVote, VotePool, VoteTargetType, VoteType};

    const VSC: [u8; 32] = [0x11; 32];
    const BLOCK: [u8; 32] = [0xaa; 32];

    fn vote(validator_id: [u8; 32], seed: u8) -> Result<ConsensusVote, Box<dyn std::error::Error>> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [seed; 32]);
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
            target_hash: BLOCK,
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
    fn no_quorum_below_threshold() -> Result<(), Box<dyn std::error::Error>> {
        let mut pool = VotePool::new();
        let result = pool.insert(&vote([0x01; 32], 1)?, 0, 100, 300, 3);
        assert!(result.is_none());
        Ok(())
    }

    #[test]
    fn emits_a_quorum_certificate_the_moment_threshold_is_crossed()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut pool = VotePool::new();
        assert!(pool.insert(&vote([0x01; 32], 1)?, 0, 100, 300, 3).is_none());
        assert!(pool.insert(&vote([0x02; 32], 2)?, 1, 100, 300, 3).is_none());

        let qc = pool
            .insert(&vote([0x03; 32], 3)?, 2, 100, 300, 3)
            .ok_or("3/3 equal shares should cross 2f+1 of 300")?;
        assert_eq!(qc.signed_voting_power, 300);
        assert_eq!(qc.total_voting_power, 300);
        assert_eq!(qc.signer_commitment, vec![0b0000_0111]);
        assert_eq!(qc.aggregate_proof.len(), 3);
        Ok(())
    }

    #[test]
    fn a_repeated_signer_does_not_double_count_voting_power()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut pool = VotePool::new();
        assert!(pool.insert(&vote([0x01; 32], 1)?, 0, 150, 300, 3).is_none());
        // Same bit position again: still short of quorum, not 300/300.
        assert!(pool.insert(&vote([0x01; 32], 1)?, 0, 150, 300, 3).is_none());
        Ok(())
    }

    #[test]
    fn does_not_emit_a_second_certificate_for_an_already_certified_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut pool = VotePool::new();
        assert!(pool.insert(&vote([0x01; 32], 1)?, 0, 100, 300, 3).is_none());
        assert!(pool.insert(&vote([0x02; 32], 2)?, 1, 100, 300, 3).is_none());
        assert!(pool.insert(&vote([0x03; 32], 3)?, 2, 100, 300, 3).is_some());
        // A fourth signer's vote for the same already-certified target.
        assert!(pool.insert(&vote([0x04; 32], 4)?, 3, 100, 400, 4).is_none());
        Ok(())
    }

    fn vote_for(
        validator_id: [u8; 32],
        seed: u8,
        target_hash: [u8; 32],
    ) -> Result<ConsensusVote, Box<dyn std::error::Error>> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [seed; 32]);
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
            target_hash,
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

    const OTHER_BLOCK: [u8; 32] = [0xbb; 32];

    #[test]
    fn a_second_vote_for_a_different_target_is_recorded_as_equivocation_not_tallied()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut pool = VotePool::new();
        let first = vote_for([0x01; 32], 1, BLOCK)?;
        let second = vote_for([0x01; 32], 1, OTHER_BLOCK)?;

        assert!(pool.insert(&first, 0, 100, 300, 3).is_none());
        // Same signer (bit_position 0), different target: diverted to
        // evidence, never tallied toward OTHER_BLOCK's own quorum.
        assert!(pool.insert(&second, 0, 100, 300, 3).is_none());

        let evidence = pool.equivocations();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].vote_a, first);
        assert_eq!(evidence[0].vote_b, second);

        // The equivocating signer's power never counted toward
        // OTHER_BLOCK: two more honest signers for it still fall
        // short of quorum (200 of 300).
        assert!(
            pool.insert(&vote_for([0x02; 32], 2, OTHER_BLOCK)?, 1, 100, 300, 3)
                .is_none()
        );
        assert!(
            pool.insert(&vote_for([0x03; 32], 3, OTHER_BLOCK)?, 2, 100, 300, 3)
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn repeating_the_same_target_is_not_equivocation() -> Result<(), Box<dyn std::error::Error>> {
        let mut pool = VotePool::new();
        let first = vote([0x01; 32], 1)?;
        assert!(pool.insert(&first, 0, 100, 300, 3).is_none());
        // The exact same vote again.
        assert!(pool.insert(&first, 0, 100, 300, 3).is_none());
        assert!(pool.equivocations().is_empty());
        Ok(())
    }

    #[test]
    fn equivocation_is_independent_per_round_key() -> Result<(), Box<dyn std::error::Error>> {
        let mut pool = VotePool::new();
        let round_0 = vote_for([0x01; 32], 1, BLOCK)?;
        assert!(pool.insert(&round_0, 0, 100, 300, 3).is_none());

        // Same signer, same bit position, but a genuinely different
        // round: a different target there is not equivocation.
        let mut other_round_payload = round_0.payload.clone();
        other_round_payload.round = Round::new(1);
        other_round_payload.target_hash = OTHER_BLOCK;
        let digest = other_round_payload.signing_digest()?;
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [1; 32]);
        let round_1 = ConsensusVote {
            payload: other_round_payload,
            signature: SignatureEnvelope {
                algorithm_id: keypair.key_descriptor().algorithm_id(),
                key_reference: None,
                signature: keypair.sign(&digest).to_vec(),
            },
        };
        assert!(pool.insert(&round_1, 0, 100, 300, 3).is_none());
        assert!(pool.equivocations().is_empty());
        Ok(())
    }
}

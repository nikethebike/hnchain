//! ADR-0048, "Decided: Vote Journal (Write-Ahead Safety State)": the
//! on-disk record of this validator's own lock/valid state and the last
//! vote it sent per stage. Before any `prevote`/`precommit` leaves the
//! process, the journal is updated and durably written (rule 1) so a
//! crash can never leave this validator able to sign a *different* vote
//! for a `(height, round, vote type)` it already voted on (rule 2).
//!
//! This module owns the record's shape, its canonical wire encoding,
//! and the pure startup decision table (rules 3 and 4, from
//! [`decide_startup_action`]). It performs no file I/O itself — this
//! crate's own boundary is "no network I/O, no node process lifecycle"
//! (see the crate doc), and a durable write is squarely a storage
//! concern, not a consensus one: [`hn_storage::write_atomically`]/
//! [`hn_storage::read_optional`] are the generic bytes-in/bytes-out
//! primitive a caller (`hn-node`) uses against [`VoteJournal::encode`]/
//! [`VoteJournal::decode`]'s output, and the fatal exit rule 3 calls for
//! is `hn-node`'s own `std::process::exit`, not anything this crate
//! could perform on its own.
//!
//! Rule 5 (genesis bootstrap vs. a validator rejoining with a wiped
//! data directory) needs a peer's finalized height at handshake time,
//! which the network layer does not carry yet — deferred, see
//! [`JournalStartupAction::Vote`]'s own doc for exactly what this
//! currently falls back to.

use hn_core::{BlockHeight, PrimitiveError, Round};
use hn_crypto::Digest;
use hn_hncs::{
    Decoder, HncsError, write_fixed_bytes, write_optional, write_u8, write_u16, write_u32,
    write_u64,
};
use hn_state::{QuorumCertificate, StateError};

use crate::event::ConsensusTarget;
use crate::step::ConsensusStep;

/// `journal_version` for the current [`VoteJournal`] shape.
pub const JOURNAL_VERSION_1: u8 = 1;

/// One vote this validator has already sent for some `(height, round,
/// vote type)`, recorded so a restart re-sends the identical vote
/// instead of risking a different one (ADR-0048 rule 2). Which vote
/// type this is (`prevote` vs. `precommit`) is which of
/// [`VoteJournal::last_prevote`]/[`VoteJournal::last_precommit`] it is
/// stored under, not a field of its own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournaledVote {
    /// The height this vote was cast at.
    pub height: BlockHeight,
    /// The round, within `height`, this vote was cast at.
    pub round: Round,
    /// What this vote targeted.
    pub target: ConsensusTarget,
}

/// ADR-0048, "Decided: Vote Journal" — the exact record named there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoteJournal {
    /// HNChain protocol lineage (ADR-0006).
    pub chain_id: u8,
    /// Network environment (ADR-0003).
    pub network_id: u16,
    /// This validator's own id — a journal written by one validator
    /// must never be mistaken for another's (e.g. an operator copying
    /// a data directory between machines).
    pub validator_id: Digest,
    /// The height [`Self::step`]/lock/valid state below describes.
    pub height: BlockHeight,
    /// The round, within [`Self::height`], described below.
    pub round: Round,
    /// Where in the round this validator was.
    pub step: ConsensusStep,
    /// Mirrors [`crate::ConsensusState::locked_value`].
    pub locked_value: Option<Digest>,
    /// Mirrors [`crate::ConsensusState::locked_round`].
    pub locked_round: Option<Round>,
    /// Mirrors [`crate::ConsensusState::valid_value`].
    pub valid_value: Option<Digest>,
    /// Mirrors [`crate::ConsensusState::valid_round`].
    pub valid_round: Option<Round>,
    /// Mirrors [`crate::ConsensusState::valid_qc`].
    pub valid_qc: Option<QuorumCertificate>,
    /// The last `prevote` this validator sent, if any, at the height
    /// above (rule 2).
    pub last_prevote: Option<JournaledVote>,
    /// The last `precommit` this validator sent, if any, at the height
    /// above (rule 2).
    pub last_precommit: Option<JournaledVote>,
}

/// Why [`VoteJournal::decode`] rejected some bytes.
#[derive(Debug, Eq, PartialEq)]
pub enum JournalDecodeError {
    /// The leading version tag is not one this build knows how to
    /// read.
    UnsupportedVersion {
        /// The version tag actually present.
        value: u8,
    },
    /// A step/target tag byte did not match any known value.
    UnknownTag {
        /// The field the bad tag was read for (for a clearer log
        /// line).
        field: &'static str,
        /// The tag value actually present.
        value: u8,
    },
    /// A `ConsensusTarget` was `Nil` but its hash bytes were not the
    /// canonical all-zero digest, mirroring
    /// [`hn_state::StateError::NonCanonicalNilTarget`]'s own rule for
    /// vote targets.
    NonCanonicalNilTarget,
    /// The embedded `valid_qc` failed to decode.
    ValidQc(StateError),
    /// A primitive HNCS decoding rule was violated (truncated input,
    /// trailing bytes, ...).
    Encoding(HncsError),
}

impl VoteJournal {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> Result<Vec<u8>, StateError> {
        let mut out = Vec::new();
        write_u8(&mut out, JOURNAL_VERSION_1);
        write_u8(&mut out, self.chain_id);
        write_u16(&mut out, self.network_id);
        write_fixed_bytes(&mut out, &self.validator_id);
        write_u64(&mut out, self.height.get());
        write_u64(&mut out, self.round.get());
        write_u8(&mut out, self.step.as_u8());
        write_optional(&mut out, self.locked_value.as_ref(), |out, value| {
            write_fixed_bytes(out, value);
            Ok(())
        })
        .map_err(StateError::Encoding)?;
        write_optional(&mut out, self.locked_round.as_ref(), |out, value| {
            write_u64(out, value.get());
            Ok(())
        })
        .map_err(StateError::Encoding)?;
        write_optional(&mut out, self.valid_value.as_ref(), |out, value| {
            write_fixed_bytes(out, value);
            Ok(())
        })
        .map_err(StateError::Encoding)?;
        write_optional(&mut out, self.valid_round.as_ref(), |out, value| {
            write_u64(out, value.get());
            Ok(())
        })
        .map_err(StateError::Encoding)?;

        let encoded_valid_qc = match &self.valid_qc {
            Some(qc) => Some(qc.encode()?),
            None => None,
        };
        write_optional(&mut out, encoded_valid_qc.as_deref(), |out, bytes| {
            write_bytes_unbounded(out, bytes)
        })
        .map_err(StateError::Encoding)?;

        write_optional(&mut out, self.last_prevote.as_ref(), |out, vote| {
            write_journaled_vote(out, vote);
            Ok(())
        })
        .map_err(StateError::Encoding)?;
        write_optional(&mut out, self.last_precommit.as_ref(), |out, vote| {
            write_journaled_vote(out, vote);
            Ok(())
        })
        .map_err(StateError::Encoding)?;

        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`VoteJournal::encode`].
    pub fn decode(bytes: &[u8]) -> Result<Self, JournalDecodeError> {
        let mut decoder = Decoder::new(bytes);

        let version = decoder.read_u8().map_err(JournalDecodeError::Encoding)?;
        if version != JOURNAL_VERSION_1 {
            return Err(JournalDecodeError::UnsupportedVersion { value: version });
        }

        let chain_id = decoder.read_u8().map_err(JournalDecodeError::Encoding)?;
        let network_id = decoder.read_u16().map_err(JournalDecodeError::Encoding)?;
        let validator_id = decoder
            .read_fixed_bytes::<32>()
            .map_err(JournalDecodeError::Encoding)?;
        let height = BlockHeight::new(decoder.read_u64().map_err(JournalDecodeError::Encoding)?);
        let round = Round::new(decoder.read_u64().map_err(JournalDecodeError::Encoding)?);
        let step_tag = decoder.read_u8().map_err(JournalDecodeError::Encoding)?;
        let step = ConsensusStep::from_u8(step_tag).ok_or(JournalDecodeError::UnknownTag {
            field: "step",
            value: step_tag,
        })?;

        let locked_value = decoder
            .read_optional(|decoder| decoder.read_fixed_bytes::<32>())
            .map_err(JournalDecodeError::Encoding)?;
        let locked_round = decoder
            .read_optional(|decoder| decoder.read_u64().map(Round::new))
            .map_err(JournalDecodeError::Encoding)?;
        let valid_value = decoder
            .read_optional(|decoder| decoder.read_fixed_bytes::<32>())
            .map_err(JournalDecodeError::Encoding)?;
        let valid_round = decoder
            .read_optional(|decoder| decoder.read_u64().map(Round::new))
            .map_err(JournalDecodeError::Encoding)?;
        let valid_qc_bytes: Option<&[u8]> = decoder
            .read_optional(read_bytes_unbounded)
            .map_err(JournalDecodeError::Encoding)?;
        let valid_qc = match valid_qc_bytes {
            Some(bytes) => {
                Some(QuorumCertificate::decode(bytes).map_err(JournalDecodeError::ValidQc)?)
            }
            None => None,
        };

        let last_prevote = decoder
            .read_optional(read_journaled_vote)
            .map_err(JournalDecodeError::Encoding)?
            .transpose()?;
        let last_precommit = decoder
            .read_optional(read_journaled_vote)
            .map_err(JournalDecodeError::Encoding)?
            .transpose()?;

        decoder.finish().map_err(JournalDecodeError::Encoding)?;

        Ok(Self {
            chain_id,
            network_id,
            validator_id,
            height,
            round,
            step,
            locked_value,
            locked_round,
            valid_value,
            valid_round,
            valid_qc,
            last_prevote,
            last_precommit,
        })
    }
}

/// Writes a byte string with no size cap of its own, unlike
/// `hn_hncs::write_bytes` (which always takes a `max_len`) — a
/// `QuorumCertificate`'s own encoded length already has every bound its
/// fields need enforced by its own `encode`/`decode`, so re-bounding the
/// *whole already-encoded blob* here would just be a second, redundant
/// (and arbitrary) limit.
fn write_bytes_unbounded(out: &mut Vec<u8>, bytes: &[u8]) -> hn_hncs::HncsResult<()> {
    let length = u32::try_from(bytes.len()).map_err(|_| HncsError::LengthFieldOverflow {
        length: bytes.len(),
    })?;
    write_u32(out, length);
    out.extend_from_slice(bytes);
    Ok(())
}

fn read_bytes_unbounded<'a>(decoder: &mut Decoder<'a>) -> hn_hncs::HncsResult<&'a [u8]> {
    // `usize::MAX` as the bound: this reads back exactly what
    // `write_bytes_unbounded` wrote, with no second length cap.
    decoder.read_bytes(usize::MAX)
}

fn write_journaled_vote(out: &mut Vec<u8>, vote: &JournaledVote) {
    write_u64(out, vote.height.get());
    write_u64(out, vote.round.get());
    let (tag, hash) = match vote.target {
        ConsensusTarget::Block(hash) => (0x01_u8, hash),
        ConsensusTarget::Nil => (0x02_u8, [0_u8; 32]),
    };
    write_u8(out, tag);
    write_fixed_bytes(out, &hash);
}

fn read_journaled_vote(
    decoder: &mut Decoder<'_>,
) -> hn_hncs::HncsResult<Result<JournaledVote, JournalDecodeError>> {
    let height = decoder.read_u64()?;
    let round = decoder.read_u64()?;
    let tag = decoder.read_u8()?;
    let hash = decoder.read_fixed_bytes::<32>()?;

    let target = match tag {
        0x01 => ConsensusTarget::Block(hash),
        0x02 => {
            if hash != [0_u8; 32] {
                return Ok(Err(JournalDecodeError::NonCanonicalNilTarget));
            }
            ConsensusTarget::Nil
        }
        other => {
            return Ok(Err(JournalDecodeError::UnknownTag {
                field: "journaled_vote.target",
                value: other,
            }));
        }
    };

    Ok(Ok(JournaledVote {
        height: BlockHeight::new(height),
        round: Round::new(round),
        target,
    }))
}

/// What a validator should do about its vote journal at startup
/// (ADR-0048 rules 3 and 4; rule 5 is deferred, see [`Self::Vote`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalStartupAction {
    /// No usable prior voting record applies — start fresh and vote
    /// normally from this height.
    ///
    /// Covers two cases:
    /// - rule 3's middle case: a journal at or below `latest_finalized`
    ///   is stale and safely discarded (per "Commit ordering," this
    ///   validator cannot have voted at a height the store has not yet
    ///   moved past, since the journal for `h + 1` is created only
    ///   after block `h` commits);
    /// - rule 5's own genesis case (empty store, no journal) — **rule
    ///   5 is deferred**: distinguishing a true, network-wide genesis
    ///   bootstrap from a validator rejoining with a wiped data
    ///   directory needs a peer's finalized height at handshake time,
    ///   which does not exist on the wire yet. This falls back to this
    ///   codebase's behavior from before ADR-0048 for exactly this one
    ///   case — a known, documented gap, not a silently accepted one.
    Vote,
    /// Rule 3's first case: the journal's own height matches
    /// `latest_finalized + 1` exactly. Restore the state machine from
    /// it (lock, valid value, round, step) before resuming.
    Restore(Box<VoteJournal>),
    /// Rule 4: the journal is missing or corrupt and the block store is
    /// non-empty — this validator cannot know what it already signed.
    /// Abstain from voting through `abstain_through` (still participate
    /// otherwise: apply finalized blocks, sync, may propose) and resume
    /// voting at `abstain_through.checked_next()`.
    AbstainThrough {
        /// The one height to abstain through.
        abstain_through: BlockHeight,
    },
    /// Rule 3's fatal case: the journal's own height is higher than
    /// `latest_finalized + 1` — the block store has lost data relative
    /// to what this validator already voted on. The caller must log
    /// both heights and exit with a dedicated non-zero code; it must
    /// not continue in any role (ADR-0048: "It does not continue as a
    /// non-voting observer: running in any role on a store that is
    /// known to be inconsistent risks further corruption and hides the
    /// problem from the operator").
    Fatal {
        /// The journal's own height.
        journal_height: BlockHeight,
        /// The block store's own latest finalized height (`BlockHeight::GENESIS`
        /// for an empty store).
        store_height: BlockHeight,
    },
}

/// What [`VoteJournal::decode`]/the caller's own file read found at
/// startup.
#[derive(Debug)]
pub enum JournalReadOutcome {
    /// No journal file exists at all.
    Missing,
    /// A journal file exists but did not decode (truncated, wrong
    /// version, ...).
    Corrupt(JournalDecodeError),
    /// A journal file exists and decoded successfully.
    Present(Box<VoteJournal>),
}

/// Rules 3 and 4 (rule 5 deferred — see [`JournalStartupAction::Vote`]).
///
/// `latest_finalized` mirrors exactly what `hn-node`'s own startup
/// already computes from `BlockStore::latest_finalized_block` (`None`
/// for an empty store, `Some(height)` otherwise) — callers should pass
/// that value straight through, not re-derive it.
pub fn decide_startup_action(
    journal: JournalReadOutcome,
    latest_finalized: Option<BlockHeight>,
) -> Result<JournalStartupAction, PrimitiveError> {
    let store_height = latest_finalized.unwrap_or(BlockHeight::GENESIS);
    let expected_height = store_height.checked_next()?;

    match journal {
        JournalReadOutcome::Missing | JournalReadOutcome::Corrupt(_) => {
            if latest_finalized.is_some() {
                Ok(JournalStartupAction::AbstainThrough {
                    abstain_through: expected_height,
                })
            } else {
                Ok(JournalStartupAction::Vote)
            }
        }
        JournalReadOutcome::Present(record) => {
            if record.height == expected_height {
                Ok(JournalStartupAction::Restore(record))
            } else if record.height.get() <= store_height.get() {
                Ok(JournalStartupAction::Vote)
            } else {
                Ok(JournalStartupAction::Fatal {
                    journal_height: record.height,
                    store_height,
                })
            }
        }
    }
}

/// Rule 2: "For a given (height, round, vote type), if the journal
/// holds a vote, the node may re-send exactly that vote (same target)
/// and never sign a different one. This also protects against a bug
/// that tries to sign twice."
///
/// `journaled` is whatever [`VoteJournal::last_prevote`] or
/// [`VoteJournal::last_precommit`] (matching the vote type actually
/// being cast) already held *before* this call — the caller's own
/// vote-casting site must consult this before signing anything new.
/// Returns the target that must actually be signed and sent: the
/// journaled one, verbatim, if it already covers this exact `(height,
/// round)`; the freshly decided `proposed_target` otherwise (nothing
/// was journaled for this round yet, so there is nothing to protect
/// against re-signing differently).
#[must_use]
pub fn vote_to_send(
    journaled: Option<&JournaledVote>,
    height: BlockHeight,
    round: Round,
    proposed_target: ConsensusTarget,
) -> ConsensusTarget {
    match journaled {
        Some(vote) if vote.height == height && vote.round == round => vote.target,
        _ => proposed_target,
    }
}

#[cfg(test)]
mod tests {
    use hn_core::Epoch;
    use hn_state::{VoteTargetType, VoteType};

    use super::*;

    const VALIDATOR_ID: Digest = [0x11; 32];
    const BLOCK_A: Digest = [0xAA; 32];

    fn sample_qc(height: u64, round: u64, target_hash: Digest) -> QuorumCertificate {
        QuorumCertificate {
            certificate_type: VoteType::Prevote,
            chain_id: 7,
            network_id: 1,
            epoch: Epoch::new(0),
            height: BlockHeight::new(height),
            round: Round::new(round),
            validator_set_commitment: [0x22; 32],
            target_type: VoteTargetType::Block,
            target_hash,
            total_voting_power: 400,
            signed_voting_power: 300,
            signer_commitment: Vec::new(),
            aggregate_proof: Vec::new(),
        }
    }

    fn minimal_journal() -> VoteJournal {
        VoteJournal {
            chain_id: 7,
            network_id: 1,
            validator_id: VALIDATOR_ID,
            height: BlockHeight::new(5),
            round: Round::new(2),
            step: ConsensusStep::Propose,
            locked_value: None,
            locked_round: None,
            valid_value: None,
            valid_round: None,
            valid_qc: None,
            last_prevote: None,
            last_precommit: None,
        }
    }

    fn fully_populated_journal() -> VoteJournal {
        VoteJournal {
            locked_value: Some(BLOCK_A),
            locked_round: Some(Round::new(1)),
            valid_value: Some(BLOCK_A),
            valid_round: Some(Round::new(1)),
            valid_qc: Some(sample_qc(5, 1, BLOCK_A)),
            last_prevote: Some(JournaledVote {
                height: BlockHeight::new(5),
                round: Round::new(2),
                target: ConsensusTarget::Block(BLOCK_A),
            }),
            last_precommit: Some(JournaledVote {
                height: BlockHeight::new(5),
                round: Round::new(1),
                target: ConsensusTarget::Nil,
            }),
            ..minimal_journal()
        }
    }

    #[test]
    fn round_trips_a_minimal_journal() -> Result<(), StateError> {
        let journal = minimal_journal();
        let bytes = journal.encode()?;
        assert_eq!(VoteJournal::decode(&bytes), Ok(journal));
        Ok(())
    }

    #[test]
    fn round_trips_a_fully_populated_journal() -> Result<(), StateError> {
        let journal = fully_populated_journal();
        let bytes = journal.encode()?;
        assert_eq!(VoteJournal::decode(&bytes), Ok(journal));
        Ok(())
    }

    #[test]
    fn rejects_an_unsupported_version() -> Result<(), StateError> {
        let mut bytes = minimal_journal().encode()?;
        bytes[0] = JOURNAL_VERSION_1 + 1;
        assert_eq!(
            VoteJournal::decode(&bytes),
            Err(JournalDecodeError::UnsupportedVersion {
                value: JOURNAL_VERSION_1 + 1
            })
        );
        Ok(())
    }

    #[test]
    fn rejects_an_unknown_step_tag() -> Result<(), StateError> {
        let mut bytes = minimal_journal().encode()?;
        // version(1) + chain_id(1) + network_id(2) + validator_id(32) +
        // height(8) + round(8) = offset 52, where the step tag lives.
        let step_offset = 1 + 1 + 2 + 32 + 8 + 8;
        bytes[step_offset] = 0x00;
        assert_eq!(
            VoteJournal::decode(&bytes),
            Err(JournalDecodeError::UnknownTag {
                field: "step",
                value: 0x00
            })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> Result<(), StateError> {
        let mut bytes = minimal_journal().encode()?;
        bytes.push(0xFF);
        assert!(matches!(
            VoteJournal::decode(&bytes),
            Err(JournalDecodeError::Encoding(
                HncsError::TrailingBytes { .. }
            ))
        ));
        Ok(())
    }

    #[test]
    fn rejects_a_non_canonical_nil_journaled_vote() -> Result<(), StateError> {
        let mut journal = minimal_journal();
        journal.last_prevote = Some(JournaledVote {
            height: BlockHeight::new(5),
            round: Round::new(2),
            target: ConsensusTarget::Nil,
        });
        let bytes = journal.encode()?;

        // Flip the encoded nil hash's leading byte so it is no longer
        // all-zero, simulating a corrupted/non-canonical record.
        let hash_offset = bytes.len() - 32;
        let mut tampered = bytes;
        tampered[hash_offset] = 0x01;

        assert_eq!(
            VoteJournal::decode(&tampered),
            Err(JournalDecodeError::NonCanonicalNilTarget)
        );
        Ok(())
    }

    // --- decide_startup_action: rules 3 and 4 ---

    #[test]
    fn restores_when_journal_height_matches_latest_finalized_plus_one() -> Result<(), PrimitiveError>
    {
        let journal = VoteJournal {
            height: BlockHeight::new(6),
            ..minimal_journal()
        };
        let action = decide_startup_action(
            JournalReadOutcome::Present(Box::new(journal.clone())),
            Some(BlockHeight::new(5)),
        )?;
        assert_eq!(action, JournalStartupAction::Restore(Box::new(journal)));
        Ok(())
    }

    #[test]
    fn discards_a_stale_journal_exactly_at_latest_finalized() -> Result<(), PrimitiveError> {
        let journal = VoteJournal {
            height: BlockHeight::new(5),
            ..minimal_journal()
        };
        let action = decide_startup_action(
            JournalReadOutcome::Present(Box::new(journal)),
            Some(BlockHeight::new(5)),
        )?;
        assert_eq!(action, JournalStartupAction::Vote);
        Ok(())
    }

    #[test]
    fn discards_a_stale_journal_below_latest_finalized() -> Result<(), PrimitiveError> {
        let journal = VoteJournal {
            height: BlockHeight::new(2),
            ..minimal_journal()
        };
        let action = decide_startup_action(
            JournalReadOutcome::Present(Box::new(journal)),
            Some(BlockHeight::new(5)),
        )?;
        assert_eq!(action, JournalStartupAction::Vote);
        Ok(())
    }

    #[test]
    fn is_fatal_when_journal_height_exceeds_latest_finalized_plus_one() -> Result<(), PrimitiveError>
    {
        let journal = VoteJournal {
            height: BlockHeight::new(9),
            ..minimal_journal()
        };
        let action = decide_startup_action(
            JournalReadOutcome::Present(Box::new(journal)),
            Some(BlockHeight::new(5)),
        )?;
        assert_eq!(
            action,
            JournalStartupAction::Fatal {
                journal_height: BlockHeight::new(9),
                store_height: BlockHeight::new(5),
            }
        );
        Ok(())
    }

    #[test]
    fn abstains_when_journal_missing_and_store_non_empty() -> Result<(), PrimitiveError> {
        let action = decide_startup_action(JournalReadOutcome::Missing, Some(BlockHeight::new(5)))?;
        assert_eq!(
            action,
            JournalStartupAction::AbstainThrough {
                abstain_through: BlockHeight::new(6)
            }
        );
        Ok(())
    }

    #[test]
    fn abstains_when_journal_corrupt_and_store_non_empty() -> Result<(), PrimitiveError> {
        let action = decide_startup_action(
            JournalReadOutcome::Corrupt(JournalDecodeError::UnsupportedVersion { value: 9 }),
            Some(BlockHeight::new(5)),
        )?;
        assert_eq!(
            action,
            JournalStartupAction::AbstainThrough {
                abstain_through: BlockHeight::new(6)
            }
        );
        Ok(())
    }

    /// Rule 5 is deferred (see `JournalStartupAction::Vote`'s own doc):
    /// an empty store with no journal falls back to this codebase's
    /// pre-ADR-0048 behavior rather than abstaining, since abstaining
    /// here would deadlock a genuine, simultaneous genesis bootstrap
    /// (every validator would refuse to vote at height 1 forever).
    #[test]
    fn votes_normally_when_store_is_empty_and_journal_is_missing() -> Result<(), PrimitiveError> {
        let action = decide_startup_action(JournalReadOutcome::Missing, None)?;
        assert_eq!(action, JournalStartupAction::Vote);
        Ok(())
    }

    #[test]
    fn restores_from_an_empty_store_when_the_journal_is_already_at_height_one()
    -> Result<(), PrimitiveError> {
        let journal = VoteJournal {
            height: BlockHeight::new(1),
            ..minimal_journal()
        };
        let action =
            decide_startup_action(JournalReadOutcome::Present(Box::new(journal.clone())), None)?;
        assert_eq!(action, JournalStartupAction::Restore(Box::new(journal)));
        Ok(())
    }

    // --- "Commit ordering" scenarios, named after the ADR's own text ---

    /// "Crash after the store committed h but before the journal moved
    /// on: the journal says h, latest_finalized + 1 is h + 1, rule 3
    /// discards it. Safe, because the node had not voted at h + 1."
    #[test]
    fn commit_ordering_crash_after_store_commits_h_before_journal_moves_on()
    -> Result<(), PrimitiveError> {
        let height_h = BlockHeight::new(10);
        let journal = VoteJournal {
            height: height_h,
            ..minimal_journal()
        };
        let action = decide_startup_action(
            JournalReadOutcome::Present(Box::new(journal)),
            Some(height_h),
        )?;
        assert_eq!(action, JournalStartupAction::Vote);
        Ok(())
    }

    /// "Crash while committing h (store transaction not durable): the
    /// store is at h - 1, the journal at h is valid and is restored,
    /// the node re-enters height h with its lock intact."
    #[test]
    fn commit_ordering_crash_while_committing_h_restores_from_the_journal()
    -> Result<(), PrimitiveError> {
        let height_h = BlockHeight::new(10);
        let height_h_minus_one = BlockHeight::new(9);
        let journal = VoteJournal {
            height: height_h,
            locked_value: Some(BLOCK_A),
            locked_round: Some(Round::new(0)),
            ..minimal_journal()
        };
        let action = decide_startup_action(
            JournalReadOutcome::Present(Box::new(journal.clone())),
            Some(height_h_minus_one),
        )?;
        assert_eq!(action, JournalStartupAction::Restore(Box::new(journal)));
        Ok(())
    }

    // --- vote_to_send: rule 2 ---

    #[test]
    fn sends_the_proposed_target_when_nothing_was_journaled_for_this_round() {
        let target = vote_to_send(
            None,
            BlockHeight::new(5),
            Round::new(1),
            ConsensusTarget::Nil,
        );
        assert_eq!(target, ConsensusTarget::Nil);
    }

    #[test]
    fn sends_the_proposed_target_when_the_journaled_vote_is_for_a_different_round() {
        let journaled = JournaledVote {
            height: BlockHeight::new(5),
            round: Round::new(0),
            target: ConsensusTarget::Block(BLOCK_A),
        };
        let target = vote_to_send(
            Some(&journaled),
            BlockHeight::new(5),
            Round::new(1),
            ConsensusTarget::Nil,
        );
        assert_eq!(target, ConsensusTarget::Nil);
    }

    #[test]
    fn re_sends_the_journaled_target_for_the_same_round_even_if_it_differs_from_what_was_just_decided()
     {
        let journaled = JournaledVote {
            height: BlockHeight::new(5),
            round: Round::new(1),
            target: ConsensusTarget::Block(BLOCK_A),
        };
        // Whatever the state machine just decided this time (here,
        // Nil) must never override an already-signed vote for the
        // exact same (height, round) -- this is the entire protection
        // rule 2 exists for.
        let target = vote_to_send(
            Some(&journaled),
            BlockHeight::new(5),
            Round::new(1),
            ConsensusTarget::Nil,
        );
        assert_eq!(target, ConsensusTarget::Block(BLOCK_A));
    }
}

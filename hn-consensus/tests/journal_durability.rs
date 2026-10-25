//! ADR-0048's own "Verification Requirements" item 4 ("crash tests for
//! the journal"), the part of it that can be exercised without a real
//! node process: `VoteJournal::encode`/`decode` driven through the
//! actual `hn_storage::write_atomically`/`read_optional` primitive
//! (rather than the in-process-only round-trip `journal.rs`'s own unit
//! tests use), plus `decide_startup_action` on what a fresh read of
//! that real file produces.
//!
//! The two tests this crate cannot host on its own — "4 processes, kill
//! one right after it prevoted, restart it, assert no conflicting
//! vote" and "truncate the block store under a valid journal, assert
//! the fatal exit of rule 3" — need a real `hn-node` process and are
//! deferred to phase 4 of ADR-0048 (`hn-node` does not build yet; see
//! this crate's own recent commits).

use std::path::Path;

use hn_consensus::{
    ConsensusStep, ConsensusTarget, JournalReadOutcome, JournalStartupAction, JournaledVote,
    VoteJournal, decide_startup_action,
};
use hn_core::{BlockHeight, Round};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const VALIDATOR_ID: [u8; 32] = [0x01; 32];
const BLOCK_A: [u8; 32] = [0xAA; 32];

fn sample_journal(height: u64, round: u64) -> VoteJournal {
    VoteJournal {
        chain_id: 7,
        network_id: 1,
        validator_id: VALIDATOR_ID,
        height: BlockHeight::new(height),
        round: Round::new(round),
        step: ConsensusStep::Precommit,
        locked_value: Some(BLOCK_A),
        locked_round: Some(Round::new(round)),
        valid_value: Some(BLOCK_A),
        valid_round: Some(Round::new(round)),
        valid_qc: None,
        last_prevote: Some(JournaledVote {
            height: BlockHeight::new(height),
            round: Round::new(round),
            target: ConsensusTarget::Block(BLOCK_A),
        }),
        last_precommit: None,
    }
}

fn read_journal(path: &Path) -> TestResult<JournalReadOutcome> {
    Ok(match hn_storage::read_optional(path)? {
        None => JournalReadOutcome::Missing,
        Some(bytes) => match VoteJournal::decode(&bytes) {
            Ok(journal) => JournalReadOutcome::Present(Box::new(journal)),
            Err(error) => JournalReadOutcome::Corrupt(error),
        },
    })
}

/// The ordinary path: write, "restart" (nothing survives in-process —
/// this test reads the file fresh, not the original `VoteJournal`
/// value), restore.
#[test]
fn a_journal_written_then_read_back_fresh_restores_identically() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("vote_journal");
    let journal = sample_journal(10, 1);

    hn_storage::write_atomically(&path, &journal.encode()?)?;

    let outcome = read_journal(&path)?;
    let action = decide_startup_action(outcome, Some(BlockHeight::new(9)))?;
    assert_eq!(action, JournalStartupAction::Restore(Box::new(journal)));
    Ok(())
}

/// Rule 4: no file at all (never written, or the data directory is
/// genuinely fresh for the journal specifically) with a non-empty
/// store abstains rather than guessing.
#[test]
fn a_missing_journal_file_with_a_non_empty_store_abstains() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("vote_journal");

    let outcome = read_journal(&path)?;
    let action = decide_startup_action(outcome, Some(BlockHeight::new(9)))?;
    assert_eq!(
        action,
        JournalStartupAction::AbstainThrough {
            abstain_through: BlockHeight::new(10)
        }
    );
    Ok(())
}

/// Rule 4's other half: a file exists but does not decode (here,
/// truncated — the same observable shape a disk-corruption or a crash
/// mid-write-without-the-atomic-primitive would leave, though the
/// atomic primitive itself is tested separately in `hn-storage`).
#[test]
fn a_truncated_journal_file_with_a_non_empty_store_abstains() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("vote_journal");
    let journal = sample_journal(10, 1);
    let mut bytes = journal.encode()?;
    bytes.truncate(bytes.len() / 2);
    hn_storage::write_atomically(&path, &bytes)?;

    let outcome = read_journal(&path)?;
    assert!(matches!(outcome, JournalReadOutcome::Corrupt(_)));
    let action = decide_startup_action(outcome, Some(BlockHeight::new(9)))?;
    assert_eq!(
        action,
        JournalStartupAction::AbstainThrough {
            abstain_through: BlockHeight::new(10)
        }
    );
    Ok(())
}

/// Simulates "drop all in-memory state right after the journal was
/// written, before the vote itself was ever sent" (Verification
/// Requirement 4): writing the journal is the complete, durable
/// action by itself — nothing further is needed for a subsequent
/// restart to recover the exact same lock/valid state and resume
/// voting from exactly where it left off.
#[test]
fn restarting_immediately_after_the_journal_write_recovers_the_exact_pre_send_state() -> TestResult
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("vote_journal");
    let journal = sample_journal(10, 2);

    // "Write before send": the write below is the entirety of rule 1's
    // own durable action. A real caller would sign and send the vote
    // immediately after this returns; simulating "the process died
    // before that happened" means doing nothing more here at all.
    hn_storage::write_atomically(&path, &journal.encode()?)?;

    let restored = match decide_startup_action(read_journal(&path)?, Some(BlockHeight::new(9)))? {
        JournalStartupAction::Restore(restored) => restored,
        other => return Err(format!("expected Restore, got {other:?}").into()),
    };
    assert_eq!(restored.locked_value, journal.locked_value);
    assert_eq!(restored.locked_round, journal.locked_round);
    assert_eq!(restored.step, journal.step);
    assert_eq!(restored.last_prevote, journal.last_prevote);
    Ok(())
}

/// A second write (the next vote's own journal update) fully replaces
/// the first on disk -- restarting after it must never resurrect the
/// now-superseded earlier state.
#[test]
fn a_later_write_fully_supersedes_an_earlier_one_after_a_simulated_restart() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("vote_journal");

    hn_storage::write_atomically(&path, &sample_journal(10, 1).encode()?)?;
    let second = sample_journal(10, 2);
    hn_storage::write_atomically(&path, &second.encode()?)?;

    let action = decide_startup_action(read_journal(&path)?, Some(BlockHeight::new(9)))?;
    assert_eq!(action, JournalStartupAction::Restore(Box::new(second)));
    Ok(())
}

#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Consensus state machine boundaries for HNChain.
//!
//! This crate must not perform network I/O directly and must not rely on node
//! process lifecycle behavior.
//!
//! [`ConsensusState`] (ADR-0009, "Timeout And View Change"; ADR-0034,
//! "Consensus State Machine Skeleton") is this crate's first real
//! content: the Tendermint-style round state machine ADR-0009 already
//! decided the shape of (`propose -> prevote -> precommit`, each with
//! its own timeout; a round's own absence of a qualifying `precommit`
//! quorum before timeout is itself sufficient justification to advance
//! — no separate timeout-certificate object). [`ConsensusStep`] gives
//! every stage the diagram names a directly observable value —
//! `NewHeight`, `Propose`, `Prevote`, `Precommit`, `Finalize`,
//! `Timeout` — rather than treating the terminal outcomes as implicit.
//! [`ConsensusEvent`] is what a driving caller feeds in (a proposal, an
//! already-verified [`hn_state::QuorumCertificate`], or a timeout it
//! decided to fire) and [`ConsensusAction`] is what
//! [`ConsensusState::apply`] decides the local validator should do
//! about it (cast a vote, treat a block as final, advance a round) —
//! `ConsensusState` itself never verifies a signature, computes a
//! quorum, checks validator eligibility, or does any signing/networking
//! itself; the same "this layer trusts already-resolved input" boundary
//! [`hn_state::ConsensusVote::verify`]/
//! [`hn_state::QuorumCertificate::verify_signatures`] already draw one
//! layer down.
//!
//! [`ConsensusState`]'s own `locked_value`/`locked_round`/
//! `valid_value`/`valid_round`/`valid_qc` (ADR-0048, "Decided:
//! ConsensusState v2") are Tendermint's textbook `lockedValue`/
//! `lockedRound`/`validValue`/`validRound` pairs, tracked separately —
//! ADR-0034's own merged `locked_block`/`highest_qc` field is
//! superseded. ADR-0048's own line-by-line audit found the merge was
//! not actually the safety hole ADR-0034 feared (`highest_qc.round`
//! was always exactly `lockedRound`); the real holes it closes are
//! unpersisted voting state across a restart (R1–R11 plus a vote
//! journal, a later pass), locking without a validated proposal (R6),
//! no equivocation evidence, no re-proposal of a partial lock's own
//! `validValue` (R1), no round skipping for a lagging validator
//! (R10), and no decide-at-any-round (R8).
//!
//! [`ConsensusEngine`] (ADR-0035, "Wiring The Consensus Engine To
//! `hn-state`") is what actually calls the verification/eligibility/
//! persistence primitives `ConsensusState` itself stays deliberately
//! unaware of, wrapping the pure state machine with exactly three real
//! connections: `handle_quorum_certificate`/`verify_vote` call
//! `QuorumCertificate::verify_signatures`/`ConsensusVote::verify` plus
//! `hn_state::is_eligible_signer` (against an `ordered_active_set` the
//! caller computed via `hn_state::active_set` — this crate cannot
//! enumerate storage any more than `hn-state` itself can) before
//! trusting an event; `commit_finalized_block` is `hn_state::
//! apply_and_commit_block`'s first real caller anywhere outside
//! `hn-state`/`hn-storage`'s own tests, applying and atomically
//! committing a finalized block's transactions to a real backend. Its
//! `proposed_blocks` cache — an in-memory `block_hash -> transactions`
//! map, populated by `handle_proposal` — is deliberately not
//! ADR-0019's `BlockStore`: real block/header/receipts storage remains
//! undecided, named explicitly in ADR-0035's own "Explicitly Not
//! Resolved" rather than guessed at ahead of a real need.
//!
//! [`VotePool`] (ADR-0037, "Decided: Vote Aggregation") closes the gap
//! ADR-0035 left open: `ConsensusEngine::record_vote` calls
//! `verify_vote` and then feeds the now-trusted vote into the pool,
//! returning a freshly-built `hn_state::QuorumCertificate` the moment
//! one target's signed voting power crosses quorum — ready to hand
//! straight to `handle_quorum_certificate`. [`round_proposer`]
//! (ADR-0037, "Decided: Leader Election Scaffold") picks who proposes a
//! given height/round — ADR-0011's own explicitly-sanctioned devnet
//! baseline, not its still-open real formula.
//!
//! [`VoteJournal`] (ADR-0048, "Decided: Vote Journal (Write-Ahead
//! Safety State)") is the persisted counterpart to `ConsensusState`'s
//! own lock/valid state: this module owns its shape, its wire encoding,
//! and [`decide_startup_action`]'s pure restore/abstain/fatal decision
//! (rules 3 and 4) — but performs no file I/O itself, matching this
//! crate's own "no network I/O, no node process lifecycle" boundary.
//! The durable write/read (`hn_storage::write_atomically`/
//! `read_optional`), the fatal exit, and the per-vote write-before-send
//! hook (rules 1 and 2) are `hn-node`'s own job, wiring this module's
//! pure pieces to a real data directory. Rule 5 (genesis bootstrap vs.
//! a rejoining validator with a wiped data directory) is deferred — see
//! [`JournalStartupAction::Vote`]'s own documentation.

mod action;
mod engine;
mod error;
mod event;
mod journal;
mod leader;
mod state;
mod step;
mod vote_pool;

pub use action::ConsensusAction;
pub use engine::ConsensusEngine;
pub use error::{ConsensusError, ConsensusResult};
pub use event::{ConsensusEvent, ConsensusTarget};
pub use journal::{
    JOURNAL_VERSION_1, JournalDecodeError, JournalReadOutcome, JournalStartupAction, JournaledVote,
    VoteJournal, decide_startup_action,
};
pub use leader::round_proposer;
pub use state::ConsensusState;
pub use step::ConsensusStep;
pub use vote_pool::{EquivocationEvidence, VotePool};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}

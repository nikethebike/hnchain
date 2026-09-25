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
//! already-quorum-checked [`hn_state::QuorumCertificate`], or a timeout
//! it decided to fire) and [`ConsensusAction`] is what
//! [`ConsensusState::apply`] decides the local validator should do
//! about it (cast a vote, treat a block as final, advance a round) —
//! this crate never verifies a signature, computes a quorum, checks
//! validator eligibility, or does any signing/networking itself; all of
//! that is the caller's job before constructing an event, the same
//! "this layer trusts already-resolved input" boundary
//! [`hn_state::ConsensusVote::verify`]/
//! [`hn_state::QuorumCertificate::verify_signatures`] already draw one
//! layer down.
//!
//! `locked_block`/`highest_qc` (ADR-0034, "Locking rule") are a
//! deliberate simplification of Tendermint's textbook algorithm, which
//! tracks `lockedRound`/`validRound` as two separate pairs — merged
//! into one field here, named explicitly as not yet a byzantine-safe
//! claim in ADR-0034's own "Explicitly Not Resolved."

mod action;
mod error;
mod event;
mod state;
mod step;

pub use action::ConsensusAction;
pub use error::{ConsensusError, ConsensusResult};
pub use event::{ConsensusEvent, ConsensusTarget};
pub use state::ConsensusState;
pub use step::ConsensusStep;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}

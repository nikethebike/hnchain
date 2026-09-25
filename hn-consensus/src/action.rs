use hn_core::{BlockHeight, Round};
use hn_crypto::Digest;
use hn_state::QuorumCertificate;

use crate::event::ConsensusTarget;

/// What [`crate::ConsensusState::apply`] decides the local validator
/// should do, given an event (ADR-0034, "Consensus State Machine
/// Skeleton"). This crate never signs or broadcasts anything itself —
/// an `Action` is an instruction for the driving caller to act on
/// (cast a vote, treat a block as final, move to the next round), not
/// a performed effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConsensusAction {
    /// No action — an explicit `BeginRound`/`BeginNewHeight` transition
    /// that has nothing for the caller to sign or broadcast.
    None,
    /// Cast a `prevote` for `target`.
    Prevote(ConsensusTarget),
    /// Cast a `precommit` for `target`.
    Precommit(ConsensusTarget),
    /// `block_hash` is final at this height, certified by `commit_qc` —
    /// ADR-0013's own "one `precommit` `QuorumCertificate` is
    /// sufficient" rule. Not yet a `FinalityProof` (ADR-0034,
    /// "Explicitly Not Resolved").
    Finalized {
        /// The finalized block's hash.
        block_hash: Digest,
        /// The `precommit` quorum certificate that finalized it.
        commit_qc: QuorumCertificate,
    },
    /// No qualifying `precommit` quorum formed this round; the state
    /// machine is about to attempt `round` at the same height.
    RoundAdvanced {
        /// The round about to begin.
        round: Round,
    },
    /// Finalization complete; the state machine is about to begin
    /// `height`.
    NewHeight {
        /// The height about to begin.
        height: BlockHeight,
    },
}

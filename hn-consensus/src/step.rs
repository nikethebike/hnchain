/// A round's position within the Tendermint-style state machine
/// (ADR-0009, "Timeout And View Change"; ADR-0034, "Consensus State
/// Machine Skeleton") — `NewHeight -> Propose -> Prevote -> Precommit
/// -> Finalize/Timeout`. `Finalize`/`Timeout` are real, directly
/// observable resting values here, not just names for an edge: a test
/// can assert `state.step == ConsensusStep::Finalize` immediately after
/// a qualifying `precommit` quorum, before anything decides what
/// happens next.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ConsensusStep {
    /// Freshly entered a height; not yet attempting a round.
    NewHeight = 0x01,
    /// Waiting for a valid proposal (or its timeout) at the current
    /// round.
    Propose = 0x02,
    /// Waiting for a `prevote` quorum (or its timeout) at the current
    /// round.
    Prevote = 0x03,
    /// Waiting for a `precommit` quorum (or its timeout) at the current
    /// round.
    Precommit = 0x04,
    /// A `precommit` quorum formed for a real block — this height is
    /// final.
    Finalize = 0x05,
    /// No qualifying `precommit` quorum formed before timeout — this
    /// round failed; the next round (same height) is about to begin.
    Timeout = 0x06,
}

impl ConsensusStep {
    /// This step's own wire tag (ADR-0048, "Decided: Vote Journal" —
    /// the journal persists `step` directly). `pub(crate)`: only
    /// [`crate::journal`] needs it so far.
    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Recovers a step from [`Self::as_u8`]'s own tag, or `None` for any
    /// value this version never wrote.
    pub(crate) fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::NewHeight),
            0x02 => Some(Self::Propose),
            0x03 => Some(Self::Prevote),
            0x04 => Some(Self::Precommit),
            0x05 => Some(Self::Finalize),
            0x06 => Some(Self::Timeout),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ConsensusStep;

    #[test]
    fn every_step_round_trips_through_its_own_wire_tag() {
        for step in [
            ConsensusStep::NewHeight,
            ConsensusStep::Propose,
            ConsensusStep::Prevote,
            ConsensusStep::Precommit,
            ConsensusStep::Finalize,
            ConsensusStep::Timeout,
        ] {
            assert_eq!(ConsensusStep::from_u8(step.as_u8()), Some(step));
        }
    }

    #[test]
    fn rejects_an_unknown_wire_tag() {
        assert_eq!(ConsensusStep::from_u8(0x00), None);
        assert_eq!(ConsensusStep::from_u8(0x07), None);
    }
}

use hn_core::{BlockHeight, Round};
use hn_crypto::Digest;

/// Selects `ordered_active_set`'s proposer for `height`/`round` (ADR-0037,
/// "Decided: Leader Election Scaffold — Deterministic Round-Robin").
///
/// **Not** ADR-0011's final decision. ADR-0011 decided the mechanism
/// (deterministic weighted round-robin) but deliberately left the exact
/// proposer-priority arithmetic open, and its own "Rejected Options"
/// section explicitly sanctions plain static rotation "as a simple
/// baseline for devnet or early testing if it is clearly marked
/// non-production" — this function is exactly that baseline, not a step
/// toward the real formula.
///
/// `ordered_active_set` must be non-empty and in the same
/// ascending-`validator_id` order every other consumer of an active set
/// in this codebase already assumes; `ordered_active_set[index]` where
/// `index = (height + round) % len` — both height and round feed the
/// rotation so round 0 of every height does not always select the same
/// proposer.
#[must_use]
pub fn round_proposer(
    ordered_active_set: &[Digest],
    height: BlockHeight,
    round: Round,
) -> Option<Digest> {
    let len = u64::try_from(ordered_active_set.len()).ok()?;
    if len == 0 {
        return None;
    }
    let index = height.get().wrapping_add(round.get()) % len;
    ordered_active_set.get(index as usize).copied()
}

#[cfg(test)]
mod tests {
    use hn_core::{BlockHeight, Round};

    use super::round_proposer;

    const V0: [u8; 32] = [0x01; 32];
    const V1: [u8; 32] = [0x02; 32];
    const V2: [u8; 32] = [0x03; 32];

    #[test]
    fn rotates_by_round_within_one_height() {
        let set = [V0, V1, V2];
        assert_eq!(
            round_proposer(&set, BlockHeight::new(0), Round::new(0)),
            Some(V0)
        );
        assert_eq!(
            round_proposer(&set, BlockHeight::new(0), Round::new(1)),
            Some(V1)
        );
        assert_eq!(
            round_proposer(&set, BlockHeight::new(0), Round::new(2)),
            Some(V2)
        );
        assert_eq!(
            round_proposer(&set, BlockHeight::new(0), Round::new(3)),
            Some(V0)
        );
    }

    #[test]
    fn round_zero_does_not_repeat_the_same_proposer_every_height() {
        let set = [V0, V1, V2];
        assert_eq!(
            round_proposer(&set, BlockHeight::new(0), Round::new(0)),
            Some(V0)
        );
        assert_eq!(
            round_proposer(&set, BlockHeight::new(1), Round::new(0)),
            Some(V1)
        );
        assert_eq!(
            round_proposer(&set, BlockHeight::new(2), Round::new(0)),
            Some(V2)
        );
    }

    #[test]
    fn returns_none_for_an_empty_active_set() {
        assert_eq!(
            round_proposer(&[], BlockHeight::new(0), Round::new(0)),
            None
        );
    }
}

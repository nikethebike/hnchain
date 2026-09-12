use hn_crypto::{Digest, hash_profile_0x0001};

use crate::error::{StateError, StateResult};
use crate::governance_payload::GovernancePayloadV1;

/// Computes a proposal's own id (ADR-0025, "Decided: `proposal_id`
/// Derivation"):
///
/// `proposal_id = HASH_PROFILE_0x0001("hnchain.governance.proposal.v1",
/// HNCS(GovernancePayloadV1 { Propose fields }))`
///
/// Mirrors [`crate::tx_id`]'s own derivation exactly — a domain-
/// separated hash over the proposal's own canonical creation content,
/// stable regardless of anything about the `propose` transaction's own
/// authorization (unlike a raw digest of the whole envelope, which
/// would make `proposal_id` depend on signature bytes).
///
/// `payload` must be [`GovernancePayloadV1::Propose`] —
/// [`StateError::ProposalIdRequiresProposeOperation`] otherwise, since
/// a `Vote` payload has no creation content of its own to derive an id
/// from.
pub fn proposal_id(payload: &GovernancePayloadV1) -> StateResult<Digest> {
    if !matches!(payload, GovernancePayloadV1::Propose { .. }) {
        return Err(StateError::ProposalIdRequiresProposeOperation);
    }
    Ok(hash_profile_0x0001(
        "hnchain.governance.proposal.v1",
        &payload.encode()?,
    )?)
}

#[cfg(test)]
mod tests {
    use super::proposal_id;
    use crate::error::StateResult;
    use crate::governance_payload::{GovernancePayloadV1, VoteChoice};

    #[test]
    fn matches_independent_oracle() -> StateResult<()> {
        let payload = GovernancePayloadV1::Propose {
            title: "Raise MAX_ACTIVE_SET_SIZE".to_string(),
            content_hash: [0xaa; 32],
        };
        assert_eq!(
            hex(&proposal_id(&payload)?),
            "a3076f1465e4f00a9bcf2432715a6ccf70075cd29613bc91374c7ce7c1d88a21"
        );
        Ok(())
    }

    #[test]
    fn distinct_proposals_hash_differently() -> StateResult<()> {
        let a = GovernancePayloadV1::Propose {
            title: "Proposal A".to_string(),
            content_hash: [0x11; 32],
        };
        let b = GovernancePayloadV1::Propose {
            title: "Proposal B".to_string(),
            content_hash: [0x22; 32],
        };
        assert_ne!(proposal_id(&a)?, proposal_id(&b)?);
        Ok(())
    }

    #[test]
    fn rejects_a_vote_payload() {
        let payload = GovernancePayloadV1::Vote {
            proposal_id: [0xbb; 32],
            choice: VoteChoice::For,
        };
        assert_eq!(
            proposal_id(&payload),
            Err(crate::error::StateError::ProposalIdRequiresProposeOperation)
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

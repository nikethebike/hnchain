use hn_core::Round;
use hn_crypto::{Digest, hash_profile_0x0001};
use hn_hncs::{Decoder, write_fixed_bytes, write_u8, write_u16, write_u64};

use crate::error::{StateError, StateResult};

/// `proposal_round_claim_version` for the current
/// [`ProposalRoundClaimV1`] shape (ADR-0049, mirroring ADR-0012's own
/// "Nested Structure Versions" treatment of `VoteSigningPayloadV1`).
pub const PROPOSAL_ROUND_CLAIM_VERSION_1: u16 = 1;

/// What a proposer actually signs to claim "this block is my proposal
/// for this round" (ADR-0049, "Decided: `ProposalRoundClaimV1` — A New
/// Signing Payload").
///
/// Exists because `block_hash` alone (ADR-0042's own proposer-signing
/// payload, sufficient before ADR-0049) says nothing about *which
/// round* a delivery is for — and re-proposing a locked value
/// (ADR-0048, R1) requires resending the *same* `block_hash` at a
/// later round than the one its own header names, which
/// `round_proposer`'s own periodic reuse of each validator makes
/// forgeable if the round claim is not itself part of what gets
/// signed: a relay could take a validator's real, old, legitimately
/// signed proposal and re-broadcast it claiming an arbitrary later
/// `for_round` at which `round_proposer` happens to name that same
/// validator again, and nothing would catch it. `height` is not a
/// separate field here — it is already inside `block_hash` (a
/// commitment to the entire header, height included), so binding
/// `block_hash` already binds height transitively.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProposalRoundClaimV1 {
    /// HNChain protocol lineage (ADR-0006, "Chain And Network
    /// Binding").
    pub chain_id: u8,
    /// Network environment (ADR-0003, "Decision 4").
    pub network_id: u16,
    /// The round this delivery claims to be for — not necessarily
    /// `block_hash`'s own header's `round` (ADR-0049's whole reason
    /// to exist: the two differ exactly when re-proposing).
    pub for_round: Round,
    /// The proposed block's own hash (`BlockEnvelope::block_hash`).
    pub block_hash: Digest,
}

impl ProposalRoundClaimV1 {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, PROPOSAL_ROUND_CLAIM_VERSION_1);
        write_u8(&mut out, self.chain_id);
        write_u16(&mut out, self.network_id);
        write_u64(&mut out, self.for_round.get());
        write_fixed_bytes(&mut out, &self.block_hash);
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`ProposalRoundClaimV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let version = decoder.read_u16().map_err(StateError::Encoding)?;
        if version != PROPOSAL_ROUND_CLAIM_VERSION_1 {
            return Err(StateError::UnsupportedProposalRoundClaimVersion { value: version });
        }
        let chain_id = decoder.read_u8().map_err(StateError::Encoding)?;
        let network_id = decoder.read_u16().map_err(StateError::Encoding)?;
        let for_round = Round::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let block_hash = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        decoder.finish().map_err(StateError::Encoding)?;
        Ok(Self {
            chain_id,
            network_id,
            for_round,
            block_hash,
        })
    }

    /// Computes this payload's signing digest:
    /// `HASH_PROFILE_0x0001("hnchain.proposal.roundclaim.v1", HNCS(ProposalRoundClaimV1))`
    /// (ADR-0049, mirroring `VoteSigningPayloadV1::signing_digest`'s
    /// own exact shape).
    pub fn signing_digest(&self) -> StateResult<Digest> {
        Ok(hash_profile_0x0001(
            "hnchain.proposal.roundclaim.v1",
            &self.encode()?,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::ProposalRoundClaimV1;
    use crate::error::{StateError, StateResult};
    use hn_core::Round;

    const BLOCK_HASH: [u8; 32] = [0x22; 32];

    fn sample() -> ProposalRoundClaimV1 {
        ProposalRoundClaimV1 {
            chain_id: 1,
            network_id: 1,
            for_round: Round::new(3),
            block_hash: BLOCK_HASH,
        }
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        let claim = sample();
        let decoded = ProposalRoundClaimV1::decode(&claim.encode()?)?;
        assert_eq!(decoded, claim);
        Ok(())
    }

    #[test]
    fn rejects_an_unsupported_version() -> StateResult<()> {
        let mut bytes = Vec::new();
        hn_hncs::write_u16(&mut bytes, 2);
        assert_eq!(
            ProposalRoundClaimV1::decode(&bytes),
            Err(StateError::UnsupportedProposalRoundClaimVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> StateResult<()> {
        let claim = sample();
        let mut bytes = claim.encode()?;
        bytes.push(0xff);
        assert!(matches!(
            ProposalRoundClaimV1::decode(&bytes),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }

    #[test]
    fn a_different_round_produces_a_different_digest() -> StateResult<()> {
        let claim_a = sample();
        let claim_b = ProposalRoundClaimV1 {
            for_round: Round::new(4),
            ..sample()
        };
        assert_ne!(claim_a.signing_digest()?, claim_b.signing_digest()?);
        Ok(())
    }

    #[test]
    fn matches_independent_oracle() -> StateResult<()> {
        // Computed independently (Python's hashlib.sha512_256) from
        // the documented wire shape and `hash_profile_0x0001`'s own
        // published construction (domain-tag-version || tag ||
        // length-prefixed payload), not derived from this module's
        // own `encode`/`signing_digest` -- the same "don't let the
        // implementation check its own homework" convention every
        // other `matches_independent_oracle` test in this workspace
        // already follows.
        let claim = sample();
        assert_eq!(
            hex(&claim.signing_digest()?),
            "f5b2ef453705145267d93f7909eb2230f34c7fdbc74e01685bf9096eacd89c20"
        );
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

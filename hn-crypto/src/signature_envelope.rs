use hn_hncs::{Decoder, HncsResult, write_bytes, write_u8, write_u16};

use crate::identity::{
    ED25519_ALGORITHM_ID, ED25519_SIGNATURE_LEN, IdentityError, IdentityResult, KeyDescriptor,
};

/// Maximum length, in bytes, of `SignatureEnvelope.signature`. An
/// implementation resource bound, not derived — deliberately generous
/// beyond Ed25519's 64-byte signatures: ADR-0002 reserves post-quantum
/// algorithm identifiers "because key and signature envelopes must not
/// assume fixed Ed25519-sized payloads," and SLH-DSA signatures in
/// particular run tens of kilobytes, not hundreds of bytes.
pub const SIGNATURE_MAX_LEN: usize = 65536;

/// A signature envelope (ADR-0002, "Decided: `SignatureEnvelope` concrete
/// field list"; ADR-0026, "Decided: `key_reference` in
/// `SignatureEnvelope`").
///
/// Concrete shape is `envelope_version`, `algorithm_id`, `key_reference`,
/// `signature` — `verification_context`, named in ADR-0002's original
/// conceptual structure, is deliberately not represented: it is fully
/// redundant. Its own conceptual fields (protocol name, object type,
/// object version, signature purpose) are already what a
/// `HASH_PROFILE_0x0001` domain tag encodes, and its remaining fields
/// (network ID, chain ID) are already explicit fields on every signing
/// payload this project defines.
///
/// `key_reference` was dropped by ADR-0002 and reintroduced by ADR-0026,
/// exactly as ADR-0002's own text anticipated ("a future multisignature/
/// threshold specification reintroducing multiple simultaneously-active
/// keys per role would need to reintroduce something like
/// `key_reference`... against a concrete rule"). It is `Some` only when
/// the signer's account has an active `account_signing`-role multisig
/// configuration (`hn-state::PermissionValueV1`, ADR-0026) — absent
/// otherwise, in which case the key to verify against is still fully
/// context-derived (identity, role, height — ADR-0002's own still-open
/// `active_key(...)` mechanism), unchanged from before ADR-0026.
/// [`SignatureEnvelope::verify`] takes an already-resolved
/// [`KeyDescriptor`] either way — resolving *which* descriptor that is
/// (a state read, and for `key_reference: Some`, an index lookup into
/// the account's stored authorized-key list) is out of this crate's
/// scope, the same boundary `hn-state`'s crates already draw around
/// storage.
///
/// `algorithm_id` is kept: a verifier must know which algorithm produced
/// `signature` before it can even structurally interpret the signature
/// bytes, so dropping it would make canonical decoding state-dependent —
/// unlike `verification_context`, it is not redundant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignatureEnvelope {
    /// The algorithm that produced `signature` (ADR-0002, "Algorithm
    /// Identifier").
    pub algorithm_id: u16,
    /// An index into the signer's stored `authorized_keys` list
    /// (`hn-state::MultisigConfigV1`, ADR-0026), present if and only if
    /// the signer's account has an active `account_signing` multisig
    /// configuration. Absent means the single-key default, unchanged
    /// from before ADR-0026.
    pub key_reference: Option<u8>,
    /// The raw signature bytes, in the encoding `algorithm_id` defines.
    pub signature: Vec<u8>,
}

impl SignatureEnvelope {
    /// `envelope_version` for the original three-field shape
    /// (`key_reference` absent) — remains valid and decodable forever
    /// (ADR-0022, Structure Versioning): this is additive, not a
    /// breaking replacement.
    pub const ENVELOPE_VERSION_1: u16 = 1;

    /// `envelope_version` for an envelope carrying a present
    /// `key_reference` (ADR-0026).
    pub const ENVELOPE_VERSION_2: u16 = 2;

    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> IdentityResult<Vec<u8>> {
        let mut out = Vec::new();
        self.encode_into(&mut out)?;
        Ok(out)
    }

    /// Appends this value's canonical HNCS bytes to `out`. Shared by
    /// [`SignatureEnvelope::encode`] and by callers embedding a
    /// `SignatureEnvelope` inside a larger structure (for example a list
    /// of them, `hn_hncs::write_list`'s own element closure is
    /// `HncsResult`-typed, which this signature matches directly).
    pub fn encode_into(&self, out: &mut Vec<u8>) -> HncsResult<()> {
        let envelope_version = if self.key_reference.is_some() {
            Self::ENVELOPE_VERSION_2
        } else {
            Self::ENVELOPE_VERSION_1
        };
        write_u16(out, envelope_version);
        write_u16(out, self.algorithm_id);
        if let Some(key_reference) = self.key_reference {
            write_u8(out, key_reference);
        }
        write_bytes(out, &self.signature, SIGNATURE_MAX_LEN)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`SignatureEnvelope::encode`].
    pub fn decode(bytes: &[u8]) -> IdentityResult<Self> {
        let mut decoder = Decoder::new(bytes);
        let envelope = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(envelope)
    }

    /// Decodes this value's fields from `decoder` without requiring the
    /// decoder to be exhausted afterward. Shared by
    /// [`SignatureEnvelope::decode`] and by callers embedding a
    /// `SignatureEnvelope` inside a larger structure — unlike
    /// `encode_into`, this cannot be `HncsResult`-typed (an unsupported
    /// `envelope_version` is a domain-specific rejection, not a byte-
    /// framing error), so embedding this inside `hn_hncs::read_list`
    /// is not possible directly; callers needing a list of envelopes
    /// hand-roll the count-prefixed loop instead (see
    /// `hn-state::vote::QuorumCertificate`).
    pub fn decode_from(decoder: &mut Decoder<'_>) -> IdentityResult<Self> {
        let envelope_version = decoder.read_u16()?;
        let algorithm_id = decoder.read_u16()?;
        let key_reference = match envelope_version {
            Self::ENVELOPE_VERSION_1 => None,
            Self::ENVELOPE_VERSION_2 => Some(decoder.read_u8()?),
            value => return Err(IdentityError::UnsupportedEnvelopeVersion { value }),
        };
        let signature = decoder.read_bytes(SIGNATURE_MAX_LEN)?.to_vec();

        Ok(Self {
            algorithm_id,
            key_reference,
            signature,
        })
    }

    /// Verifies this envelope's `signature` over `message` under
    /// `descriptor`.
    ///
    /// `descriptor` must already be resolved by the caller — the specific
    /// [`KeyDescriptor`] that was active for the signer's identity and
    /// role at the relevant height (ADR-0002, "Decided: `SignatureEnvelope`
    /// concrete field list": `key_reference` is context-derived, not
    /// looked up by this function). `message` must already be the
    /// canonical signing payload the caller constructed (ADR-0002, "No
    /// Implicit Signing Payloads") — this function does not construct or
    /// interpret signing payloads itself, mirroring
    /// [`KeyDescriptor::verify`].
    pub fn verify(&self, descriptor: &KeyDescriptor, message: &[u8]) -> IdentityResult<()> {
        if self.algorithm_id != ED25519_ALGORITHM_ID {
            return Err(IdentityError::UnsupportedAlgorithm {
                value: self.algorithm_id,
            });
        }
        if self.algorithm_id != descriptor.algorithm_id() {
            return Err(IdentityError::AlgorithmMismatch);
        }

        let signature: [u8; ED25519_SIGNATURE_LEN] = self
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| IdentityError::SignatureVerificationFailed)?;

        descriptor.verify(message, &signature)
    }
}

#[cfg(test)]
mod tests {
    use super::{IdentityError, SignatureEnvelope};
    use crate::identity::{Ed25519KeyPair, IdentityResult, KeyRole};

    fn sample() -> SignatureEnvelope {
        SignatureEnvelope {
            algorithm_id: 0x0001,
            key_reference: None,
            signature: vec![0xab; 64],
        }
    }

    /// A version-2 envelope (`key_reference` present, ADR-0026).
    fn sample_with_key_reference() -> SignatureEnvelope {
        SignatureEnvelope {
            algorithm_id: 0x0001,
            key_reference: Some(0x05),
            signature: vec![0xab; 64],
        }
    }

    #[test]
    fn encodes_matching_independent_oracle() -> IdentityResult<()> {
        assert_eq!(
            hex(&sample().encode()?),
            "0100010040000000abababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababab"
        );
        Ok(())
    }

    #[test]
    fn encodes_with_key_reference_matching_independent_oracle() -> IdentityResult<()> {
        // Cross-checked independently: u16(2) || u16(1) || u8(5) ||
        // u32(64) || 0xab * 64.
        assert_eq!(
            hex(&sample_with_key_reference().encode()?),
            "020001000540000000abababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababab"
        );
        Ok(())
    }

    #[test]
    fn round_trips_through_decode() -> IdentityResult<()> {
        for envelope in [sample(), sample_with_key_reference()] {
            let decoded = SignatureEnvelope::decode(&envelope.encode()?)?;
            assert_eq!(decoded, envelope);
        }
        Ok(())
    }

    #[test]
    fn rejects_unsupported_envelope_version() -> IdentityResult<()> {
        let mut encoded = sample().encode()?;
        encoded[0] = 0x03; // envelope_version low byte, little-endian
        assert_eq!(
            SignatureEnvelope::decode(&encoded),
            Err(IdentityError::UnsupportedEnvelopeVersion { value: 3 })
        );
        Ok(())
    }

    #[test]
    fn verifies_a_real_ed25519_signature() -> IdentityResult<()> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x55; 32]);
        let descriptor = keypair.key_descriptor();
        let message = b"hnchain test message";
        let signature = keypair.sign(message);

        let envelope = SignatureEnvelope {
            algorithm_id: 0x0001,
            key_reference: None,
            signature: signature.to_vec(),
        };

        envelope.verify(&descriptor, message)
    }

    #[test]
    fn rejects_wrong_message() -> IdentityResult<()> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x66; 32]);
        let descriptor = keypair.key_descriptor();
        let signature = keypair.sign(b"the real message");

        let envelope = SignatureEnvelope {
            algorithm_id: 0x0001,
            key_reference: None,
            signature: signature.to_vec(),
        };

        assert_eq!(
            envelope.verify(&descriptor, b"a different message"),
            Err(IdentityError::SignatureVerificationFailed)
        );
        Ok(())
    }

    #[test]
    fn rejects_unsupported_algorithm_before_touching_the_descriptor() -> IdentityResult<()> {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x77; 32]);
        let descriptor = keypair.key_descriptor();

        let envelope = SignatureEnvelope {
            algorithm_id: 0x0002, // secp256k1, reserved but inactive
            key_reference: None,
            signature: vec![0xcc; 64],
        };

        assert_eq!(
            envelope.verify(&descriptor, b"message"),
            Err(IdentityError::UnsupportedAlgorithm { value: 0x0002 })
        );
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

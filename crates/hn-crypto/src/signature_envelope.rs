use hn_hncs::{Decoder, write_bytes, write_u16};

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
/// field list").
///
/// Concrete shape is `envelope_version`, `algorithm_id`, `signature` only
/// — `key_reference` and `verification_context`, both named in ADR-0002's
/// original conceptual structure, are deliberately not represented:
///
/// - `verification_context` is fully redundant. Its own conceptual fields
///   (protocol name, object type, object version, signature purpose) are
///   already what a `HASH_PROFILE_0x0001` domain tag encodes, and its
///   remaining fields (network ID, chain ID) are already explicit fields
///   on every signing payload this project defines.
/// - `key_reference` is resolved as context-derived rather than stored:
///   under this profile's "exactly one active signing key per role"
///   invariant (ADR-0002), the key to verify against is fully determined
///   by identity, role, and height — data the caller already has, not
///   something this envelope needs to carry. [`SignatureEnvelope::verify`]
///   therefore takes an already-resolved [`KeyDescriptor`] rather than
///   looking one up itself; resolving *which* descriptor that is (a state
///   read) is out of this crate's scope, the same boundary
///   `hn-state`'s crates already draw around storage.
///
/// `algorithm_id` is kept: a verifier must know which algorithm produced
/// `signature` before it can even structurally interpret the signature
/// bytes, so dropping it would make canonical decoding state-dependent —
/// unlike the other two fields, this one is not redundant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignatureEnvelope {
    /// The algorithm that produced `signature` (ADR-0002, "Algorithm
    /// Identifier").
    pub algorithm_id: u16,
    /// The raw signature bytes, in the encoding `algorithm_id` defines.
    pub signature: Vec<u8>,
}

impl SignatureEnvelope {
    /// `envelope_version` for the current shape.
    pub const ENVELOPE_VERSION_1: u16 = 1;

    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> IdentityResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, Self::ENVELOPE_VERSION_1);
        write_u16(&mut out, self.algorithm_id);
        write_bytes(&mut out, &self.signature, SIGNATURE_MAX_LEN)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`SignatureEnvelope::encode`].
    pub fn decode(bytes: &[u8]) -> IdentityResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let envelope_version = decoder.read_u16()?;
        if envelope_version != Self::ENVELOPE_VERSION_1 {
            return Err(IdentityError::UnsupportedEnvelopeVersion {
                value: envelope_version,
            });
        }

        let algorithm_id = decoder.read_u16()?;
        let signature = decoder.read_bytes(SIGNATURE_MAX_LEN)?.to_vec();

        decoder.finish()?;

        Ok(Self {
            algorithm_id,
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
    fn round_trips_through_decode() -> IdentityResult<()> {
        let envelope = sample();
        let decoded = SignatureEnvelope::decode(&envelope.encode()?)?;
        assert_eq!(decoded, envelope);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_envelope_version() -> IdentityResult<()> {
        let mut encoded = sample().encode()?;
        encoded[0] = 0x02; // envelope_version low byte, little-endian
        assert_eq!(
            SignatureEnvelope::decode(&encoded),
            Err(IdentityError::UnsupportedEnvelopeVersion { value: 2 })
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

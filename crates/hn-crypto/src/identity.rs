use ed25519_dalek::{Signature as DalekSignature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;

/// Numeric identifier of the Ed25519 signing algorithm (ADR-0002 profile
/// `0x0001`, `PureEdDSA Ed25519`, RFC 8032).
pub const ED25519_ALGORITHM_ID: u16 = 0x0001;

/// Ed25519 public key length in bytes (ADR-0002).
pub const ED25519_PUBLIC_KEY_LEN: usize = 32;

/// Ed25519 signature length in bytes (ADR-0002).
pub const ED25519_SIGNATURE_LEN: usize = 64;

/// `KeyDescriptor.key_role` registry (ADR-0002, "Key Roles").
///
/// ADR-0002 names these roles without assigning numeric identifiers; this
/// assigns them `uint8` values following the same closed-registry pattern
/// used for `domain_id` (ADR-0007), `address_namespace` (ADR-0003), and
/// `chain_id` (ADR-0008): `0x00` reserved, one value per named role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum KeyRole {
    /// Authorizes account-level transactions.
    AccountSigning = 0x01,
    /// Authorizes validator consensus votes and proposals.
    ValidatorConsensus = 0x02,
    /// Authorizes validator peer-identity / network-layer operations.
    ValidatorNetwork = 0x03,
    /// Authorizes governance participation.
    Governance = 0x04,
    /// Authorizes bridge operator actions.
    BridgeOperator = 0x05,
    /// Authorizes identity recovery operations.
    IdentityRecovery = 0x06,
}

impl KeyRole {
    /// Returns the registry value for this role.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Errors produced while constructing or verifying identity material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentityError {
    /// The supplied bytes are not a canonical Ed25519 public key.
    InvalidPublicKey,
    /// Ed25519 signature verification failed.
    SignatureVerificationFailed,
}

impl core::fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidPublicKey => formatter.write_str("invalid Ed25519 public key"),
            Self::SignatureVerificationFailed => {
                formatter.write_str("Ed25519 signature verification failed")
            }
        }
    }
}

impl std::error::Error for IdentityError {}

/// Result type for identity operations.
pub type IdentityResult<T> = Result<T, IdentityError>;

/// An Ed25519 public key, paired with the `key_role` it is authorized for.
///
/// This is the concretely-specified subset of ADR-0002's `KeyDescriptor`
/// (`descriptor_version`, `algorithm_id`, `key_role`, `public_key`).
/// `validity_rules` and `metadata_commitment` are not represented: ADR-0002
/// names them but does not define their shape, and this crate does not
/// invent one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyDescriptor {
    key_role: KeyRole,
    public_key: [u8; ED25519_PUBLIC_KEY_LEN],
}

impl KeyDescriptor {
    /// `descriptor_version` for this shape.
    pub const DESCRIPTOR_VERSION: u16 = 1;

    /// Builds a descriptor from raw canonical public key bytes, rejecting
    /// non-canonical Ed25519 points.
    pub fn from_public_key_bytes(
        key_role: KeyRole,
        public_key: [u8; ED25519_PUBLIC_KEY_LEN],
    ) -> IdentityResult<Self> {
        VerifyingKey::from_bytes(&public_key).map_err(|_| IdentityError::InvalidPublicKey)?;
        Ok(Self {
            key_role,
            public_key,
        })
    }

    /// `algorithm_id` for this descriptor. Always Ed25519 (`0x0001`) today,
    /// since it is the only active ADR-0002 algorithm.
    pub fn algorithm_id(&self) -> u16 {
        ED25519_ALGORITHM_ID
    }

    /// The authorized `key_role`.
    pub fn key_role(&self) -> KeyRole {
        self.key_role
    }

    /// The raw canonical Ed25519 public key bytes.
    pub fn public_key_bytes(&self) -> [u8; ED25519_PUBLIC_KEY_LEN] {
        self.public_key
    }

    /// Verifies `signature` over `message` under this descriptor's public
    /// key.
    ///
    /// `message` must already be the canonical signing payload the caller
    /// constructed (ADR-0002, "No Implicit Signing Payloads"); this
    /// function does not construct or interpret signing payloads itself.
    pub fn verify(
        &self,
        message: &[u8],
        signature: &[u8; ED25519_SIGNATURE_LEN],
    ) -> IdentityResult<()> {
        let verifying_key = VerifyingKey::from_bytes(&self.public_key)
            .map_err(|_| IdentityError::InvalidPublicKey)?;
        let signature = DalekSignature::from_bytes(signature);
        verifying_key
            .verify(message, &signature)
            .map_err(|_| IdentityError::SignatureVerificationFailed)
    }
}

/// An Ed25519 keypair able to sign for a specific `KeyRole`.
///
/// Wraps a reviewed external implementation (`ed25519-dalek`) behind an
/// HNChain-owned type, per this crate's own scope: it must not define
/// custom cryptographic algorithms.
pub struct Ed25519KeyPair {
    signing_key: SigningKey,
    key_role: KeyRole,
}

impl Ed25519KeyPair {
    /// Generates a new keypair for `key_role` using the operating system's
    /// CSPRNG.
    pub fn generate(key_role: KeyRole) -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self {
            signing_key,
            key_role,
        }
    }

    /// Reconstructs a keypair from a 32-byte Ed25519 seed. Deterministic;
    /// intended for tests and conformance vectors, not for generating keys
    /// that must remain secret from the seed's own source.
    pub fn from_seed(key_role: KeyRole, seed: [u8; 32]) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(&seed),
            key_role,
        }
    }

    /// The `KeyDescriptor` for this keypair's public half.
    pub fn key_descriptor(&self) -> KeyDescriptor {
        KeyDescriptor {
            key_role: self.key_role,
            public_key: self.signing_key.verifying_key().to_bytes(),
        }
    }

    /// Signs `message`, returning a raw 64-byte Ed25519 signature.
    ///
    /// `message` must already be the canonical signing payload the caller
    /// constructed (ADR-0002, "No Implicit Signing Payloads"); this
    /// function does not construct or interpret signing payloads itself.
    pub fn sign(&self, message: &[u8]) -> [u8; ED25519_SIGNATURE_LEN] {
        self.signing_key.sign(message).to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::{Ed25519KeyPair, IdentityError, KeyRole};

    #[test]
    fn signs_and_verifies() {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x11; 32]);
        let descriptor = keypair.key_descriptor();
        let message = b"hnchain conformance message";

        let signature = keypair.sign(message);

        assert_eq!(descriptor.verify(message, &signature), Ok(()));
    }

    #[test]
    fn rejects_wrong_message() {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x22; 32]);
        let descriptor = keypair.key_descriptor();
        let signature = keypair.sign(b"original message");

        assert_eq!(
            descriptor.verify(b"tampered message", &signature),
            Err(IdentityError::SignatureVerificationFailed)
        );
    }

    #[test]
    fn rejects_signature_from_a_different_key() {
        let keypair_a = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x33; 32]);
        let keypair_b = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x44; 32]);
        let message = b"same message, different keys";

        let signature = keypair_a.sign(message);

        assert_eq!(
            keypair_b.key_descriptor().verify(message, &signature),
            Err(IdentityError::SignatureVerificationFailed)
        );
    }

    #[test]
    fn key_role_registry_values() {
        assert_eq!(KeyRole::AccountSigning.as_u8(), 0x01);
        assert_eq!(KeyRole::ValidatorConsensus.as_u8(), 0x02);
        assert_eq!(KeyRole::ValidatorNetwork.as_u8(), 0x03);
        assert_eq!(KeyRole::Governance.as_u8(), 0x04);
        assert_eq!(KeyRole::BridgeOperator.as_u8(), 0x05);
        assert_eq!(KeyRole::IdentityRecovery.as_u8(), 0x06);
    }

    #[test]
    fn matches_independent_oracle_for_seed_derived_keypair()
    -> Result<(), Box<dyn std::error::Error>> {
        // Cross-checked independently via Python's `cryptography` library
        // (Ed25519PrivateKey.from_private_bytes / .sign), not derived from
        // this crate's own ed25519-dalek dependency:
        //
        //   from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
        //   sk = Ed25519PrivateKey.from_private_bytes(bytes([0x11] * 32))
        //   sk.public_key().public_bytes(Raw, Raw).hex()
        //   sk.sign(b"hnchain conformance message").hex()
        let seed = [0x11_u8; 32];
        let expected_public_key =
            hex_to_array("d04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c9778737")?;
        let message = b"hnchain conformance message";
        let expected_signature_hex = "f20621c456cde6d8e13e86e5d5a06ff274f62da40fdcdcd1455c32bdfd92b773560a02ce706bf99d9b67c01cc8859ea7eee124038a0f5a76a6d2454cc5f36006";

        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, seed);

        assert_eq!(
            keypair.key_descriptor().public_key_bytes(),
            expected_public_key
        );

        let signature = keypair.sign(message);
        assert_eq!(hex_string(&signature), expected_signature_hex);
        assert_eq!(keypair.key_descriptor().verify(message, &signature), Ok(()));

        Ok(())
    }

    fn hex_string(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn hex_to_array(hex: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect::<Result<Vec<u8>, _>>()?;
        <[u8; 32]>::try_from(bytes).map_err(|bytes| -> Box<dyn std::error::Error> {
            format!("expected 32 bytes, got {}", bytes.len()).into()
        })
    }

    #[test]
    fn key_descriptor_rejects_invalid_curve_point() {
        // [0x02; 32] does not decompress to a point on the curve (not
        // every 255-bit y-coordinate has a corresponding x); found by
        // scanning uniform-byte patterns against this crate's actual
        // ed25519-dalek dependency, not assumed from a general claim
        // about which byte patterns are "non-canonical" (an earlier
        // attempt using [0xFF; 32] was wrong: dalek, and even OpenSSL's
        // Ed25519 parser, both accept that one).
        let bytes = [0x02_u8; 32];
        assert_eq!(
            super::KeyDescriptor::from_public_key_bytes(KeyRole::AccountSigning, bytes),
            Err(IdentityError::InvalidPublicKey)
        );
    }
}

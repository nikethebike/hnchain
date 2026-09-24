use hn_core::AccountNonce;
use hn_crypto::{Digest, KeyDescriptor, KeyRole, SignatureEnvelope, hash_profile_0x0001};
use hn_hncs::{
    Decoder, HncsError, validate_count, write_bool, write_bytes, write_fixed_bytes, write_u8,
    write_u16, write_u32, write_u64, write_u128,
};

use crate::access_list::AccessListV1;
use crate::error::{StateError, StateResult};
use crate::governance_payload::GovernancePayloadV1;
use crate::identity_transition::{fetch_identity, resolve_account_signing_key};
use crate::key_descriptor::{decode_key_descriptor, encode_key_descriptor};
use crate::permission_transition::{fetch_permission, verify_multisig_authorization};
use crate::permission_update_payload::PermissionUpdatePayloadV1;
use crate::stake_payload::StakePayloadV1;
use crate::state_store::StateReader;
use crate::transfer_payload::TransferPayloadV1;
use crate::unstake_payload::UnstakePayloadV1;
use crate::validator_update_payload::ValidatorUpdatePayloadV1;
use crate::validity_window::ValidityWindowV1;

/// `tx_version` for the current `TransactionEnvelope` shape (ADR-0006,
/// "Decided: versioned transaction envelope").
pub const TX_VERSION_1: u16 = 1;

/// Maximum size, in bytes, of a raw (pre-decode) `TransactionEnvelope`
/// (ADR-0006, "Decided: Transaction size limit"). An implementation DoS
/// bound picked with headroom, not derived — checked on raw bytes as
/// the first validation stage, before HNCS decoding even begins (the
/// "cheap checks before expensive ones" ordering ADR-0006/ADR-0008 both
/// fixed after finding every validation-pipeline diagram had it
/// backwards). Also used as `payload`'s own bounded-bytes `max_len`:
/// `payload` can never exceed the whole envelope it is part of, so no
/// separate, smaller bound is meaningful.
pub const MAX_TRANSACTION_SIZE: usize = 262_144;

/// The `tx_type` registry (ADR-0006, "Decided: `tx_type` registry"),
/// closed for `tx_version = 1`. Assigns identifiers only — it does not
/// by itself say whether a payload schema exists to interpret that
/// type's bytes; see [`decode_transaction_payload`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TxType {
    /// ADR-0006, "Payload": native/asset balance transfer.
    Transfer = 0x01,
    /// Blocked on HNVM, which has no design yet.
    ContractDeploy = 0x02,
    /// Blocked on HNVM, which has no design yet.
    ContractCall = 0x03,
    /// ADR-0006, "Payload": increases `bonded_stake`.
    Stake = 0x04,
    /// ADR-0006, "Payload": decreases `bonded_stake`, starts unbonding.
    Unstake = 0x05,
    /// ADR-0006, "Payload": validator lifecycle/key operations.
    ValidatorUpdate = 0x06,
    /// ADR-0025: governance `propose`/`vote`.
    Governance = 0x07,
    /// ADR-0026: sets an account's `account_signing` multisig
    /// configuration.
    PermissionUpdate = 0x08,
    /// Scope not yet concrete — no protocol module operation has been
    /// specified that would use it.
    System = 0x09,
}

impl TxType {
    /// Decodes a registry byte, rejecting `0x00` (reserved) and any
    /// value outside the closed `0x01..=0x09` range.
    pub fn from_u8(value: u8) -> StateResult<Self> {
        match value {
            0x01 => Ok(Self::Transfer),
            0x02 => Ok(Self::ContractDeploy),
            0x03 => Ok(Self::ContractCall),
            0x04 => Ok(Self::Stake),
            0x05 => Ok(Self::Unstake),
            0x06 => Ok(Self::ValidatorUpdate),
            0x07 => Ok(Self::Governance),
            0x08 => Ok(Self::PermissionUpdate),
            0x09 => Ok(Self::System),
            _ => Err(StateError::InvalidTxType { value }),
        }
    }

    /// Returns the registry value for this type.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// A decoded `payload`, discriminated by `tx_type` (ADR-0006, §5 /
/// ADR-0025 / ADR-0026). Only the 6 `tx_type`s with a decided payload
/// schema have a variant — [`TxType::ContractDeploy`]/
/// [`TxType::ContractCall`]/[`TxType::System`] have no payload schema
/// yet ([`decode_transaction_payload`] rejects them, not this enum
/// silently omitting a case).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransactionPayload {
    /// [`TxType::Transfer`].
    Transfer(TransferPayloadV1),
    /// [`TxType::Stake`].
    Stake(StakePayloadV1),
    /// [`TxType::Unstake`].
    Unstake(UnstakePayloadV1),
    /// [`TxType::ValidatorUpdate`].
    ValidatorUpdate(ValidatorUpdatePayloadV1),
    /// [`TxType::Governance`].
    Governance(GovernancePayloadV1),
    /// [`TxType::PermissionUpdate`].
    PermissionUpdate(PermissionUpdatePayloadV1),
}

/// Decodes `bytes` (a `TransactionEnvelope.payload` value already
/// extracted from an envelope) per `tx_type`'s own decided schema.
///
/// [`StateError::UndecidedTransactionPayload`] for
/// [`TxType::ContractDeploy`]/[`TxType::ContractCall`]/
/// [`TxType::System`] — a structurally valid `tx_type` registry value
/// with no payload schema to interpret it by yet, not a malformed
/// encoding.
pub fn decode_transaction_payload(
    tx_type: TxType,
    bytes: &[u8],
) -> StateResult<TransactionPayload> {
    match tx_type {
        TxType::Transfer => TransferPayloadV1::decode(bytes).map(TransactionPayload::Transfer),
        TxType::Stake => StakePayloadV1::decode(bytes).map(TransactionPayload::Stake),
        TxType::Unstake => UnstakePayloadV1::decode(bytes).map(TransactionPayload::Unstake),
        TxType::ValidatorUpdate => {
            ValidatorUpdatePayloadV1::decode(bytes).map(TransactionPayload::ValidatorUpdate)
        }
        TxType::Governance => {
            GovernancePayloadV1::decode(bytes).map(TransactionPayload::Governance)
        }
        TxType::PermissionUpdate => {
            PermissionUpdatePayloadV1::decode(bytes).map(TransactionPayload::PermissionUpdate)
        }
        TxType::ContractDeploy | TxType::ContractCall | TxType::System => {
            Err(StateError::UndecidedTransactionPayload {
                tx_type: tx_type.as_u8(),
            })
        }
    }
}

/// A canonical transaction (ADR-0006, "Decided: versioned transaction
/// envelope" and every field-level decision since).
///
/// `payload` stays opaque canonical bytes here, not a
/// [`TransactionPayload`] field directly: 3 of the 9 `tx_type`s have no
/// decided payload schema yet (`decode` only needs `tx_type` itself to
/// be a valid registry byte, not every `tx_type`'s payload to be
/// interpretable — the same "envelope stays generic, doesn't need
/// every payload schema decided" boundary
/// [`crate::tx_id`]/[`crate::block_hash`] already draw by treating
/// their own inputs as already-canonical bytes). Callers needing the
/// interpreted payload call [`decode_transaction_payload`] separately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionEnvelope {
    /// Structure Version (ADR-0022) for this envelope shape.
    pub tx_version: u16,
    /// Which chain lineage this transaction targets (ADR-0006, "Chain
    /// And Network Binding").
    pub chain_id: u8,
    /// Which network environment this transaction targets (ADR-0003).
    pub network_id: u16,
    /// Which operation this transaction performs.
    pub tx_type: TxType,
    /// The sender's own `address_body` (ADR-0003) — not a full
    /// `AddressPayload`.
    pub sender: Digest,
    /// Present if and only if `sender` has no stored Identity State yet
    /// (ADR-0027, "Decided: bootstrap mechanism"). Whether that
    /// condition holds is a validation-time, state-dependent question
    /// this type's own `decode` does not check — the same boundary
    /// `SignatureEnvelope.key_reference`'s presence rule already draws.
    pub bootstrap_key: Option<KeyDescriptor>,
    /// Replay protection / ordering (ADR-0006, "Nonce").
    pub nonce: AccountNonce,
    /// Fee cap, not an exact charge (ADR-0006, "Fee Limit").
    pub fee_limit: u128,
    /// Height-based inclusion window (ADR-0006, "Validity Window").
    pub validity_window: ValidityWindowV1,
    /// Hint-only declared read/write set (ADR-0006, "Access List").
    pub access_list: AccessListV1,
    /// `tx_type`-discriminated payload bytes, already canonical.
    /// `decode_transaction_payload` interprets them; this type does
    /// not.
    pub payload: Vec<u8>,
    /// Authorizing signature(s) — ordinarily one; more than one only
    /// once `sender` has an active multisig configuration (ADR-0026).
    pub signatures: Vec<SignatureEnvelope>,
}

impl TransactionEnvelope {
    /// Encodes this value as canonical HNCS bytes.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        self.encode_into(&mut out)?;
        Ok(out)
    }

    /// Appends this value's canonical HNCS bytes to `out`. Shared by
    /// [`TransactionEnvelope::encode`] and by
    /// [`TransactionEnvelope::signing_payload`], which projects every
    /// field except `signatures` into a [`TransactionSigningPayload`]
    /// that encodes the same way (ADR-0006, "Signing Payload":
    /// "`TransactionSigningPayload` mirrors `TransactionEnvelope` minus
    /// `signatures`").
    fn encode_into(&self, out: &mut Vec<u8>) -> StateResult<()> {
        write_u16(out, self.tx_version);
        write_u8(out, self.chain_id);
        write_u16(out, self.network_id);
        write_u8(out, self.tx_type.as_u8());
        write_fixed_bytes(out, &self.sender);
        encode_bootstrap_key(out, self.bootstrap_key.as_ref())?;
        write_u64(out, self.nonce.get());
        write_u128(out, self.fee_limit);
        self.validity_window
            .encode_into(out)
            .map_err(StateError::Encoding)?;
        self.access_list
            .encode_into(out)
            .map_err(StateError::Encoding)?;
        write_bytes(out, &self.payload, MAX_TRANSACTION_SIZE).map_err(StateError::Encoding)?;
        encode_signatures(out, &self.signatures)?;
        Ok(())
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`TransactionEnvelope::encode`].
    ///
    /// Checks `bytes.len() <= MAX_TRANSACTION_SIZE`
    /// ([`StateError::TransactionTooLarge`]) before attempting to decode
    /// anything else — cheap checks before expensive ones, the same
    /// validation-pipeline ordering ADR-0006/ADR-0008 both fixed this
    /// session's own consensus-track pass after finding it backwards.
    ///
    /// This is a purely structural decode: `bootstrap_key`'s presence is
    /// read as-encoded, not checked against any state (that is a
    /// validation-time concern, ADR-0027), and `payload` is left as
    /// opaque bytes — decoding never requires knowing whether `tx_type`
    /// has a decided payload schema.
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        if bytes.len() > MAX_TRANSACTION_SIZE {
            return Err(StateError::TransactionTooLarge { size: bytes.len() });
        }

        let mut decoder = Decoder::new(bytes);

        let tx_version = decoder.read_u16().map_err(StateError::Encoding)?;
        if tx_version != TX_VERSION_1 {
            return Err(StateError::UnsupportedTxVersion { value: tx_version });
        }
        let chain_id = decoder.read_u8().map_err(StateError::Encoding)?;
        let network_id = decoder.read_u16().map_err(StateError::Encoding)?;
        let tx_type = TxType::from_u8(decoder.read_u8().map_err(StateError::Encoding)?)?;
        let sender = decoder
            .read_fixed_bytes::<32>()
            .map_err(StateError::Encoding)?;
        let bootstrap_key = decode_bootstrap_key(&mut decoder)?;
        let nonce = AccountNonce::new(decoder.read_u64().map_err(StateError::Encoding)?);
        let fee_limit = decoder.read_u128().map_err(StateError::Encoding)?;
        let validity_window =
            ValidityWindowV1::decode_from(&mut decoder).map_err(StateError::Encoding)?;
        let access_list = AccessListV1::decode_from(&mut decoder).map_err(StateError::Encoding)?;
        let payload = decoder
            .read_bytes(MAX_TRANSACTION_SIZE)
            .map_err(StateError::Encoding)?
            .to_vec();
        let signatures = decode_signatures(&mut decoder)?;

        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self {
            tx_version,
            chain_id,
            network_id,
            tx_type,
            sender,
            bootstrap_key,
            nonce,
            fee_limit,
            validity_window,
            access_list,
            payload,
            signatures,
        })
    }

    /// Projects this envelope's fields (all except `signatures`) into
    /// its [`TransactionSigningPayload`] (ADR-0006, "Signing Payload").
    pub fn signing_payload(&self) -> TransactionSigningPayload {
        TransactionSigningPayload {
            tx_version: self.tx_version,
            chain_id: self.chain_id,
            network_id: self.network_id,
            tx_type: self.tx_type,
            sender: self.sender,
            bootstrap_key: self.bootstrap_key,
            nonce: self.nonce,
            fee_limit: self.fee_limit,
            validity_window: self.validity_window,
            access_list: self.access_list.clone(),
            payload: self.payload.clone(),
        }
    }

    /// Verifies that `signatures` authorize this transaction — the one
    /// composing call every underlying primitive
    /// (`resolve_account_signing_key`/`verify_multisig_authorization`/
    /// `SignatureEnvelope::verify`) was already built for but nothing
    /// yet tied together (ADR-0026/ADR-0027/ADR-0028).
    ///
    /// Scope matches [`crate::vote::ConsensusVote::verify`]'s own: only
    /// the cryptographic authorization question. It does **not** check
    /// `nonce`/`fee_limit`/`validity_window`/`access_list`/`payload`
    /// validity, does not execute `payload`, and — for a bootstrap
    /// transaction (`bootstrap_key: Some`) — does **not** write the new
    /// `IdentityValueV1` itself: a successful `Ok(())` here means the
    /// caller should separately call
    /// [`crate::apply_identity_bootstrap`] (using `self.bootstrap_key`)
    /// to obtain that leaf, mirroring how [`crate::apply_permission_update`]
    /// is already a separate step from authorization everywhere else in
    /// this crate — `verify` only ever reads (`reader: &impl
    /// StateReader`), it cannot write.
    ///
    /// Dispatches on `sender`'s current `PermissionValueV1` exactly as
    /// ADR-0026 decided:
    ///
    /// - `account_signing_multisig: Some(config)` — `bootstrap_key` must
    ///   be absent ([`StateError::UnexpectedBootstrapKey`]: an account
    ///   with an active multisig configuration already has a populated
    ///   `IdentityValueV1` from when that configuration was first
    ///   activated, ADR-0027's own "Decided: interaction with ADR-0026",
    ///   so there is never a bootstrap case here), then
    ///   [`verify_multisig_authorization`].
    /// - `account_signing_multisig: None` (or no `PermissionValueV1` at
    ///   all) — single-key mode: [`fetch_identity`] +
    ///   [`resolve_account_signing_key`] resolve the one key to check
    ///   against (handling both the ordinary and the bootstrap case,
    ///   ADR-0027), then `signatures` must contain **exactly one** entry
    ///   ([`StateError::ExpectedExactlyOneSignature`] otherwise — unlike
    ///   multisig mode, single-key mode has no "extras tolerated" rule)
    ///   with no `key_reference`
    ///   ([`StateError::UnexpectedKeyReference`] otherwise), verified
    ///   with an ordinary [`hn_crypto::SignatureEnvelope::verify`] call.
    pub fn verify(&self, reader: &impl StateReader) -> StateResult<()> {
        let message = self.signing_payload().signing_digest()?;
        let multisig = fetch_permission(reader, &self.sender)?
            .and_then(|permission| permission.account_signing_multisig);

        match multisig {
            Some(config) => {
                if self.bootstrap_key.is_some() {
                    return Err(StateError::UnexpectedBootstrapKey);
                }
                verify_multisig_authorization(&config, &self.signatures, &message)
            }
            None => {
                let identity = fetch_identity(reader, &self.sender)?;
                let key = resolve_account_signing_key(
                    identity.as_ref(),
                    self.bootstrap_key.as_ref(),
                    &self.sender,
                    self.network_id,
                )?;
                verify_single_signature(&self.signatures, &key, &message)
            }
        }
    }
}

/// Verifies `signatures` under single-key mode: exactly one entry, no
/// `key_reference`, checked against `key`. The counterpart
/// [`verify_multisig_authorization`] handles the multisig case.
fn verify_single_signature(
    signatures: &[SignatureEnvelope],
    key: &KeyDescriptor,
    message: &Digest,
) -> StateResult<()> {
    if signatures.len() != 1 {
        return Err(StateError::ExpectedExactlyOneSignature {
            count: signatures.len(),
        });
    }
    let envelope = &signatures[0];
    if envelope.key_reference.is_some() {
        return Err(StateError::UnexpectedKeyReference);
    }
    envelope
        .verify(key, message)
        .map_err(StateError::SignatureVerificationFailed)
}

/// The canonical subset of a [`TransactionEnvelope`] that `sender`'s
/// signature verifies over (ADR-0006, "Signing Payload") —
/// `TransactionEnvelope` minus `signatures`; `signatures` cannot be
/// included in its own signing payload. `bootstrap_key` (ADR-0027) is
/// an ordinary field here like every other, no carve-out: unlike
/// `signatures`, including it creates no circularity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionSigningPayload {
    /// See [`TransactionEnvelope::tx_version`].
    pub tx_version: u16,
    /// See [`TransactionEnvelope::chain_id`].
    pub chain_id: u8,
    /// See [`TransactionEnvelope::network_id`].
    pub network_id: u16,
    /// See [`TransactionEnvelope::tx_type`].
    pub tx_type: TxType,
    /// See [`TransactionEnvelope::sender`].
    pub sender: Digest,
    /// See [`TransactionEnvelope::bootstrap_key`].
    pub bootstrap_key: Option<KeyDescriptor>,
    /// See [`TransactionEnvelope::nonce`].
    pub nonce: AccountNonce,
    /// See [`TransactionEnvelope::fee_limit`].
    pub fee_limit: u128,
    /// See [`TransactionEnvelope::validity_window`].
    pub validity_window: ValidityWindowV1,
    /// See [`TransactionEnvelope::access_list`].
    pub access_list: AccessListV1,
    /// See [`TransactionEnvelope::payload`].
    pub payload: Vec<u8>,
}

impl TransactionSigningPayload {
    /// Encodes this value as canonical HNCS bytes — field-for-field
    /// identical to [`TransactionEnvelope::encode`] minus `signatures`.
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, self.tx_version);
        write_u8(&mut out, self.chain_id);
        write_u16(&mut out, self.network_id);
        write_u8(&mut out, self.tx_type.as_u8());
        write_fixed_bytes(&mut out, &self.sender);
        encode_bootstrap_key(&mut out, self.bootstrap_key.as_ref())?;
        write_u64(&mut out, self.nonce.get());
        write_u128(&mut out, self.fee_limit);
        self.validity_window
            .encode_into(&mut out)
            .map_err(StateError::Encoding)?;
        self.access_list
            .encode_into(&mut out)
            .map_err(StateError::Encoding)?;
        write_bytes(&mut out, &self.payload, MAX_TRANSACTION_SIZE).map_err(StateError::Encoding)?;
        Ok(out)
    }

    /// Computes this payload's signing digest (ADR-0006, "Decided:
    /// signing payload hash mechanism"):
    /// `HASH_PROFILE_0x0001("hnchain.transaction.signing.v1",
    /// HNCS(TransactionSigningPayload))`.
    pub fn signing_digest(&self) -> StateResult<Digest> {
        Ok(hash_profile_0x0001(
            "hnchain.transaction.signing.v1",
            &self.encode()?,
        )?)
    }
}

fn encode_bootstrap_key(out: &mut Vec<u8>, key: Option<&KeyDescriptor>) -> StateResult<()> {
    write_bool(out, key.is_some());
    if let Some(key) = key {
        encode_key_descriptor(out, key).map_err(StateError::Encoding)?;
    }
    Ok(())
}

fn decode_bootstrap_key(decoder: &mut Decoder<'_>) -> StateResult<Option<KeyDescriptor>> {
    let has_key = decoder.read_bool().map_err(StateError::Encoding)?;
    if has_key {
        Ok(Some(decode_key_descriptor(
            decoder,
            KeyRole::AccountSigning,
        )?))
    } else {
        Ok(None)
    }
}

/// Hand-rolled `u32 count || elements`, not `write_list`:
/// `SignatureEnvelope::decode_from` returns `IdentityResult`, not
/// `HncsResult`, so it cannot be used as a `write_list`/`read_list`
/// element closure directly — the same reason
/// `QuorumCertificate.aggregate_proof` already hand-rolls this.
fn encode_signatures(out: &mut Vec<u8>, signatures: &[SignatureEnvelope]) -> StateResult<()> {
    validate_count(signatures.len(), MAX_SIGNATURES).map_err(StateError::Encoding)?;
    let count = u32::try_from(signatures.len()).map_err(|_| {
        StateError::Encoding(HncsError::LengthFieldOverflow {
            length: signatures.len(),
        })
    })?;
    write_u32(out, count);
    for signature in signatures {
        signature.encode_into(out).map_err(StateError::Encoding)?;
    }
    Ok(())
}

fn decode_signatures(decoder: &mut Decoder<'_>) -> StateResult<Vec<SignatureEnvelope>> {
    let count = decoder.read_u32().map_err(StateError::Encoding)? as usize;
    validate_count(count, MAX_SIGNATURES).map_err(StateError::Encoding)?;
    let mut signatures = Vec::with_capacity(count);
    for _ in 0..count {
        signatures.push(
            SignatureEnvelope::decode_from(decoder)
                .map_err(StateError::InvalidSignatureEnvelope)?,
        );
    }
    Ok(signatures)
}

/// Maximum number of entries in `signatures`. An implementation DoS
/// bound, the same class of decision as
/// [`crate::vote::MAX_QUORUM_SIGNATURES`] — generous over any realistic
/// `MultisigConfigV1.authorized_keys` size
/// ([`crate::permission_value::MAX_AUTHORIZED_KEYS`] is 16), not
/// derived from a specific use case.
pub const MAX_SIGNATURES: usize = 32;

#[cfg(test)]
mod tests {
    use hn_core::AccountNonce;
    use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope};

    use super::{TX_VERSION_1, TransactionEnvelope, TxType};
    use crate::access_list::AccessListV1;
    use crate::error::{StateError, StateResult};
    use crate::transfer_payload::TransferPayloadV1;
    use crate::validity_window::ValidityWindowV1;

    const SENDER: [u8; 32] = [0x11; 32];

    fn sample() -> StateResult<TransactionEnvelope> {
        let payload = TransferPayloadV1 {
            recipient: [0x22; 32],
            asset_id: None,
            amount: 1_000,
        };
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x33; 32]);
        Ok(TransactionEnvelope {
            tx_version: TX_VERSION_1,
            chain_id: 1,
            network_id: 1,
            tx_type: TxType::Transfer,
            sender: SENDER,
            bootstrap_key: None,
            nonce: AccountNonce::new(0),
            fee_limit: 100,
            validity_window: ValidityWindowV1 {
                min_height: None,
                max_height: None,
            },
            access_list: AccessListV1 {
                reads: vec![],
                writes: vec![],
            },
            payload: payload.encode()?,
            signatures: vec![SignatureEnvelope {
                algorithm_id: keypair.key_descriptor().algorithm_id(),
                key_reference: None,
                signature: keypair.sign(b"placeholder").to_vec(),
            }],
        })
    }

    fn sample_with_bootstrap_key() -> StateResult<TransactionEnvelope> {
        let mut envelope = sample()?;
        let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x44; 32]);
        envelope.bootstrap_key = Some(keypair.key_descriptor());
        Ok(envelope)
    }

    #[test]
    fn round_trips_through_decode() -> StateResult<()> {
        for envelope in [sample()?, sample_with_bootstrap_key()?] {
            let decoded = TransactionEnvelope::decode(&envelope.encode()?)?;
            assert_eq!(decoded, envelope);
        }
        Ok(())
    }

    #[test]
    fn signing_payload_excludes_signatures_but_keeps_everything_else() -> StateResult<()> {
        let envelope = sample_with_bootstrap_key()?;
        let signing_payload = envelope.signing_payload();
        assert_eq!(signing_payload.tx_version, envelope.tx_version);
        assert_eq!(signing_payload.bootstrap_key, envelope.bootstrap_key);
        assert_eq!(signing_payload.payload, envelope.payload);
        // Signing payload bytes never include a signature count/entries
        // at all -- verified indirectly: the encoded envelope is longer
        // than the encoded signing payload by more than a trivial
        // amount whenever signatures is non-empty.
        assert!(envelope.encode()?.len() > signing_payload.encode()?.len());
        Ok(())
    }

    #[test]
    fn rejects_unsupported_tx_version() -> StateResult<()> {
        let mut encoded = sample()?.encode()?;
        encoded[0] = 0x02; // tx_version low byte, little-endian
        assert_eq!(
            TransactionEnvelope::decode(&encoded),
            Err(StateError::UnsupportedTxVersion { value: 2 })
        );
        Ok(())
    }

    #[test]
    fn rejects_reserved_tx_type() -> StateResult<()> {
        let mut encoded = sample()?.encode()?;
        let tx_type_index = 2 + 1 + 2; // tx_version(2) + chain_id(1) + network_id(2)
        encoded[tx_type_index] = 0x00;
        assert_eq!(
            TransactionEnvelope::decode(&encoded),
            Err(StateError::InvalidTxType { value: 0x00 })
        );
        Ok(())
    }

    #[test]
    fn rejects_oversized_envelopes() {
        let oversized = vec![0_u8; super::MAX_TRANSACTION_SIZE + 1];
        assert_eq!(
            TransactionEnvelope::decode(&oversized),
            Err(StateError::TransactionTooLarge {
                size: super::MAX_TRANSACTION_SIZE + 1
            })
        );
    }

    #[test]
    fn rejects_trailing_bytes() -> StateResult<()> {
        let mut encoded = sample()?.encode()?;
        encoded.push(0x00);
        assert!(matches!(
            TransactionEnvelope::decode(&encoded),
            Err(StateError::Encoding(_))
        ));
        Ok(())
    }

    #[test]
    fn decode_transaction_payload_interprets_a_transfer() -> StateResult<()> {
        let envelope = sample()?;
        let payload = super::decode_transaction_payload(envelope.tx_type, &envelope.payload)?;
        assert!(matches!(payload, super::TransactionPayload::Transfer(_)));
        Ok(())
    }

    #[test]
    fn decode_transaction_payload_rejects_undecided_tx_types() {
        assert_eq!(
            super::decode_transaction_payload(TxType::System, &[]),
            Err(StateError::UndecidedTransactionPayload {
                tx_type: TxType::System.as_u8()
            })
        );
    }

    mod verify {
        use hn_crypto::{KeyDescriptor, account_address_body};

        use super::{Ed25519KeyPair, KeyRole, SENDER, SignatureEnvelope, StateError, StateResult};
        use crate::account::{AccountSection, account_section_state_key};
        use crate::identity_value::IdentityValueV1;
        use crate::permission_value::{MultisigConfigV1, PermissionValueV1};
        use crate::state_store::StateReader;
        use crate::transaction_envelope::TransactionEnvelope;

        const NETWORK_ID: u16 = 1;

        /// In-memory [`StateReader`], the same test-only pattern
        /// `vote::tests::MapReader` already uses.
        struct MapReader(std::collections::BTreeMap<[u8; 32], Vec<u8>>);

        impl StateReader for MapReader {
            fn get(&self, state_key: &[u8; 32]) -> StateResult<Option<Vec<u8>>> {
                Ok(self.0.get(state_key).cloned())
            }
        }

        fn keypair(seed: u8) -> Ed25519KeyPair {
            Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [seed; 32])
        }

        fn address_of(descriptor: &KeyDescriptor) -> StateResult<[u8; 32]> {
            Ok(account_address_body(
                NETWORK_ID,
                descriptor.algorithm_id(),
                &descriptor.public_key_bytes(),
            )?)
        }

        fn signed(
            mut envelope: TransactionEnvelope,
            keypair: &Ed25519KeyPair,
        ) -> StateResult<TransactionEnvelope> {
            let digest = envelope.signing_payload().signing_digest()?;
            envelope.signatures = vec![SignatureEnvelope {
                algorithm_id: keypair.key_descriptor().algorithm_id(),
                key_reference: None,
                signature: keypair.sign(&digest).to_vec(),
            }];
            Ok(envelope)
        }

        fn bare_envelope(sender: [u8; 32]) -> StateResult<TransactionEnvelope> {
            let mut envelope = super::sample()?;
            envelope.sender = sender;
            envelope.network_id = NETWORK_ID;
            envelope.bootstrap_key = None;
            Ok(envelope)
        }

        #[test]
        fn bootstrap_succeeds_and_needs_no_stored_state() -> StateResult<()> {
            let keypair = keypair(0x01);
            let descriptor = keypair.key_descriptor();
            let sender = address_of(&descriptor)?;
            let mut envelope = bare_envelope(sender)?;
            envelope.bootstrap_key = Some(descriptor);
            let envelope = signed(envelope, &keypair)?;

            let reader = MapReader(std::collections::BTreeMap::new());
            envelope.verify(&reader)
        }

        #[test]
        fn bootstrap_rejects_a_key_that_does_not_derive_the_sender_address() -> StateResult<()> {
            let keypair = keypair(0x02);
            let mut envelope = bare_envelope(SENDER)?; // SENDER is unrelated to `keypair`
            envelope.bootstrap_key = Some(keypair.key_descriptor());
            let envelope = signed(envelope, &keypair)?;

            let reader = MapReader(std::collections::BTreeMap::new());
            assert_eq!(
                envelope.verify(&reader),
                Err(StateError::BootstrapKeyAddressMismatch)
            );
            Ok(())
        }

        #[test]
        fn ordinary_single_key_transaction_succeeds_against_stored_identity() -> StateResult<()> {
            let keypair = keypair(0x03);
            let descriptor = keypair.key_descriptor();
            let sender = address_of(&descriptor)?;
            let envelope = signed(bare_envelope(sender)?, &keypair)?;

            let identity_key = account_section_state_key(&sender, AccountSection::Identity)?;
            let identity = IdentityValueV1 { key: descriptor };
            let reader = MapReader(std::collections::BTreeMap::from([(
                identity_key,
                identity.encode()?,
            )]));

            envelope.verify(&reader)
        }

        #[test]
        fn rejects_a_bootstrap_key_when_identity_already_exists() -> StateResult<()> {
            let keypair = keypair(0x04);
            let descriptor = keypair.key_descriptor();
            let sender = address_of(&descriptor)?;
            let mut envelope = bare_envelope(sender)?;
            envelope.bootstrap_key = Some(descriptor);
            let envelope = signed(envelope, &keypair)?;

            let identity_key = account_section_state_key(&sender, AccountSection::Identity)?;
            let identity = IdentityValueV1 { key: descriptor };
            let reader = MapReader(std::collections::BTreeMap::from([(
                identity_key,
                identity.encode()?,
            )]));

            assert_eq!(
                envelope.verify(&reader),
                Err(StateError::UnexpectedBootstrapKey)
            );
            Ok(())
        }

        #[test]
        fn rejects_a_wrong_signature_in_single_key_mode() -> StateResult<()> {
            let signer = keypair(0x05);
            let stored = keypair(0x06);
            let sender = address_of(&stored.key_descriptor())?;
            // Signed by a *different* key than the one on file.
            let envelope = signed(bare_envelope(sender)?, &signer)?;

            let identity_key = account_section_state_key(&sender, AccountSection::Identity)?;
            let identity = IdentityValueV1 {
                key: stored.key_descriptor(),
            };
            let reader = MapReader(std::collections::BTreeMap::from([(
                identity_key,
                identity.encode()?,
            )]));

            assert!(matches!(
                envelope.verify(&reader),
                Err(StateError::SignatureVerificationFailed(_))
            ));
            Ok(())
        }

        #[test]
        fn rejects_more_than_one_signature_in_single_key_mode() -> StateResult<()> {
            let keypair = keypair(0x07);
            let descriptor = keypair.key_descriptor();
            let sender = address_of(&descriptor)?;
            let mut envelope = signed(bare_envelope(sender)?, &keypair)?;
            let extra = envelope.signatures[0].clone();
            envelope.signatures.push(extra);

            let identity_key = account_section_state_key(&sender, AccountSection::Identity)?;
            let identity = IdentityValueV1 { key: descriptor };
            let reader = MapReader(std::collections::BTreeMap::from([(
                identity_key,
                identity.encode()?,
            )]));

            assert_eq!(
                envelope.verify(&reader),
                Err(StateError::ExpectedExactlyOneSignature { count: 2 })
            );
            Ok(())
        }

        #[test]
        fn multisig_transaction_succeeds_against_stored_configuration() -> StateResult<()> {
            let keypairs = [keypair(0x08), keypair(0x09)];
            let mut authorized_keys: Vec<_> = keypairs
                .iter()
                .map(Ed25519KeyPair::key_descriptor)
                .collect();
            authorized_keys.sort_by_key(KeyDescriptor::public_key_bytes);
            let config = MultisigConfigV1 {
                threshold: 2,
                authorized_keys,
            };

            let mut envelope = bare_envelope(SENDER)?;
            let digest = envelope.signing_payload().signing_digest()?;
            envelope.signatures = keypairs
                .iter()
                .map(|keypair| -> StateResult<SignatureEnvelope> {
                    let descriptor = keypair.key_descriptor();
                    let key_reference = config
                        .authorized_keys
                        .iter()
                        .position(|candidate| {
                            candidate.public_key_bytes() == descriptor.public_key_bytes()
                        })
                        .ok_or(StateError::MissingKeyReference)?
                        as u8;
                    Ok(SignatureEnvelope {
                        algorithm_id: descriptor.algorithm_id(),
                        key_reference: Some(key_reference),
                        signature: keypair.sign(&digest).to_vec(),
                    })
                })
                .collect::<StateResult<Vec<_>>>()?;

            let permission_key = account_section_state_key(&SENDER, AccountSection::Permission)?;
            let permission = PermissionValueV1 {
                account_signing_multisig: Some(config),
            };
            let reader = MapReader(std::collections::BTreeMap::from([(
                permission_key,
                permission.encode()?,
            )]));

            envelope.verify(&reader)
        }

        #[test]
        fn multisig_rejects_a_bootstrap_key() -> StateResult<()> {
            let keypair = keypair(0x0a);
            let config = MultisigConfigV1 {
                threshold: 1,
                authorized_keys: vec![keypair.key_descriptor()],
            };
            let mut envelope = bare_envelope(SENDER)?;
            envelope.bootstrap_key = Some(keypair.key_descriptor());
            let envelope = signed(envelope, &keypair)?;

            let permission_key = account_section_state_key(&SENDER, AccountSection::Permission)?;
            let permission = PermissionValueV1 {
                account_signing_multisig: Some(config),
            };
            let reader = MapReader(std::collections::BTreeMap::from([(
                permission_key,
                permission.encode()?,
            )]));

            assert_eq!(
                envelope.verify(&reader),
                Err(StateError::UnexpectedBootstrapKey)
            );
            Ok(())
        }
    }
}

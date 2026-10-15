//! Building and signing a native HNCOIN transfer (ADR-0047, "Decided:
//! Building And Signing A Transfer", steps 1 and 5: derive the
//! sender's own address from the account-signing key and the network
//! id, then build and sign the real `TransactionEnvelope` — no
//! transaction-format logic of its own; this calls the exact same
//! `hn-state`/`hn-crypto` types the node and its own tests already
//! use).

use std::fmt;

use hn_core::AccountNonce;
use hn_crypto::{Digest, Ed25519KeyPair, HashError, SignatureEnvelope, account_address_body};
use hn_state::{
    AccessListV1, StateError, TX_VERSION_1, TransactionEnvelope, TransferPayloadV1, TxType,
    ValidityWindowV1,
};

/// Everything [`build_signed_transfer`] needs beyond the signing
/// keypair itself.
#[derive(Clone, Debug)]
pub struct TransferRequest {
    /// See [`hn_state::TransactionEnvelope::chain_id`].
    pub chain_id: u8,
    /// See [`hn_state::TransactionEnvelope::network_id`].
    pub network_id: u16,
    /// The recipient's `address_body`.
    pub recipient: Digest,
    /// The amount to move, in `hnit`.
    pub amount: u128,
    /// The sender's own nonce for this transaction. `0` carries
    /// `bootstrap_key` (ADR-0027); any other value does not.
    pub nonce: u64,
    /// See [`hn_state::TransactionEnvelope::fee_limit`].
    pub fee_limit: u128,
    /// See [`hn_state::TransactionEnvelope::validity_window`].
    pub validity_window: ValidityWindowV1,
}

/// Why building or signing a transfer failed — both of its own
/// sources (address derivation, canonical encoding) are themselves
/// infallible for any input this module's own callers ever construct
/// (a fixed-size digest, a bounded amount), so in practice neither
/// variant is reachable; the type stays honest about that possibility
/// existing in the underlying APIs rather than silently discarding it.
#[derive(Debug)]
pub enum TransferError {
    /// Deriving the sender's own address failed.
    AddressDerivation(HashError),
    /// Encoding the envelope or computing its signing digest failed.
    Encoding(StateError),
}

impl fmt::Display for TransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AddressDerivation(error) => {
                write!(formatter, "could not derive sender address: {error}")
            }
            Self::Encoding(error) => write!(formatter, "could not build transaction: {error}"),
        }
    }
}

impl std::error::Error for TransferError {}

/// Builds and signs a real native-HNCOIN `transfer` (ADR-0047,
/// "Decided: Building And Signing A Transfer"): derives `keypair`'s
/// own sender address, builds the canonical `TransferPayloadV1`
/// (`asset_id: None`), assembles the `TransactionEnvelope` (with
/// `bootstrap_key` present if and only if `request.nonce == 0`,
/// ADR-0027), computes its real signing digest, and signs it with
/// `keypair`.
pub fn build_signed_transfer(
    keypair: &Ed25519KeyPair,
    request: &TransferRequest,
) -> Result<TransactionEnvelope, TransferError> {
    let descriptor = keypair.key_descriptor();
    let sender = account_address_body(
        request.network_id,
        descriptor.algorithm_id(),
        &descriptor.public_key_bytes(),
    )
    .map_err(TransferError::AddressDerivation)?;

    let payload = TransferPayloadV1 {
        recipient: request.recipient,
        asset_id: None,
        amount: request.amount,
    }
    .encode()
    .map_err(TransferError::Encoding)?;

    let mut envelope = TransactionEnvelope {
        tx_version: TX_VERSION_1,
        chain_id: request.chain_id,
        network_id: request.network_id,
        tx_type: TxType::Transfer,
        sender,
        bootstrap_key: (request.nonce == 0).then_some(descriptor),
        nonce: AccountNonce::new(request.nonce),
        fee_limit: request.fee_limit,
        validity_window: request.validity_window,
        access_list: AccessListV1 {
            reads: vec![],
            writes: vec![],
        },
        payload,
        signatures: vec![],
    };

    let digest = envelope
        .signing_payload()
        .signing_digest()
        .map_err(TransferError::Encoding)?;
    let signature = keypair.sign(&digest);
    envelope.signatures = vec![SignatureEnvelope {
        algorithm_id: descriptor.algorithm_id(),
        key_reference: None,
        signature: signature.to_vec(),
    }];

    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use hn_core::BlockHeight;
    use hn_crypto::{Ed25519KeyPair, KeyRole};
    use hn_state::tx_id;

    use super::{TransferRequest, ValidityWindowV1, build_signed_transfer};

    /// The exact fixed inputs this oracle vector cross-checks
    /// (ADR-0047 step 5: "the same seed, recipient, amount, nonce and
    /// window" -- computed independently in Python via
    /// `cryptography`'s own Ed25519 and `hashlib.sha512_256`'s own
    /// SHA-512/256, in `oracle_transfer_vector.py` (not committed to
    /// the repo, the same throwaway-oracle-script convention this
    /// workspace already uses for every other `matches_independent_oracle`
    /// test), never hand-transcribed into this file.
    fn fixed_request(nonce: u64) -> TransferRequest {
        TransferRequest {
            chain_id: 1,
            network_id: 1,
            recipient: [0x99; 32],
            amount: 1_500_000_000,
            nonce,
            fee_limit: 100,
            validity_window: ValidityWindowV1 {
                min_height: None,
                max_height: Some(BlockHeight::new(500)),
            },
        }
    }

    fn fixed_keypair() -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0x7A; 32])
    }

    #[test]
    fn nonce_zero_matches_the_independent_oracle_byte_for_byte()
    -> Result<(), Box<dyn std::error::Error>> {
        let keypair = fixed_keypair();
        let envelope = build_signed_transfer(&keypair, &fixed_request(0))?;
        let encoded = envelope.encode()?;

        assert_eq!(
            hex(&encoded),
            "01000101000116f566a366190082a6ba5df92d28b3e9e3791b0d6ace097d74c13069cffe801f01010020000000ba42458e83ba7926ba8b8f3e9ab9caaf0f1c4918dda8c551084f7aeb1065b74b0000000000000000640000000000000000000000000000000001f4010000000000000000000000000000330000000100999999999999999999999999999999999999999999999999999999999999999900002f6859000000000000000000000000010000000100010040000000e973630edfad6f0a27e732e1917b9b992c539cefce7cc18fedd1b40f42186e35d53bf5e052d4df83b528910b301df4b38d0b814866b84f8f5d4b4df38cf48802"
        );
        assert_eq!(
            hex(&tx_id(&encoded)?),
            "d8b1f782563909cae7231eab88977a55c777a3f073014dfe99b41b83f8673887"
        );
        assert!(envelope.bootstrap_key.is_some());
        Ok(())
    }

    #[test]
    fn nonce_one_matches_the_independent_oracle_byte_for_byte_and_has_no_bootstrap_key()
    -> Result<(), Box<dyn std::error::Error>> {
        let keypair = fixed_keypair();
        let envelope = build_signed_transfer(&keypair, &fixed_request(1))?;
        let encoded = envelope.encode()?;

        assert_eq!(
            hex(&encoded),
            "01000101000116f566a366190082a6ba5df92d28b3e9e3791b0d6ace097d74c13069cffe801f000100000000000000640000000000000000000000000000000001f4010000000000000000000000000000330000000100999999999999999999999999999999999999999999999999999999999999999900002f685900000000000000000000000001000000010001004000000096d0f3b556ca5c05341dd86d26eb90347ddf604dee06df2ff4ca45ab4c0073790a924d16c6fd0b8316184073751103d2433e1740fa3a891244d6bca6886def09"
        );
        assert_eq!(
            hex(&tx_id(&encoded)?),
            "ab8ace47a08ab5dc09abb74b55903868c63c1f3e48ec85dc7ebfc599693eeba5"
        );
        assert!(envelope.bootstrap_key.is_none());
        Ok(())
    }

    #[test]
    fn decodes_back_through_the_nodes_own_decoder() -> Result<(), Box<dyn std::error::Error>> {
        let keypair = fixed_keypair();
        for nonce in [0, 1] {
            let envelope = build_signed_transfer(&keypair, &fixed_request(nonce))?;
            let encoded = envelope.encode()?;
            let decoded = hn_state::TransactionEnvelope::decode(&encoded)?;
            assert_eq!(decoded, envelope);
        }
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

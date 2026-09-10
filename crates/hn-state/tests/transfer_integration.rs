//! End-to-end proof that `hn-state`'s `transfer` state transition
//! (`apply_transfer`) composes with real account state: build two full
//! account states (sender, recipient), apply a native-HNCOIN transfer
//! between them, and recompute the state root.
//!
//! Every expected value is cross-checked against an independent Python
//! oracle: both derived addresses, the "before" root over both accounts'
//! full leaf sets, the transfer payload's encoding, and the "after" root
//! once the transfer is applied — not derived from this crate's own
//! implementation.

use hn_crypto::{Digest, account_address_body};
use hn_state::{
    AccountSection, AccountType, AssetValueV1, BalanceValueV1, EmptyHashTable, EnvelopeValueV1,
    Leaf, LifecycleState, LifecycleValueV1, NonceValueV1, SectionVersionsV1, TransferParty,
    TransferPayloadV1, account_extension_payload_state_key, account_extension_registry_state_key,
    account_section_state_key, apply_transfer, compute_state_root, leaf_hash, value_hash,
};

const SENDER_PUBLIC_KEY: [u8; 32] = [
    0xd0, 0x4a, 0xb2, 0x32, 0x74, 0x2b, 0xb4, 0xab, 0x3a, 0x13, 0x68, 0xbd, 0x46, 0x15, 0xe4, 0xe6,
    0xd0, 0x22, 0x4a, 0xb7, 0x1a, 0x01, 0x6b, 0xaf, 0x85, 0x20, 0xa3, 0x32, 0xc9, 0x77, 0x87, 0x37,
];
const RECIPIENT_PUBLIC_KEY: [u8; 32] = [0xab; 32];

const SENDER_START_BALANCE: u128 = 1_000;
const RECIPIENT_START_BALANCE: u128 = 100;
const TRANSFER_AMOUNT: u128 = 300;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn transfer_between_two_accounts_matches_independent_oracle() -> TestResult {
    let sender_address = account_address_body(0x0001, 0x0001, &SENDER_PUBLIC_KEY)?;
    let recipient_address = account_address_body(0x0001, 0x0001, &RECIPIENT_PUBLIC_KEY)?;

    assert_eq!(
        hex(&sender_address),
        "d040e6d2ad41fbbe91c3a2192642a9e4396c5c5e85ffe3f190d10aba66d7df7a"
    );
    assert_eq!(
        hex(&recipient_address),
        "a7f0880ea0c5f0e910063e76c8e02a54d4f9ff47dac7ea3e127b728d80cf0ddc"
    );

    let mut sender_leaves = full_account_leaves(&sender_address, SENDER_START_BALANCE)?;
    let mut recipient_leaves = full_account_leaves(&recipient_address, RECIPIENT_START_BALANCE)?;

    let empty_table = EmptyHashTable::build()?;

    let mut before_leaves = sender_leaves.clone();
    before_leaves.extend(recipient_leaves.clone());
    let before_root = compute_state_root(&before_leaves, &empty_table)?;
    assert_eq!(
        hex(&before_root),
        "8b2c65b3322787c153792befdfc843270be5bf47ad3239942f277a96fdd71a8d"
    );

    let payload = TransferPayloadV1 {
        recipient: recipient_address,
        asset_id: None,
        amount: TRANSFER_AMOUNT,
    };
    assert_eq!(
        hex(&payload.encode()?),
        "0100a7f0880ea0c5f0e910063e76c8e02a54d4f9ff47dac7ea3e127b728d80cf0ddc\
         002c010000000000000000000000000000"
    );

    let sender_party = TransferParty {
        address: sender_address,
        balance: BalanceValueV1 {
            native_balance: SENDER_START_BALANCE,
        },
        assets: AssetValueV1 { holdings: vec![] },
    };
    let recipient_party = TransferParty {
        address: recipient_address,
        balance: BalanceValueV1 {
            native_balance: RECIPIENT_START_BALANCE,
        },
        assets: AssetValueV1 { holdings: vec![] },
    };

    let [new_sender_balance_leaf, new_recipient_balance_leaf] =
        apply_transfer(&sender_party, &recipient_party, &payload)?;

    replace_leaf(&mut sender_leaves, new_sender_balance_leaf);
    replace_leaf(&mut recipient_leaves, new_recipient_balance_leaf);

    let mut after_leaves = sender_leaves;
    after_leaves.extend(recipient_leaves);
    let after_root = compute_state_root(&after_leaves, &empty_table)?;
    assert_eq!(
        hex(&after_root),
        "e1923fabe90fe1120bf15e25334303a37c601f718ef2df1425eb7a5c2ec9a02d"
    );

    Ok(())
}

/// Builds the full 10-leaf write set for one account (8 required
/// sections + extension registry/payload leaves), matching
/// `account_integration.rs`'s pattern: real encodings for sections with
/// a decided value schema, placeholder values for the rest.
fn full_account_leaves(address: &Digest, native_balance: u128) -> TestResult<Vec<Leaf>> {
    let mut leaves = Vec::new();

    let envelope = EnvelopeValueV1 {
        account_type: AccountType::Standard,
        address: *address,
        section_versions: SectionVersionsV1 {
            identity_version: 1,
            balance_version: 1,
            nonce_version: 1,
            permission_version: 1,
            metadata_version: 1,
            asset_version: 1,
            lifecycle_version: 1,
        },
    };
    let envelope_key = account_section_state_key(address, AccountSection::Envelope)?;
    let envelope_vh = value_hash(&envelope.encode())?;
    leaves.push((envelope_key, leaf_hash(&envelope_key, &envelope_vh)?));

    let nonce = NonceValueV1 {
        nonce: hn_core::AccountNonce::INITIAL,
    };
    let nonce_key = account_section_state_key(address, AccountSection::Nonce)?;
    let nonce_vh = value_hash(&nonce.encode())?;
    leaves.push((nonce_key, leaf_hash(&nonce_key, &nonce_vh)?));

    let balance = BalanceValueV1 { native_balance };
    let balance_key = account_section_state_key(address, AccountSection::Balance)?;
    let balance_vh = value_hash(&balance.encode())?;
    leaves.push((balance_key, leaf_hash(&balance_key, &balance_vh)?));

    let asset = AssetValueV1 { holdings: vec![] };
    let asset_key = account_section_state_key(address, AccountSection::Asset)?;
    let asset_vh = value_hash(&asset.encode()?)?;
    leaves.push((asset_key, leaf_hash(&asset_key, &asset_vh)?));

    let lifecycle = LifecycleValueV1 {
        state: LifecycleState::Created,
    };
    let lifecycle_key = account_section_state_key(address, AccountSection::Lifecycle)?;
    let lifecycle_vh = value_hash(&lifecycle.encode())?;
    leaves.push((lifecycle_key, leaf_hash(&lifecycle_key, &lifecycle_vh)?));

    let placeholder_sections = [
        (AccountSection::Identity, "identity-placeholder-v1"),
        (AccountSection::Permission, "permission-placeholder-v1"),
        (AccountSection::Metadata, "metadata-placeholder-v1"),
    ];
    for (section, placeholder) in placeholder_sections {
        let key = account_section_state_key(address, section)?;
        let vh = value_hash(placeholder.as_bytes())?;
        leaves.push((key, leaf_hash(&key, &vh)?));
    }

    let registry_key = account_extension_registry_state_key(address)?;
    let registry_vh = value_hash(b"extension-registry-placeholder-v1")?;
    leaves.push((registry_key, leaf_hash(&registry_key, &registry_vh)?));

    let payload_key = account_extension_payload_state_key(address, 1)?;
    let payload_vh = value_hash(b"extension-payload-1-placeholder-v1")?;
    leaves.push((payload_key, leaf_hash(&payload_key, &payload_vh)?));

    Ok(leaves)
}

/// Replaces the leaf sharing `new_leaf`'s `state_key`, leaving every
/// other leaf untouched.
fn replace_leaf(leaves: &mut [Leaf], new_leaf: Leaf) {
    for leaf in leaves.iter_mut() {
        if leaf.0 == new_leaf.0 {
            *leaf = new_leaf;
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

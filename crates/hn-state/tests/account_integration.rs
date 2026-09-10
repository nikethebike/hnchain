//! End-to-end proof that `hn-crypto`'s address derivation and `hn-state`'s
//! `hn-smt-256-v1` key derivation / tree math actually compose: derive an
//! account address, derive its envelope/section/extension state keys,
//! hash real `EnvelopeValueV1`/`NonceValueV1`/`BalanceValueV1`/
//! `AssetValueV1`/`LifecycleValueV1` leaves plus placeholder leaves for
//! the sections whose value schema is not yet decided, and compute a
//! state root — the full path a real state transition would take for the
//! `accounts` domain.
//!
//! Every expected value is cross-checked against an independent Python
//! oracle, not derived from this crate's own implementation.

use hn_core::AccountNonce;
use hn_crypto::account_address_body;
use hn_state::{
    AccountSection, AccountType, AssetValueV1, BalanceValueV1, EmptyHashTable, EnvelopeValueV1,
    LifecycleState, LifecycleValueV1, NonceValueV1, SectionVersionsV1,
    account_extension_payload_state_key, account_extension_registry_state_key,
    account_section_state_key, compute_state_root, leaf_hash, value_hash,
};

const PUBLIC_KEY: [u8; 32] = [
    0xd0, 0x4a, 0xb2, 0x32, 0x74, 0x2b, 0xb4, 0xab, 0x3a, 0x13, 0x68, 0xbd, 0x46, 0x15, 0xe4, 0xe6,
    0xd0, 0x22, 0x4a, 0xb7, 0x1a, 0x01, 0x6b, 0xaf, 0x85, 0x20, 0xa3, 0x32, 0xc9, 0x77, 0x87, 0x37,
];

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn account_address_derives_expected_section_state_keys() -> TestResult {
    let account_address = account_address_body(0x0001, 0x0001, &PUBLIC_KEY)?;
    assert_eq!(
        hex(&account_address),
        "d040e6d2ad41fbbe91c3a2192642a9e4396c5c5e85ffe3f190d10aba66d7df7a"
    );

    let cases = [
        (
            AccountSection::Envelope,
            "b833d8e5aaaa6f704e1c265a8b967b6f9a3a3aa64102fb1d368e0b02d361aa19",
        ),
        (
            AccountSection::Identity,
            "cfe5dc9ad2ab24682a522d40daacff4fe86fb9b596a33678b7b8ac6bccc71efe",
        ),
        (
            AccountSection::Balance,
            "9972a366020a6210fe2a2b5aa633570771e3703d02d4e91ef3d3aadc67b81411",
        ),
        (
            AccountSection::Nonce,
            "cf2aa7587749c9ff30736e090ffb8761ac8b1f9b2a8b4b53dd17b90bafbcedf6",
        ),
        (
            AccountSection::Permission,
            "18bf6cb567ec4c260eb2aaefab3ac7e7d051523d6c76a2ec850cbf0d6a767723",
        ),
        (
            AccountSection::Metadata,
            "3c1e65829894f6db3f0739b5946a759dc2107c3f83ef520f8f4775b0f75e44d0",
        ),
        (
            AccountSection::Asset,
            "3d58d7c90576b0389e07d909e84c6dd6c58012b038288dbdf08ca1ce83d3f7f1",
        ),
        (
            AccountSection::Lifecycle,
            "4b16107d32b1ffe116e2a9fcaf424ce2b84075d4cc55db418e64ae188be2977d",
        ),
    ];

    for (section, expected) in cases {
        let key = account_section_state_key(&account_address, section)?;
        assert_eq!(hex(&key), expected);
    }

    Ok(())
}

#[test]
fn account_extension_keys_match_independent_oracle() -> TestResult {
    let account_address = account_address_body(0x0001, 0x0001, &PUBLIC_KEY)?;

    let registry = account_extension_registry_state_key(&account_address)?;
    assert_eq!(
        hex(&registry),
        "5f00a56476b3e0f049f0a83fe95e5c5dc738cae6ee88856e314f5216aceec935"
    );

    let payload = account_extension_payload_state_key(&account_address, 1)?;
    assert_eq!(
        hex(&payload),
        "5e8b0ccbcc5386d358fcdf6d1fe5744566dc24b5913f5df1b0ca9951bd5ec699"
    );

    Ok(())
}

#[test]
fn full_account_state_root_matches_independent_oracle() -> TestResult {
    let account_address = account_address_body(0x0001, 0x0001, &PUBLIC_KEY)?;

    let mut leaves = Vec::new();

    let envelope = EnvelopeValueV1 {
        account_type: AccountType::Standard,
        address: account_address,
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
    let envelope_key = account_section_state_key(&account_address, AccountSection::Envelope)?;
    let envelope_vh = value_hash(&envelope.encode())?;
    leaves.push((envelope_key, leaf_hash(&envelope_key, &envelope_vh)?));

    let nonce = NonceValueV1 {
        nonce: AccountNonce::INITIAL,
    };
    let nonce_key = account_section_state_key(&account_address, AccountSection::Nonce)?;
    let nonce_vh = value_hash(&nonce.encode())?;
    leaves.push((nonce_key, leaf_hash(&nonce_key, &nonce_vh)?));

    let balance = BalanceValueV1 { native_balance: 0 };
    let balance_key = account_section_state_key(&account_address, AccountSection::Balance)?;
    let balance_vh = value_hash(&balance.encode())?;
    leaves.push((balance_key, leaf_hash(&balance_key, &balance_vh)?));

    let asset = AssetValueV1 { holdings: vec![] };
    let asset_key = account_section_state_key(&account_address, AccountSection::Asset)?;
    let asset_vh = value_hash(&asset.encode()?)?;
    leaves.push((asset_key, leaf_hash(&asset_key, &asset_vh)?));

    let lifecycle = LifecycleValueV1 {
        state: LifecycleState::Created,
    };
    let lifecycle_key = account_section_state_key(&account_address, AccountSection::Lifecycle)?;
    let lifecycle_vh = value_hash(&lifecycle.encode())?;
    leaves.push((lifecycle_key, leaf_hash(&lifecycle_key, &lifecycle_vh)?));

    let placeholder_sections = [
        (AccountSection::Identity, "identity-placeholder-v1"),
        (AccountSection::Permission, "permission-placeholder-v1"),
        (AccountSection::Metadata, "metadata-placeholder-v1"),
    ];
    for (section, placeholder) in placeholder_sections {
        let key = account_section_state_key(&account_address, section)?;
        let vh = value_hash(placeholder.as_bytes())?;
        leaves.push((key, leaf_hash(&key, &vh)?));
    }

    let registry_key = account_extension_registry_state_key(&account_address)?;
    let registry_vh = value_hash(b"extension-registry-placeholder-v1")?;
    leaves.push((registry_key, leaf_hash(&registry_key, &registry_vh)?));

    let payload_key = account_extension_payload_state_key(&account_address, 1)?;
    let payload_vh = value_hash(b"extension-payload-1-placeholder-v1")?;
    leaves.push((payload_key, leaf_hash(&payload_key, &payload_vh)?));

    let empty_table = EmptyHashTable::build()?;
    let root = compute_state_root(&leaves, &empty_table)?;

    assert_eq!(
        hex(&root),
        "f33a26c3aeafe19e7fbe5f1b4cea1fac9726e21cb0067c28848b9ba2953973bb"
    );

    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

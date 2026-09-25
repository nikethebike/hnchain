//! Prints a devnet genesis JSON file to stdout (ADR-0038, "Decided:
//! Devnet Example Genesis"). Not part of `hn-node`'s own public API —
//! a one-off generation tool, run once to produce the committed
//! `hn-node/genesis/devnet.json`, using the real
//! `Ed25519KeyPair::from_seed`/`account_address_body` derivations
//! rather than a separately-maintained script, so the committed file is
//! guaranteed consistent with the actual implementation.

use hn_crypto::{Ed25519KeyPair, HashError, KeyRole, account_address_body};

const VALIDATOR_COUNT: u8 = 4;
const NETWORK_ID: u16 = 1;
const DEVNET_BONDED_STAKE: u128 = hn_state::MINIMUM_VALIDATOR_BOND * 10;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn allocation_json(seed: u8, amount: u128) -> Result<serde_json::Value, HashError> {
    let keypair = Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [seed; 32]);
    let descriptor = keypair.key_descriptor();
    let address = account_address_body(
        NETWORK_ID,
        descriptor.algorithm_id(),
        &descriptor.public_key_bytes(),
    )?;
    Ok(serde_json::json!({
        "address": hex(&address),
        "amount": amount.to_string(),
    }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let validators: Vec<serde_json::Value> = (0..VALIDATOR_COUNT)
        .map(|index| {
            let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [index; 32]);
            let descriptor = keypair.key_descriptor();
            serde_json::json!({
                "validator_id": hex(&[index; 32]),
                "consensus_key_algorithm_id": descriptor.algorithm_id(),
                "consensus_key_public_key": hex(&descriptor.public_key_bytes()),
                "bonded_stake": DEVNET_BONDED_STAKE.to_string(),
            })
        })
        .collect();

    let genesis = serde_json::json!({
        "manifest_version": 1,
        "chain_id": hn_core::ChainId::HNCHAIN.get(),
        "network_id": NETWORK_ID,
        "genesis_time": 1_758_758_400_u64,
        "genesis_message": "HNChain Devnet Genesis - not for production use",
        "validators": validators,
        "allocations": {
            "reserve": allocation_json(0xA0, hn_state::RESERVE_ALLOCATION)?,
            "founder": allocation_json(0xA1, hn_state::FOUNDER_ALLOCATION)?,
            "community": allocation_json(0xA2, hn_state::COMMUNITY_ALLOCATION)?,
        },
    });

    println!("{}", serde_json::to_string_pretty(&genesis)?);
    Ok(())
}

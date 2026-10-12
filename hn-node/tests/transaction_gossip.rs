//! ADR-0045's own "Decided: Transaction Gossip" proof: a transaction
//! injected into one real validator process, over a real `127.0.0.1`
//! TCP connection from a minimal test-only client (not a full
//! validator — never completes any part of the consensus protocol),
//! reaches a *second* real process purely through the real
//! announce/request/response gossip this pass wires up — never a
//! shared in-memory value, and never block building/validation
//! (ADR-0045's own later steps, not exercised here at all).

mod support;

use std::time::Duration;

use hn_core::AccountNonce;
use hn_crypto::{Ed25519KeyPair, KeyRole, SignatureEnvelope, account_address_body};
use hn_network::{Channel, MessageType};
use hn_state::{
    AccessListV1, TX_VERSION_1, TransactionEnvelope, TransferPayloadV1, TxType, ValidityWindowV1,
};
use support::{
    NETWORK_ID, TestResult, connect_handshake_and_send, hex, spawn_node, wait_until_all_log,
    write_devnet_genesis,
};

const VALIDATOR_COUNT: u8 = 2;
const BASE_PORT: u16 = 31_900;
const BASE_TIMEOUT_MS: u64 = 200;
const WAIT: Duration = Duration::from_secs(90);
const RECIPIENT: [u8; 32] = [0x44; 32];

/// The devnet genesis's own real Reserve allocation account — a real,
/// known keypair (seed `[0xA0; 32]`, matching `write_devnet_genesis`'s
/// own convention exactly), not a separately-seeded fixture account.
/// Its own real `BalanceValueV1` already exists at genesis
/// (`hn_state::RESERVE_ALLOCATION`, comfortably above this test's own
/// tiny transfer amount), so no out-of-band funding step is needed.
fn reserve_keypair() -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed(KeyRole::AccountSigning, [0xA0; 32])
}

fn signed_bootstrap_transfer() -> TestResult<TransactionEnvelope> {
    let keypair = reserve_keypair();
    let sender = account_address_body(
        NETWORK_ID,
        keypair.key_descriptor().algorithm_id(),
        &keypair.key_descriptor().public_key_bytes(),
    )?;
    let payload = TransferPayloadV1 {
        recipient: RECIPIENT,
        asset_id: None,
        amount: 100,
    }
    .encode()?;
    let mut envelope = TransactionEnvelope {
        tx_version: TX_VERSION_1,
        chain_id: hn_core::ChainId::HNCHAIN.get(),
        network_id: NETWORK_ID,
        tx_type: TxType::Transfer,
        sender,
        bootstrap_key: Some(keypair.key_descriptor()),
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
        payload,
        signatures: vec![],
    };
    let digest = envelope.signing_payload().signing_digest()?;
    envelope.signatures = vec![SignatureEnvelope {
        algorithm_id: keypair.key_descriptor().algorithm_id(),
        key_reference: None,
        signature: keypair.sign(&digest).to_vec(),
    }];
    Ok(envelope)
}

#[test]
fn a_transaction_injected_into_one_node_gossips_to_another() -> TestResult {
    let data_root = std::env::temp_dir().join(format!(
        "hn-node-transaction-gossip-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;

    let mut nodes = Vec::new();
    for validator_index in 0..VALIDATOR_COUNT {
        let data_dir = data_root.join(format!("validator-{validator_index}"));
        std::fs::create_dir_all(&data_dir)?;
        nodes.push(spawn_node(
            &genesis_path,
            &data_dir,
            validator_index,
            BASE_PORT,
            &(0..VALIDATOR_COUNT).collect::<Vec<_>>(),
            BASE_TIMEOUT_MS,
        )?);
    }

    // Wait for the two nodes to actually establish their own peer
    // connection and finalize at least one real height together before
    // injecting anything -- `broadcast`'s own `TransactionAnnounce`
    // (below) reaches only `ctx.established` at the moment it runs,
    // and ADR-0045's own "announce once" has no retry if it goes out
    // to an empty peer set. Without this wait, this test would race
    // node 0's own admission against node 0/node 1's still-in-progress
    // handshake -- a real ordering requirement this test must respect,
    // not a gap in the gossip logic itself.
    {
        let node_refs: Vec<&support::NodeProcess> = nodes.iter().collect();
        wait_until_all_log(
            &node_refs,
            |line| line.starts_with("FINALIZED height=1 "),
            WAIT,
        )?;
    }

    let envelope = signed_bootstrap_transfer()?;
    let expected_tx_id = hex(&hn_state::tx_id(&envelope.encode()?)?);

    // Inject directly into node 0 only -- as a bare TransactionResponse,
    // not preceded by this test's own Announce: `handle_peer_message`'s
    // own TransactionResponse arm admits whatever it decodes regardless
    // of whether it was actually requested (ADR-0045 names no "only
    // admit what I asked for" requirement, and a real gossip network
    // routinely receives unsolicited-but-valid responses this same way
    // in practice).
    let node0_addr = support::listen_addr(BASE_PORT, 0);
    let _stream = connect_handshake_and_send(
        node0_addr,
        hn_core::ChainId::HNCHAIN.get(),
        NETWORK_ID,
        Channel::Transactions,
        MessageType::TransactionResponse,
        hn_network::TransactionResponseV1 { envelope }.encode()?,
    )?;

    let expected_line = format!("ADMITTED tx={expected_tx_id}");
    let node_refs: Vec<&support::NodeProcess> = nodes.iter().collect();
    wait_until_all_log(&node_refs, |line| line == expected_line.as_str(), WAIT)?;

    Ok(())
}

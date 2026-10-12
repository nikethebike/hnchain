//! ADR-0045's own end-to-end proof, across the full six-step plan: a
//! transaction injected into one real validator process, over a real
//! `127.0.0.1` TCP connection, really gossips, really gets selected
//! into a real proposed block, really executes at every one of four
//! real validator processes, and really finalizes with `applied=1` --
//! and all four independently recompute the exact same post-block
//! `state_root` (ADR-0045, "Decided: Validator Execution": every
//! validator re-executes rather than trusting the proposer's claim).

mod support;

use std::time::Duration;

use hn_network::{Channel, MessageType};
use support::{
    TestResult, connect_handshake_and_send, height_field, signed_reserve_transfer, spawn_node,
    state_root_field, wait_until_all_log, write_devnet_genesis,
};

const VALIDATOR_COUNT: u8 = 4;
const BASE_PORT: u16 = 32_000;
const BASE_TIMEOUT_MS: u64 = 200;
const WAIT: Duration = Duration::from_secs(90);
const RECIPIENT: [u8; 32] = [0x55; 32];

#[test]
fn a_real_transaction_finalizes_identically_across_four_validators() -> TestResult {
    let data_root = std::env::temp_dir().join(format!(
        "hn-node-transaction-finalization-test-{}",
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
    let node_refs: Vec<&support::NodeProcess> = nodes.iter().collect();

    // Wait for every validator to finish its own handshakes and
    // finalize an empty height=1 together before injecting anything --
    // the same real ordering requirement `transaction_gossip.rs`'s own
    // test already established: `broadcast`'s one-shot
    // `TransactionAnnounce` reaches only whichever peers are already
    // `established` at the moment it runs.
    let genesis_lines = wait_until_all_log(
        &node_refs,
        |line| line.starts_with("FINALIZED height=1 "),
        WAIT,
    )?;
    let genesis_state_roots: Vec<&str> = genesis_lines
        .iter()
        .map(|line| state_root_field(line))
        .collect::<TestResult<_>>()?;
    assert!(
        genesis_state_roots
            .iter()
            .all(|root| *root == genesis_state_roots[0]),
        "all four validators must already agree on height=1's own state_root: {genesis_lines:?}"
    );

    let envelope = signed_reserve_transfer(RECIPIENT, 100)?;
    let node0_addr = support::listen_addr(BASE_PORT, 0);
    let _stream = connect_handshake_and_send(
        node0_addr,
        hn_core::ChainId::HNCHAIN.get(),
        support::NETWORK_ID,
        Channel::Transactions,
        MessageType::TransactionResponse,
        hn_network::TransactionResponseV1 { envelope }.encode()?,
    )?;

    // The real transfer must land in the very next height every
    // validator proposes after admission -- not necessarily height=2
    // by wall-clock accident, but genuinely the first height at or
    // after the one injection raced against; `height_field` on
    // whichever `FINALIZED ... applied=1` line comes first off of
    // node 0 pins down the exact height every other node is then
    // asked to match.
    let node0_ref = [node_refs[0]];
    let applied_lines = wait_until_all_log(
        &node0_ref,
        |line| line.starts_with("FINALIZED height=") && line.contains(" applied=1 "),
        WAIT,
    )?;
    let applied_height = height_field(&applied_lines[0])?;
    let expected_prefix = format!("FINALIZED height={applied_height} ");

    let finalized_lines = wait_until_all_log(
        &node_refs,
        |line| line.starts_with(&expected_prefix) && line.contains(" applied=1 "),
        WAIT,
    )?;
    let state_roots: Vec<&str> = finalized_lines
        .iter()
        .map(|line| state_root_field(line))
        .collect::<TestResult<_>>()?;
    assert!(
        state_roots.iter().all(|root| *root == state_roots[0]),
        "all four validators must independently recompute the same post-transfer state_root: {finalized_lines:?}"
    );
    assert_ne!(
        state_roots[0], genesis_state_roots[0],
        "a real, applied transfer must actually change the state_root"
    );

    Ok(())
}

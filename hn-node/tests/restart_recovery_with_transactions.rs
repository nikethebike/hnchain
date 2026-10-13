//! ADR-0044's own main end-to-end proof, combining its two separate
//! predecessors: `restart_recovery.rs`'s "kill a node, bring it back,
//! it catches up" (ADR-0039) and `transaction_finalization.rs`'s real
//! cross-node transaction agreement (ADR-0045) -- now with real,
//! non-empty history for a restarted node to actually have to fetch,
//! verify, and replay, not just an empty-block height to jump to.
//!
//! Four validators finalize a real transfer, one is killed, the
//! network keeps finalizing (including a *second* real transfer) well
//! past ten blocks without it, and the restarted validator must not
//! only observe and replay its way back to the same height, but go on
//! to independently recompute the exact same `state_root` as its
//! peers once live together again -- `state_root` commits to every
//! account's own balance, so agreement on it is the same proof of
//! "same balances" `transaction_finalization.rs` already relies on,
//! without this codebase needing a dedicated balance-query RPC to
//! check it directly.

mod support;

use std::time::Duration;

use hn_network::{Channel, MessageType};
use support::{
    NodeProcess, TestResult, connect_handshake_and_send, height_field, signed_reserve_transfer,
    spawn_node, state_root_field, wait_until_all_log, write_devnet_genesis,
};

const VALIDATOR_COUNT: u8 = 4;
const BASE_PORT: u16 = 32_200;
const BASE_TIMEOUT_MS: u64 = 150;
const WAIT: Duration = Duration::from_secs(90);
const ALL_VALIDATORS: [u8; 4] = [0, 1, 2, 3];
const RESTARTED_VALIDATOR: u8 = 3;
const RECIPIENT: [u8; 32] = [0x66; 32];
const MINIMUM_SURVIVING_HEIGHT: u64 = 10;

#[test]
fn a_restarted_validator_catches_up_to_the_same_state_root_after_real_transfers() -> TestResult {
    let data_root = std::env::temp_dir().join(format!(
        "hn-node-restart-recovery-with-transactions-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;
    let data_dir = |index: u8| data_root.join(format!("validator-{index}"));

    let mut nodes = Vec::new();
    for validator_index in ALL_VALIDATORS {
        std::fs::create_dir_all(data_dir(validator_index))?;
        nodes.push(spawn_node(
            &genesis_path,
            &data_dir(validator_index),
            validator_index,
            BASE_PORT,
            &ALL_VALIDATORS,
            BASE_TIMEOUT_MS,
        )?);
    }

    // Connectivity first, same ordering requirement
    // `transaction_gossip.rs`'s own test already established.
    {
        let node_refs: Vec<&NodeProcess> = nodes.iter().collect();
        wait_until_all_log(
            &node_refs,
            |line| line.starts_with("FINALIZED height=1 "),
            WAIT,
        )?;
    }

    let node0_addr = support::listen_addr(BASE_PORT, 0);

    // Real transfer #1, while all four are still up.
    let first_transfer = signed_reserve_transfer(RECIPIENT, 100, 0)?;
    let _stream = connect_handshake_and_send(
        node0_addr,
        hn_core::ChainId::HNCHAIN.get(),
        support::NETWORK_ID,
        Channel::Transactions,
        MessageType::TransactionResponse,
        hn_network::TransactionResponseV1 {
            envelope: first_transfer,
        }
        .encode()?,
    )?;
    {
        let node_refs: Vec<&NodeProcess> = nodes.iter().collect();
        wait_until_all_log(
            &node_refs,
            |line| line.starts_with("FINALIZED height=") && line.contains(" applied=1 "),
            WAIT,
        )?;
    }

    // Kill validator 3 for real, waiting for the OS to free its own
    // listening port before moving on (same requirement
    // `restart_recovery.rs`'s own test already established).
    let killed = nodes.remove(usize::from(RESTARTED_VALIDATOR));
    killed.kill_and_wait()?;

    // A second real transfer (nonce 1 -- the Reserve account's own
    // identity is already bootstrapped by the first), injected while
    // one validator is down, so the restarted node's own eventual
    // catch-up has real, non-empty history to actually replay, not
    // just an empty-block height to jump to.
    let second_transfer = signed_reserve_transfer(RECIPIENT, 50, 1)?;
    let _stream2 = connect_handshake_and_send(
        node0_addr,
        hn_core::ChainId::HNCHAIN.get(),
        support::NETWORK_ID,
        Channel::Transactions,
        MessageType::TransactionResponse,
        hn_network::TransactionResponseV1 {
            envelope: second_transfer,
        }
        .encode()?,
    )?;

    // The three survivors keep finalizing well past ten blocks without
    // the dead validator -- proving the network's own progress is
    // real and ongoing while it is down.
    let surviving_height = {
        let node_refs: Vec<&NodeProcess> = nodes.iter().collect();
        let lines = wait_until_all_log(
            &node_refs,
            |line| {
                line.starts_with("FINALIZED height=")
                    && height_field(line).is_ok_and(|height| height >= MINIMUM_SURVIVING_HEIGHT)
            },
            WAIT,
        )?;
        lines
            .iter()
            .map(|line| height_field(line))
            .collect::<TestResult<Vec<_>>>()?
            .into_iter()
            .max()
            .ok_or("no surviving node logged a height")?
    };
    assert!(surviving_height >= MINIMUM_SURVIVING_HEIGHT);

    // Restart validator 3 -- same genesis, same data dir, same
    // identity, as a brand new OS process.
    let restarted = spawn_node(
        &genesis_path,
        &data_dir(RESTARTED_VALIDATOR),
        RESTARTED_VALIDATOR,
        BASE_PORT,
        &ALL_VALIDATORS,
        BASE_TIMEOUT_MS,
    )?;
    nodes.push(restarted);
    let restarted_ref = nodes.last().ok_or("restarted node missing")?;

    // It must really fetch, verify, and replay its way to the
    // network's own real current height -- including the two real
    // transfers above, not just empty blocks.
    wait_until_all_log(
        &[restarted_ref],
        |line| line.starts_with("SYNC COMPLETE height="),
        WAIT,
    )?;

    // Once caught up, it resumes live consensus -- its own first
    // FINALIZED line after that pins down a height every other node
    // (now live together again) is also about to reach.
    let common_height = {
        let lines = wait_until_all_log(
            &[restarted_ref],
            |line| line.starts_with("FINALIZED height="),
            WAIT,
        )?;
        height_field(&lines[0])?
    };
    let expected_prefix = format!("FINALIZED height={common_height} ");
    let node_refs: Vec<&NodeProcess> = nodes.iter().collect();
    let finalized_lines =
        wait_until_all_log(&node_refs, |line| line.starts_with(&expected_prefix), WAIT)?;

    // Every one of the four -- including the one that was dead and
    // just caught up -- independently recomputes the exact same
    // state_root: the same cryptographic proof of "every account's
    // own balance agrees" `transaction_finalization.rs` already
    // relies on, now exercised across a real restart.
    let state_roots: Vec<&str> = finalized_lines
        .iter()
        .map(|line| state_root_field(line))
        .collect::<TestResult<_>>()?;
    assert!(
        state_roots.iter().all(|root| *root == state_roots[0]),
        "all four validators (including the restarted one) must agree on height={common_height}'s own state_root: {finalized_lines:?}"
    );

    Ok(())
}

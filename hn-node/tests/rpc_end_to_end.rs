//! ADR-0046's own main end-to-end proof: a real transfer submitted
//! over HTTP to one validator (`POST /v1/transactions`), polled to
//! `finalized` over HTTP on that same node, and its effect read back
//! over HTTP on a *different* node (`GET /v1/accounts/{address}`) —
//! the first real cross-node agreement check this codebase can make
//! without parsing log lines at all. Also proves `GET /v1/status`'s
//! own `latest_height` genuinely advances over time, and that a
//! restarted validator's own `syncing` flag is observably `true`
//! while it is catching up and `false` again once it rejoins live
//! consensus (ADR-0044's own real sync, now visible over RPC).
//!
//! Four validators, like `restart_recovery_with_transactions.rs`
//! (not one — see that file's own module doc, and `rpc_listen.rs`'s,
//! for the real, unrelated `act_on` recursion bug a lone validator
//! hits).

mod support;

use std::time::Duration;

use hn_crypto::account_address_body;
use support::{
    NETWORK_ID, NodeProcess, TestResult, height_field, http_get_json, http_post_json, listen_addr,
    reserve_keypair, signed_reserve_transfer, spawn_node_with_rpc_listen, wait_until_all_log,
    wait_until_rpc, write_devnet_genesis,
};

const VALIDATOR_COUNT: u8 = 4;
const BASE_PORT: u16 = 32_600;
const RPC_BASE_PORT: u16 = 32_700;
const BASE_TIMEOUT_MS: u64 = 150;
const WAIT: Duration = Duration::from_secs(90);
const ALL_VALIDATORS: [u8; 4] = [0, 1, 2, 3];
const RESTARTED_VALIDATOR: u8 = 3;
const RECIPIENT: [u8; 32] = [0x77; 32];
const MINIMUM_SURVIVING_HEIGHT: u64 = 10;

#[test]
fn rpc_submission_cross_node_balances_status_growth_and_restart_syncing() -> TestResult {
    let data_root = std::env::temp_dir().join(format!(
        "hn-node-rpc-end-to-end-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;
    let data_dir = |index: u8| data_root.join(format!("validator-{index}"));

    // Every validator gets its own RPC listener, at the same index
    // offset as its own P2P listen address -- node A (submission) is
    // validator 0, node B (balance/status reads) is validator 1, and
    // the later-restarted validator 3 needs one too, to observe its
    // own `syncing` flag.
    let mut nodes = Vec::new();
    for validator_index in ALL_VALIDATORS {
        std::fs::create_dir_all(data_dir(validator_index))?;
        nodes.push(spawn_node_with_rpc_listen(
            &genesis_path,
            &data_dir(validator_index),
            validator_index,
            BASE_PORT,
            &ALL_VALIDATORS,
            BASE_TIMEOUT_MS,
            Some(listen_addr(RPC_BASE_PORT, validator_index)),
        )?);
    }
    {
        let node_refs: Vec<&NodeProcess> = nodes.iter().collect();
        wait_until_all_log(
            &node_refs,
            |line| line.starts_with("FINALIZED height=1 "),
            WAIT,
        )?;
    }

    let node_a_rpc = listen_addr(RPC_BASE_PORT, 0);
    let node_b_rpc = listen_addr(RPC_BASE_PORT, 1);

    // Submit a real transfer over HTTP to node A.
    let transfer = signed_reserve_transfer(RECIPIENT, 100, 0)?;
    let tx_bytes = transfer.encode()?;
    let tx_hex = support::hex(&tx_bytes);
    let tx_id_hex = support::hex(&hn_state::tx_id(&tx_bytes)?);

    let (submit_status, submit_body) = http_post_json(
        node_a_rpc,
        "/v1/transactions",
        &serde_json::json!({ "tx": tx_hex }),
    )?;
    assert_eq!(
        submit_status, 202,
        "unexpected submit response: {submit_body}"
    );
    assert_eq!(submit_body["tx_id"], tx_id_hex);
    assert_eq!(submit_body["status"], "accepted");

    // Poll node A over HTTP until it reports the same transaction
    // finalized -- not a log line, the real `/v1/transactions/{tx_id}`
    // endpoint.
    let finalized_tx = wait_until_rpc(
        node_a_rpc,
        &format!("/v1/transactions/{tx_id_hex}"),
        |value| value["status"] == "finalized",
        WAIT,
    )?;
    assert_eq!(finalized_tx["receipt"]["status"], "success");

    // Read the recipient's own balance back on node B -- a different
    // process than the one the transaction was submitted to, so this
    // is a genuine cross-node agreement check, not a local echo.
    let recipient_hex = support::hex(&RECIPIENT);
    let (account_status, account) =
        http_get_json(node_b_rpc, &format!("/v1/accounts/{recipient_hex}"))?;
    assert_eq!(account_status, 200);
    assert_eq!(account["balance"], "100");
    // The recipient never sent anything of its own, so it never
    // bootstraps an identity -- a real balance with no identity yet.
    assert_eq!(account["exists"], false);

    // The sender's own balance moved too, readable from the same
    // node B.
    let sender_keypair = reserve_keypair();
    let sender = account_address_body(
        NETWORK_ID,
        sender_keypair.key_descriptor().algorithm_id(),
        &sender_keypair.key_descriptor().public_key_bytes(),
    )?;
    let (_, sender_account) = http_get_json(
        node_b_rpc,
        &format!("/v1/accounts/{}", support::hex(&sender)),
    )?;
    assert_eq!(sender_account["exists"], true);
    assert_eq!(sender_account["nonce"], 1);

    // status.height genuinely advances over time, read from node B.
    let (_, status_before) = http_get_json(node_b_rpc, "/v1/status")?;
    let height_before = status_before["latest_height"]
        .as_u64()
        .ok_or("missing latest_height")?;
    wait_until_rpc(
        node_b_rpc,
        "/v1/status",
        |value| {
            value["latest_height"]
                .as_u64()
                .is_some_and(|height| height > height_before)
        },
        WAIT,
    )?;

    // Kill validator 3, let the other three keep finalizing well past
    // height 10 without it (same shape as
    // restart_recovery_with_transactions.rs), then restart it with its
    // own RPC listener and watch its `syncing` flag over HTTP: true
    // while it is still catching up, then false once it rejoins live
    // consensus.
    let killed = nodes.remove(usize::from(RESTARTED_VALIDATOR));
    killed.kill_and_wait()?;

    {
        let node_refs: Vec<&NodeProcess> = nodes.iter().collect();
        wait_until_all_log(
            &node_refs,
            |line| {
                line.starts_with("FINALIZED height=")
                    && height_field(line).is_ok_and(|height| height >= MINIMUM_SURVIVING_HEIGHT)
            },
            WAIT,
        )?;
    }

    let restarted_rpc = listen_addr(RPC_BASE_PORT, RESTARTED_VALIDATOR);
    let restarted = spawn_node_with_rpc_listen(
        &genesis_path,
        &data_dir(RESTARTED_VALIDATOR),
        RESTARTED_VALIDATOR,
        BASE_PORT,
        &ALL_VALIDATORS,
        BASE_TIMEOUT_MS,
        Some(restarted_rpc),
    )?;
    nodes.push(restarted);

    wait_until_rpc(
        restarted_rpc,
        "/v1/status",
        |value| value["syncing"] == true,
        WAIT,
    )?;
    wait_until_rpc(
        restarted_rpc,
        "/v1/status",
        |value| value["syncing"] == false,
        WAIT,
    )?;

    Ok(())
}

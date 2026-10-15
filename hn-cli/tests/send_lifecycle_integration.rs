//! ADR-0047's own "Conformance / Test Requirements" integration
//! checklist, against real processes: a real `hn-node` cluster and
//! the real, compiled `hnchain` binary (never hand-built transactions
//! or an in-process shortcut).
//!
//! > `key generate` creates an account, the devnet reserve funds it
//! > through a normal transfer, `send --wait` from that new account to
//! > a second one finalizes; `balance` on a different node shows both
//! > results; a second `send` with a stale `--nonce` is rejected with
//! > the matching exit code 3 ... `--json` output equals the RPC
//! > response body byte for byte.

mod support;

use std::time::Duration;

use hn_cli::amount::parse_decimal_hncoin;
use hn_cli::cli::exit_code;
use support::{
    RESERVE_SEED, TestResult, extract_address, listen_addr, run_hnchain, run_hnchain_text,
    spawn_node, wait_until_rpc, write_devnet_genesis, write_key_file,
};

const VALIDATOR_COUNT: u8 = 4;
const BASE_PORT: u16 = 34_000;
const RPC_BASE_PORT: u16 = 34_100;
const BASE_TIMEOUT_MS: u64 = 200;
const WAIT: Duration = Duration::from_secs(60);

#[test]
fn a_new_accounts_full_lifecycle_across_two_nodes() -> TestResult {
    let data_root =
        std::env::temp_dir().join(format!("hn-cli-send-lifecycle-test-{}", std::process::id()));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;

    let mut nodes = Vec::new();
    for validator_index in 0..VALIDATOR_COUNT {
        let data_dir = data_root.join(format!("validator-{validator_index}"));
        std::fs::create_dir_all(&data_dir)?;
        let rpc_listen = match validator_index {
            0 | 1 => Some(listen_addr(RPC_BASE_PORT, validator_index)),
            _ => None,
        };
        nodes.push(spawn_node(
            &genesis_path,
            &data_dir,
            validator_index,
            VALIDATOR_COUNT,
            BASE_PORT,
            BASE_TIMEOUT_MS,
            rpc_listen,
        )?);
    }

    let node_a_rpc = listen_addr(RPC_BASE_PORT, 0);
    let node_b_rpc = listen_addr(RPC_BASE_PORT, 1);
    let node_a_url = format!("http://{node_a_rpc}");
    let node_b_url = format!("http://{node_b_rpc}");

    // Wait for real liveness on both RPC-enabled nodes before anything
    // else -- otherwise an early `send` just races a server that is
    // not listening yet.
    wait_until_rpc(
        node_a_rpc,
        "/v1/status",
        |value| value["latest_height"].as_u64().is_some_and(|h| h >= 1),
        WAIT,
    )?;
    wait_until_rpc(
        node_b_rpc,
        "/v1/status",
        |value| value["latest_height"].as_u64().is_some_and(|h| h >= 1),
        WAIT,
    )?;

    let reserve_key = data_root.join("reserve.key");
    write_key_file(&reserve_key, RESERVE_SEED)?;
    let reserve_key = reserve_key.to_str().ok_or("non-UTF-8 path")?;

    // `key generate` creates two real accounts.
    let a_key = data_root.join("a.key");
    let a_key = a_key.to_str().ok_or("non-UTF-8 path")?;
    let (code, stdout, stderr) = run_hnchain_text(&["key", "generate", "--out", a_key])?;
    assert_eq!(
        code,
        exit_code::SUCCESS,
        "key generate (a) failed: {stderr}"
    );
    let a_address = extract_address(&stdout)?;

    let b_key = data_root.join("b.key");
    let b_key = b_key.to_str().ok_or("non-UTF-8 path")?;
    let (code, stdout, stderr) = run_hnchain_text(&["key", "generate", "--out", b_key])?;
    assert_eq!(
        code,
        exit_code::SUCCESS,
        "key generate (b) failed: {stderr}"
    );
    let b_address = extract_address(&stdout)?;

    // The devnet reserve funds the new account through a normal
    // transfer.
    let (code, stdout, stderr) = run_hnchain_text(&[
        "--rpc",
        &node_a_url,
        "send",
        "--key",
        reserve_key,
        "--to",
        &a_address,
        "--amount",
        "10",
        "--yes",
        "--wait",
    ])?;
    assert_eq!(
        code,
        exit_code::SUCCESS,
        "reserve funding failed: {stderr}\n{stdout}"
    );
    assert!(
        stdout.contains("receipt: success"),
        "funding did not finalize successfully: {stdout}"
    );

    // `send --wait` from the new account to a second one finalizes.
    let (code, stdout, stderr) = run_hnchain_text(&[
        "--rpc",
        &node_a_url,
        "send",
        "--key",
        a_key,
        "--to",
        &b_address,
        "--amount",
        "3",
        "--yes",
        "--wait",
    ])?;
    assert_eq!(
        code,
        exit_code::SUCCESS,
        "a-to-b transfer failed: {stderr}\n{stdout}"
    );
    assert!(
        stdout.contains("receipt: success"),
        "a-to-b transfer did not finalize successfully: {stdout}"
    );

    // `balance` on the *other* node shows both results.
    let (code, stdout, stderr) = run_hnchain_text(&["--rpc", &node_b_url, "balance", &a_address])?;
    assert_eq!(
        code,
        exit_code::SUCCESS,
        "balance(a) on node b failed: {stderr}"
    );
    assert_eq!(
        extract_hnit_balance(&stdout)?,
        parse_decimal_hncoin("7")?,
        "sender's balance on the other node should be 10 - 3 = 7 HNCOIN (no fee deducted, ADR-0030): {stdout}"
    );

    let (code, stdout, stderr) = run_hnchain_text(&["--rpc", &node_b_url, "balance", &b_address])?;
    assert_eq!(
        code,
        exit_code::SUCCESS,
        "balance(b) on node b failed: {stderr}"
    );
    assert_eq!(
        extract_hnit_balance(&stdout)?,
        parse_decimal_hncoin("3")?,
        "recipient's balance on the other node should be 3 HNCOIN: {stdout}"
    );

    // A second `send` reusing the same (now stale) nonce is rejected
    // with exit code 3.
    let (code, _stdout, stderr) = run_hnchain_text(&[
        "--rpc",
        &node_a_url,
        "send",
        "--key",
        a_key,
        "--to",
        &b_address,
        "--amount",
        "1",
        "--nonce",
        "0",
        "--yes",
    ])?;
    assert_eq!(
        code,
        exit_code::NODE_REJECTED,
        "a stale nonce resend must be rejected with exit code 3: {stderr}"
    );

    // `--json` reproduces the node's own response body byte for byte.
    let json_output = run_hnchain(&["--rpc", &node_b_url, "--json", "balance", &b_address])?;
    assert_eq!(json_output.status.code(), Some(exit_code::SUCCESS));
    let (status, raw_body) = hn_rpc::request(
        node_b_rpc,
        "GET",
        &format!("/v1/accounts/{b_address}"),
        None,
        hn_rpc::ClientConfig {
            connect_timeout: Duration::from_secs(2),
            total_timeout: Duration::from_secs(2),
        },
    )?;
    assert_eq!(status, 200);
    assert_eq!(
        json_output.stdout, raw_body,
        "--json output must equal the node's own response body byte for byte"
    );

    Ok(())
}

fn extract_hnit_balance(stdout: &str) -> TestResult<u128> {
    let line = stdout
        .lines()
        .find(|line| line.starts_with("balance:"))
        .ok_or("no balance: line in hnchain output")?;
    let hnit_text = line
        .split('(')
        .nth(1)
        .and_then(|rest| rest.strip_suffix(" hnit)"))
        .ok_or("balance line missing its (... hnit) form")?;
    Ok(hnit_text.parse()?)
}

//! ADR-0047's own "Conformance / Test Requirements": "`--wait` against
//! a stopped chain exits 4." A real `hn-node` process that can never
//! reach quorum (only one of its genesis's four validators is
//! running) admits a real, signed transaction into its mempool just
//! fine, but it can never finalize -- `send --wait` must time out
//! against it with exit code 4, not hang forever or misreport the
//! outcome as a node-unreachable or node-rejected failure.

mod support;

use std::time::Duration;

use hn_cli::cli::exit_code;
use support::{
    RESERVE_SEED, TestResult, listen_addr, run_hnchain_text, spawn_node, wait_until_rpc,
    write_devnet_genesis, write_key_file,
};

const VALIDATOR_COUNT: u8 = 4;
const BASE_PORT: u16 = 34_300;
const RPC_BASE_PORT: u16 = 34_400;
const BASE_TIMEOUT_MS: u64 = 200;

#[test]
fn wait_against_a_chain_that_cannot_reach_quorum_times_out() -> TestResult {
    let data_root =
        std::env::temp_dir().join(format!("hn-cli-wait-timeout-test-{}", std::process::id()));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;

    let data_dir = data_root.join("validator-0");
    std::fs::create_dir_all(&data_dir)?;
    let rpc_addr = listen_addr(RPC_BASE_PORT, 0);
    // Only validator 0 is ever started -- 1 of 4 is well under any
    // real quorum threshold, so this chain can admit transactions
    // (admission does not require consensus) but can never finalize
    // one.
    let _lone_node = spawn_node(
        &genesis_path,
        &data_dir,
        0,
        VALIDATOR_COUNT,
        BASE_PORT,
        BASE_TIMEOUT_MS,
        Some(rpc_addr),
    )?;
    let rpc_url = format!("http://{rpc_addr}");

    // The RPC server itself must be up before anything else -- unlike
    // the other integration test, this chain's own `latest_height`
    // never advances past 0, so that cannot be the readiness signal.
    wait_until_rpc(
        rpc_addr,
        "/v1/status",
        |value| value.get("chain_id").is_some(),
        Duration::from_secs(30),
    )?;

    let reserve_key = data_root.join("reserve.key");
    write_key_file(&reserve_key, RESERVE_SEED)?;
    let reserve_key = reserve_key.to_str().ok_or("non-UTF-8 path")?;

    let (code, stdout, stderr) = run_hnchain_text(&[
        "--rpc",
        &rpc_url,
        "send",
        "--key",
        reserve_key,
        "--to",
        &"cc".repeat(32),
        "--amount",
        "1",
        "--yes",
        "--wait",
        "--wait-timeout",
        "3",
    ])?;

    assert_eq!(
        code,
        exit_code::WAIT_TIMEOUT,
        "send --wait against a stalled chain must exit 4: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stdout.contains("admitted to the node's mempool"),
        "the transaction must have been genuinely admitted before the wait timed out: {stdout}"
    );
    assert!(
        stderr.contains("timed out"),
        "the timeout reason should be reported: {stderr}"
    );

    Ok(())
}

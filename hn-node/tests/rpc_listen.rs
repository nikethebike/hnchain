//! ADR-0046's own "RPC v1" config gate, proved end to end against real
//! `hn-node` processes: `--rpc-listen` is the only thing that decides
//! whether a node's RPC server exists at all (ADR-0046, "Decided:
//! Architecture And Crate Direction" — `rpc_server_config`/
//! `RpcHandler`, wired from `node::run`) — never a default address
//! nothing asked for.
//!
//! Two validators, not one: a lone validator holds its own entire
//! voting power and finalizes every height purely synchronously, with
//! no network round-trip ever required between one height and the
//! next — `act_on`'s own recursive self-calls
//! (`ConsensusAction::Finalized`/`RoundAdvanced`/`NewHeight` each call
//! `act_on` again directly, never returning through the event loop in
//! between) then recurse once per height with no bound, which
//! overflows the stack after a few seconds. That is a real,
//! independent bug in the single-validator case, unrelated to RPC —
//! reproduces with no `--rpc-listen` involved at all — so this file
//! works around it the same way every other multi-process test here
//! already does (two real peers, a real network round-trip per
//! height) rather than exercising it.

mod support;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use support::{
    NodeProcess, TestResult, listen_addr, reached_exact_height, spawn_node,
    spawn_node_with_rpc_listen, wait_until_all_log, write_devnet_genesis,
};

const VALIDATOR_COUNT: u8 = 2;
// Loosened from an earlier 200ms/90s (same reasoning as
// transaction_finalization.rs and friends: real validator processes
// under a CPU-starved CI runner can legitimately miss a tight round
// timeout, which only delays real progress further by forcing a
// view-change rather than tolerating the slowness).
const BASE_TIMEOUT_MS: u64 = 400;
const WAIT: Duration = Duration::from_secs(180);

// Distinct port ranges per test function: both run concurrently
// (cargo test's own default), each spawning real processes, so
// sharing a port between them would be a genuine cross-test race, not
// merely a style preference.
const BASE_PORT_ABSENT: u16 = 32_400;
const RPC_BASE_PORT_ABSENT: u16 = 32_500;
const BASE_PORT_PRESENT: u16 = 32_440;
const RPC_BASE_PORT_PRESENT: u16 = 32_540;

fn data_root(name: &str) -> TestResult<std::path::PathBuf> {
    let root =
        std::env::temp_dir().join(format!("hn-node-rpc-listen-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    Ok(root)
}

/// Polls `addr` until a TCP connect succeeds, or `timeout` elapses
/// first — the RPC listener binds slightly after process start, so a
/// single immediate connect attempt would race it.
fn wait_until_connectable(addr: SocketAddr, timeout: Duration) -> TestResult<TcpStream> {
    let deadline = Instant::now() + timeout;
    loop {
        match TcpStream::connect(addr) {
            Ok(stream) => return Ok(stream),
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(Box::new(error)),
        }
    }
}

/// Sends a bare `GET {path} HTTP/1.1` request over `stream` and
/// returns the full raw response.
fn http_get(mut stream: TcpStream, path: &str) -> TestResult<String> {
    stream.write_all(format!("GET {path} HTTP/1.1\r\n\r\n").as_bytes())?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    Ok(String::from_utf8(response)?)
}

#[test]
fn the_node_does_not_listen_for_rpc_without_the_flag() -> TestResult {
    let data_root = data_root("absent")?;
    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;

    // No --rpc-listen anywhere -- spawn_node (not the
    // _with_rpc_listen variant) never passes the flag.
    let mut nodes: Vec<NodeProcess> = Vec::new();
    for validator_index in 0..VALIDATOR_COUNT {
        let data_dir = data_root.join(format!("validator-{validator_index}"));
        std::fs::create_dir_all(&data_dir)?;
        nodes.push(spawn_node(
            &genesis_path,
            &data_dir,
            validator_index,
            BASE_PORT_ABSENT,
            &(0..VALIDATOR_COUNT).collect::<Vec<_>>(),
            BASE_TIMEOUT_MS,
        )?);
    }
    let node_refs: Vec<&NodeProcess> = nodes.iter().collect();
    wait_until_all_log(&node_refs, |line| reached_exact_height(line, 1), WAIT)?;

    for validator_index in 0..VALIDATOR_COUNT {
        let rpc_addr = listen_addr(RPC_BASE_PORT_ABSENT, validator_index);
        let result = TcpStream::connect(rpc_addr);
        assert!(
            result.is_err(),
            "expected no RPC listener at {rpc_addr} without --rpc-listen"
        );
    }
    Ok(())
}

#[test]
fn the_node_answers_rpc_status_when_the_flag_is_given() -> TestResult {
    let data_root = data_root("present")?;
    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;

    // Only validator 0 gets --rpc-listen; validator 1 is just a real
    // peer, present so consensus genuinely round-trips over the
    // network instead of finalizing synchronously with no bound (see
    // this file's own module doc).
    let rpc_addr = listen_addr(RPC_BASE_PORT_PRESENT, 0);
    let data_dir_0 = data_root.join("validator-0");
    std::fs::create_dir_all(&data_dir_0)?;
    let node0 = spawn_node_with_rpc_listen(
        &genesis_path,
        &data_dir_0,
        0,
        BASE_PORT_PRESENT,
        &[0, 1],
        BASE_TIMEOUT_MS,
        Some(rpc_addr),
    )?;
    let data_dir_1 = data_root.join("validator-1");
    std::fs::create_dir_all(&data_dir_1)?;
    let node1 = spawn_node(
        &genesis_path,
        &data_dir_1,
        1,
        BASE_PORT_PRESENT,
        &[0, 1],
        BASE_TIMEOUT_MS,
    )?;

    wait_until_all_log(
        &[&node0, &node1],
        |line| reached_exact_height(line, 1),
        WAIT,
    )?;

    let stream = wait_until_connectable(rpc_addr, Duration::from_secs(10))?;
    let response = http_get(stream, "/v1/status")?;

    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "unexpected response: {response}"
    );
    assert!(response.contains("Content-Type: application/json"));
    assert!(response.contains("\"chain_id\""));
    assert!(response.contains("\"syncing\""));

    // Validator 1 never got --rpc-listen -- no server there either.
    let other_rpc_addr = listen_addr(RPC_BASE_PORT_PRESENT, 1);
    assert!(TcpStream::connect(other_rpc_addr).is_err());
    Ok(())
}

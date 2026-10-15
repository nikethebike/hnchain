//! ADR-0047's own "Conformance / Test Requirements": "an unreachable
//! node exits 2." No process is spawned at all here -- the point is
//! exactly that nothing is listening.

mod support;

use hn_cli::cli::exit_code;
use support::{TestResult, run_hnchain_text, write_key_file};

/// A real TCP port nothing listens on for the life of this test
/// process -- distinct from any port the other integration tests in
/// this crate ever bind.
const UNREACHABLE_RPC_URL: &str = "http://127.0.0.1:34999";

#[test]
fn status_against_an_unreachable_node_exits_2() -> TestResult {
    let (code, _stdout, stderr) =
        run_hnchain_text(&["--rpc", UNREACHABLE_RPC_URL, "--timeout", "2", "status"])?;
    assert_eq!(
        code,
        exit_code::NODE_UNREACHABLE,
        "status against an unreachable node must exit 2: {stderr}"
    );
    Ok(())
}

#[test]
fn send_against_an_unreachable_node_exits_2() -> TestResult {
    // `send`'s own step order (ADR-0047) loads and validates the key
    // file *before* the first network call, so a real, loadable key
    // file is required here -- a missing one would fail with
    // USAGE_ERROR (exit 1) and never reach the network step this test
    // is actually about.
    let data_root = std::env::temp_dir().join(format!(
        "hn-cli-unreachable-node-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_root)?;
    let key_path = data_root.join("sender.key");
    write_key_file(&key_path, [0x42; 32])?;
    let key_path = key_path.to_str().ok_or("non-UTF-8 path")?;

    let (code, _stdout, stderr) = run_hnchain_text(&[
        "--rpc",
        UNREACHABLE_RPC_URL,
        "--timeout",
        "2",
        "send",
        "--key",
        key_path,
        "--to",
        &"cc".repeat(32),
        "--amount",
        "1",
        "--yes",
    ])?;
    assert_eq!(
        code,
        exit_code::NODE_UNREACHABLE,
        "send against an unreachable node must exit 2: {stderr}"
    );
    Ok(())
}

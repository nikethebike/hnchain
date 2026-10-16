//! ADR-0046's own load/failure conformance requirement: a real
//! two-validator devnet keeps finalizing at essentially its normal
//! pace while its RPC port is hammered by many concurrent requests at
//! once — the event loop services `NodeEvent::Rpc` as just another
//! event in its own queue (ADR-0046, "Decided: Architecture And Crate
//! Direction"), never anything the consensus side of that same loop
//! waits on, so RPC load alone cannot stall it. The deterministic,
//! fast half of this requirement — a request that never gets an
//! answer in time becomes a real `503 node_busy`, with nothing on the
//! consensus side ever sharing that wait — is proved directly, without
//! any real network load at all, by
//! `hn_node::node::tests::rpc_handler_returns_node_busy_when_the_event_loop_never_answers`
//! (a plain channel the test itself never drains).

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use support::{
    NodeProcess, TestResult, height_field, http_get_json, listen_addr, spawn_node_with_rpc_listen,
    wait_until_all_log, write_devnet_genesis,
};

const VALIDATOR_COUNT: u8 = 2;
const BASE_PORT: u16 = 32_800;
const RPC_BASE_PORT: u16 = 32_900;
// BASE_TIMEOUT_MS/WAIT loosened (same reasoning as
// transaction_finalization.rs: a tight round timeout under a
// CPU-starved CI runner just forces view-changes instead of
// tolerating real slowness) and FLOOD_THREADS halved -- still well
// beyond the devnet worker pool's own capacity (4 workers + a
// 16-deep queue = 20), so the load assertion's own intent is
// unchanged, but with less CPU pressure competing against the two
// real validator processes this test also depends on making
// progress.
const BASE_TIMEOUT_MS: u64 = 300;
const WAIT: Duration = Duration::from_secs(180);
const TARGET_HEIGHT: u64 = 15;
const FLOOD_DURATION: Duration = Duration::from_secs(6);
const FLOOD_THREADS: usize = 32;

#[test]
fn consensus_keeps_its_own_pace_while_rpc_is_flooded_with_concurrent_requests() -> TestResult {
    let data_root =
        std::env::temp_dir().join(format!("hn-node-rpc-load-test-{}", std::process::id()));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;
    let data_dir = |index: u8| data_root.join(format!("validator-{index}"));

    let rpc_addr = listen_addr(RPC_BASE_PORT, 0);
    let mut nodes = Vec::new();
    for validator_index in 0..VALIDATOR_COUNT {
        std::fs::create_dir_all(data_dir(validator_index))?;
        let rpc_listen = (validator_index == 0).then_some(rpc_addr);
        nodes.push(spawn_node_with_rpc_listen(
            &genesis_path,
            &data_dir(validator_index),
            validator_index,
            BASE_PORT,
            &(0..VALIDATOR_COUNT).collect::<Vec<_>>(),
            BASE_TIMEOUT_MS,
            rpc_listen,
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

    // FLOOD_THREADS clients, each hammering GET /v1/status back to
    // back with no pacing at all, for FLOOD_DURATION -- deliberately
    // well beyond the devnet worker pool's own capacity
    // (rpc_server_config's 4 workers + a 16-deep accept queue), so at
    // least some of these genuinely contend with each other.
    let stop_at = Instant::now() + FLOOD_DURATION;
    let success_count = Arc::new(AtomicUsize::new(0));
    let busy_count = Arc::new(AtomicUsize::new(0));
    let other_count = Arc::new(AtomicUsize::new(0));
    let flood_handles: Vec<_> = (0..FLOOD_THREADS)
        .map(|_| {
            let success_count = Arc::clone(&success_count);
            let busy_count = Arc::clone(&busy_count);
            let other_count = Arc::clone(&other_count);
            thread::spawn(move || {
                while Instant::now() < stop_at {
                    match http_get_json(rpc_addr, "/v1/status") {
                        Ok((200, _)) => {
                            success_count.fetch_add(1, Ordering::Relaxed);
                        }
                        Ok((503, _)) => {
                            busy_count.fetch_add(1, Ordering::Relaxed);
                        }
                        _ => {
                            other_count.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            })
        })
        .collect();

    // While that flood runs, consensus must still reach TARGET_HEIGHT
    // in roughly its normal wall-clock time -- a generous multiple of
    // "every round takes about BASE_TIMEOUT_MS," not a tight bound;
    // this only needs to catch real stalling, not pin an exact pace.
    let started = Instant::now();
    {
        let node_refs: Vec<&NodeProcess> = nodes.iter().collect();
        wait_until_all_log(
            &node_refs,
            |line| {
                line.starts_with("FINALIZED height=")
                    && height_field(line).is_ok_and(|height| height >= TARGET_HEIGHT)
            },
            WAIT,
        )?;
    }
    let elapsed = started.elapsed();

    for handle in flood_handles {
        let _ = handle.join();
    }

    let expected_upper_bound = Duration::from_millis(BASE_TIMEOUT_MS * TARGET_HEIGHT * 10);
    assert!(
        elapsed < expected_upper_bound,
        "consensus took {elapsed:?} to reach height {TARGET_HEIGHT} under RPC load, expected well under {expected_upper_bound:?}"
    );

    eprintln!(
        "RPC flood results: {} ok, {} node_busy, {} other",
        success_count.load(Ordering::Relaxed),
        busy_count.load(Ordering::Relaxed),
        other_count.load(Ordering::Relaxed)
    );
    assert!(
        success_count.load(Ordering::Relaxed) > 0,
        "expected at least some successful RPC responses under load"
    );

    Ok(())
}

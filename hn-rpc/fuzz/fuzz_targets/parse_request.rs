#![no_main]

use libfuzzer_sys::fuzz_target;

// ADR-0046, "Conformance / Test Requirements": "A cargo-fuzz target
// over parse_request exists in the repository and is run
// periodically (not a CI gate), with findings added to the corpus
// above." The corpus it refers to is `hn-rpc/src/parser.rs`'s own
// `#[cfg(test)] mod tests` table-driven bad-input cases plus
// `never_panics_on_arbitrary_byte_input` (a dependency-free
// deterministic PRNG covering the same "never panic" property this
// target checks, just without libFuzzer's coverage-guided search).
// `max_body_len` is a fixed, generous stand-in for the real caller-
// resolved bound (`hn_node`'s own `rpc_server_config`, `2 *
// MAX_TRANSACTION_SIZE + 1 KiB`) -- that exact value is never
// attacker-controlled in production, so fuzzing it brings no extra
// realism, only extra search-space noise.
const MAX_BODY_LEN: usize = 8 * 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    let _ = hn_rpc::parse_request(data, MAX_BODY_LEN);
});

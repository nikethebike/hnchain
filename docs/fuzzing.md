# Fuzzing

What is fuzzed, how to run it, what has been run, and what to do when
it finds something. This file is the project-level record; the
crate-local details live in `hn-rpc/fuzz/README.md`.

## Targets

| Target | Crate | Why it is fuzzed |
|---|---|---|
| `parse_request` | `hn-rpc/fuzz` | Hand-written HTTP/1.1 request parsing on a network-facing port (ADR-0046). The riskiest piece of that ADR. |

Fuzzing is **not a CI gate** and the fuzz crate is **not a member of the
main workspace** (its own `[workspace]` table). It needs the nightly
toolchain and `cargo-fuzz`, which the rest of the project must not be
forced onto. Anything the fuzzer finds becomes a permanent, named test
in the normal suite.

Candidates for future targets (add when the code exists, not before):
the HNCS decoders in `hn-codec`/`hn-state` that read network input
(transaction envelope, block envelope, consensus messages), the
`hn-network` frame decoder, and the CLI key-file and amount parsers
(low risk, local input).

## Running

Linux or macOS with nightly:

```sh
cargo install cargo-fuzz --locked
cd hn-rpc
cargo +nightly fuzz run parse_request -- -max_total_time=900
```

Windows cannot build `cargo-fuzz` here, so use a Linux container. Run
from the repository root:

```sh
docker run --rm -v "$PWD":/work -w /work/hn-rpc rustlang/rust:nightly \
  bash -c 'cargo install cargo-fuzz --locked && \
           cargo fuzz run parse_request -- -max_total_time=900'
```

Notes:

- `-max_total_time=900` is 15 minutes. That is the baseline duration
  recorded below.
- Run it again for longer (an hour or more) before any testnet launch
  and before mainnet.
- The container writes into your working tree. `corpus/` and
  `artifacts/` appear under `hn-rpc/fuzz/` and are git-ignored unless a
  corpus is committed (see below).
- The Docker command above is the recipe shape. If your invocation
  differs (image tag, mount, extra flags), replace it here so the next
  person can reproduce the run exactly.

## Results log

Append a row for every meaningful run. Never delete rows.

| Date | Commit | Target | Duration | Executions | Crashes / panics | Coverage |
|---|---|---|---|---|---|---|
| 2026-10-16 | `4ea5c4a` | `parse_request` | 15 min (libFuzzer in Docker) | ~340 million | 0 | Reached a plateau |

## When to run

- After **any** change to `hn-rpc/src/parser.rs` (or whatever replaces
  it), before merging if the change touches parsing logic, otherwise
  within the same week.
- After adding a new fuzz target: one full run before it is considered
  done.
- Before a testnet or mainnet launch: long run (an hour or more) per
  target, recorded above.
- If a new network-facing parser is added without a fuzz target, that
  is a review finding, not an optional extra.

## When it finds something

1. Do not "fix and move on". Keep the input that crashed
   (`hn-rpc/fuzz/artifacts/<target>/crash-*`).
2. Minimize it: `cargo +nightly fuzz tmin parse_request <crash-file>`.
3. Add it as a new **named** case in the parser's own test module
   (`hn-rpc/src/parser.rs`), asserting the exact rejection the parser
   should return instead of panicking. That test module is the permanent
   record of every input the parser has been proven to handle.
4. Fix the parser. Commit the test and the fix together, with the
   crash class in the commit message.
5. Re-run the fuzzer for at least the baseline duration and add a row
   to the results log.

## Corpus

Today the corpus is not committed. A small, minimized corpus makes
later runs start from the coverage already reached instead of from
scratch:

```sh
cd hn-rpc
cargo +nightly fuzz cmin parse_request
```

Commit `hn-rpc/fuzz/corpus/parse_request/` only if it stays small
(a few hundred KB at most). If it grows, keep it out of git and note
where it is stored.

## Not covered by fuzzing

Fuzzing finds crashes, hangs and sanitizer violations. It does not
prove the parser accepts exactly what ADR-0046 says it accepts. That is
the job of the table-driven tests and the golden requests in
`parser.rs`. Do not treat a clean fuzz run as evidence of spec
conformance.
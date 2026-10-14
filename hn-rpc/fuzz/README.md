# `hn-rpc` fuzz targets

ADR-0046 ("RPC v1"), "Conformance / Test Requirements": `parse_request`
is hand-written HTTP parsing on a network-facing port — the riskiest
part of that ADR — so, beyond the table-driven bad-input corpus and
the dependency-free randomized no-panic test already in
`hn-rpc/src/parser.rs`'s own `#[cfg(test)]` module, it also gets a real
coverage-guided fuzz target here.

This crate is deliberately **not** a member of the main workspace (see
its own `Cargo.toml`'s trailing `[workspace]` table) and is **never**
built or run by `cargo build/test/clippy --workspace`. It requires the
nightly toolchain and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz):

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run parse_request
```

Run periodically, by hand — not a CI gate. If it finds a crash or a
panic, take the minimized input `cargo fuzz` prints (or the file it
saves under `artifacts/parse_request/`) and add it as a new named case
in `hn-rpc/src/parser.rs`'s own test module, asserting the exact
rejection `parse_request` should have returned instead of panicking —
the corpus there is the permanent record of every input this parser
has ever been proven to handle correctly; this fuzz target only ever
feeds it new cases, never replaces it.

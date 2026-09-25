use hn_crypto::Digest;

use crate::error::StateResult;

/// Reads canonical state values by state key (ADR-0019, "Core interfaces":
/// `StateReader`).
///
/// This is the "State Access Interface" layer of ADR-0019's own boundary
/// diagram (`Execution Engine -> State Access Interface -> Authenticated
/// State Tree -> Storage Interface -> Storage Backend`) — a protocol-
/// facing interface this crate defines, per its own charter ("owns
/// protocol state interfaces without binding them to a concrete storage
/// engine"), not a storage-backend adapter. `hn-storage` provides
/// concrete implementations (an in-memory one and, per ADR-0019's
/// "Decided: initial storage backend", a `redb`-backed durable one);
/// this crate never depends on `hn-storage` itself, only the reverse, so
/// protocol code here (for example [`crate::active_key`]) can be written
/// once against `impl StateReader` and run unchanged against any backend
/// that implements it later.
///
/// Covers `StateReader`/`StateWriter`/`StateCommitter` of ADR-0019's
/// nine named conceptual interfaces (`StateReader`, `StateWriter`,
/// `StateTransaction`, `StateCommitter`, `BlockStore`, `ProofStore`,
/// `SnapshotStore`, `PruningController`, `ArchiveStore`) — these three
/// are the ones with a real caller today ([`crate::active_key`],
/// [`crate::fetch_validator_record`], [`crate::apply_and_commit_block`]
/// as of ADR-0033); the rest have no consumer anywhere in this codebase
/// yet, and defining them now would be speculative architecture ahead
/// of actual need, the same discipline already applied when
/// [`crate::active_set`]/[`crate::is_eligible_signer`] were built
/// without inventing a storage trait at all.
///
/// `StateResult`-wrapped, not the bare `Option`/`()` this trait carried
/// before a durable backend existed: an in-memory backend genuinely
/// cannot fail this way, but a real one can (disk I/O, corruption —
/// exactly what this signature was already documented as expecting to
/// need once such a backend existed). Errors surface as
/// [`crate::StateError::Storage`], a backend-agnostic `String` rather
/// than a typed sub-error — per ADR-0019's "Backend Independence" rule,
/// this interface must not leak which specific backend failed.
pub trait StateReader {
    /// Returns the canonical HNCS bytes stored at `state_key`, or `None`
    /// if nothing is stored there. Per ADR-0019 ("Canonical Bytes At
    /// Boundaries"), this is the object's canonical encoding — for
    /// example [`crate::ValidatorRecordV1::encode`]'s output — not a
    /// leaf hash or any backend-specific representation.
    fn get(&self, state_key: &Digest) -> StateResult<Option<Vec<u8>>>;
}

/// Writes canonical state values by state key (ADR-0019, "Core
/// interfaces": `StateWriter`). See [`StateReader`]'s documentation for
/// the scope and fallibility reasoning shared by both traits.
pub trait StateWriter {
    /// Stores `value` (already-canonical HNCS bytes) at `state_key`,
    /// replacing whatever was stored there before.
    fn set(&mut self, state_key: Digest, value: Vec<u8>) -> StateResult<()>;
}

/// Atomically applies a whole write set (ADR-0019, "Decided:
/// Atomic State Commit" / "Write Set Boundary" — the `StateCommitter`
/// role of its own nine-interface boundary diagram; ADR-0033, "Atomic
/// Write-Set Commit"). Deliberately narrower than ADR-0019's full
/// "Atomic State Commit" scope, which also lists block header, block
/// body, receipts, events, and consensus metadata as needing to land in
/// the same atomic commit — none of those exist as concrete stored
/// objects anywhere in this codebase yet, so this trait covers exactly
/// what [`Write`] already gives it: a deterministic write set (see
/// ADR-0033's own "Explicitly Not Resolved").
///
/// Contract: all-or-nothing. If `commit` returns `Err`, none of
/// `writes` may be observable afterward via [`StateReader::get`]; if it
/// returns `Ok`, every one of them must be. This is a real,
/// backend-specific guarantee a naive loop of [`StateWriter::set`]
/// calls does not provide (an error partway through would leave the
/// earlier entries applied and the rest not) — the entire reason this
/// is its own trait, with no default implementation: every implementor
/// must consciously decide how it achieves atomicity, not silently
/// inherit a non-atomic one.
pub trait StateCommitter {
    /// Applies every entry in `writes`, atomically.
    fn commit(&mut self, writes: &[Write]) -> StateResult<()>;
}

/// One `(state_key, value)` write-set entry — every `apply_*` function
/// in this crate's real output (ADR-0031, "Write-Set Value Bytes And
/// Overlay State Reader"), replacing the earlier `Leaf`-only
/// `(state_key, leaf_hash)` shape those functions used to return.
/// `value` is exactly what [`StateReader::get`] would return for
/// `state_key` after this write applies (already-canonical HNCS bytes,
/// not a hash) — the same "canonical bytes at boundaries" contract
/// [`StateReader`]/[`StateWriter`] already state, bundled once so it can
/// travel as a single return value through an `apply_*` function, an
/// [`crate::AppliedTransaction::write_set`], and into
/// [`crate::OverlayReader`]. A [`crate::tree::Leaf`] (hash pair) is
/// still derivable on demand from a `Write` via
/// [`crate::leaf_for_write`], never computed redundantly alongside it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Write {
    /// The state key this write targets (ADR-0007).
    pub state_key: Digest,
    /// The already-canonical HNCS bytes to store there.
    pub value: Vec<u8>,
}

use hn_crypto::Digest;

/// Reads canonical state values by state key (ADR-0019, "Core interfaces":
/// `StateReader`).
///
/// This is the "State Access Interface" layer of ADR-0019's own boundary
/// diagram (`Execution Engine -> State Access Interface -> Authenticated
/// State Tree -> Storage Interface -> Storage Backend`) — a protocol-
/// facing interface this crate defines, per its own charter ("owns
/// protocol state interfaces without binding them to a concrete storage
/// engine"), not a storage-backend adapter. `hn-storage` provides
/// concrete implementations (starting with an in-memory one); this crate
/// never depends on `hn-storage` itself, only the reverse, so protocol
/// code here (for example [`crate::active_key`]) can be written once
/// against `impl StateReader` and run unchanged against any backend that
/// implements it later.
///
/// Deliberately covers only `StateReader`/`StateWriter` of ADR-0019's
/// nine named conceptual interfaces (`StateReader`, `StateWriter`,
/// `StateTransaction`, `StateCommitter`, `BlockStore`, `ProofStore`,
/// `SnapshotStore`, `PruningController`, `ArchiveStore`) — these two are
/// the ones with a real caller today ([`crate::active_key`],
/// [`crate::fetch_validator_record`]); the rest have no consumer
/// anywhere in this codebase yet, and defining them now would be
/// speculative architecture ahead of actual need, the same discipline
/// already applied when [`crate::active_set`]/[`crate::is_eligible_signer`]
/// were built without inventing a storage trait at all.
///
/// Deliberately infallible for now, not `StateResult`-wrapped: an
/// in-memory backend genuinely cannot fail this way, and ADR-0019's own
/// corruption-handling/recovery rules are still open ("Open Decisions").
/// A real durable backend will very likely need a fallible signature —
/// changing this trait to return a `Result` once that backend and its
/// error taxonomy actually exist is expected, not something this
/// decision forecloses.
pub trait StateReader {
    /// Returns the canonical HNCS bytes stored at `state_key`, or `None`
    /// if nothing is stored there. Per ADR-0019 ("Canonical Bytes At
    /// Boundaries"), this is the object's canonical encoding — for
    /// example [`crate::ValidatorRecordV1::encode`]'s output — not a
    /// leaf hash or any backend-specific representation.
    fn get(&self, state_key: &Digest) -> Option<Vec<u8>>;
}

/// Writes canonical state values by state key (ADR-0019, "Core
/// interfaces": `StateWriter`). See [`StateReader`]'s documentation for
/// the scope and infallibility reasoning shared by both traits.
pub trait StateWriter {
    /// Stores `value` (already-canonical HNCS bytes) at `state_key`,
    /// replacing whatever was stored there before.
    fn set(&mut self, state_key: Digest, value: Vec<u8>);
}

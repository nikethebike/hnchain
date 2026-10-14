use crate::error::RpcError;

/// `GET /v1/status` (ADR-0046, "Decided: v1 Endpoint Set"). Raw Rust
/// values, not yet the hex/decimal-string wire shapes ADR-0046's
/// "Decided: Value Encoding Rules" settles — that mapping is the still-
/// unbuilt HTTP layer's own job (ADR-0046, "Decided: HTTP Server Is
/// Hand-Written On `std::net`...").
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusReply {
    /// HNChain protocol lineage (ADR-0006, "Chain And Network
    /// Binding").
    pub chain_id: u8,
    /// Network environment (ADR-0003).
    pub network_id: u16,
    /// Genesis block's own `block_hash` — fixed for this chain's whole
    /// lifetime, unlike `latest_block_hash` below.
    pub genesis_hash: [u8; 32],
    /// This node's own protocol version, as `(major, minor, patch)`.
    pub protocol_version: (u16, u16, u16),
    /// This node's own last finalized height.
    pub latest_height: u64,
    /// That height's own block hash (genesis's own hash, before any
    /// real block has finalized).
    pub latest_block_hash: [u8; 32],
    /// That height's own timestamp, in Unix milliseconds.
    pub latest_timestamp: u64,
    /// Whether this node is currently running ADR-0044's historical
    /// sync rather than participating in live rounds.
    pub syncing: bool,
    /// The number of currently-established peer connections.
    pub peer_count: u64,
    /// The number of currently-admitted mempool transactions.
    pub mempool_size: u64,
}

/// One finalized transaction's outcome, as embedded in a [`BlockReply`]
/// (ADR-0046, "Decided: v1 Endpoint Set" — block shape's own `receipts`
/// list).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiptEntry {
    /// The transaction this receipt belongs to.
    pub tx_id: [u8; 32],
    /// Whether the transaction's payload succeeded
    /// (`hn_state::ReceiptStatus::Success`); nonce and fee effects
    /// still applied either way.
    pub success: bool,
}

/// A finalized block's own `commit_qc`, summarized rather than
/// embedded in full (ADR-0046, "Decided: v1 Endpoint Set" — block
/// shape's own "`commit_qc` summary").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitQcSummary {
    /// The height this certificate finalized.
    pub height: u64,
    /// The consensus round it was formed at.
    pub round: u64,
    /// The number of validators whose signatures this certificate
    /// aggregates.
    pub signer_count: usize,
}

/// A finalized block's header fields (ADR-0046, "Decided: v1 Endpoint
/// Set" — block shape's own "Header fields (all `BlockHeader`
/// fields...)"), field order matching `hn_state::BlockHeader` exactly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockHeaderReply {
    /// Structure version for this header shape.
    pub header_version: u16,
    /// HNChain protocol lineage.
    pub chain_id: u8,
    /// Network environment.
    pub network_id: u16,
    /// This block's position in the chain.
    pub height: u64,
    /// The consensus round this block was finalized at.
    pub round: u64,
    /// The consensus-protocol validator-set epoch.
    pub epoch: u64,
    /// Which set of ADR-defined structure-version profiles is active
    /// at this height.
    pub protocol_epoch: u64,
    /// The parent header's own `block_hash`.
    pub parent_block_hash: [u8; 32],
    /// The proposing validator's own `address_body`.
    pub proposer: [u8; 32],
    /// This block's timestamp, in Unix milliseconds.
    pub timestamp: u64,
    /// `hn-list-merkle-v1` commitment over each transaction's `tx_id`.
    pub transactions_root: [u8; 32],
    /// The post-execution state commitment.
    pub state_root: [u8; 32],
    /// `hn-list-merkle-v1` commitment over each receipt's digest.
    pub receipts_root: [u8; 32],
    /// Commitment to consensus-visible events.
    pub events_root: [u8; 32],
    /// The active validator set's `validator_set_commitment`.
    pub consensus_root: [u8; 32],
    /// `hn-list-merkle-v1` commitment over included evidence.
    pub evidence_root: [u8; 32],
    /// Commitment to the active protocol parameter set.
    pub protocol_parameters_hash: [u8; 32],
    /// Commitment to `BlockBody.extra_data`.
    pub extra_data_hash: [u8; 32],
}

/// `GET /v1/blocks/latest` and `GET /v1/blocks/{height}` (ADR-0046,
/// "Decided: v1 Endpoint Set" — full transaction bytes are
/// deliberately not embedded; each is fetched separately through
/// `GET /v1/transactions/{tx_id}`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockReply {
    /// This block's header fields.
    pub header: BlockHeaderReply,
    /// Every included transaction's own `tx_id`, in block order.
    pub tx_ids: Vec<[u8; 32]>,
    /// Every included transaction's own receipt outcome, in the same
    /// order as `tx_ids`.
    pub receipts: Vec<ReceiptEntry>,
    /// The certificate that finalized this block, summarized.
    pub commit_qc: CommitQcSummary,
}

/// `GET /v1/accounts/{address}` (ADR-0046, "Decided: v1 Endpoint
/// Set").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountReply {
    /// Whether this account has bootstrapped an identity
    /// (`hn_state::fetch_identity` returning `Some`) — an account with
    /// a balance but no identity does not yet "exist" in this sense.
    pub exists: bool,
    /// The account's current native HNCOIN balance, in hnits.
    pub balance: u128,
    /// The account's current nonce.
    pub nonce: u64,
    /// The height this snapshot was read at.
    pub height: u64,
}

/// `GET /v1/transactions/{tx_id}`'s own three-way outcome (ADR-0046,
/// "Decided: v1 Endpoint Set" — "`status`: `finalized` ... \| `pending`
/// ... \| `unknown`").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionStatus {
    /// Included in a finalized block.
    Finalized {
        /// The block height it was included at.
        height: u64,
        /// Its position within that block's own transaction list.
        index: u32,
        /// Whether its payload succeeded.
        success: bool,
    },
    /// Currently admitted to this node's own mempool, not yet
    /// finalized.
    Pending,
    /// Neither finalized nor currently pending on this node.
    Unknown,
}

/// `GET /v1/transactions/{tx_id}`'s full reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionReply {
    /// The transaction this reply is about (echoed back, since
    /// `Unknown`/`Pending` would otherwise carry no identifying field
    /// at all).
    pub tx_id: [u8; 32],
    /// This transaction's current status.
    pub status: TransactionStatus,
}

/// `POST /v1/transactions`'s success reply (ADR-0046, "Decided:
/// Transaction Submission" — `202 {"tx_id": "...", "status":
/// "accepted"}`; `accepted` means admitted to this node's own mempool,
/// never finalized).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubmittedReply {
    /// The newly-admitted transaction's own `tx_id`.
    pub tx_id: [u8; 32],
}

/// `GET /v1/mempool` (ADR-0046, "Decided: v1 Endpoint Set" — "counts
/// only; no tx listing in v1").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MempoolReply {
    /// The number of currently-admitted transactions.
    pub size: u64,
    /// This mempool's own fixed capacity (`hn_node::MEMPOOL_MAX_TXS`).
    pub capacity: u64,
}

/// Mirrors `hn_state::ValidatorStatus` (ADR-0010, "Validator Status")
/// — redeclared here rather than reused directly, since this crate
/// depends on `hn_core` only, never `hn_state` (ADR-0046, "Decided:
/// Architecture And Crate Direction").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidatorStatusReply {
    /// A record exists, but bonded stake has not yet met the minimum
    /// bond.
    Registered,
    /// Bonded stake meets the minimum bond; awaiting activation.
    Candidate,
    /// Eligible to participate in consensus.
    Active,
    /// Not currently participating, by choice or after jailing lapses.
    Inactive,
    /// Excluded from signing and from `total_voting_power` right now.
    Jailed,
    /// Permanently left the validator set.
    Exited,
}

/// One validator record this node currently holds, as reported by
/// `GET /v1/validators` (ADR-0046, "Decided: v1 Endpoint Set" —
/// "address, status, bonded stake").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatorEntry {
    /// The validator's own `validator_id` (ADR-0010).
    pub address: [u8; 32],
    /// The validator's current lifecycle status.
    pub status: ValidatorStatusReply,
    /// The validator's current bonded stake.
    pub bonded_stake: u128,
}

/// `GET /v1/validators`'s full reply.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatorsReply {
    /// Every validator record this node currently holds.
    pub validators: Vec<ValidatorEntry>,
}

/// One answer to an [`crate::RpcCall`] — exactly one variant per call,
/// a flat enum rather than per-call return types so the node's event
/// loop (ADR-0046, "Decided: Architecture And Crate Direction") can
/// carry a single reply type across its bounded one-shot channel
/// regardless of which call produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RpcReply {
    /// Answers [`crate::RpcCall::Status`].
    Status(StatusReply),
    /// Answers [`crate::RpcCall::LatestBlock`] / `BlockAtHeight`. Boxed:
    /// `BlockReply` is by far this enum's largest variant (an 18-field
    /// header plus two `Vec`s), and every other variant would otherwise
    /// pay for its size (`clippy::large_enum_variant`).
    Block(Box<BlockReply>),
    /// Answers [`crate::RpcCall::Account`].
    Account(AccountReply),
    /// Answers [`crate::RpcCall::Transaction`].
    Transaction(TransactionReply),
    /// Answers [`crate::RpcCall::SubmitTransaction`] on success.
    Submitted(SubmittedReply),
    /// Answers [`crate::RpcCall::Mempool`].
    Mempool(MempoolReply),
    /// Answers [`crate::RpcCall::Validators`].
    Validators(ValidatorsReply),
}

/// The outcome of answering one [`crate::RpcCall`] against this node's
/// own current state — `Err` for every client-facing rejection
/// ADR-0046 names (malformed input, not found, submission rejected,
/// busy); a genuine internal/storage failure is not one of these and
/// is never folded into it (ADR-0046 says nothing about a node's own
/// storage breaking — that is this crate's caller's own hard-error
/// path, same as everywhere else in this codebase).
pub type RpcResult = Result<RpcReply, RpcError>;

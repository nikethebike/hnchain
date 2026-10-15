//! `hnchain`'s process entry point (ADR-0047, "Decided: v1 Command
//! Set"). Parses argv into an [`hn_cli::cli::Invocation`] and runs
//! `key generate`/`key show` (ADR-0047, "Decided: Key Files"), the six
//! read-only `/v1` commands (status, balance, tx show, block show,
//! validators, mempool), and `send`'s own build-sign-submit flow
//! (ADR-0047, "Decided: Building And Signing A Transfer").

use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

use hn_cli::cli::{self, BalanceTarget, BlockRef, SendArgs, exit_code};
use hn_cli::{amount, hex, keyfile, profile, rpc, sanitize, send, transfer};
use hn_crypto::{Ed25519KeyPair, account_address_body};
use hn_state::ValidityWindowV1;
use serde_json::Value;

/// `send --wait`'s own default poll deadline when `--wait-timeout` is
/// not given -- a CLI-local operational default, not a protocol value
/// (ADR-0047 names no default of its own for this).
const DEFAULT_WAIT_TIMEOUT_SECS: u64 = 30;

/// How often `send --wait` polls `GET /v1/transactions/{tx_id}`.
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(500);

fn main() {
    let invocation = match cli::parse(std::env::args().skip(1)) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
    };

    match invocation.command {
        cli::Command::Help => {
            print!("{}", cli::help_text());
            std::process::exit(exit_code::SUCCESS);
        }
        cli::Command::KeyGenerate { out } => match keyfile::generate(&out) {
            Ok(keypair) => {
                print_key_summary(&keypair);
                if !keyfile::enforces_unix_permissions() {
                    eprintln!(
                        "hnchain: this platform has no Unix file permissions -- hnchain cannot enforce that this key file stays unreadable by other accounts"
                    );
                }
                println!(
                    "hnchain: this key file is NOT encrypted -- protect it with file permissions and your own backups"
                );
                std::process::exit(exit_code::SUCCESS);
            }
            Err(error) => {
                eprintln!("hnchain: {error}");
                std::process::exit(exit_code::USAGE_ERROR);
            }
        },
        cli::Command::KeyShow { key } => match keyfile::load(&key) {
            Ok(keypair) => {
                print_key_summary(&keypair);
                std::process::exit(exit_code::SUCCESS);
            }
            Err(error) => {
                eprintln!("hnchain: {error}");
                std::process::exit(exit_code::USAGE_ERROR);
            }
        },
        cli::Command::Status => {
            run_read_command(&invocation.global, "/v1/status".to_string(), print_status)
        }
        cli::Command::Balance(target) => {
            let address = match resolve_balance_address(target) {
                Ok(address) => address,
                Err(message) => {
                    eprintln!("hnchain: {message}");
                    std::process::exit(exit_code::USAGE_ERROR);
                }
            };
            run_read_command(
                &invocation.global,
                format!("/v1/accounts/{}", hex::encode(&address)),
                print_balance,
            );
        }
        cli::Command::TxShow { tx_id } => {
            let tx_id = match hex::decode_32(&tx_id) {
                Ok(tx_id) => tx_id,
                Err(error) => {
                    eprintln!("hnchain: invalid tx_id: {error}");
                    std::process::exit(exit_code::USAGE_ERROR);
                }
            };
            run_read_command(
                &invocation.global,
                format!("/v1/transactions/{}", hex::encode(&tx_id)),
                print_transaction,
            );
        }
        cli::Command::BlockShow { target } => {
            let path = match target {
                BlockRef::Latest => "/v1/blocks/latest".to_string(),
                BlockRef::Height(height) => format!("/v1/blocks/{height}"),
            };
            run_read_command(&invocation.global, path, print_block);
        }
        cli::Command::Validators => {
            run_read_command(
                &invocation.global,
                "/v1/validators".to_string(),
                print_validators,
            );
        }
        cli::Command::Mempool => {
            run_read_command(&invocation.global, "/v1/mempool".to_string(), print_mempool);
        }
        cli::Command::Send(args) => run_send(&invocation.global, args),
    }
}

fn print_key_summary(keypair: &Ed25519KeyPair) {
    let descriptor = keypair.key_descriptor();
    let public_key = descriptor.public_key_bytes();
    match account_address_body(profile::NETWORK_ID, descriptor.algorithm_id(), &public_key) {
        Ok(address) => println!("address:    {}", hex::encode(&address)),
        Err(error) => println!("address:    <could not derive: {error}>"),
    }
    println!("public_key: {}", hex::encode(&public_key));
}

/// `balance`'s own target resolution: a bare address is decoded as-is;
/// `--key <file>` is loaded and its own address derived, exactly as
/// `key show` does.
fn resolve_balance_address(target: BalanceTarget) -> Result<[u8; 32], String> {
    match target {
        BalanceTarget::Address(address) => {
            hex::decode_32(&address).map_err(|error| format!("invalid address: {error}"))
        }
        BalanceTarget::Key(path) => {
            let keypair = keyfile::load(&path).map_err(|error| error.to_string())?;
            let descriptor = keypair.key_descriptor();
            account_address_body(
                profile::NETWORK_ID,
                descriptor.algorithm_id(),
                &descriptor.public_key_bytes(),
            )
            .map_err(|error| format!("could not derive address: {error}"))
        }
    }
}

/// Performs `GET path`, then either prints the node's own response
/// bytes unchanged (`--json`) or parses and hands the body to `print`
/// for human-readable output -- in both cases mapping the outcome to
/// this binary's own exit codes (ADR-0047, "Decided: Output, Exit
/// Codes, Errors").
fn run_read_command(global: &cli::GlobalOptions, path: String, print: fn(&Value)) -> ! {
    let (status, body) = match rpc::get(global, &path) {
        Ok(result) => result,
        Err(rpc::RpcFailure::Usage(message)) => {
            eprintln!("hnchain: {message}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
        Err(rpc::RpcFailure::Unreachable(message)) => {
            eprintln!("hnchain: {message}");
            std::process::exit(exit_code::NODE_UNREACHABLE);
        }
    };
    let succeeded = (200..300).contains(&status);

    if global.json {
        let _ = std::io::stdout().write_all(&body);
        std::process::exit(if succeeded {
            exit_code::SUCCESS
        } else {
            exit_code::NODE_REJECTED
        });
    }

    let value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("hnchain: malformed response from node: {error}");
            std::process::exit(exit_code::NODE_UNREACHABLE);
        }
    };

    if succeeded {
        print(&value);
        std::process::exit(exit_code::SUCCESS);
    }

    let code = value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("unknown_error");
    eprintln!(
        "hnchain: node rejected the request: {}",
        sanitize::sanitize_for_terminal(code)
    );
    std::process::exit(exit_code::NODE_REJECTED);
}

fn field_str(value: &Value, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_str)
        .map(sanitize::sanitize_for_terminal)
        .unwrap_or_else(|| "?".to_string())
}

fn field_num(value: &Value, name: &str) -> String {
    value
        .get(name)
        .map(|field| sanitize::sanitize_for_terminal(&field.to_string()))
        .unwrap_or_else(|| "?".to_string())
}

/// Formats a `hnit` decimal-string field (balances, stakes) in both
/// units, falling back to the raw text unchanged if it is not the
/// plain non-negative integer ADR-0046 promises.
fn field_amount(value: &Value, name: &str) -> String {
    let raw = field_str(value, name);
    match raw.parse::<u128>() {
        Ok(hnit) => amount::format_amount(hnit),
        Err(_) => raw,
    }
}

fn print_status(value: &Value) {
    println!("chain_id:          {}", field_num(value, "chain_id"));
    println!("network_id:        {}", field_num(value, "network_id"));
    println!("genesis_hash:      {}", field_str(value, "genesis_hash"));
    println!(
        "protocol_version:  {}",
        field_str(value, "protocol_version")
    );
    println!("latest_height:     {}", field_num(value, "latest_height"));
    println!(
        "latest_block_hash: {}",
        field_str(value, "latest_block_hash")
    );
    println!(
        "latest_timestamp:  {}",
        field_num(value, "latest_timestamp")
    );
    println!("syncing:           {}", field_num(value, "syncing"));
    println!("peer_count:        {}", field_num(value, "peer_count"));
    println!("mempool_size:      {}", field_num(value, "mempool_size"));
}

fn print_balance(value: &Value) {
    println!("exists:  {}", field_num(value, "exists"));
    println!("balance: {}", field_amount(value, "balance"));
    println!("nonce:   {}", field_num(value, "nonce"));
    println!("height:  {}", field_num(value, "height"));
}

fn print_transaction(value: &Value) {
    println!("tx_id:  {}", field_str(value, "tx_id"));
    println!("status: {}", field_str(value, "status"));
    if value.get("height").is_some() {
        println!("height: {}", field_num(value, "height"));
        println!("index:  {}", field_num(value, "index"));
        if let Some(receipt) = value.get("receipt") {
            println!("receipt.status: {}", field_str(receipt, "status"));
        }
    }
}

fn print_block(value: &Value) {
    let Some(header) = value.get("header") else {
        println!("(malformed block: no header)");
        return;
    };
    println!("height:                   {}", field_num(header, "height"));
    println!(
        "chain_id:                 {}",
        field_num(header, "chain_id")
    );
    println!(
        "network_id:               {}",
        field_num(header, "network_id")
    );
    println!("round:                    {}", field_num(header, "round"));
    println!("epoch:                    {}", field_num(header, "epoch"));
    println!(
        "protocol_epoch:           {}",
        field_num(header, "protocol_epoch")
    );
    println!(
        "parent_block_hash:        {}",
        field_str(header, "parent_block_hash")
    );
    println!(
        "proposer:                 {}",
        field_str(header, "proposer")
    );
    println!(
        "timestamp:                {}",
        field_num(header, "timestamp")
    );
    println!(
        "transactions_root:        {}",
        field_str(header, "transactions_root")
    );
    println!(
        "state_root:               {}",
        field_str(header, "state_root")
    );
    println!(
        "receipts_root:            {}",
        field_str(header, "receipts_root")
    );
    println!(
        "events_root:              {}",
        field_str(header, "events_root")
    );
    println!(
        "consensus_root:           {}",
        field_str(header, "consensus_root")
    );
    println!(
        "evidence_root:            {}",
        field_str(header, "evidence_root")
    );
    println!(
        "protocol_parameters_hash: {}",
        field_str(header, "protocol_parameters_hash")
    );
    println!(
        "extra_data_hash:          {}",
        field_str(header, "extra_data_hash")
    );

    let tx_count = value
        .get("tx_ids")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    println!("tx_count:                 {tx_count}");

    if let Some(commit_qc) = value.get("commit_qc") {
        println!(
            "commit_qc:                height={} round={} signer_count={}",
            field_num(commit_qc, "height"),
            field_num(commit_qc, "round"),
            field_num(commit_qc, "signer_count"),
        );
    }
}

fn print_validators(value: &Value) {
    let Some(validators) = value.get("validators").and_then(Value::as_array) else {
        println!("(malformed validators response)");
        return;
    };
    println!("{} validator(s)", validators.len());
    for validator in validators {
        println!(
            "  {}  {:<10} {}",
            field_str(validator, "address"),
            field_str(validator, "status"),
            field_amount(validator, "bonded_stake"),
        );
    }
}

fn print_mempool(value: &Value) {
    println!("size:     {}", field_num(value, "size"));
    println!("capacity: {}", field_num(value, "capacity"));
}

/// Performs `GET path`, requiring a parsed `2xx` JSON response --
/// `send`'s own status/account/transaction-status reads always need
/// real fields to keep going, unlike the six read-only commands above
/// (which also support `--json` passthrough on a rejection).
fn fetch_required_json(global: &cli::GlobalOptions, path: &str) -> Value {
    let (status, body) = match rpc::get(global, path) {
        Ok(result) => result,
        Err(rpc::RpcFailure::Usage(message)) => {
            eprintln!("hnchain: {message}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
        Err(rpc::RpcFailure::Unreachable(message)) => {
            eprintln!("hnchain: {message}");
            std::process::exit(exit_code::NODE_UNREACHABLE);
        }
    };
    let value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("hnchain: malformed response from node: {error}");
            std::process::exit(exit_code::NODE_UNREACHABLE);
        }
    };
    if !(200..300).contains(&status) {
        let code = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown_error");
        eprintln!(
            "hnchain: node rejected the request: {}",
            sanitize::sanitize_for_terminal(code)
        );
        std::process::exit(exit_code::NODE_REJECTED);
    }
    value
}

/// Extracts `/v1/status`'s own pinning-relevant fields.
fn status_fields(value: &Value) -> Option<(u8, u16, [u8; 32], u64)> {
    let chain_id = u8::try_from(value.get("chain_id")?.as_u64()?).ok()?;
    let network_id = u16::try_from(value.get("network_id")?.as_u64()?).ok()?;
    let genesis_hash = hex::decode_32(value.get("genesis_hash")?.as_str()?).ok()?;
    let latest_height = value.get("latest_height")?.as_u64()?;
    Some((chain_id, network_id, genesis_hash, latest_height))
}

/// `send`'s own build-sign-submit-wait flow (ADR-0047, "Decided:
/// Building And Signing A Transfer"): aborts on the first failure,
/// exactly as the ADR orders its eight steps.
fn run_send(global: &cli::GlobalOptions, args: SendArgs) -> ! {
    let keypair = match keyfile::load(&args.key) {
        Ok(keypair) => keypair,
        Err(error) => {
            eprintln!("hnchain: {error}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
    };
    let descriptor = keypair.key_descriptor();
    let sender = match account_address_body(
        profile::NETWORK_ID,
        descriptor.algorithm_id(),
        &descriptor.public_key_bytes(),
    ) {
        Ok(sender) => sender,
        Err(error) => {
            eprintln!("hnchain: could not derive sender address: {error}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
    };
    let recipient = match hex::decode_32(&args.to) {
        Ok(recipient) => recipient,
        Err(error) => {
            eprintln!("hnchain: invalid --to address: {error}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
    };
    let transfer_amount =
        match send::resolve_amount(args.amount.as_deref(), args.amount_hnit.as_deref()) {
            Ok(amount) => amount,
            Err(message) => {
                eprintln!("hnchain: {message}");
                std::process::exit(exit_code::USAGE_ERROR);
            }
        };
    let fee_limit = match send::resolve_fee_limit(args.fee_limit.as_deref()) {
        Ok(fee_limit) => fee_limit,
        Err(message) => {
            eprintln!("hnchain: {message}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
    };

    // Step 2: GET /v1/status, refuse to sign on a network mismatch.
    let status_value = fetch_required_json(global, "/v1/status");
    let Some((chain_id, network_id, genesis_hash, latest_height)) = status_fields(&status_value)
    else {
        eprintln!("hnchain: malformed status response from node");
        std::process::exit(exit_code::NODE_UNREACHABLE);
    };
    if let Err(mismatch) = profile::validate_status(chain_id, network_id, genesis_hash) {
        eprintln!("hnchain: refusing to sign: {mismatch}");
        std::process::exit(exit_code::USAGE_ERROR);
    }

    // Step 3/4: GET /v1/accounts/{sender}, choose the nonce.
    let account_value =
        fetch_required_json(global, &format!("/v1/accounts/{}", hex::encode(&sender)));
    let Some(committed_nonce) = account_value.get("nonce").and_then(Value::as_u64) else {
        eprintln!("hnchain: malformed account response from node");
        std::process::exit(exit_code::NODE_UNREACHABLE);
    };
    let nonce = send::resolve_nonce(args.nonce, committed_nonce);
    let max_height = send::resolve_max_height(args.no_expiry, latest_height);

    // Step 5: build and sign.
    let request = transfer::TransferRequest {
        chain_id,
        network_id,
        recipient,
        amount: transfer_amount,
        nonce,
        fee_limit,
        validity_window: ValidityWindowV1 {
            min_height: None,
            max_height,
        },
    };
    let envelope = match transfer::build_signed_transfer(&keypair, &request) {
        Ok(envelope) => envelope,
        Err(error) => {
            eprintln!("hnchain: could not build transaction: {error}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
    };
    let encoded = match envelope.encode() {
        Ok(encoded) => encoded,
        Err(error) => {
            eprintln!("hnchain: could not encode transaction: {error}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
    };

    // Step 6: summary and confirmation.
    println!("network:    chain_id={chain_id} network_id={network_id}");
    println!("sender:     {}", hex::encode(&sender));
    println!("recipient:  {}", hex::encode(&recipient));
    println!("amount:     {}", amount::format_amount(transfer_amount));
    println!(
        "fee_limit:  {} (placeholder, not a protocol value)",
        amount::format_amount(fee_limit)
    );
    println!("nonce:      {nonce}");
    match max_height {
        Some(height) => println!("expires at: height {}", height.get()),
        None => println!("expires at: never (--no-expiry)"),
    }

    let is_terminal = std::io::stdin().is_terminal();
    if send::requires_explicit_yes(is_terminal, args.yes) {
        eprintln!("hnchain: confirmation required -- pass --yes for non-interactive use");
        std::process::exit(exit_code::USAGE_ERROR);
    }
    if !args.yes {
        print!("Proceed? [y/N] ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() {
            eprintln!("hnchain: could not read confirmation");
            std::process::exit(exit_code::USAGE_ERROR);
        }
        if !send::answer_confirms(&answer) {
            eprintln!("hnchain: aborted");
            std::process::exit(exit_code::USAGE_ERROR);
        }
    }

    // Step 7: submit.
    let payload = serde_json::json!({ "tx": hex::encode(&encoded) });
    let (submit_status, submit_body) = match rpc::post(global, "/v1/transactions", &payload) {
        Ok(result) => result,
        Err(rpc::RpcFailure::Usage(message)) => {
            eprintln!("hnchain: {message}");
            std::process::exit(exit_code::USAGE_ERROR);
        }
        Err(rpc::RpcFailure::Unreachable(message)) => {
            eprintln!("hnchain: {message}");
            std::process::exit(exit_code::NODE_UNREACHABLE);
        }
    };
    let submit_value: Value = match serde_json::from_slice(&submit_body) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("hnchain: malformed submission response: {error}");
            std::process::exit(exit_code::NODE_UNREACHABLE);
        }
    };
    if !(200..300).contains(&submit_status) {
        let code = submit_value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown_error");
        eprintln!(
            "hnchain: node rejected the transaction: {}",
            sanitize::sanitize_for_terminal(code)
        );
        std::process::exit(exit_code::NODE_REJECTED);
    }
    let Some(tx_id) = submit_value.get("tx_id").and_then(Value::as_str) else {
        eprintln!("hnchain: malformed submission response from node");
        std::process::exit(exit_code::NODE_UNREACHABLE);
    };
    let tx_id = tx_id.to_string();
    println!("tx_id:  {}", sanitize::sanitize_for_terminal(&tx_id));
    println!("status: admitted to the node's mempool");

    // Step 8: optional wait.
    if !args.wait {
        std::process::exit(exit_code::SUCCESS);
    }

    let wait_timeout =
        Duration::from_secs(args.wait_timeout_secs.unwrap_or(DEFAULT_WAIT_TIMEOUT_SECS));
    let deadline = Instant::now() + wait_timeout;
    loop {
        let tx_value = fetch_required_json(global, &format!("/v1/transactions/{tx_id}"));
        if tx_value.get("status").and_then(Value::as_str) == Some("finalized") {
            println!("finalized at height {}", field_num(&tx_value, "height"));
            let receipt_status = tx_value
                .get("receipt")
                .and_then(|receipt| receipt.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            println!(
                "receipt: {}",
                sanitize::sanitize_for_terminal(receipt_status)
            );
            std::process::exit(exit_code::SUCCESS);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            eprintln!("hnchain: --wait timed out before the transaction finalized");
            std::process::exit(exit_code::WAIT_TIMEOUT);
        }
        std::thread::sleep(WAIT_POLL_INTERVAL.min(remaining));
    }
}

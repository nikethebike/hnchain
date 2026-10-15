//! `hnchain`'s process entry point (ADR-0047, "Decided: v1 Command
//! Set"). Parses argv into an [`hn_cli::cli::Invocation`] and runs
//! `key generate`/`key show` (ADR-0047, "Decided: Key Files") and the
//! six read-only `/v1` commands (status, balance, tx show, block
//! show, validators, mempool); `send`'s own build-sign-submit flow
//! (ADR-0047, "Decided: Building And Signing A Transfer") lands in a
//! later pass.

use std::io::Write;

use hn_cli::cli::{self, BalanceTarget, BlockRef, exit_code};
use hn_cli::{hex, keyfile, profile, rpc, sanitize};
use hn_crypto::{Ed25519KeyPair, account_address_body};
use serde_json::Value;

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
        cli::Command::Send(_) => {
            eprintln!("hnchain: this command is not implemented yet");
            std::process::exit(exit_code::USAGE_ERROR);
        }
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
        Ok(hnit) => hn_cli::amount::format_amount(hnit),
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

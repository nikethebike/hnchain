//! `hnchain`'s own argument parser (ADR-0047, "Decided: v1 Command
//! Set").

use std::fmt;
use std::path::PathBuf;

/// Exit codes this binary ever returns (ADR-0047, "Decided: Output,
/// Exit Codes, Errors") — named constants rather than bare literals
/// scattered through `main`, so every call site states which of the
/// five meanings it intends.
pub mod exit_code {
    /// Success.
    pub const SUCCESS: i32 = 0;
    /// A usage error or invalid input; nothing was sent.
    pub const USAGE_ERROR: i32 = 1;
    /// Could not reach or understand the node (connect, timeout, bad
    /// response).
    pub const NODE_UNREACHABLE: i32 = 2;
    /// The node rejected the request (4xx/422 with an error code).
    pub const NODE_REJECTED: i32 = 3;
    /// `--wait` timed out before the transaction finalized.
    pub const WAIT_TIMEOUT: i32 = 4;
}

/// A `--flag value` parse failure, a missing required flag, an
/// unknown subcommand, or two mutually-exclusive options both given —
/// every case this module's own `parse` rejects maps to
/// [`exit_code::USAGE_ERROR`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageError(pub String);

impl fmt::Display for UsageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::error::Error for UsageError {}

/// `--rpc`/`--json`/`--timeout` (ADR-0047, "Decided: v1 Command Set"
/// — "Global options"): recognized only as a contiguous run of flags
/// before the subcommand name, the same `tool [global flags] <command>
/// [command flags]` shape `git`/`kubectl` already use.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GlobalOptions {
    /// `--rpc <url>` — overrides the active profile's own default RPC
    /// URL. `None` means "use the profile's own default."
    pub rpc: Option<String>,
    /// `--json` — print the node's JSON response unchanged and
    /// nothing else.
    pub json: bool,
    /// `--timeout <seconds>` — overrides the active profile's own
    /// default request timeout.
    pub timeout_secs: Option<u64>,
}

/// `block show <height|latest>`'s own positional argument.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BlockRef {
    /// `latest`.
    Latest,
    /// A decimal height.
    Height(u64),
}

/// `balance`'s own target: either a raw address, or a key file to
/// derive one from — exactly one, never both, never neither
/// (ADR-0047's own command table: "`balance <address>` /
/// `--key <file>`").
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BalanceTarget {
    /// The literal `address_body` hex text given positionally.
    Address(String),
    /// `--key <file>` — the address is derived from this key file.
    Key(PathBuf),
}

/// `send`'s own flags (ADR-0047, "Decided: Building And Signing A
/// Transfer" / "Decided: Amounts..."). Amount/fee-limit fields stay
/// raw `String`s here — this module's own job is structural parsing
/// (which flags were given), not the strict decimal grammar a later
/// pass (ADR-0047's own amount parser) validates them against.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendArgs {
    /// `--key <file>` (required): the sender's own key file.
    pub key: PathBuf,
    /// `--to <address>` (required): the recipient's `address_body` hex
    /// text.
    pub to: String,
    /// `--amount <decimal HNCOIN>`, mutually exclusive with
    /// `amount_hnit`.
    pub amount: Option<String>,
    /// `--amount-hnit <integer hnit>`, mutually exclusive with
    /// `amount`.
    pub amount_hnit: Option<String>,
    /// `--fee-limit <decimal HNCOIN>`, overriding the profile's own
    /// placeholder default.
    pub fee_limit: Option<String>,
    /// `--no-expiry` — omit the validity window's own `max_height`.
    pub no_expiry: bool,
    /// `--nonce <n>` — overrides the committed nonce this command
    /// would otherwise read from the node.
    pub nonce: Option<u64>,
    /// `--yes` — skip the interactive confirmation prompt.
    pub yes: bool,
    /// `--wait` — poll `GET /v1/transactions/{tx_id}` until finalized
    /// or `--wait-timeout` elapses.
    pub wait: bool,
    /// `--wait-timeout <seconds>`, only meaningful with `--wait`.
    pub wait_timeout_secs: Option<u64>,
}

/// The nine `v1` subcommands (ADR-0047, "Decided: v1 Command Set"),
/// plus `Help` for `--help`/`-h`/`help`/no arguments at all.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    /// `--help` / `-h` / `help` / no subcommand at all.
    Help,
    /// `key generate --out <file>`.
    KeyGenerate {
        /// Where to write the new key file.
        out: PathBuf,
    },
    /// `key show --key <file>`.
    KeyShow {
        /// The key file to read.
        key: PathBuf,
    },
    /// `status`.
    Status,
    /// `balance <address>` / `balance --key <file>`.
    Balance(BalanceTarget),
    /// `send --key <file> --to <address> --amount <amount>` (and the
    /// rest of [`SendArgs`]'s own flags).
    Send(SendArgs),
    /// `tx show <tx_id>`.
    TxShow {
        /// The transaction id, as hex text.
        tx_id: String,
    },
    /// `block show <height|latest>`.
    BlockShow {
        /// Which block.
        target: BlockRef,
    },
    /// `validators`.
    Validators,
    /// `mempool`.
    Mempool,
}

/// One fully-parsed invocation: the global options every command
/// shares, plus the command itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invocation {
    /// `--rpc`/`--json`/`--timeout`.
    pub global: GlobalOptions,
    /// The subcommand and its own flags.
    pub command: Command,
}

/// Parses `args` (excluding the program name — callers pass
/// `std::env::args().skip(1)`) into an [`Invocation`] (ADR-0047,
/// "Decided: v1 Command Set" — "Argument parsing is a small
/// hand-written subcommand parser... consistent with the project's
/// minimal-dependency posture").
pub fn parse(args: impl Iterator<Item = String>) -> Result<Invocation, UsageError> {
    let mut tokens: Vec<String> = args.collect();

    let mut global = GlobalOptions::default();
    while let Some(flag) = tokens.first() {
        match flag.as_str() {
            "--rpc" => {
                tokens.remove(0);
                global.rpc = Some(take_value(&mut tokens, "--rpc")?);
            }
            "--json" => {
                tokens.remove(0);
                global.json = true;
            }
            "--timeout" => {
                tokens.remove(0);
                global.timeout_secs = Some(take_number(&mut tokens, "--timeout")?);
            }
            _ => break,
        }
    }

    let Some(head) = tokens.first().cloned() else {
        return Ok(Invocation {
            global,
            command: Command::Help,
        });
    };

    if matches!(head.as_str(), "--help" | "-h" | "help") {
        return Ok(Invocation {
            global,
            command: Command::Help,
        });
    }

    tokens.remove(0);
    let command = match head.as_str() {
        "key" => parse_key(&mut tokens)?,
        "status" => expect_no_more(&tokens, "status").map(|()| Command::Status)?,
        "balance" => parse_balance(&mut tokens)?,
        "send" => parse_send(&mut tokens)?,
        "tx" => parse_tx(&mut tokens)?,
        "block" => parse_block(&mut tokens)?,
        "validators" => expect_no_more(&tokens, "validators").map(|()| Command::Validators)?,
        "mempool" => expect_no_more(&tokens, "mempool").map(|()| Command::Mempool)?,
        other => return Err(UsageError(format!("unrecognized command: {other}"))),
    };

    Ok(Invocation { global, command })
}

fn parse_key(tokens: &mut Vec<String>) -> Result<Command, UsageError> {
    let sub = take_positional(tokens, "key generate|show")?;
    match sub.as_str() {
        "generate" => {
            let out = take_flag_value(tokens, "--out")?;
            expect_no_more(tokens, "key generate")?;
            Ok(Command::KeyGenerate {
                out: PathBuf::from(out),
            })
        }
        "show" => {
            let key = take_flag_value(tokens, "--key")?;
            expect_no_more(tokens, "key show")?;
            Ok(Command::KeyShow {
                key: PathBuf::from(key),
            })
        }
        other => Err(UsageError(format!(
            "unrecognized `key` subcommand: {other}"
        ))),
    }
}

fn parse_balance(tokens: &mut Vec<String>) -> Result<Command, UsageError> {
    let key_flag = find_flag_value(tokens, "--key")?;
    let address = if tokens.first().is_some_and(|token| !token.starts_with("--")) {
        Some(tokens.remove(0))
    } else {
        None
    };
    expect_no_more(tokens, "balance")?;

    match (address, key_flag) {
        (Some(_), Some(_)) => Err(UsageError(
            "balance takes either an address or --key, not both".to_string(),
        )),
        (None, None) => Err(UsageError(
            "balance needs either an address or --key <file>".to_string(),
        )),
        (Some(address), None) => Ok(Command::Balance(BalanceTarget::Address(address))),
        (None, Some(key)) => Ok(Command::Balance(BalanceTarget::Key(PathBuf::from(key)))),
    }
}

fn parse_send(tokens: &mut Vec<String>) -> Result<Command, UsageError> {
    let mut key = None;
    let mut to = None;
    let mut amount = None;
    let mut amount_hnit = None;
    let mut fee_limit = None;
    let mut no_expiry = false;
    let mut nonce = None;
    let mut yes = false;
    let mut wait = false;
    let mut wait_timeout_secs = None;

    while let Some(flag) = tokens.first().cloned() {
        match flag.as_str() {
            "--key" => {
                tokens.remove(0);
                key = Some(take_value(tokens, "--key")?);
            }
            "--to" => {
                tokens.remove(0);
                to = Some(take_value(tokens, "--to")?);
            }
            "--amount" => {
                tokens.remove(0);
                amount = Some(take_value(tokens, "--amount")?);
            }
            "--amount-hnit" => {
                tokens.remove(0);
                amount_hnit = Some(take_value(tokens, "--amount-hnit")?);
            }
            "--fee-limit" => {
                tokens.remove(0);
                fee_limit = Some(take_value(tokens, "--fee-limit")?);
            }
            "--no-expiry" => {
                tokens.remove(0);
                no_expiry = true;
            }
            "--nonce" => {
                tokens.remove(0);
                nonce = Some(take_number(tokens, "--nonce")?);
            }
            "--yes" => {
                tokens.remove(0);
                yes = true;
            }
            "--wait" => {
                tokens.remove(0);
                wait = true;
            }
            "--wait-timeout" => {
                tokens.remove(0);
                wait_timeout_secs = Some(take_number(tokens, "--wait-timeout")?);
            }
            other => return Err(UsageError(format!("unrecognized flag for send: {other}"))),
        }
    }

    if amount.is_some() && amount_hnit.is_some() {
        return Err(UsageError(
            "send takes either --amount or --amount-hnit, not both".to_string(),
        ));
    }

    Ok(Command::Send(SendArgs {
        key: PathBuf::from(required(key, "--key")?),
        to: required(to, "--to")?,
        amount,
        amount_hnit,
        fee_limit,
        no_expiry,
        nonce,
        yes,
        wait,
        wait_timeout_secs,
    }))
}

fn parse_tx(tokens: &mut Vec<String>) -> Result<Command, UsageError> {
    let sub = take_positional(tokens, "tx show")?;
    if sub != "show" {
        return Err(UsageError(format!("unrecognized `tx` subcommand: {sub}")));
    }
    let tx_id = take_positional(tokens, "tx show <tx_id>")?;
    expect_no_more(tokens, "tx show")?;
    Ok(Command::TxShow { tx_id })
}

fn parse_block(tokens: &mut Vec<String>) -> Result<Command, UsageError> {
    let sub = take_positional(tokens, "block show")?;
    if sub != "show" {
        return Err(UsageError(format!(
            "unrecognized `block` subcommand: {sub}"
        )));
    }
    let raw = take_positional(tokens, "block show <height|latest>")?;
    expect_no_more(tokens, "block show")?;
    let target = if raw == "latest" {
        BlockRef::Latest
    } else {
        BlockRef::Height(
            raw.parse()
                .map_err(|_| UsageError(format!("not a valid height or `latest`: {raw}")))?,
        )
    };
    Ok(Command::BlockShow { target })
}

fn required<T>(value: Option<T>, flag: &str) -> Result<T, UsageError> {
    value.ok_or_else(|| UsageError(format!("missing required flag {flag}")))
}

fn take_value(tokens: &mut Vec<String>, flag: &str) -> Result<String, UsageError> {
    if tokens.is_empty() {
        return Err(UsageError(format!("{flag} needs a value")));
    }
    Ok(tokens.remove(0))
}

fn take_number(tokens: &mut Vec<String>, flag: &str) -> Result<u64, UsageError> {
    let value = take_value(tokens, flag)?;
    value
        .parse()
        .map_err(|_| UsageError(format!("{flag} needs a non-negative integer, got: {value}")))
}

fn take_flag_value(tokens: &mut Vec<String>, flag: &str) -> Result<String, UsageError> {
    if tokens.first().map(String::as_str) != Some(flag) {
        return Err(UsageError(format!("missing required flag {flag}")));
    }
    tokens.remove(0);
    take_value(tokens, flag)
}

/// Finds `flag` anywhere in `tokens` and removes both it and its own
/// value, returning that value — unlike [`take_flag_value`], `flag`
/// need not be the very next token (`balance`'s own `--key` can
/// appear before or after nothing else, since `balance` has no other
/// flags to conflict with positionally).
fn find_flag_value(tokens: &mut Vec<String>, flag: &str) -> Result<Option<String>, UsageError> {
    let Some(index) = tokens.iter().position(|token| token == flag) else {
        return Ok(None);
    };
    tokens.remove(index);
    if index >= tokens.len() {
        return Err(UsageError(format!("{flag} needs a value")));
    }
    Ok(Some(tokens.remove(index)))
}

fn take_positional(tokens: &mut Vec<String>, usage: &str) -> Result<String, UsageError> {
    if tokens.first().is_some_and(|token| token.starts_with("--")) || tokens.is_empty() {
        return Err(UsageError(format!("usage: {usage}")));
    }
    Ok(tokens.remove(0))
}

fn expect_no_more(tokens: &[String], command: &str) -> Result<(), UsageError> {
    if tokens.is_empty() {
        Ok(())
    } else {
        Err(UsageError(format!(
            "unrecognized extra arguments for {command}: {}",
            tokens.join(" ")
        )))
    }
}

/// The full `--help` text (ADR-0047, "Decided: v1 Command Set").
#[must_use]
pub fn help_text() -> &'static str {
    "hnchain -- HNChain operator and developer CLI (ADR-0047)\n\
     \n\
     USAGE:\n\
     \x20   hnchain [--rpc <url>] [--json] [--timeout <seconds>] <command> [options]\n\
     \n\
     COMMANDS:\n\
     \x20   key generate --out <file>              create a new account key file\n\
     \x20   key show --key <file>                  print an existing key file's address and public key\n\
     \x20   status                                 GET /v1/status\n\
     \x20   balance <address> | --key <file>       account balance and nonce\n\
     \x20   send --key <file> --to <address> --amount <amount>\n\
     \x20                                           build, sign and submit a transfer\n\
     \x20   tx show <tx_id>                        GET /v1/transactions/{tx_id}\n\
     \x20   block show <height|latest>             GET /v1/blocks/...\n\
     \x20   validators                             GET /v1/validators\n\
     \x20   mempool                                GET /v1/mempool\n\
     \n\
     GLOBAL OPTIONS:\n\
     \x20   --rpc <url>        override the active profile's own default RPC URL\n\
     \x20   --json             print the node's JSON response unchanged and nothing else\n\
     \x20   --timeout <secs>   override the active profile's own default request timeout\n\
     \n\
     EXIT CODES:\n\
     \x20   0  success\n\
     \x20   1  usage error or invalid input (nothing was sent)\n\
     \x20   2  could not reach or understand the node\n\
     \x20   3  the node rejected the request\n\
     \x20   4  --wait timed out before the transaction finalized\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(tokens: &[&str]) -> impl Iterator<Item = String> {
        tokens
            .iter()
            .map(|token| token.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn no_arguments_is_help() {
        assert_eq!(
            parse(args(&[])),
            Ok(Invocation {
                global: GlobalOptions::default(),
                command: Command::Help,
            })
        );
    }

    #[test]
    fn help_flag_variants_all_parse_to_help() {
        for flag in ["--help", "-h", "help"] {
            assert_eq!(
                parse(args(&[flag])).map(|invocation| invocation.command),
                Ok(Command::Help)
            );
        }
    }

    #[test]
    fn global_options_parse_before_the_command() -> Result<(), UsageError> {
        let invocation = parse(args(&[
            "--rpc",
            "http://127.0.0.1:8080",
            "--json",
            "--timeout",
            "30",
            "status",
        ]))?;
        assert_eq!(
            invocation.global,
            GlobalOptions {
                rpc: Some("http://127.0.0.1:8080".to_string()),
                json: true,
                timeout_secs: Some(30),
            }
        );
        assert_eq!(invocation.command, Command::Status);
        Ok(())
    }

    #[test]
    fn status_takes_no_arguments() {
        assert_eq!(
            parse(args(&["status"])).map(|invocation| invocation.command),
            Ok(Command::Status)
        );
        assert!(parse(args(&["status", "extra"])).is_err());
    }

    #[test]
    fn validators_and_mempool_take_no_arguments() {
        assert_eq!(
            parse(args(&["validators"])).map(|invocation| invocation.command),
            Ok(Command::Validators)
        );
        assert_eq!(
            parse(args(&["mempool"])).map(|invocation| invocation.command),
            Ok(Command::Mempool)
        );
    }

    #[test]
    fn key_generate_parses_out_path() -> Result<(), UsageError> {
        let invocation = parse(args(&["key", "generate", "--out", "my.key"]))?;
        assert_eq!(
            invocation.command,
            Command::KeyGenerate {
                out: PathBuf::from("my.key"),
            }
        );
        Ok(())
    }

    #[test]
    fn key_generate_without_out_is_a_usage_error() {
        assert!(parse(args(&["key", "generate"])).is_err());
    }

    #[test]
    fn key_show_parses_key_path() -> Result<(), UsageError> {
        let invocation = parse(args(&["key", "show", "--key", "my.key"]))?;
        assert_eq!(
            invocation.command,
            Command::KeyShow {
                key: PathBuf::from("my.key"),
            }
        );
        Ok(())
    }

    #[test]
    fn unrecognized_key_subcommand_is_a_usage_error() {
        assert!(parse(args(&["key", "rotate"])).is_err());
    }

    #[test]
    fn balance_accepts_a_bare_address() -> Result<(), UsageError> {
        let invocation = parse(args(&["balance", "aabb"]))?;
        assert_eq!(
            invocation.command,
            Command::Balance(BalanceTarget::Address("aabb".to_string()))
        );
        Ok(())
    }

    #[test]
    fn balance_accepts_a_key_file() -> Result<(), UsageError> {
        let invocation = parse(args(&["balance", "--key", "my.key"]))?;
        assert_eq!(
            invocation.command,
            Command::Balance(BalanceTarget::Key(PathBuf::from("my.key")))
        );
        Ok(())
    }

    #[test]
    fn balance_rejects_both_address_and_key() {
        assert!(parse(args(&["balance", "aabb", "--key", "my.key"])).is_err());
    }

    #[test]
    fn balance_rejects_neither_address_nor_key() {
        assert!(parse(args(&["balance"])).is_err());
    }

    #[test]
    fn send_parses_required_and_optional_flags() -> Result<(), UsageError> {
        let invocation = parse(args(&[
            "send",
            "--key",
            "my.key",
            "--to",
            "aabb",
            "--amount",
            "1.5",
            "--yes",
            "--wait",
            "--wait-timeout",
            "60",
        ]))?;
        assert_eq!(
            invocation.command,
            Command::Send(SendArgs {
                key: PathBuf::from("my.key"),
                to: "aabb".to_string(),
                amount: Some("1.5".to_string()),
                amount_hnit: None,
                fee_limit: None,
                no_expiry: false,
                nonce: None,
                yes: true,
                wait: true,
                wait_timeout_secs: Some(60),
            })
        );
        Ok(())
    }

    #[test]
    fn send_requires_key_and_to() {
        assert!(parse(args(&["send", "--to", "aabb", "--amount", "1"])).is_err());
        assert!(parse(args(&["send", "--key", "my.key", "--amount", "1"])).is_err());
    }

    #[test]
    fn send_rejects_both_amount_and_amount_hnit() {
        assert!(
            parse(args(&[
                "send",
                "--key",
                "my.key",
                "--to",
                "aabb",
                "--amount",
                "1",
                "--amount-hnit",
                "1000000000",
            ]))
            .is_err()
        );
    }

    #[test]
    fn send_rejects_an_unrecognized_flag() {
        assert!(
            parse(args(&[
                "send",
                "--key",
                "my.key",
                "--to",
                "aabb",
                "--nonsense",
                "1"
            ]))
            .is_err()
        );
    }

    #[test]
    fn tx_show_parses_the_tx_id() -> Result<(), UsageError> {
        let invocation = parse(args(&["tx", "show", "deadbeef"]))?;
        assert_eq!(
            invocation.command,
            Command::TxShow {
                tx_id: "deadbeef".to_string(),
            }
        );
        Ok(())
    }

    #[test]
    fn block_show_parses_latest() -> Result<(), UsageError> {
        let invocation = parse(args(&["block", "show", "latest"]))?;
        assert_eq!(
            invocation.command,
            Command::BlockShow {
                target: BlockRef::Latest,
            }
        );
        Ok(())
    }

    #[test]
    fn block_show_parses_a_height() -> Result<(), UsageError> {
        let invocation = parse(args(&["block", "show", "42"]))?;
        assert_eq!(
            invocation.command,
            Command::BlockShow {
                target: BlockRef::Height(42),
            }
        );
        Ok(())
    }

    #[test]
    fn block_show_rejects_a_non_numeric_non_latest_argument() {
        assert!(parse(args(&["block", "show", "soon"])).is_err());
    }

    #[test]
    fn unrecognized_command_is_a_usage_error() {
        assert!(parse(args(&["frobnicate"])).is_err());
    }

    #[test]
    fn help_text_mentions_every_command_and_exit_code() {
        let text = help_text();
        for command in [
            "key generate",
            "key show",
            "status",
            "balance",
            "send",
            "tx show",
            "block show",
            "validators",
            "mempool",
        ] {
            assert!(text.contains(command), "help text missing: {command}");
        }
        for code in ["0", "1", "2", "3", "4"] {
            assert!(text.contains(code), "help text missing exit code {code}");
        }
    }
}

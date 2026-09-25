use std::net::SocketAddr;
use std::path::PathBuf;

use crate::hex;

/// `hn-node`'s process configuration (ADR-0038, "Decided: Node Config —
/// Extend The Existing Flag Parser, No New Format"). Hand-parsed from
/// `--flag value` pairs — no CLI-parsing dependency added, continuing
/// ADR-0037's own minimal-dependency posture.
#[derive(Clone, Debug)]
pub struct NodeConfig {
    /// Path to this network's genesis file (ADR-0038).
    pub genesis_path: PathBuf,
    /// Directory holding this process's own `redb` state file.
    pub data_dir: PathBuf,
    /// This node's own listen address.
    pub listen: SocketAddr,
    /// Every peer this node dials or expects to be dialed by.
    pub peers: Vec<SocketAddr>,
    /// This validator's consensus (vote/QC signing) key seed.
    pub consensus_key_seed: [u8; 32],
    /// This node's network/handshake key seed.
    pub network_key_seed: [u8; 32],
    /// Base round-stage timeout, in milliseconds (ADR-0037, "Decided:
    /// Timeout Duration").
    pub base_timeout_ms: u64,
}

/// A `--flag value` parse failure, or a missing required flag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "config error: {}", self.0)
    }
}

impl std::error::Error for ConfigError {}

impl NodeConfig {
    /// Parses `args` (excluding the program name — callers pass
    /// `std::env::args().skip(1)`) into a [`NodeConfig`]. A leading
    /// `--config <path>` flag is expanded first (ADR-0038): every
    /// non-blank, non-`#`-comment line in that file is split on
    /// whitespace into further `--flag value` tokens, spliced in place
    /// of the `--config` flag itself, so a config file and literal CLI
    /// flags are parsed by the exact same logic below.
    ///
    /// Defaults: `--base-timeout-ms 500`. Every other flag is required.
    pub fn parse(args: impl Iterator<Item = String>) -> Result<Self, ConfigError> {
        let tokens = expand_config_flag(args.collect())?;
        Self::parse_tokens(tokens)
    }

    fn parse_tokens(tokens: Vec<String>) -> Result<Self, ConfigError> {
        let mut genesis_path = None;
        let mut data_dir = None;
        let mut listen = None;
        let mut peers = Vec::new();
        let mut consensus_key_seed = None;
        let mut network_key_seed = None;
        let mut base_timeout_ms: u64 = 500;

        let mut tokens = tokens.into_iter();
        while let Some(flag) = tokens.next() {
            let value = tokens
                .next()
                .ok_or_else(|| ConfigError(format!("{flag} needs a value")))?;
            match flag.as_str() {
                "--genesis" => genesis_path = Some(PathBuf::from(value)),
                "--data-dir" => data_dir = Some(PathBuf::from(value)),
                "--listen" => listen = Some(parse_addr(&flag, &value)?),
                "--peer" => peers.push(parse_addr(&flag, &value)?),
                "--consensus-key-seed" => consensus_key_seed = Some(parse_seed(&flag, &value)?),
                "--network-key-seed" => network_key_seed = Some(parse_seed(&flag, &value)?),
                "--base-timeout-ms" => base_timeout_ms = parse(&flag, &value)?,
                other => return Err(ConfigError(format!("unrecognized flag {other}"))),
            }
        }

        Ok(Self {
            genesis_path: required(genesis_path, "--genesis")?,
            data_dir: required(data_dir, "--data-dir")?,
            listen: required(listen, "--listen")?,
            peers,
            consensus_key_seed: required(consensus_key_seed, "--consensus-key-seed")?,
            network_key_seed: required(network_key_seed, "--network-key-seed")?,
            base_timeout_ms,
        })
    }
}

/// Expands a leading `--config <path>` flag (if present) into the file's
/// own whitespace-separated `--flag value` tokens, spliced ahead of any
/// remaining literal args. Only recognized as the very first token —
/// this pass does not support merging a config file with CLI flags that
/// override individual fields, only "read from a file instead of the
/// command line."
fn expand_config_flag(mut args: Vec<String>) -> Result<Vec<String>, ConfigError> {
    if args.first().map(String::as_str) != Some("--config") {
        return Ok(args);
    }
    args.remove(0);
    if args.is_empty() {
        return Err(ConfigError("--config needs a value".to_string()));
    }
    let path = args.remove(0);
    let contents = std::fs::read_to_string(&path)
        .map_err(|error| ConfigError(format!("reading --config file {path}: {error}")))?;

    let mut tokens: Vec<String> = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        tokens.extend(line.split_whitespace().map(str::to_string));
    }
    tokens.extend(args);
    Ok(tokens)
}

fn parse<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, ConfigError> {
    value
        .parse()
        .map_err(|_| ConfigError(format!("{flag} has an invalid value: {value}")))
}

fn parse_addr(flag: &str, value: &str) -> Result<SocketAddr, ConfigError> {
    parse::<SocketAddr>(flag, value)
}

fn parse_seed(flag: &str, value: &str) -> Result<[u8; 32], ConfigError> {
    let bytes = hex::decode(value)
        .ok_or_else(|| ConfigError(format!("{flag} is not valid hex: {value}")))?;
    bytes
        .try_into()
        .map_err(|_| ConfigError(format!("{flag} must be exactly 32 bytes of hex")))
}

fn required<T>(value: Option<T>, flag: &str) -> Result<T, ConfigError> {
    value.ok_or_else(|| ConfigError(format!("missing required flag {flag}")))
}

#[cfg(test)]
mod tests {
    use super::NodeConfig;

    fn args(flags: &[&str]) -> impl Iterator<Item = String> {
        flags
            .iter()
            .map(|flag| flag.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    const SEED: &str = "0101010101010101010101010101010101010101010101010101010101010101";

    #[test]
    fn rejects_a_seed_that_is_not_32_bytes() {
        let result = NodeConfig::parse(args(&[
            "--genesis",
            "genesis.json",
            "--data-dir",
            "/tmp/node0",
            "--listen",
            "127.0.0.1:30000",
            "--consensus-key-seed",
            "aa",
            "--network-key-seed",
            SEED,
        ]));
        assert!(result.is_err());
    }

    #[test]
    fn parses_required_flags_with_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let seed = &SEED[..64];
        let config = NodeConfig::parse(args(&[
            "--genesis",
            "genesis.json",
            "--data-dir",
            "/tmp/node0",
            "--listen",
            "127.0.0.1:30000",
            "--consensus-key-seed",
            seed,
            "--network-key-seed",
            seed,
        ]))?;
        assert_eq!(
            config.listen,
            "127.0.0.1:30000".parse::<std::net::SocketAddr>()?
        );
        assert_eq!(config.base_timeout_ms, 500);
        assert!(config.peers.is_empty());
        Ok(())
    }

    #[test]
    fn collects_repeated_peer_flags() -> Result<(), super::ConfigError> {
        let seed = &SEED[..64];
        let config = NodeConfig::parse(args(&[
            "--genesis",
            "genesis.json",
            "--data-dir",
            "/tmp/node0",
            "--listen",
            "127.0.0.1:30000",
            "--peer",
            "127.0.0.1:30001",
            "--peer",
            "127.0.0.1:30002",
            "--consensus-key-seed",
            seed,
            "--network-key-seed",
            seed,
        ]))?;
        assert_eq!(config.peers.len(), 2);
        Ok(())
    }

    #[test]
    fn reads_flags_from_a_config_file() -> Result<(), Box<dyn std::error::Error>> {
        let seed = &SEED[..64];
        let dir = std::env::temp_dir().join(format!("hn-node-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("node.conf");
        std::fs::write(
            &path,
            format!(
                "# a comment\n--genesis genesis.json\n--data-dir /tmp/node0\n\n--listen 127.0.0.1:30000\n--consensus-key-seed {seed}\n--network-key-seed {seed}\n"
            ),
        )?;

        let config = NodeConfig::parse(
            vec!["--config".to_string(), path.to_string_lossy().to_string()].into_iter(),
        )?;
        assert_eq!(config.listen, "127.0.0.1:30000".parse()?);
        Ok(())
    }

    #[test]
    fn rejects_a_missing_required_flag() {
        let result = NodeConfig::parse(args(&["--genesis", "genesis.json"]));
        assert!(result.is_err());
    }

    #[test]
    fn rejects_an_unrecognized_flag() {
        let result = NodeConfig::parse(args(&["--nonsense", "1"]));
        assert!(result.is_err());
    }
}

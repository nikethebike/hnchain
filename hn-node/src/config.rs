use std::path::PathBuf;

/// `hn-node`'s process configuration (ADR-0037, "Decided: `hn-node`
/// Process"). Hand-parsed from `--flag value` pairs — no CLI-parsing
/// dependency added, matching this ADR's own minimal-dependency
/// posture for the transport decision.
///
/// One shared `base_port` plus each process's own `validator_index`
/// fully determines the whole devnet cluster's topology: validator `i`
/// always listens on `127.0.0.1:{base_port + i}`, so no separate
/// peer-list file or flag is needed.
#[derive(Clone, Debug)]
pub struct NodeConfig {
    /// This process's own validator index (`0..validator_count`).
    pub validator_index: u8,
    /// Total number of validators in the devnet cluster (this process's
    /// own peer list is every other index in `0..validator_count`).
    pub validator_count: u8,
    /// The shared base TCP port every validator's own listen port is
    /// derived from.
    pub base_port: u16,
    /// HNChain protocol lineage (ADR-0006).
    pub chain_id: u8,
    /// Network environment (ADR-0003).
    pub network_id: u16,
    /// Directory holding this process's own `redb` state file.
    pub data_dir: PathBuf,
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
    /// `std::env::args().skip(1)`) into a [`NodeConfig`]. Defaults:
    /// `--chain-id 1`, `--network-id 1`, `--base-timeout-ms 500`.
    pub fn parse(args: impl Iterator<Item = String>) -> Result<Self, ConfigError> {
        let mut validator_index = None;
        let mut validator_count = None;
        let mut base_port = None;
        let mut chain_id: u8 = 1;
        let mut network_id: u16 = 1;
        let mut data_dir = None;
        let mut base_timeout_ms: u64 = 500;

        let mut args = args.peekable();
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| ConfigError(format!("{flag} needs a value")))?;
            match flag.as_str() {
                "--validator-index" => validator_index = Some(parse(&flag, &value)?),
                "--validator-count" => validator_count = Some(parse(&flag, &value)?),
                "--base-port" => base_port = Some(parse(&flag, &value)?),
                "--chain-id" => chain_id = parse(&flag, &value)?,
                "--network-id" => network_id = parse(&flag, &value)?,
                "--data-dir" => data_dir = Some(PathBuf::from(value)),
                "--base-timeout-ms" => base_timeout_ms = parse(&flag, &value)?,
                other => return Err(ConfigError(format!("unrecognized flag {other}"))),
            }
        }

        Ok(Self {
            validator_index: required(validator_index, "--validator-index")?,
            validator_count: required(validator_count, "--validator-count")?,
            base_port: required(base_port, "--base-port")?,
            chain_id,
            network_id,
            data_dir: required(data_dir, "--data-dir")?,
            base_timeout_ms,
        })
    }
}

fn parse<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, ConfigError> {
    value
        .parse()
        .map_err(|_| ConfigError(format!("{flag} has an invalid value: {value}")))
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

    #[test]
    fn parses_required_flags_with_defaults() -> Result<(), super::ConfigError> {
        let config = NodeConfig::parse(args(&[
            "--validator-index",
            "1",
            "--validator-count",
            "4",
            "--base-port",
            "30000",
            "--data-dir",
            "/tmp/node1",
        ]))?;
        assert_eq!(config.validator_index, 1);
        assert_eq!(config.validator_count, 4);
        assert_eq!(config.base_port, 30000);
        assert_eq!(config.chain_id, 1);
        assert_eq!(config.network_id, 1);
        assert_eq!(config.base_timeout_ms, 500);
        Ok(())
    }

    #[test]
    fn overrides_defaults() -> Result<(), super::ConfigError> {
        let config = NodeConfig::parse(args(&[
            "--validator-index",
            "0",
            "--validator-count",
            "4",
            "--base-port",
            "30000",
            "--data-dir",
            "/tmp/node0",
            "--chain-id",
            "7",
            "--network-id",
            "42",
            "--base-timeout-ms",
            "250",
        ]))?;
        assert_eq!(config.chain_id, 7);
        assert_eq!(config.network_id, 42);
        assert_eq!(config.base_timeout_ms, 250);
        Ok(())
    }

    #[test]
    fn rejects_a_missing_required_flag() {
        let result = NodeConfig::parse(args(&["--validator-index", "0"]));
        assert!(result.is_err());
    }

    #[test]
    fn rejects_an_unrecognized_flag() {
        let result = NodeConfig::parse(args(&["--nonsense", "1"]));
        assert!(result.is_err());
    }
}

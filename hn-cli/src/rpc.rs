//! Shared RPC call plumbing for every command that talks to a node
//! (ADR-0047, "Decided: Output, Exit Codes, Errors"). Resolves
//! `--rpc`/`--timeout` against the active profile and performs the
//! request, returning the raw `(status, body)` bytes -- never parsed
//! here, so `--json` output can stay byte-for-byte identical to the
//! node's own response (ADR-0047's own conformance requirement).

use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use hn_rpc::ClientConfig;

use crate::cli::GlobalOptions;
use crate::{profile, rpc_url};

/// A request never got a status code back: either `--rpc`'s own URL
/// could not be used ([`Self::Usage`], nothing was sent -- the
/// caller's own [`crate::cli::exit_code::USAGE_ERROR`]), or the node
/// could not be reached or understood ([`Self::Unreachable`] --
/// [`crate::cli::exit_code::NODE_UNREACHABLE`]).
#[derive(Debug)]
pub enum RpcFailure {
    /// `--rpc`'s own URL is not a `http://host:port` this client can
    /// use.
    Usage(String),
    /// Connecting, writing, reading, or decoding the HTTP response
    /// failed.
    Unreachable(String),
}

impl fmt::Display for RpcFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) | Self::Unreachable(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for RpcFailure {}

const DEFAULT_TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

fn resolve(global: &GlobalOptions) -> Result<(SocketAddr, ClientConfig), RpcFailure> {
    let url = global.rpc.as_deref().unwrap_or(profile::DEFAULT_RPC_URL);
    let addr = rpc_url::parse(url).map_err(|error| RpcFailure::Usage(error.to_string()))?;

    let total_timeout = global
        .timeout_secs
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_TOTAL_TIMEOUT);
    let connect_timeout = DEFAULT_CONNECT_TIMEOUT.min(total_timeout);

    Ok((
        addr,
        ClientConfig {
            connect_timeout,
            total_timeout,
        },
    ))
}

/// Performs `GET path` against the active `--rpc` endpoint.
pub fn get(global: &GlobalOptions, path: &str) -> Result<(u16, Vec<u8>), RpcFailure> {
    let (addr, config) = resolve(global)?;
    hn_rpc::request(addr, "GET", path, None, config)
        .map_err(|error| RpcFailure::Unreachable(error.to_string()))
}

/// Performs `POST path` with a JSON body against the active `--rpc`
/// endpoint (`send`'s own `POST /v1/transactions`).
pub fn post(
    global: &GlobalOptions,
    path: &str,
    payload: &serde_json::Value,
) -> Result<(u16, Vec<u8>), RpcFailure> {
    let (addr, config) = resolve(global)?;
    let body = serde_json::to_vec(payload).map_err(|error| {
        RpcFailure::Unreachable(format!("could not encode request body: {error}"))
    })?;
    hn_rpc::request(addr, "POST", path, Some(&body), config)
        .map_err(|error| RpcFailure::Unreachable(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{RpcFailure, resolve};
    use crate::cli::GlobalOptions;

    #[test]
    fn resolve_uses_the_profiles_own_default_url_when_rpc_is_not_given()
    -> Result<(), Box<dyn std::error::Error>> {
        let (addr, _config) = resolve(&GlobalOptions::default())?;
        assert_eq!(addr.to_string(), "127.0.0.1:8080");
        Ok(())
    }

    #[test]
    fn resolve_honors_an_explicit_rpc_override() -> Result<(), Box<dyn std::error::Error>> {
        let global = GlobalOptions {
            rpc: Some("http://127.0.0.1:9090".to_string()),
            ..GlobalOptions::default()
        };
        let (addr, _config) = resolve(&global)?;
        assert_eq!(addr.to_string(), "127.0.0.1:9090");
        Ok(())
    }

    #[test]
    fn resolve_rejects_a_malformed_rpc_override() {
        let global = GlobalOptions {
            rpc: Some("not-a-url".to_string()),
            ..GlobalOptions::default()
        };
        assert!(matches!(resolve(&global), Err(RpcFailure::Usage(_))));
    }

    #[test]
    fn resolve_clamps_connect_timeout_to_a_shorter_total_timeout()
    -> Result<(), Box<dyn std::error::Error>> {
        let global = GlobalOptions {
            timeout_secs: Some(1),
            ..GlobalOptions::default()
        };
        let (_addr, config) = resolve(&global)?;
        assert_eq!(config.total_timeout.as_secs(), 1);
        assert_eq!(config.connect_timeout.as_secs(), 1);
        Ok(())
    }
}

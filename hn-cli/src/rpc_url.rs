//! Parsing `--rpc <url>`/the profile's own default into the plain
//! [`SocketAddr`] [`hn_rpc::request`] needs.

use std::fmt;
use std::net::{SocketAddr, ToSocketAddrs};

/// Why `url` could not be used as an RPC endpoint -- always a usage
/// error (nothing was ever sent), since this runs before any network
/// call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpcUrlError(pub String);

impl fmt::Display for RpcUrlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl std::error::Error for RpcUrlError {}

/// Parses `http://host:port` into a [`SocketAddr`]. The RPC client is
/// plain HTTP only (ADR-0046/ADR-0047: no TLS), so any other scheme is
/// rejected rather than silently downgraded.
pub fn parse(url: &str) -> Result<SocketAddr, RpcUrlError> {
    let authority = url.strip_prefix("http://").ok_or_else(|| {
        RpcUrlError(format!(
            "unsupported RPC URL (only http:// is supported): {url}"
        ))
    })?;
    if authority.is_empty() || authority.contains('/') {
        return Err(RpcUrlError(format!(
            "malformed RPC URL (expected http://host:port): {url}"
        )));
    }

    let mut addresses = authority
        .to_socket_addrs()
        .map_err(|error| RpcUrlError(format!("could not resolve RPC URL {url}: {error}")))?;
    addresses
        .next()
        .ok_or_else(|| RpcUrlError(format!("RPC URL resolved to no address: {url}")))
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_an_ip_and_port() -> Result<(), Box<dyn std::error::Error>> {
        let addr = parse("http://127.0.0.1:8080")?;
        assert_eq!(addr.to_string(), "127.0.0.1:8080");
        Ok(())
    }

    #[test]
    fn rejects_a_non_http_scheme() {
        assert!(parse("https://127.0.0.1:8080").is_err());
    }

    #[test]
    fn rejects_a_url_with_a_path() {
        assert!(parse("http://127.0.0.1:8080/v1/status").is_err());
    }

    #[test]
    fn rejects_a_missing_port() {
        assert!(parse("http://127.0.0.1").is_err());
    }

    #[test]
    fn rejects_an_empty_authority() {
        assert!(parse("http://").is_err());
    }
}

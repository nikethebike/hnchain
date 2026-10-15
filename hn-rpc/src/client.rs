use std::fmt;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// A minimal HTTP/1.1 client matching this crate's own server contract
/// exactly (ADR-0047, "Decided: HTTP Client Lives In `hn-rpc`"): one
/// request per connection, `Content-Length` bodies, no chunked
/// encoding, no redirects, no TLS — the same deliberately tiny subset
/// [`crate::spawn_server`] answers, so this client never needs
/// anything that server does not already support. Shared by this
/// workspace's own callers (`hn-cli`, `hn-node`'s own integration
/// tests) rather than each hand-rolling it independently.
#[derive(Clone, Copy, Debug)]
pub struct ClientConfig {
    /// The deadline for completing the TCP handshake.
    pub connect_timeout: Duration,
    /// The deadline for everything after that: writing the request and
    /// reading the complete response. Not tracked to the millisecond
    /// the way the server's own per-connection deadline is (ADR-0046) —
    /// a client is not defending itself against an adversarial peer,
    /// only against one that never answers at all — so this is applied
    /// directly as both the write and the read timeout.
    pub total_timeout: Duration,
}

/// Why a [`request`]/[`get_json`]/[`post_json`] call failed.
#[derive(Debug)]
pub enum ClientError {
    /// A real I/O failure: connecting, writing, or reading.
    Io(std::io::Error),
    /// The response bytes were not a well-formed HTTP response this
    /// client knows how to read (no header/body separator, a
    /// non-numeric or missing status code).
    MalformedResponse(String),
    /// The response body was not valid JSON ([`get_json`]/[`post_json`]
    /// only).
    Json(serde_json::Error),
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "RPC client I/O error: {error}"),
            Self::MalformedResponse(reason) => {
                write!(formatter, "malformed RPC response: {reason}")
            }
            Self::Json(error) => write!(formatter, "malformed RPC response JSON: {error}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<std::io::Error> for ClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Sends one bare HTTP/1.1 request to `addr` and returns its status
/// code and raw response body — the primitive [`get_json`]/[`post_json`]
/// build on. `method` and `path` are written exactly as given, with no
/// validation of their own (this is a client, not the server's own
/// `parse_request`); callers that only ever call `GET`/`POST` against
/// `/v1/...` paths (every caller in this workspace) never need more.
pub fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    config: ClientConfig,
) -> Result<(u16, Vec<u8>), ClientError> {
    let mut stream = TcpStream::connect_timeout(&addr, config.connect_timeout)?;
    stream.set_read_timeout(Some(config.total_timeout))?;
    stream.set_write_timeout(Some(config.total_timeout))?;

    let mut request = format!("{method} {path} HTTP/1.1\r\n");
    if let Some(body) = body {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes())?;
    if let Some(body) = body {
        stream.write_all(body)?;
    }

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let separator_at = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| ClientError::MalformedResponse("no header/body separator".to_string()))?;
    let status_line = std::str::from_utf8(&raw[..separator_at])
        .map_err(|error| ClientError::MalformedResponse(error.to_string()))?
        .lines()
        .next()
        .ok_or_else(|| ClientError::MalformedResponse("empty status line".to_string()))?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| ClientError::MalformedResponse("missing status code".to_string()))?
        .parse()
        .map_err(|_| ClientError::MalformedResponse("non-numeric status code".to_string()))?;
    let response_body = raw[separator_at + 4..].to_vec();
    Ok((status, response_body))
}

/// [`request`] with method `GET` and no body, decoding the response
/// body as JSON.
pub fn get_json(
    addr: SocketAddr,
    path: &str,
    config: ClientConfig,
) -> Result<(u16, serde_json::Value), ClientError> {
    let (status, body) = request(addr, "GET", path, None, config)?;
    Ok((status, serde_json::from_slice(&body)?))
}

/// [`request`] with method `POST`, encoding `payload` as the JSON
/// request body, decoding the response body as JSON.
pub fn post_json(
    addr: SocketAddr,
    path: &str,
    payload: &serde_json::Value,
    config: ClientConfig,
) -> Result<(u16, serde_json::Value), ClientError> {
    let body = serde_json::to_vec(payload)?;
    let (status, response_body) = request(addr, "POST", path, Some(&body), config)?;
    Ok((status, serde_json::from_slice(&response_body)?))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    use super::{ClientConfig, get_json, post_json, request};

    fn fast_config() -> ClientConfig {
        ClientConfig {
            connect_timeout: Duration::from_secs(2),
            total_timeout: Duration::from_secs(2),
        }
    }

    /// Reads from `stream` until a read times out or reports EOF,
    /// accumulating everything received — the request this client
    /// sends arrives as two separate `write_all` calls (headers, then
    /// body), which loopback TCP is free to deliver as two separate
    /// reads here, so a *single* bounded read can genuinely capture
    /// only the first part. Looping until nothing more arrives for a
    /// while is also what makes it safe to respond afterward: nothing
    /// is left unread in the kernel's own receive buffer by then, so
    /// closing the connection gets a clean end rather than a real
    /// `RST` a client can observe as `ConnectionReset` (the exact
    /// `dishonest_sync_peer.rs` TCP-close race from the ADR-0044
    /// session, reproduced again here for the same underlying reason:
    /// this client never half-closes its own write side after sending
    /// a request, so a server that waited for EOF *instead* of a
    /// timeout would deadlock against it).
    fn read_whatever_arrives(stream: &mut TcpStream, timeout: Duration) -> Vec<u8> {
        let _ = stream.set_read_timeout(Some(timeout));
        let mut received = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read_len) => received.extend_from_slice(&chunk[..read_len]),
            }
        }
        received
    }

    /// A tiny one-shot server: accepts exactly one connection, drains
    /// the real request via [`read_whatever_arrives`], and writes
    /// back a fixed, already-complete HTTP response.
    fn serve_once(response: &'static [u8]) -> std::io::Result<std::net::SocketAddr> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = read_whatever_arrives(&mut stream, Duration::from_millis(200));
                let _ = stream.write_all(response);
            }
        });
        Ok(addr)
    }

    #[test]
    fn request_reads_status_and_body() -> Result<(), Box<dyn std::error::Error>> {
        let addr = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        )?;
        let (status, body) = request(addr, "GET", "/v1/status", None, fast_config())?;
        assert_eq!(status, 200);
        assert_eq!(body, b"ok");
        Ok(())
    }

    #[test]
    fn get_json_decodes_the_body() -> Result<(), Box<dyn std::error::Error>> {
        let addr = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"ok\":true}\r\n",
        )?;
        let (status, value) = get_json(addr, "/v1/status", fast_config())?;
        assert_eq!(status, 200);
        assert_eq!(value["ok"], true);
        Ok(())
    }

    #[test]
    fn post_json_sends_the_encoded_body() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let handle = thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let (mut stream, _) = listener.accept()?;
            let received = read_whatever_arrives(&mut stream, Duration::from_millis(500));
            stream.write_all(
                b"HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
            )?;
            Ok(received)
        });

        let (status, value) = post_json(
            addr,
            "/v1/transactions",
            &serde_json::json!({ "tx": "deadbeef" }),
            fast_config(),
        )?;
        assert_eq!(status, 202);
        assert_eq!(value, serde_json::json!({}));

        let sent = handle.join().map_err(|_| "server thread panicked")??;
        let sent_text = String::from_utf8(sent)?;
        assert!(sent_text.starts_with("POST /v1/transactions HTTP/1.1\r\n"));
        assert!(sent_text.ends_with("{\"tx\":\"deadbeef\"}"));
        Ok(())
    }

    #[test]
    fn request_rejects_an_unreachable_address() -> Result<(), Box<dyn std::error::Error>> {
        let unreachable = "127.0.0.1:1".parse()?;
        let result = request(unreachable, "GET", "/v1/status", None, fast_config());
        assert!(result.is_err());
        Ok(())
    }
}

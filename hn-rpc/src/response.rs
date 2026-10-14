use std::io::{self, Write};

use crate::parser::HttpError;

/// One HTTP response this server writes (ADR-0046's own "Decided: HTTP
/// Server..." — "Responses always set `Content-Length`, `Content-Type`
/// and `Connection: close`, and expose no server name or version").
/// The real `/v1` endpoint bodies and unified JSON error format
/// (ADR-0046's own routing/encoding step) are not yet built; this type
/// is the minimal shape both that step and
/// [`crate::server::spawn_server`]'s own [`HttpError`] fallback
/// (`from_http_error`) already need: a status, a content type, and
/// already-encoded body bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The `Content-Type` header's own value.
    pub content_type: &'static str,
    /// The already-encoded response body.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// The response for a request [`crate::parse_request`] rejected —
    /// the same unified `{"error": "..."}` JSON body
    /// [`crate::json::error_response`] gives every other rejection on
    /// this surface, never anything resembling a stack trace or
    /// internal path (ADR-0046, "Security Considerations").
    #[must_use]
    pub fn from_http_error(error: HttpError) -> Self {
        crate::json::error_response(error.status_code(), error.code())
    }

    /// Writes this response to `writer` as a complete HTTP/1.1
    /// message: status line, `Content-Type`, `Content-Length`,
    /// `Connection: close` (ADR-0046's own "One request per
    /// connection"; no keep-alive, no pipelining), a blank line, then
    /// the body — nothing else, no server name or version header.
    pub fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        write!(
            writer,
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.status,
            reason_phrase(self.status),
            self.content_type,
            self.body.len()
        )?;
        writer.write_all(&self.body)?;
        writer.flush()
    }
}

/// The reason phrase for every status this crate itself ever emits —
/// not a general HTTP status registry; nothing depends on this
/// signaling as the only name a given code could have, and clients
/// are expected to key off the numeric status, not this text.
const fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        417 => "Expectation Failed",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

#[cfg(test)]
mod tests {
    use super::HttpResponse;
    use crate::parser::HttpError;

    #[test]
    fn writes_a_well_formed_message() -> Result<(), Box<dyn std::error::Error>> {
        let response = HttpResponse {
            status: 200,
            content_type: "application/json",
            body: b"{}".to_vec(),
        };
        let mut buffer = Vec::new();
        response.write_to(&mut buffer)?;
        assert_eq!(
            buffer,
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"
        );
        Ok(())
    }

    #[test]
    fn from_http_error_uses_the_errors_own_status_code() {
        let response = HttpResponse::from_http_error(HttpError::MethodNotAllowed);
        assert_eq!(response.status, 405);
        assert!(!response.body.is_empty());
    }
}

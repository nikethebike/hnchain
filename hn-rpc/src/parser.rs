/// Maximum length of the request-target token (ADR-0046, "Decided:
/// HTTP Server Is Hand-Written On `std::net`..." — "at most 2 KiB").
pub const MAX_REQUEST_TARGET_LEN: usize = 2048;

/// Maximum length of the whole header section — every header line,
/// including the request line, up to but not including the final
/// blank line (ADR-0046's own "Header section at most 8 KiB and 32
/// headers").
pub const MAX_HEADER_SECTION_LEN: usize = 8192;

/// Maximum number of header lines, not counting the request line
/// itself (ADR-0046's own "32 headers").
pub const MAX_HEADER_COUNT: usize = 32;

/// The two methods this server answers at all (ADR-0046, "Decided:
/// HTTP Server Is Hand-Written On `std::net`..." — "`GET` and `POST`
/// only; anything else is `405`"). There is deliberately no catch-all
/// variant: an unrecognized method token is [`HttpError::MethodNotAllowed`]
/// at parse time, not a third value of this type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpMethod {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
}

/// One successfully-parsed request (ADR-0046's own `ParsedRequest`,
/// "Decided: HTTP Server..." — "Parsing is a pure function
/// `parse_request(&[u8]) -> Result<ParsedRequest, HttpError>`").
/// Every field borrows directly from the buffer [`parse_request`] was
/// given — no copying, no socket access, nothing beyond what that one
/// buffer already contains.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedRequest<'a> {
    /// The request method.
    pub method: HttpMethod,
    /// The request target, already validated as origin-form
    /// (ADR-0046's own "origin-form only"), with no `%`/`?` and within
    /// [`MAX_REQUEST_TARGET_LEN`] — still just text, not yet matched
    /// against the route table (that is the routing layer's own job,
    /// not this parser's).
    pub target: &'a str,
    /// Every header, in request order, as `(name, value)` with
    /// `value`'s leading/trailing optional whitespace already
    /// stripped. Names are case-sensitive here; HTTP header names are
    /// case-insensitive, so callers compare with
    /// `str::eq_ignore_ascii_case`, the same convention this module's
    /// own `Content-Length`/`Transfer-Encoding`/`Expect` checks use
    /// internally.
    pub headers: Vec<(&'a str, &'a str)>,
    /// The request body — empty for `GET` (ADR-0046's own "A `GET`
    /// with a body is `400`"); for `POST`, exactly `Content-Length`
    /// bytes, already validated against `max_body_len`.
    pub body: &'a [u8],
}

/// Why [`parse_request`] rejected a buffer — one variant per rule
/// ADR-0046's own "Decided: HTTP Server Is Hand-Written On
/// `std::net`..." names, so a caller (today, this module's own test
/// corpus; later, the socket layer building an HTTP response) can
/// match on exactly which rule fired rather than a bare string.
/// [`HttpError::status_code`] is the numeric HTTP status that ADR
/// names for each rule; almost everything is `400` by that ADR's own
/// minimalism ("a strict subset chosen to remove whole bug classes
/// rather than to be general"), not a distinct code per rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpError {
    /// The buffer ends before a complete request (header section not
    /// yet terminated by a blank line, or a `POST` body shorter than
    /// its own declared `Content-Length`) could be read from it.
    TruncatedRequest,
    /// A bare `\r` not immediately followed by `\n` in the header
    /// section.
    BareCarriageReturn,
    /// A bare `\n` not immediately preceded by `\r` in the header
    /// section.
    BareLineFeed,
    /// A NUL byte in the header section.
    NulByte,
    /// A control character (other than the `\r\n` line terminator or
    /// an in-value `\t`) in the header section.
    ControlCharacter,
    /// The header section (request line through the last header line,
    /// not counting the final blank line) exceeds
    /// [`MAX_HEADER_SECTION_LEN`].
    HeaderSectionTooLarge,
    /// The request line is not exactly three space-separated tokens
    /// (`METHOD SP target SP HTTP-version`).
    MalformedRequestLine,
    /// The method token is not exactly `GET` or `POST`.
    MethodNotAllowed,
    /// The HTTP-version token is not exactly `HTTP/1.1`.
    UnsupportedHttpVersion,
    /// The target does not start with `/` — absolute-form (a full
    /// URL) or authority-form (`host:port`), neither of which this
    /// server accepts.
    TargetNotOriginForm,
    /// The target contains a `%` (no percent-decoding is ever needed:
    /// every id this surface takes is already hex) or a `?` (no query
    /// parameters exist in v1).
    TargetHasPercentOrQuery,
    /// The target exceeds [`MAX_REQUEST_TARGET_LEN`].
    TargetTooLong,
    /// The target contains a byte outside the printable-ASCII range a
    /// request target may use.
    TargetHasInvalidBytes,
    /// More than [`MAX_HEADER_COUNT`] header lines.
    TooManyHeaders,
    /// A header line starts with whitespace — obsolete line folding
    /// (`obs-fold`), a historical continuation-line mechanism this
    /// server does not support.
    ObsoleteLineFolding,
    /// A header line contains no `:` separating name from value.
    MalformedHeaderLine,
    /// A header name contains a byte that is not a valid HTTP token
    /// character.
    InvalidHeaderName,
    /// A header value contains a byte outside the printable-ASCII
    /// (plus tab) range a header value may use.
    InvalidHeaderValue,
    /// A `Transfer-Encoding` header is present — chunked transfer is
    /// not supported at all.
    ChunkedTransferEncodingNotSupported,
    /// An `Expect` header is present.
    ExpectNotSupported,
    /// A `POST` with no `Content-Length` header.
    MissingContentLength,
    /// More than one `Content-Length` header — never trusted even
    /// when the values happen to agree (ADR-0046's own "Duplicate or
    /// conflicting `Content-Length` is `400`").
    DuplicateContentLength,
    /// A `Content-Length` value that is not composed entirely of
    /// decimal digits (no sign, no whitespace), or that overflows
    /// `usize`.
    InvalidContentLength,
    /// A declared `Content-Length` exceeding the caller-supplied
    /// `max_body_len`.
    BodyTooLarge,
    /// A `GET` with a `Content-Length` header, or with any bytes
    /// present after the header section at all.
    GetRequestHasBody,
    /// More bytes follow a `POST` body than its own `Content-Length`
    /// declared — nothing valid can follow one request on a
    /// connection this server never pipelines (ADR-0046's own "One
    /// request per connection").
    TrailingBytesAfterBody,
}

impl HttpError {
    /// The HTTP status this rejection maps to (ADR-0046's own named
    /// numeric code for each rule).
    #[must_use]
    pub const fn status_code(self) -> u16 {
        match self {
            Self::MethodNotAllowed => 405,
            Self::ChunkedTransferEncodingNotSupported => 501,
            Self::ExpectNotSupported => 417,
            _ => 400,
        }
    }
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?} ({})", self.status_code())
    }
}

impl std::error::Error for HttpError {}

/// Parses one HTTP request from `bytes` (ADR-0046's own
/// `parse_request`) — a pure function over an already-in-memory
/// buffer, no socket access at all: the socket layer (ADR-0046's own
/// "a thin separate layer" — accept, a bounded read loop, deadlines,
/// write) decides when it has read enough to call this, and this
/// function alone decides whether what it was given is a well-formed
/// request. `max_body_len` is the caller's own resolved body-size
/// limit (ADR-0046's own `2 * MAX_TRANSACTION_SIZE + 1 KiB`, a
/// `hn_state` constant this crate does not depend on and so cannot
/// hardcode — the same "caller already resolved it" boundary this
/// codebase already draws elsewhere, for example
/// `hn_node::Mempool::select_for_block_with_limits`'s own `max_bytes`
/// parameter).
pub fn parse_request(bytes: &[u8], max_body_len: usize) -> Result<ParsedRequest<'_>, HttpError> {
    let (lines, body_start) = split_header_lines(bytes)?;
    let Some((&request_line, header_lines)) = lines.split_first() else {
        return Err(HttpError::TruncatedRequest);
    };
    if header_lines.len() > MAX_HEADER_COUNT {
        return Err(HttpError::TooManyHeaders);
    }

    let (method, target) = parse_request_line(request_line)?;
    validate_target(target)?;

    let mut headers: Vec<(&str, &str)> = Vec::with_capacity(header_lines.len());
    for line in header_lines {
        headers.push(parse_header_line(line)?);
    }

    if headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
    {
        return Err(HttpError::ChunkedTransferEncodingNotSupported);
    }
    if headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("expect"))
    {
        return Err(HttpError::ExpectNotSupported);
    }

    let content_length = resolve_content_length(&headers, method)?;
    let body_candidate = &bytes[body_start..];
    let body = match (method, content_length) {
        (HttpMethod::Get, _) => {
            if !body_candidate.is_empty() {
                return Err(HttpError::GetRequestHasBody);
            }
            &bytes[body_start..body_start]
        }
        (HttpMethod::Post, Some(declared_len)) => {
            if declared_len > max_body_len {
                return Err(HttpError::BodyTooLarge);
            }
            if body_candidate.len() < declared_len {
                return Err(HttpError::TruncatedRequest);
            }
            if body_candidate.len() > declared_len {
                return Err(HttpError::TrailingBytesAfterBody);
            }
            &body_candidate[..declared_len]
        }
        (HttpMethod::Post, None) => return Err(HttpError::MissingContentLength),
    };

    Ok(ParsedRequest {
        method,
        target,
        headers,
        body,
    })
}

/// Splits `bytes` into CRLF-terminated lines up to (not including)
/// the blank line that ends the header section, returning those lines
/// together with the byte offset immediately after that blank line
/// (where the body, if any, begins). Rejects any bare `\r`/`\n`, NUL
/// byte, or other control character encountered along the way, and
/// bails out early once the header section itself would already
/// exceed [`MAX_HEADER_SECTION_LEN`] regardless of what the rest of
/// the buffer holds — an arbitrarily large adversarial buffer never
/// makes this function do more than `MAX_HEADER_SECTION_LEN` work.
fn split_header_lines(bytes: &[u8]) -> Result<(Vec<&[u8]>, usize), HttpError> {
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        if index > MAX_HEADER_SECTION_LEN {
            return Err(HttpError::HeaderSectionTooLarge);
        }
        match bytes[index] {
            b'\r' => {
                let Some(&next) = bytes.get(index + 1) else {
                    return Err(HttpError::TruncatedRequest);
                };
                if next != b'\n' {
                    return Err(HttpError::BareCarriageReturn);
                }
                let line = &bytes[line_start..index];
                index += 2;
                if line.is_empty() {
                    return Ok((lines, index));
                }
                lines.push(line);
                line_start = index;
            }
            b'\n' => return Err(HttpError::BareLineFeed),
            0x00 => return Err(HttpError::NulByte),
            b'\t' => index += 1,
            byte if byte < 0x20 || byte == 0x7F => return Err(HttpError::ControlCharacter),
            _ => index += 1,
        }
    }
    Err(HttpError::TruncatedRequest)
}

/// Parses the request line (the first line [`split_header_lines`]
/// returns) into its method and target tokens — exactly three
/// space-separated tokens, no more, no fewer.
fn parse_request_line(line: &[u8]) -> Result<(HttpMethod, &str), HttpError> {
    let mut tokens = line.split(|&byte| byte == b' ');
    let (Some(method_bytes), Some(target_bytes), Some(version_bytes), None) =
        (tokens.next(), tokens.next(), tokens.next(), tokens.next())
    else {
        return Err(HttpError::MalformedRequestLine);
    };

    let method = match method_bytes {
        b"GET" => HttpMethod::Get,
        b"POST" => HttpMethod::Post,
        _ => return Err(HttpError::MethodNotAllowed),
    };
    if version_bytes != b"HTTP/1.1" {
        return Err(HttpError::UnsupportedHttpVersion);
    }
    let target = std::str::from_utf8(target_bytes).map_err(|_| HttpError::TargetHasInvalidBytes)?;
    Ok((method, target))
}

/// Validates a request target already known to be ASCII and free of
/// control characters (every header-section byte already passed
/// [`split_header_lines`]'s own scan) — origin-form, within
/// [`MAX_REQUEST_TARGET_LEN`], and with no `%`/`?`.
fn validate_target(target: &str) -> Result<(), HttpError> {
    if target.len() > MAX_REQUEST_TARGET_LEN {
        return Err(HttpError::TargetTooLong);
    }
    if !target.starts_with('/') {
        return Err(HttpError::TargetNotOriginForm);
    }
    if target.contains('%') || target.contains('?') {
        return Err(HttpError::TargetHasPercentOrQuery);
    }
    if !target.bytes().all(is_printable_ascii) {
        return Err(HttpError::TargetHasInvalidBytes);
    }
    Ok(())
}

/// One header line into its `(name, value)` pair — rejects obsolete
/// line folding, a missing `:` separator, an invalid token character
/// in the name, or an invalid byte in the value.
fn parse_header_line(line: &[u8]) -> Result<(&str, &str), HttpError> {
    if line
        .first()
        .is_some_and(|&byte| byte == b' ' || byte == b'\t')
    {
        return Err(HttpError::ObsoleteLineFolding);
    }
    let Some(colon_index) = line.iter().position(|&byte| byte == b':') else {
        return Err(HttpError::MalformedHeaderLine);
    };
    let (name_bytes, rest) = line.split_at(colon_index);
    let value_bytes = &rest[1..];

    if name_bytes.is_empty() || !name_bytes.iter().copied().all(is_token_char) {
        return Err(HttpError::InvalidHeaderName);
    }
    if !value_bytes.iter().copied().all(is_header_value_byte) {
        return Err(HttpError::InvalidHeaderValue);
    }

    let name = std::str::from_utf8(name_bytes).map_err(|_| HttpError::InvalidHeaderName)?;
    let value = std::str::from_utf8(value_bytes)
        .map_err(|_| HttpError::InvalidHeaderValue)?
        .trim_matches(|character| character == ' ' || character == '\t');
    Ok((name, value))
}

/// Resolves this request's own `Content-Length`, enforcing
/// ADR-0046's own rules: required and unique for `POST`
/// (`Ok(Some(_))` or an error — never silently `None`), absent for
/// `GET` (checked here only for the *header's* presence; a `GET`
/// with an actual body but no such header is rejected by
/// [`parse_request`]'s own later body-length check instead).
fn resolve_content_length(
    headers: &[(&str, &str)],
    method: HttpMethod,
) -> Result<Option<usize>, HttpError> {
    let mut values = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| *value);
    let Some(first) = values.next() else {
        return match method {
            HttpMethod::Get => Ok(None),
            HttpMethod::Post => Err(HttpError::MissingContentLength),
        };
    };
    if values.next().is_some() {
        return Err(HttpError::DuplicateContentLength);
    }
    if method == HttpMethod::Get {
        return Err(HttpError::GetRequestHasBody);
    }
    if first.is_empty() || !first.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(HttpError::InvalidContentLength);
    }
    let length = first
        .parse::<usize>()
        .map_err(|_| HttpError::InvalidContentLength)?;
    Ok(Some(length))
}

const fn is_printable_ascii(byte: u8) -> bool {
    byte >= 0x20 && byte < 0x7F
}

const fn is_header_value_byte(byte: u8) -> bool {
    is_printable_ascii(byte) || byte == b'\t'
}

const fn is_token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

#[cfg(test)]
mod tests {
    use super::{HttpError, HttpMethod, parse_request};

    const MAX_BODY: usize = 1024;

    fn parse(input: &str) -> Result<super::ParsedRequest<'_>, HttpError> {
        parse_request(input.as_bytes(), MAX_BODY)
    }

    #[test]
    fn accepts_a_minimal_get() -> Result<(), Box<dyn std::error::Error>> {
        let result = parse("GET /v1/status HTTP/1.1\r\n\r\n")?;
        assert_eq!(result.method, HttpMethod::Get);
        assert_eq!(result.target, "/v1/status");
        assert!(result.headers.is_empty());
        assert!(result.body.is_empty());
        Ok(())
    }

    #[test]
    fn accepts_a_get_with_headers() -> Result<(), Box<dyn std::error::Error>> {
        let result = parse(
            "GET /v1/status HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\n\r\n",
        )?;
        assert_eq!(
            result.headers,
            vec![("Host", "localhost"), ("Accept", "application/json")]
        );
        Ok(())
    }

    #[test]
    fn accepts_a_post_with_a_matching_content_length() -> Result<(), Box<dyn std::error::Error>> {
        let result = parse("POST /v1/transactions HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello")?;
        assert_eq!(result.method, HttpMethod::Post);
        assert_eq!(result.body, b"hello");
        Ok(())
    }

    #[test]
    fn content_length_header_name_is_case_insensitive() -> Result<(), Box<dyn std::error::Error>> {
        let result = parse("POST /v1/transactions HTTP/1.1\r\ncontent-length: 2\r\n\r\nhi")?;
        assert_eq!(result.body, b"hi");
        Ok(())
    }

    #[test]
    fn header_value_whitespace_is_trimmed() -> Result<(), Box<dyn std::error::Error>> {
        let result = parse("GET /v1/status HTTP/1.1\r\nHost:  localhost  \r\n\r\n")?;
        assert_eq!(result.headers, vec![("Host", "localhost")]);
        Ok(())
    }

    // -- The table-driven bad-input corpus (ADR-0046's own "Parser
    // (mandatory, because it is hand-written)" conformance list).
    // Each case asserts the exact rejection, not merely "is an Err".

    #[test]
    fn rejects_a_method_other_than_get_or_post() {
        assert_eq!(
            parse("DELETE /v1/status HTTP/1.1\r\n\r\n"),
            Err(HttpError::MethodNotAllowed)
        );
        assert_eq!(
            parse("PUT /v1/status HTTP/1.1\r\n\r\n"),
            Err(HttpError::MethodNotAllowed)
        );
        assert_eq!(
            parse("get /v1/status HTTP/1.1\r\n\r\n"),
            Err(HttpError::MethodNotAllowed)
        );
    }

    #[test]
    fn rejects_an_unsupported_http_version() {
        assert_eq!(
            parse("GET /v1/status HTTP/1.0\r\n\r\n"),
            Err(HttpError::UnsupportedHttpVersion)
        );
        assert_eq!(
            parse("GET /v1/status HTTP/2\r\n\r\n"),
            Err(HttpError::UnsupportedHttpVersion)
        );
    }

    #[test]
    fn rejects_request_line_variants_with_the_wrong_token_count() {
        assert_eq!(
            parse("GET  /v1/status HTTP/1.1\r\n\r\n"),
            Err(HttpError::MalformedRequestLine)
        );
        assert_eq!(
            parse("GET HTTP/1.1\r\n\r\n"),
            Err(HttpError::MalformedRequestLine)
        );
        assert_eq!(
            parse("GET /v1/status HTTP/1.1 extra\r\n\r\n"),
            Err(HttpError::MalformedRequestLine)
        );
    }

    #[test]
    fn rejects_absolute_form_and_authority_form_targets() {
        assert_eq!(
            parse("GET http://example.com/v1/status HTTP/1.1\r\n\r\n"),
            Err(HttpError::TargetNotOriginForm)
        );
        assert_eq!(
            parse("GET example.com:443 HTTP/1.1\r\n\r\n"),
            Err(HttpError::TargetNotOriginForm)
        );
    }

    #[test]
    fn rejects_a_percent_or_a_question_mark_in_the_target() {
        assert_eq!(
            parse("GET /v1/status%20 HTTP/1.1\r\n\r\n"),
            Err(HttpError::TargetHasPercentOrQuery)
        );
        assert_eq!(
            parse("GET /v1/status?x=1 HTTP/1.1\r\n\r\n"),
            Err(HttpError::TargetHasPercentOrQuery)
        );
    }

    #[test]
    fn rejects_an_oversized_target() {
        let oversized_target = format!("/{}", "a".repeat(super::MAX_REQUEST_TARGET_LEN));
        let request = format!("GET {oversized_target} HTTP/1.1\r\n\r\n");
        assert_eq!(parse(&request), Err(HttpError::TargetTooLong));
    }

    #[test]
    fn rejects_a_bare_line_feed() {
        assert_eq!(
            parse("GET /v1/status HTTP/1.1\r\nHost: localhost\n\r\n"),
            Err(HttpError::BareLineFeed)
        );
    }

    #[test]
    fn rejects_a_bare_carriage_return() {
        assert_eq!(
            parse("GET /v1/status HTTP/1.1\r\nHost: localhost\r\r\n"),
            Err(HttpError::BareCarriageReturn)
        );
    }

    #[test]
    fn rejects_a_nul_byte_in_the_header_section() {
        let request = b"GET /v1/status HTTP/1.1\r\nHost: local\x00host\r\n\r\n";
        assert_eq!(parse_request(request, MAX_BODY), Err(HttpError::NulByte));
    }

    #[test]
    fn rejects_a_control_character_in_the_header_section() {
        let request = b"GET /v1/status HTTP/1.1\r\nHost: local\x01host\r\n\r\n";
        assert_eq!(
            parse_request(request, MAX_BODY),
            Err(HttpError::ControlCharacter)
        );
    }

    #[test]
    fn rejects_obsolete_line_folding() {
        assert_eq!(
            parse("GET /v1/status HTTP/1.1\r\nHost: localhost\r\n continuation\r\n\r\n"),
            Err(HttpError::ObsoleteLineFolding)
        );
    }

    #[test]
    fn rejects_a_header_line_with_no_colon() {
        assert_eq!(
            parse("GET /v1/status HTTP/1.1\r\nHostlocalhost\r\n\r\n"),
            Err(HttpError::MalformedHeaderLine)
        );
    }

    #[test]
    fn rejects_an_oversized_header_section() {
        let mut request = String::from("GET /v1/status HTTP/1.1\r\n");
        while request.len() <= super::MAX_HEADER_SECTION_LEN {
            request.push_str("X-Pad: 0123456789012345678901234567890123456789\r\n");
        }
        request.push_str("\r\n");
        assert_eq!(parse(&request), Err(HttpError::HeaderSectionTooLarge));
    }

    #[test]
    fn rejects_more_than_the_maximum_header_count() {
        let mut request = String::from("GET /v1/status HTTP/1.1\r\n");
        for index in 0..=super::MAX_HEADER_COUNT {
            request.push_str(&format!("X-{index}: v\r\n"));
        }
        request.push_str("\r\n");
        assert_eq!(parse(&request), Err(HttpError::TooManyHeaders));
    }

    #[test]
    fn rejects_duplicate_content_length() {
        assert_eq!(
            parse(
                "POST /v1/transactions HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\nhi"
            ),
            Err(HttpError::DuplicateContentLength)
        );
    }

    #[test]
    fn rejects_conflicting_content_length() {
        assert_eq!(
            parse(
                "POST /v1/transactions HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 5\r\n\r\nhello"
            ),
            Err(HttpError::DuplicateContentLength)
        );
    }

    #[test]
    fn rejects_a_non_decimal_content_length() {
        assert_eq!(
            parse("POST /v1/transactions HTTP/1.1\r\nContent-Length: -5\r\n\r\nhello"),
            Err(HttpError::InvalidContentLength)
        );
        assert_eq!(
            parse("POST /v1/transactions HTTP/1.1\r\nContent-Length: +5\r\n\r\nhello"),
            Err(HttpError::InvalidContentLength)
        );
        assert_eq!(
            parse("POST /v1/transactions HTTP/1.1\r\nContent-Length: 1 2\r\n\r\nhello"),
            Err(HttpError::InvalidContentLength)
        );
    }

    #[test]
    fn rejects_a_post_with_no_content_length() {
        assert_eq!(
            parse("POST /v1/transactions HTTP/1.1\r\n\r\nhello"),
            Err(HttpError::MissingContentLength)
        );
    }

    #[test]
    fn rejects_content_length_plus_transfer_encoding() {
        assert_eq!(
            parse(
                "POST /v1/transactions HTTP/1.1\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\nhello"
            ),
            Err(HttpError::ChunkedTransferEncodingNotSupported)
        );
    }

    #[test]
    fn rejects_any_transfer_encoding() {
        assert_eq!(
            parse("GET /v1/status HTTP/1.1\r\nTransfer-Encoding: identity\r\n\r\n"),
            Err(HttpError::ChunkedTransferEncodingNotSupported)
        );
    }

    #[test]
    fn rejects_expect() {
        assert_eq!(
            parse(
                "POST /v1/transactions HTTP/1.1\r\nContent-Length: 5\r\nExpect: 100-continue\r\n\r\nhello"
            ),
            Err(HttpError::ExpectNotSupported)
        );
    }

    #[test]
    fn rejects_a_get_with_a_body() {
        assert_eq!(
            parse("GET /v1/status HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello"),
            Err(HttpError::GetRequestHasBody)
        );
        assert_eq!(
            parse("GET /v1/status HTTP/1.1\r\n\r\nhello"),
            Err(HttpError::GetRequestHasBody)
        );
    }

    #[test]
    fn rejects_a_body_over_the_caller_supplied_limit() {
        let body = "x".repeat(MAX_BODY + 1);
        let request = format!(
            "POST /v1/transactions HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(parse(&request), Err(HttpError::BodyTooLarge));
    }

    #[test]
    fn rejects_a_body_shorter_than_its_own_content_length() {
        assert_eq!(
            parse("POST /v1/transactions HTTP/1.1\r\nContent-Length: 10\r\n\r\nhi"),
            Err(HttpError::TruncatedRequest)
        );
    }

    #[test]
    fn rejects_trailing_bytes_after_the_declared_body() {
        assert_eq!(
            parse("POST /v1/transactions HTTP/1.1\r\nContent-Length: 2\r\n\r\nhello"),
            Err(HttpError::TrailingBytesAfterBody)
        );
    }

    #[test]
    fn rejects_truncated_requests() {
        assert_eq!(
            parse_request(b"GET /v1/status HTTP/1.1\r\n", MAX_BODY),
            Err(HttpError::TruncatedRequest)
        );
        assert_eq!(
            parse_request(b"GET /v1/status HTTP/1.1\r\n\r", MAX_BODY),
            Err(HttpError::TruncatedRequest)
        );
        assert_eq!(
            parse_request(b"", MAX_BODY),
            Err(HttpError::TruncatedRequest)
        );
        assert_eq!(
            parse_request(b"GET", MAX_BODY),
            Err(HttpError::TruncatedRequest)
        );
    }

    #[test]
    fn status_codes_match_the_adrs_own_named_codes() {
        assert_eq!(HttpError::MethodNotAllowed.status_code(), 405);
        assert_eq!(
            HttpError::ChunkedTransferEncodingNotSupported.status_code(),
            501
        );
        assert_eq!(HttpError::ExpectNotSupported.status_code(), 417);
        assert_eq!(HttpError::TargetTooLong.status_code(), 400);
        assert_eq!(HttpError::TruncatedRequest.status_code(), 400);
    }

    /// A randomized no-panic / no-over-read test over arbitrary byte
    /// input (ADR-0046's own conformance requirement) — a small,
    /// dependency-free deterministic PRNG (this workspace adds no
    /// `rand`/`proptest` dependency for test-only fuzzing, matching
    /// its own general minimal-dependency posture) generates many
    /// random-length, random-content buffers; `parse_request` must
    /// return `Ok` or `Err` for every one of them, never panic
    /// (including via an out-of-bounds slice, which Rust itself turns
    /// into a panic — the same failure mode this test exists to
    /// catch).
    #[test]
    fn never_panics_on_arbitrary_byte_input() {
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next_byte = || {
            // xorshift64*
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 56) as u8
        };

        for _ in 0..20_000 {
            let len = (next_byte() as usize) % 300;
            let buffer: Vec<u8> = (0..len).map(|_| next_byte()).collect();
            let _ = parse_request(&buffer, MAX_BODY);
        }
    }
}

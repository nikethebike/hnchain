use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::parser::{HttpError, ParsedRequest, parse_request};
use crate::response::HttpResponse;

/// Answers one successfully-parsed request. `Send + Sync` because the
/// same handler is shared across every worker thread (ADR-0046's own
/// "fixed worker pool"); nothing about the handler itself is this
/// module's concern beyond that bound — the real routing/JSON logic
/// (ADR-0046's own still-unbuilt endpoint step) lives entirely on the
/// other side of it.
pub trait RequestHandler: Send + Sync {
    /// Produces this request's own response.
    fn handle(&self, request: ParsedRequest<'_>) -> HttpResponse;
}

impl<F> RequestHandler for F
where
    F: Fn(ParsedRequest<'_>) -> HttpResponse + Send + Sync,
{
    fn handle(&self, request: ParsedRequest<'_>) -> HttpResponse {
        self(request)
    }
}

/// Everything [`spawn_server`] needs beyond the listener itself and
/// the handler (ADR-0046's own "Decided: HTTP Server..." — "a fixed
/// worker pool and a bounded accept queue (implementation constants,
/// documented)" plus "a total deadline for reading one full request
/// ... in addition to per-read timeouts, and a write timeout").
#[derive(Clone, Copy, Debug)]
pub struct ServerConfig {
    /// The number of worker threads servicing accepted connections.
    pub worker_count: usize,
    /// The bounded accept queue's own capacity — a connection accepted
    /// while every worker is busy and this queue is already full is
    /// closed immediately (ADR-0046's own "Connections beyond capacity
    /// are closed immediately"), never left waiting indefinitely.
    pub accept_queue_capacity: usize,
    /// The total time budget for reading one complete request off a
    /// single connection, from the moment a worker starts servicing it
    /// — the slowloris defence ADR-0046 names: a client trickling
    /// bytes in just under `per_read_timeout` each time still gets cut
    /// off once this total elapses.
    pub total_read_deadline: Duration,
    /// The timeout on each individual `read` call — a single
    /// unresponsive read never blocks a worker past this, independent
    /// of `total_read_deadline`.
    pub per_read_timeout: Duration,
    /// The timeout on writing the response.
    pub write_timeout: Duration,
    /// The maximum `POST` body [`crate::parse_request`] accepts —
    /// threaded straight through, the same "caller already resolved
    /// it" parameter that function's own documentation already names.
    pub max_body_len: usize,
}

/// Starts accepting connections on `listener` and servicing them
/// against `handler`, using a fixed pool of [`ServerConfig::worker_count`]
/// threads reading from one shared, bounded accept queue (ADR-0046's
/// own "Decided: HTTP Server..."). Returns immediately: the accept
/// loop and every worker run on their own background threads for the
/// remaining lifetime of the process, matching this crate's "Stop" is
/// OS-level process termination" posture (no graceful shutdown exists
/// anywhere else in this workspace either, `hn_node::run`'s own
/// documentation names the same choice explicitly).
pub fn spawn_server(listener: TcpListener, config: ServerConfig, handler: Arc<dyn RequestHandler>) {
    let (sender, receiver) = mpsc::sync_channel::<TcpStream>(config.accept_queue_capacity);
    let receiver = Arc::new(Mutex::new(receiver));

    for _ in 0..config.worker_count {
        let receiver = Arc::clone(&receiver);
        let handler = Arc::clone(&handler);
        thread::spawn(move || worker_loop(&receiver, &config, &handler));
    }

    thread::spawn(move || {
        for accepted in listener.incoming() {
            let Ok(stream) = accepted else {
                continue;
            };
            // A full queue means every worker is already busy and this
            // queue already holds `accept_queue_capacity` connections
            // waiting -- closing the new one immediately (by simply
            // dropping it, never calling `.recv()` on it) is exactly
            // ADR-0046's own "closed immediately", not a wait of any
            // length.
            drop(sender.try_send(stream));
        }
    });
}

fn worker_loop(
    receiver: &Arc<Mutex<Receiver<TcpStream>>>,
    config: &ServerConfig,
    handler: &Arc<dyn RequestHandler>,
) {
    loop {
        let stream = {
            // The lock is held only long enough to pull one connection
            // off the queue -- never while servicing it, so one slow
            // connection can never stall another worker's own ability
            // to claim the next one.
            let Ok(guard) = receiver.lock() else {
                return;
            };
            match guard.recv() {
                Ok(stream) => stream,
                Err(_) => return,
            }
        };
        service_connection(stream, config, handler);
    }
}

fn service_connection(
    mut stream: TcpStream,
    config: &ServerConfig,
    handler: &Arc<dyn RequestHandler>,
) {
    let deadline = Instant::now() + config.total_read_deadline;
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            // Slow-client / slowloris defence: the total budget for
            // reading one whole request is spent, regardless of how
            // promptly each individual read responded. Dropped, not
            // answered -- the client already used its one chance.
            return;
        }
        if stream
            .set_read_timeout(Some(remaining.min(config.per_read_timeout)))
            .is_err()
        {
            return;
        }

        match stream.read(&mut chunk) {
            Ok(0) => return, // The peer closed its own end; nothing more will ever arrive.
            Ok(read_len) => buffer.extend_from_slice(&chunk[..read_len]),
            Err(error) if is_timeout(&error) => continue,
            Err(_) => return,
        }

        match parse_request(&buffer, config.max_body_len) {
            Ok(request) => {
                let response = handler.handle(request);
                write_response(&mut stream, &response, config.write_timeout);
                return;
            }
            Err(HttpError::TruncatedRequest) => continue,
            Err(error) => {
                write_response(
                    &mut stream,
                    &HttpResponse::from_http_error(error),
                    config.write_timeout,
                );
                return;
            }
        }
    }
}

fn write_response(stream: &mut TcpStream, response: &HttpResponse, write_timeout: Duration) {
    if stream.set_write_timeout(Some(write_timeout)).is_err() {
        return;
    }
    // A write failing (a client that already hung up, or one that
    // never reads its own response) is this connection's problem
    // alone -- the worker simply moves on to the next one, never
    // panicking over it.
    let _ = response.write_to(stream);
}

fn is_timeout(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};

    use super::{HttpResponse, RequestHandler, ServerConfig, spawn_server};
    use crate::parser::ParsedRequest;

    fn fast_config() -> ServerConfig {
        ServerConfig {
            worker_count: 2,
            accept_queue_capacity: 1,
            total_read_deadline: Duration::from_millis(300),
            per_read_timeout: Duration::from_millis(50),
            write_timeout: Duration::from_millis(500),
            max_body_len: 4096,
        }
    }

    struct EchoStatusHandler;
    impl RequestHandler for EchoStatusHandler {
        fn handle(&self, request: ParsedRequest<'_>) -> HttpResponse {
            HttpResponse {
                status: 200,
                content_type: "text/plain; charset=utf-8",
                body: request.target.as_bytes().to_vec(),
            }
        }
    }

    fn bind_loopback() -> std::io::Result<TcpListener> {
        TcpListener::bind("127.0.0.1:0")
    }

    #[test]
    fn answers_a_well_formed_request() -> Result<(), Box<dyn std::error::Error>> {
        let listener = bind_loopback()?;
        let addr = listener.local_addr()?;
        spawn_server(listener, fast_config(), Arc::new(EchoStatusHandler));

        let mut client = TcpStream::connect(addr)?;
        client.write_all(b"GET /v1/status HTTP/1.1\r\n\r\n")?;
        let mut response = Vec::new();
        client.read_to_end(&mut response)?;

        let text = String::from_utf8(response)?;
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.ends_with("/v1/status"));
        Ok(())
    }

    /// ADR-0046's own conformance requirement: "oversized body rejected
    /// without reading it all" -- a `Content-Length` declaring far more
    /// than `max_body_len` is rejected from the header alone
    /// (`parse_request`'s own check order: the declared length is
    /// compared against the limit before anything about the actual
    /// body bytes present is even considered), so the response arrives
    /// immediately, never waiting around for bytes this client never
    /// even sends.
    #[test]
    fn rejects_an_oversized_declared_body_without_reading_it()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = bind_loopback()?;
        let addr = listener.local_addr()?;
        // A generous total deadline: if this test took anywhere close
        // to it, that alone would mean the server was actually waiting
        // on body bytes rather than rejecting from the header alone.
        let config = ServerConfig {
            total_read_deadline: Duration::from_secs(5),
            ..fast_config()
        };
        spawn_server(listener, config, Arc::new(EchoStatusHandler));

        let mut client = TcpStream::connect(addr)?;
        let declared_len = config.max_body_len * 1000;
        client.write_all(
            format!("POST /v1/transactions HTTP/1.1\r\nContent-Length: {declared_len}\r\n\r\n")
                .as_bytes(),
        )?;
        // Deliberately never sends a single body byte.

        let started = Instant::now();
        client.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut response = Vec::new();
        client.read_to_end(&mut response)?;
        let elapsed = started.elapsed();

        let text = String::from_utf8(response)?;
        assert!(
            text.starts_with("HTTP/1.1 400"),
            "unexpected response: {text}"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "response took {elapsed:?}, far longer than a header-only rejection should"
        );
        Ok(())
    }

    /// ADR-0046's own conformance requirement: "a slowloris client
    /// (bytes trickled below the per-read timeout) is dropped at the
    /// total deadline."
    #[test]
    fn drops_a_slowloris_client_at_the_total_deadline() -> Result<(), Box<dyn std::error::Error>> {
        let listener = bind_loopback()?;
        let addr = listener.local_addr()?;
        let config = ServerConfig {
            total_read_deadline: Duration::from_millis(200),
            per_read_timeout: Duration::from_millis(30),
            ..fast_config()
        };
        spawn_server(listener, config, Arc::new(EchoStatusHandler));

        let mut client = TcpStream::connect(addr)?;
        let started = Instant::now();
        // Trickle one byte at a time, each well inside the per-read
        // timeout, for longer than the total deadline -- the request
        // itself (deliberately) never completes.
        for byte in b"GET /v1/status HTTP/1.1\r\n" {
            if client.write_all(&[*byte]).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // The server must have closed its end by now (total deadline
        // already passed); reading returns EOF (0) rather than
        // blocking forever.
        client.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut probe = [0_u8; 1];
        let read_result = client.read(&mut probe);
        assert!(matches!(read_result, Ok(0)) || read_result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }

    /// ADR-0046's own conformance requirement: "connection flood
    /// beyond pool capacity does not stall accepted work."
    #[test]
    fn closes_connections_beyond_pool_and_queue_capacity() -> Result<(), Box<dyn std::error::Error>>
    {
        // The barrier has exactly 2 parties: the one worker (while
        // servicing `first`) and this test thread itself -- never a
        // second in-flight request, since with `worker_count: 1` no
        // other thread exists to ever reach the same barrier from a
        // second request's own handler call, which would just
        // deadlock both.
        let barrier = Arc::new(Barrier::new(2));
        struct BlockOnFirstCallHandler {
            barrier: Arc<Barrier>,
            call_count: AtomicUsize,
        }
        impl RequestHandler for BlockOnFirstCallHandler {
            fn handle(&self, _request: ParsedRequest<'_>) -> HttpResponse {
                let call_index = self.call_count.fetch_add(1, Ordering::SeqCst);
                if call_index == 0 {
                    self.barrier.wait();
                }
                HttpResponse {
                    status: 200,
                    content_type: "text/plain; charset=utf-8",
                    body: Vec::new(),
                }
            }
        }

        let listener = bind_loopback()?;
        let addr = listener.local_addr()?;
        // Capacity for exactly 2 connections in flight at once: 1
        // worker actively servicing one, 1 more waiting in the queue.
        let config = ServerConfig {
            worker_count: 1,
            accept_queue_capacity: 1,
            total_read_deadline: Duration::from_millis(2000),
            per_read_timeout: Duration::from_millis(200),
            write_timeout: Duration::from_millis(500),
            max_body_len: 4096,
        };
        let handler = Arc::new(BlockOnFirstCallHandler {
            barrier: Arc::clone(&barrier),
            call_count: AtomicUsize::new(0),
        });
        spawn_server(
            listener,
            config,
            Arc::clone(&handler) as Arc<dyn RequestHandler>,
        );

        // First connection: claimed by the one worker, which blocks on
        // the barrier (waiting on this test thread, not on any other
        // request) until released below.
        let mut first = TcpStream::connect(addr)?;
        first.write_all(b"GET /v1/status HTTP/1.1\r\n\r\n")?;
        // Give the worker a moment to actually claim it and reach the
        // barrier before the next connection is opened, so it is
        // genuinely busy already.
        std::thread::sleep(Duration::from_millis(150));

        // Second connection: sits in the accept queue (capacity 1),
        // not yet serviced -- the one worker is still blocked above.
        let mut second = TcpStream::connect(addr)?;
        second.write_all(b"GET /v1/status HTTP/1.1\r\n\r\n")?;
        std::thread::sleep(Duration::from_millis(150));

        // Third connection: both the one worker and the one queue slot
        // are already taken -- this one must be closed immediately
        // rather than held.
        let mut third = TcpStream::connect(addr)?;
        third.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut probe = [0_u8; 1];
        let third_result = third.read(&mut probe);
        assert!(matches!(third_result, Ok(0)) || third_result.is_err());

        // Release the first request's handler; the worker then
        // answers `first` and moves on to the still-queued `second`.
        barrier.wait();

        first.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut first_response = Vec::new();
        first.read_to_end(&mut first_response)?;
        assert!(String::from_utf8(first_response)?.starts_with("HTTP/1.1 200"));

        second.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut second_response = Vec::new();
        second.read_to_end(&mut second_response)?;
        assert!(String::from_utf8(second_response)?.starts_with("HTTP/1.1 200"));

        assert_eq!(handler.call_count.load(Ordering::SeqCst), 2);
        Ok(())
    }

    /// ADR-0046's own conformance requirement: "a closed-early client
    /// does not panic a worker."
    #[test]
    fn a_client_closing_mid_request_does_not_take_down_its_worker()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = bind_loopback()?;
        let addr = listener.local_addr()?;
        spawn_server(listener, fast_config(), Arc::new(EchoStatusHandler));

        {
            let mut early_close = TcpStream::connect(addr)?;
            // An incomplete request line, then the socket is dropped
            // before ever sending the blank line that would complete
            // the header section.
            early_close.write_all(b"GET /v1/status HTTP/1.1\r\n")?;
        }
        // The same worker pool must still be able to service an
        // entirely ordinary subsequent request -- it never panicked
        // or got stuck on the previous, abandoned one.
        std::thread::sleep(Duration::from_millis(50));
        let mut client = TcpStream::connect(addr)?;
        client.write_all(b"GET /v1/status HTTP/1.1\r\n\r\n")?;
        client.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut response = Vec::new();
        client.read_to_end(&mut response)?;
        assert!(String::from_utf8(response)?.starts_with("HTTP/1.1 200"));
        Ok(())
    }
}

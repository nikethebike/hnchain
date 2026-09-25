use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{self, Sender};
use std::thread;

use crate::envelope::{MAX_PAYLOAD_LEN, P2PMessageEnvelopeV1};
use crate::error::{NetworkError, NetworkResult};

/// Maximum length, in bytes, of one length-prefixed TCP frame's body
/// (ADR-0037, "Decided: Wire Framing"). `MAX_PAYLOAD_LEN` (the largest
/// `P2PMessageEnvelopeV1.payload` allowed) plus fixed headroom for the
/// envelope's own other fields (`envelope_version`, `protocol_version`,
/// `chain_id`, `network_id`, `channel`, `message_type`, `message_id`,
/// the payload's own length prefix) — comfortably more than the roughly
/// 50 bytes those fields actually take, not a tight bound.
pub const MAX_FRAME_LEN: usize = MAX_PAYLOAD_LEN + 4096;

fn io_error(error: std::io::Error) -> NetworkError {
    NetworkError::Io(error.to_string())
}

/// Writes `envelope` to `stream` as one length-prefixed frame: a 4-byte
/// little-endian `u32` byte count, then exactly that many
/// [`P2PMessageEnvelopeV1::encode`] bytes (ADR-0037, "Decided: Wire
/// Framing" — TCP is a byte stream, not a message stream, so the
/// envelope's own canonical bytes need explicit framing to know where
/// one message ends and the next begins).
pub fn write_frame(stream: &mut impl Write, envelope: &P2PMessageEnvelopeV1) -> NetworkResult<()> {
    let bytes = envelope.encode()?;
    let length = u32::try_from(bytes.len()).map_err(|_| NetworkError::FrameTooLarge {
        length: bytes.len(),
    })?;
    stream.write_all(&length.to_le_bytes()).map_err(io_error)?;
    stream.write_all(&bytes).map_err(io_error)?;
    Ok(())
}

/// Reads one length-prefixed frame from `stream` and decodes it as a
/// [`P2PMessageEnvelopeV1`] — the length prefix itself is checked
/// against [`MAX_FRAME_LEN`] before any buffer for the frame body is
/// allocated (ADR-0018, "Cheap Rejection": "frame limit," the first
/// stage, ahead of envelope decode).
pub fn read_frame(stream: &mut impl Read) -> NetworkResult<P2PMessageEnvelopeV1> {
    let mut length_bytes = [0_u8; 4];
    stream.read_exact(&mut length_bytes).map_err(io_error)?;
    let length = u32::from_le_bytes(length_bytes) as usize;
    if length > MAX_FRAME_LEN {
        return Err(NetworkError::FrameTooLarge { length });
    }
    let mut body = vec![0_u8; length];
    stream.read_exact(&mut body).map_err(io_error)?;
    P2PMessageEnvelopeV1::decode(&body)
}

/// A live connection's send half (ADR-0037, "Decided: Transport —
/// Blocking `std::net` + Threads"). Sending queues `envelope` onto an
/// internal channel a dedicated writer thread drains onto the real
/// socket, so a slow peer's TCP backpressure never blocks the caller's
/// own thread.
pub struct PeerLink {
    outbound: Sender<P2PMessageEnvelopeV1>,
}

impl PeerLink {
    /// Queues `envelope` to be written to this peer.
    /// [`NetworkError::PeerConnectionClosed`] if the writer thread has
    /// already exited (the connection is dead) — never blocks waiting
    /// for the peer itself.
    pub fn send(&self, envelope: P2PMessageEnvelopeV1) -> NetworkResult<()> {
        self.outbound
            .send(envelope)
            .map_err(|_| NetworkError::PeerConnectionClosed)
    }
}

/// Spawns one reader thread and one writer thread for an already-
/// connected `stream` (ADR-0037, "Decided: Transport") and returns a
/// [`PeerLink`] to send through. Every frame successfully read is
/// decoded and pushed to `inbound` tagged with `label` — a caller-
/// chosen, `Clone` identifier for this connection (this crate does not
/// prescribe what it should be; `hn-node` uses the connection's own
/// `SocketAddr`, known immediately at accept/connect time, before any
/// handshake has resolved a real peer identity). The reader thread
/// exits (silently — a real network peer disconnecting or sending
/// malformed bytes is an ordinary, expected event, not a condition
/// worth panicking or logging an error from this deliberately
/// transport-only crate) on the first read/decode failure or once
/// `inbound`'s receiver is gone, sending `label` to `on_close` first
/// (ADR-0039, "Decided: Connection-Drop Reconnection") so a caller that
/// wants to reconnect a dropped link has an explicit signal to act on
/// rather than needing to notice a connection's silence itself; the
/// writer thread exits once its own channel is closed or a write
/// fails, without a separate `on_close` signal of its own — a dead
/// connection's read side failing is the reliable, single source of
/// truth this function reports from.
pub fn spawn_peer_link<L>(
    stream: TcpStream,
    label: L,
    inbound: Sender<(L, P2PMessageEnvelopeV1)>,
    on_close: Sender<L>,
) -> NetworkResult<PeerLink>
where
    L: Clone + Send + 'static,
{
    let mut read_stream = stream.try_clone().map_err(io_error)?;
    let mut write_stream = stream;
    let (outbound_tx, outbound_rx) = mpsc::channel::<P2PMessageEnvelopeV1>();

    thread::spawn(move || {
        while let Ok(envelope) = read_frame(&mut read_stream) {
            if inbound.send((label.clone(), envelope)).is_err() {
                break;
            }
        }
        let _ = on_close.send(label);
    });

    thread::spawn(move || {
        for envelope in outbound_rx {
            if write_frame(&mut write_stream, &envelope).is_err() {
                break;
            }
        }
    });

    Ok(PeerLink {
        outbound: outbound_tx,
    })
}

#[cfg(test)]
mod tests {
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::time::Duration;

    use hn_core::ProtocolVersion;

    use super::spawn_peer_link;
    use crate::envelope::P2PMessageEnvelopeV1;
    use crate::registry::{Channel, MessageType};

    #[test]
    fn a_real_tcp_socket_carries_a_framed_envelope_round_trip()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;

        let accept_thread = std::thread::spawn(move || listener.accept().map(|(stream, _)| stream));
        let client_stream = TcpStream::connect(addr)?;
        let server_stream = accept_thread
            .join()
            .map_err(|_| "accept thread panicked")??;

        let (client_inbound_tx, client_inbound_rx) = mpsc::channel();
        let (server_inbound_tx, server_inbound_rx) = mpsc::channel();
        let (client_closed_tx, _client_closed_rx) = mpsc::channel();
        let (server_closed_tx, _server_closed_rx) = mpsc::channel();
        // The label identifies which remote peer a stream talks to, not
        // the local side spawning it: the client's own stream connects
        // to "server," and vice versa.
        let client_link =
            spawn_peer_link(client_stream, "server", client_inbound_tx, client_closed_tx)?;
        let _server_link =
            spawn_peer_link(server_stream, "client", server_inbound_tx, server_closed_tx)?;

        let envelope = P2PMessageEnvelopeV1::new(
            ProtocolVersion::new(0, 1, 0),
            1,
            1,
            Channel::Transactions,
            MessageType::TransactionAnnounce,
            b"real socket payload".to_vec(),
        )?;
        client_link.send(envelope.clone())?;

        let (label, received) = server_inbound_rx.recv_timeout(Duration::from_secs(5))?;
        assert_eq!(label, "client");
        assert_eq!(received, envelope);

        // Nothing sent the other way yet.
        assert!(client_inbound_rx.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn on_close_fires_when_the_peer_disconnects() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let accept_thread = std::thread::spawn(move || listener.accept().map(|(stream, _)| stream));
        let client_stream = TcpStream::connect(addr)?;
        let server_stream = accept_thread
            .join()
            .map_err(|_| "accept thread panicked")??;

        let (server_inbound_tx, _server_inbound_rx) = mpsc::channel();
        let (server_closed_tx, server_closed_rx) = mpsc::channel();
        let _server_link =
            spawn_peer_link(server_stream, "client", server_inbound_tx, server_closed_tx)?;

        drop(client_stream);

        assert_eq!(
            server_closed_rx.recv_timeout(Duration::from_secs(5))?,
            "client"
        );
        Ok(())
    }

    #[test]
    fn read_frame_rejects_a_length_prefix_over_the_frame_limit()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;

        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let accept_thread = std::thread::spawn(move || listener.accept().map(|(stream, _)| stream));
        let mut client_stream = TcpStream::connect(addr)?;
        let mut server_stream = accept_thread
            .join()
            .map_err(|_| "accept thread panicked")??;

        let oversized = (super::MAX_FRAME_LEN as u32) + 1;
        client_stream.write_all(&oversized.to_le_bytes())?;

        assert_eq!(
            super::read_frame(&mut server_stream),
            Err(crate::error::NetworkError::FrameTooLarge {
                length: oversized as usize
            })
        );
        Ok(())
    }
}

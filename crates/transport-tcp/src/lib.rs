//! Loopback-only TCP transport for Google's documented Android Auto
//! developer-mode head-unit server path.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use transport_api::{SessionTransport, TransportError};

pub const DEFAULT_DEVELOPER_ADDRESS: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 5277);

#[derive(Debug)]
pub struct DeveloperTcpTransport {
    stream: TcpStream,
    peer: SocketAddr,
}

impl DeveloperTcpTransport {
    pub fn connect(
        address: SocketAddr,
        connect_timeout: Duration,
        io_timeout: Duration,
    ) -> Result<Self, TransportError> {
        if !address.ip().is_loopback() {
            return Err(TransportError::InvalidEndpoint(
                "developer transport must use an ADB-forwarded loopback address".into(),
            ));
        }
        if address.port() == 0 || connect_timeout.is_zero() || io_timeout.is_zero() {
            return Err(TransportError::InvalidEndpoint(
                "port and timeouts must be non-zero".into(),
            ));
        }

        let stream = TcpStream::connect_timeout(&address, connect_timeout)
            .map_err(|error| map_io(&error))?;
        stream.set_nodelay(true).map_err(|error| map_io(&error))?;
        stream
            .set_read_timeout(Some(io_timeout))
            .and_then(|()| stream.set_write_timeout(Some(io_timeout)))
            .map_err(|error| map_io(&error))?;
        Ok(Self {
            stream,
            peer: address,
        })
    }

    #[must_use]
    pub const fn peer(&self) -> SocketAddr {
        self.peer
    }

    /// Confirms that the forwarded peer does not immediately close the stream.
    /// A timeout means the peer remained connected without sending data.
    pub fn verify_peer_available(&self) -> Result<(), TransportError> {
        match self.stream.peek(&mut [0_u8; 1]) {
            Ok(0) => Err(TransportError::Closed),
            Ok(_) => Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(map_io(&error)),
        }
    }
}

impl SessionTransport for DeveloperTcpTransport {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, TransportError> {
        match self.stream.read(buffer) {
            Ok(0) => Err(TransportError::Closed),
            Ok(size) => Ok(size),
            Err(error) => Err(map_io(&error)),
        }
    }

    fn send_all(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.stream.write_all(bytes).map_err(|error| map_io(&error))
    }
}

/// Listening TCP transport for the AAP session that begins once a phone
/// joins this project's own Wi-Fi access point during wireless bootstrap
/// (`apps/aa-headunit-diagnostics/src/wireless_bootstrap.rs`) — the head
/// unit is the server here, unlike [`DeveloperTcpTransport`] (an
/// outbound-connecting client to an externally-forwarded port).
/// Deliberately **not** loopback-restricted, unlike `DeveloperTcpTransport`
/// — this is the one transport in the codebase accepting a connection
/// from an arbitrary network peer, so: the caller must only construct
/// this after their own access point is already up, `accept_timeout`
/// bounds the wait for that one connection (never listens indefinitely),
/// and exactly one connection is accepted before the listener itself is
/// dropped.
#[derive(Debug)]
pub struct WirelessTcpTransport {
    stream: TcpStream,
    peer: SocketAddr,
}

impl WirelessTcpTransport {
    /// Binds `address`, accepts exactly one incoming connection bounded
    /// by `accept_timeout`, then behaves like any other
    /// `SessionTransport` for the rest of the session, each `receive`/
    /// `send_all` call bounded by `io_timeout`.
    ///
    /// Implementation note: std's blocking `TcpListener::accept` has no
    /// native cancel-on-timeout, so the bounded wait is implemented by
    /// accepting on a background thread and racing it against
    /// `mpsc::Receiver::recv_timeout` on the caller's side. If
    /// `accept_timeout` elapses with nobody connecting, that background
    /// thread stays blocked in `accept` until the process exits — an
    /// accepted, bounded leak (at most one thread) for this one-shot
    /// diagnostic command, not a retry loop that could accumulate them.
    pub fn listen(
        address: SocketAddr,
        accept_timeout: Duration,
        io_timeout: Duration,
    ) -> Result<Self, TransportError> {
        if accept_timeout.is_zero() || io_timeout.is_zero() {
            return Err(TransportError::InvalidEndpoint(
                "timeouts must be non-zero".into(),
            ));
        }
        let listener = TcpListener::bind(address).map_err(|error| map_io(&error))?;
        let (result_sender, result_receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = result_sender.send(listener.accept());
        });
        let (stream, peer) = result_receiver
            .recv_timeout(accept_timeout)
            .map_err(|_| TransportError::TimedOut)?
            .map_err(|error| map_io(&error))?;
        stream.set_nodelay(true).map_err(|error| map_io(&error))?;
        stream
            .set_read_timeout(Some(io_timeout))
            .and_then(|()| stream.set_write_timeout(Some(io_timeout)))
            .map_err(|error| map_io(&error))?;
        Ok(Self { stream, peer })
    }

    #[must_use]
    pub const fn peer(&self) -> SocketAddr {
        self.peer
    }
}

impl SessionTransport for WirelessTcpTransport {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, TransportError> {
        match self.stream.read(buffer) {
            Ok(0) => Err(TransportError::Closed),
            Ok(size) => Ok(size),
            Err(error) => Err(map_io(&error)),
        }
    }

    fn send_all(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.stream.write_all(bytes).map_err(|error| map_io(&error))
    }
}

fn map_io(error: &io::Error) -> TransportError {
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => TransportError::TimedOut,
        io::ErrorKind::UnexpectedEof
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::BrokenPipe => TransportError::Closed,
        _ => TransportError::Io(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    #[test]
    fn rejects_non_loopback_and_invalid_limits() {
        assert!(matches!(
            DeveloperTcpTransport::connect(
                "192.0.2.1:5277".parse().expect("address"),
                Duration::from_millis(1),
                Duration::from_millis(1),
            ),
            Err(TransportError::InvalidEndpoint(_))
        ));
        assert!(matches!(
            DeveloperTcpTransport::connect(
                "127.0.0.1:5277".parse().expect("address"),
                Duration::ZERO,
                Duration::from_millis(1),
            ),
            Err(TransportError::InvalidEndpoint(_))
        ));
    }

    #[test]
    fn exchanges_bytes_over_loopback() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut request = [0_u8; 4];
            socket.read_exact(&mut request).expect("request");
            assert_eq!(&request, b"ping");
            socket.write_all(b"pong").expect("response");
        });

        let mut transport =
            DeveloperTcpTransport::connect(address, Duration::from_secs(1), Duration::from_secs(1))
                .expect("connect");
        assert_eq!(transport.peer(), address);
        transport.send_all(b"ping").expect("send");
        let mut response = [0_u8; 4];
        assert_eq!(transport.receive(&mut response).expect("receive"), 4);
        assert_eq!(&response, b"pong");
        server.join().expect("server");
    }

    #[test]
    fn maps_read_timeout_without_hanging() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let (_socket, _) = listener.accept().expect("accept");
            thread::sleep(Duration::from_millis(100));
        });
        let mut transport = DeveloperTcpTransport::connect(
            address,
            Duration::from_secs(1),
            Duration::from_millis(10),
        )
        .expect("connect");
        assert_eq!(
            transport.receive(&mut [0_u8; 1]),
            Err(TransportError::TimedOut)
        );
        server.join().expect("server");
    }

    #[test]
    fn wireless_tcp_transport_accepts_one_connection_and_exchanges_bytes() {
        let address: SocketAddr = "127.0.0.1:0".parse().expect("address");
        // Bind ourselves first (port 0 avoids a fixed-port race) purely to
        // learn a free port, then immediately release it — `listen` binds
        // for real. A small window exists where another process could grab
        // the same port; acceptable for this one, deterministic test.
        let probe = TcpListener::bind(address).expect("probe listener");
        let bound_address = probe.local_addr().expect("bound address");
        drop(probe);

        let listen_address = bound_address;
        let client = thread::spawn(move || {
            // Give `listen` time to bind before connecting.
            thread::sleep(Duration::from_millis(50));
            let mut stream = TcpStream::connect(listen_address).expect("client connect");
            stream.write_all(b"ping").expect("client send");
            let mut response = [0_u8; 4];
            stream.read_exact(&mut response).expect("client receive");
            assert_eq!(&response, b"pong");
        });

        let mut transport = WirelessTcpTransport::listen(
            listen_address,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .expect("listen");
        assert_eq!(transport.peer().ip(), listen_address.ip());
        let mut request = [0_u8; 4];
        assert_eq!(transport.receive(&mut request).expect("receive"), 4);
        assert_eq!(&request, b"ping");
        transport.send_all(b"pong").expect("send");
        client.join().expect("client");
    }

    #[test]
    fn wireless_tcp_transport_times_out_when_nobody_connects() {
        let address: SocketAddr = "127.0.0.1:0".parse().expect("address");
        let probe = TcpListener::bind(address).expect("probe listener");
        let bound_address = probe.local_addr().expect("bound address");
        drop(probe);

        assert_eq!(
            WirelessTcpTransport::listen(
                bound_address,
                Duration::from_millis(10),
                Duration::from_secs(1),
            )
            .map(|_| ()),
            Err(TransportError::TimedOut)
        );
    }

    #[test]
    fn distinguishes_idle_peer_from_immediate_close() {
        let idle_listener = TcpListener::bind("127.0.0.1:0").expect("idle listener");
        let idle_address = idle_listener.local_addr().expect("idle address");
        let idle_server = thread::spawn(move || {
            let (_socket, _) = idle_listener.accept().expect("idle accept");
            thread::sleep(Duration::from_millis(100));
        });
        let idle = DeveloperTcpTransport::connect(
            idle_address,
            Duration::from_secs(1),
            Duration::from_millis(10),
        )
        .expect("idle connect");
        assert_eq!(idle.verify_peer_available(), Ok(()));
        idle_server.join().expect("idle server");

        let closing_listener = TcpListener::bind("127.0.0.1:0").expect("closing listener");
        let closing_address = closing_listener.local_addr().expect("closing address");
        let closing_server = thread::spawn(move || {
            let (socket, _) = closing_listener.accept().expect("closing accept");
            drop(socket);
        });
        let closing = DeveloperTcpTransport::connect(
            closing_address,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .expect("closing connect");
        assert_eq!(closing.verify_peer_available(), Err(TransportError::Closed));
        closing_server.join().expect("closing server");
    }
}

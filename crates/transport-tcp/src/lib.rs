//! Loopback-only TCP transport for Google's documented Android Auto
//! developer-mode head-unit server path.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
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
}

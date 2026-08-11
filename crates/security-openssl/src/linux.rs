use std::fmt;
use std::io::{self, Read, Write};

use openssl::pkey::{PKey, Private};
use openssl::rsa::Rsa;
use openssl::ssl::{
    ErrorCode, Ssl, SslContextBuilder, SslMethod, SslStream, SslVerifyMode, SslVersion,
};
use openssl::x509::X509;
use protocol_aap::{TlsClient, TlsProgress};

#[derive(Debug)]
pub enum OpenSslTlsError {
    InvalidLimit,
    Credentials(String),
    Setup(String),
    Handshake(String),
    InputTooLarge { size: usize, maximum: usize },
    OutputTooLarge { size: usize, maximum: usize },
    NotStarted,
    AlreadyStarted,
    AlreadyComplete,
    HandshakeNotComplete,
    InvalidRecordData(String),
    PlaintextUnavailable,
    SessionClosed,
}

impl fmt::Display for OpenSslTlsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit => formatter.write_str("TLS buffer limit must be non-zero"),
            Self::Credentials(error) => write!(formatter, "invalid TLS credentials: {error}"),
            Self::Setup(error) => write!(formatter, "unable to create TLS client: {error}"),
            Self::Handshake(error) => write!(formatter, "TLS handshake failed: {error}"),
            Self::InputTooLarge { size, maximum } => {
                write!(formatter, "TLS input {size} exceeds limit {maximum}")
            }
            Self::OutputTooLarge { size, maximum } => {
                write!(formatter, "TLS output {size} exceeds limit {maximum}")
            }
            Self::NotStarted => formatter.write_str("TLS handshake has not been started"),
            Self::AlreadyStarted => formatter.write_str("TLS handshake is already started"),
            Self::AlreadyComplete => formatter.write_str("TLS handshake is already complete"),
            Self::HandshakeNotComplete => {
                formatter.write_str("TLS handshake must complete before application data")
            }
            Self::InvalidRecordData(error) => {
                write!(formatter, "invalid TLS application-data record: {error}")
            }
            Self::PlaintextUnavailable => formatter.write_str(
                "TLS session cannot produce plaintext without further protocol progress",
            ),
            Self::SessionClosed => formatter.write_str("TLS session was closed by the peer"),
        }
    }
}

impl std::error::Error for OpenSslTlsError {}

#[derive(Debug, Default)]
struct MemoryTransport {
    inbound: Vec<u8>,
    inbound_offset: usize,
    outbound: Vec<u8>,
}

impl MemoryTransport {
    fn feed(&mut self, bytes: &[u8]) {
        if self.inbound_offset == self.inbound.len() {
            self.inbound.clear();
            self.inbound_offset = 0;
        }
        self.inbound.extend_from_slice(bytes);
    }

    fn take_outbound(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.outbound)
    }
}

impl Read for MemoryTransport {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        let available = &self.inbound[self.inbound_offset..];
        if available.is_empty() {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        let count = available.len().min(destination.len());
        destination[..count].copy_from_slice(&available[..count]);
        self.inbound_offset += count;
        Ok(count)
    }
}

impl Write for MemoryTransport {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.outbound.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct OpenSslTlsClient {
    stream: SslStream<MemoryTransport>,
    maximum_chunk_size: usize,
    started: bool,
    complete: bool,
    version_policy: TlsVersionPolicy,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TlsVersionPolicy {
    #[default]
    SystemDefault,
    Tls12Only,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EphemeralCredentials {
    pub certificate_pem: Vec<u8>,
    pub private_key_pem: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialSummary {
    pub not_before: String,
    pub not_after: String,
}

pub fn validate_credential_pair(
    certificate_pem: &[u8],
    private_key_pem: &[u8],
) -> Result<CredentialSummary, OpenSslTlsError> {
    use openssl::asn1::Asn1Time;
    use std::cmp::Ordering;

    let (certificate, private_key) = parse_credential_pair(certificate_pem, private_key_pem)?;
    let now =
        Asn1Time::days_from_now(0).map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    if certificate
        .not_before()
        .compare(&now)
        .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?
        == Ordering::Greater
    {
        return Err(OpenSslTlsError::Credentials(
            "certificate is not valid yet".into(),
        ));
    }
    if certificate
        .not_after()
        .compare(&now)
        .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?
        == Ordering::Less
    {
        return Err(OpenSslTlsError::Credentials(
            "certificate has expired".into(),
        ));
    }
    drop(private_key);
    Ok(CredentialSummary {
        not_before: certificate.not_before().to_string(),
        not_after: certificate.not_after().to_string(),
    })
}

fn parse_credential_pair(
    certificate_pem: &[u8],
    private_key_pem: &[u8],
) -> Result<(X509, PKey<Private>), OpenSslTlsError> {
    let certificate = X509::from_pem(certificate_pem)
        .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
    let private_key = PKey::<Private>::private_key_from_pem(private_key_pem)
        .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
    let public_key = certificate
        .public_key()
        .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
    if !private_key.public_eq(&public_key) {
        return Err(OpenSslTlsError::Credentials(
            "certificate and private key do not match".into(),
        ));
    }
    Ok((certificate, private_key))
}

pub fn generate_ephemeral_credentials() -> Result<EphemeralCredentials, OpenSslTlsError> {
    use openssl::asn1::Asn1Time;
    use openssl::bn::{BigNum, MsbOption};
    use openssl::hash::MessageDigest;
    use openssl::nid::Nid;
    use openssl::x509::X509NameBuilder;

    let key = PKey::from_rsa(
        Rsa::generate(2048).map_err(|error| OpenSslTlsError::Setup(error.to_string()))?,
    )
    .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    let mut name =
        X509NameBuilder::new().map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    name.append_entry_by_nid(Nid::COMMONNAME, "Pi Auto Head Unit Bench Probe")
        .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    let name = name.build();

    let mut certificate =
        X509::builder().map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    certificate
        .set_version(2)
        .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    let mut serial = BigNum::new().map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    serial
        .rand(64, MsbOption::MAYBE_ZERO, false)
        .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    let serial = serial
        .to_asn1_integer()
        .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    certificate
        .set_serial_number(&serial)
        .and_then(|()| certificate.set_subject_name(&name))
        .and_then(|()| certificate.set_issuer_name(&name))
        .and_then(|()| certificate.set_pubkey(&key))
        .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    let not_before =
        Asn1Time::days_from_now(0).map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    let not_after =
        Asn1Time::days_from_now(1).map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    certificate
        .set_not_before(&not_before)
        .and_then(|()| certificate.set_not_after(&not_after))
        .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    certificate
        .sign(&key, MessageDigest::sha256())
        .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
    let certificate = certificate.build();

    Ok(EphemeralCredentials {
        certificate_pem: certificate
            .to_pem()
            .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?,
        private_key_pem: key
            .private_key_to_pem_pkcs8()
            .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?,
    })
}

impl OpenSslTlsClient {
    pub fn from_pem(
        certificate_pem: &[u8],
        private_key_pem: &[u8],
        maximum_chunk_size: usize,
    ) -> Result<Self, OpenSslTlsError> {
        Self::from_pem_with_policy(
            certificate_pem,
            private_key_pem,
            maximum_chunk_size,
            TlsVersionPolicy::SystemDefault,
        )
    }

    pub fn from_pem_with_policy(
        certificate_pem: &[u8],
        private_key_pem: &[u8],
        maximum_chunk_size: usize,
        version_policy: TlsVersionPolicy,
    ) -> Result<Self, OpenSslTlsError> {
        if maximum_chunk_size == 0 {
            return Err(OpenSslTlsError::InvalidLimit);
        }

        let (certificate, private_key) = parse_credential_pair(certificate_pem, private_key_pem)?;
        let mut context = SslContextBuilder::new(SslMethod::tls_client())
            .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
        context
            .set_certificate(&certificate)
            .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
        context
            .set_private_key(&private_key)
            .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
        context
            .check_private_key()
            .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
        context.set_verify(SslVerifyMode::NONE);
        if version_policy == TlsVersionPolicy::Tls12Only {
            context
                .set_min_proto_version(Some(SslVersion::TLS1_2))
                .and_then(|()| context.set_max_proto_version(Some(SslVersion::TLS1_2)))
                .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
        }

        let mut ssl = Ssl::new(&context.build())
            .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
        ssl.set_connect_state();
        let stream = SslStream::new(ssl, MemoryTransport::default())
            .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;

        Ok(Self {
            stream,
            maximum_chunk_size,
            started: false,
            complete: false,
            version_policy,
        })
    }

    #[must_use]
    pub const fn version_policy(&self) -> TlsVersionPolicy {
        self.version_policy
    }

    #[must_use]
    pub fn handshake_state(&self) -> String {
        self.stream.ssl().state_string_long().to_owned()
    }

    fn progress(&mut self) -> Result<TlsProgress, OpenSslTlsError> {
        match self.stream.connect() {
            Ok(()) => self.complete = true,
            Err(error)
                if error.code() == ErrorCode::WANT_READ
                    || error.code() == ErrorCode::WANT_WRITE => {}
            Err(error) => return Err(OpenSslTlsError::Handshake(error.to_string())),
        }

        let outbound = self.stream.get_mut().take_outbound();
        if outbound.len() > self.maximum_chunk_size {
            return Err(OpenSslTlsError::OutputTooLarge {
                size: outbound.len(),
                maximum: self.maximum_chunk_size,
            });
        }
        Ok(TlsProgress {
            outbound,
            complete: self.complete,
        })
    }
}

impl TlsClient for OpenSslTlsClient {
    type Error = OpenSslTlsError;

    fn start(&mut self) -> Result<TlsProgress, Self::Error> {
        if self.complete {
            return Err(OpenSslTlsError::AlreadyComplete);
        }
        if self.started {
            return Err(OpenSslTlsError::AlreadyStarted);
        }
        self.started = true;
        self.progress()
    }

    fn feed(&mut self, inbound: &[u8]) -> Result<TlsProgress, Self::Error> {
        if self.complete {
            return Err(OpenSslTlsError::AlreadyComplete);
        }
        if !self.started {
            return Err(OpenSslTlsError::NotStarted);
        }
        if inbound.len() > self.maximum_chunk_size {
            return Err(OpenSslTlsError::InputTooLarge {
                size: inbound.len(),
                maximum: self.maximum_chunk_size,
            });
        }
        self.stream.get_mut().feed(inbound);
        self.progress()
    }

    fn encrypt_application_data(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, Self::Error> {
        if !self.complete {
            return Err(OpenSslTlsError::HandshakeNotComplete);
        }
        if plaintext.len() > self.maximum_chunk_size {
            return Err(OpenSslTlsError::InputTooLarge {
                size: plaintext.len(),
                maximum: self.maximum_chunk_size,
            });
        }
        match self.stream.ssl_write(plaintext) {
            Ok(_) => {}
            Err(error) if error.code() == ErrorCode::ZERO_RETURN => {
                return Err(OpenSslTlsError::SessionClosed);
            }
            Err(error) => return Err(OpenSslTlsError::InvalidRecordData(error.to_string())),
        }

        let outbound = self.stream.get_mut().take_outbound();
        if outbound.len() > self.maximum_chunk_size {
            return Err(OpenSslTlsError::OutputTooLarge {
                size: outbound.len(),
                maximum: self.maximum_chunk_size,
            });
        }
        Ok(outbound)
    }

    fn decrypt_application_data(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error> {
        if !self.complete {
            return Err(OpenSslTlsError::HandshakeNotComplete);
        }
        if ciphertext.len() > self.maximum_chunk_size {
            return Err(OpenSslTlsError::InputTooLarge {
                size: ciphertext.len(),
                maximum: self.maximum_chunk_size,
            });
        }
        self.stream.get_mut().feed(ciphertext);

        let mut plaintext = Vec::new();
        let mut chunk = vec![0_u8; self.maximum_chunk_size];
        loop {
            match self.stream.ssl_read(&mut chunk) {
                Ok(0) => {
                    // A clean `Ok(0)` (as opposed to `WANT_READ`) means the
                    // peer sent a close-notify. Preserve any plaintext
                    // already decrypted this call; the closure is reported
                    // once nothing else remains.
                    if plaintext.is_empty() {
                        return Err(OpenSslTlsError::SessionClosed);
                    }
                    break;
                }
                Ok(count) => plaintext.extend_from_slice(&chunk[..count]),
                Err(error) if error.code() == ErrorCode::WANT_READ => break,
                Err(error) if error.code() == ErrorCode::WANT_WRITE => {
                    if plaintext.is_empty() {
                        return Err(OpenSslTlsError::PlaintextUnavailable);
                    }
                    break;
                }
                Err(error) if error.code() == ErrorCode::ZERO_RETURN => {
                    return Err(OpenSslTlsError::SessionClosed);
                }
                Err(error) => return Err(OpenSslTlsError::InvalidRecordData(error.to_string())),
            }
            if plaintext.len() > self.maximum_chunk_size {
                return Err(OpenSslTlsError::OutputTooLarge {
                    size: plaintext.len(),
                    maximum: self.maximum_chunk_size,
                });
            }
        }
        Ok(plaintext)
    }
}

/// Server-role TLS peer for deterministic scripted-peer tests, e.g. a fake
/// phone completing a real TLS session opposite an `OpenSslTlsClient`
/// (which is always client-role). Gated behind `test-support`; never part
/// of the production client/server boundary.
#[cfg(feature = "test-support")]
#[derive(Debug)]
pub struct TestServerTls {
    stream: SslStream<MemoryTransport>,
    maximum_chunk_size: usize,
    complete: bool,
}

#[cfg(feature = "test-support")]
impl TestServerTls {
    pub fn from_pem(
        certificate_pem: &[u8],
        private_key_pem: &[u8],
        trusted_client_certificate_pem: &[u8],
        maximum_chunk_size: usize,
    ) -> Result<Self, OpenSslTlsError> {
        if maximum_chunk_size == 0 {
            return Err(OpenSslTlsError::InvalidLimit);
        }
        let (certificate, private_key) = parse_credential_pair(certificate_pem, private_key_pem)?;
        let trusted_client_certificate = X509::from_pem(trusted_client_certificate_pem)
            .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;

        let mut context = SslContextBuilder::new(SslMethod::tls_server())
            .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
        context
            .set_certificate(&certificate)
            .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
        context
            .set_private_key(&private_key)
            .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
        context
            .check_private_key()
            .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
        context
            .cert_store_mut()
            .add_cert(trusted_client_certificate)
            .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
        context.set_verify(SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT);

        let mut ssl = Ssl::new(&context.build())
            .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;
        ssl.set_accept_state();
        let stream = SslStream::new(ssl, MemoryTransport::default())
            .map_err(|error| OpenSslTlsError::Setup(error.to_string()))?;

        Ok(Self {
            stream,
            maximum_chunk_size,
            complete: false,
        })
    }

    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.complete
    }

    /// Feeds handshake bytes (or none, to begin/continue accepting) and
    /// returns the server's next outbound chunk plus completion state.
    pub fn accept(&mut self, inbound: &[u8]) -> Result<TlsProgress, OpenSslTlsError> {
        if inbound.len() > self.maximum_chunk_size {
            return Err(OpenSslTlsError::InputTooLarge {
                size: inbound.len(),
                maximum: self.maximum_chunk_size,
            });
        }
        if !inbound.is_empty() {
            self.stream.get_mut().feed(inbound);
        }
        match self.stream.accept() {
            Ok(()) => self.complete = true,
            Err(error)
                if error.code() == ErrorCode::WANT_READ
                    || error.code() == ErrorCode::WANT_WRITE => {}
            Err(error) => return Err(OpenSslTlsError::Handshake(error.to_string())),
        }
        let outbound = self.stream.get_mut().take_outbound();
        if outbound.len() > self.maximum_chunk_size {
            return Err(OpenSslTlsError::OutputTooLarge {
                size: outbound.len(),
                maximum: self.maximum_chunk_size,
            });
        }
        Ok(TlsProgress {
            outbound,
            complete: self.complete,
        })
    }

    pub fn encrypt_application_data(
        &mut self,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, OpenSslTlsError> {
        if !self.complete {
            return Err(OpenSslTlsError::HandshakeNotComplete);
        }
        if plaintext.len() > self.maximum_chunk_size {
            return Err(OpenSslTlsError::InputTooLarge {
                size: plaintext.len(),
                maximum: self.maximum_chunk_size,
            });
        }
        match self.stream.ssl_write(plaintext) {
            Ok(_) => {}
            Err(error) if error.code() == ErrorCode::ZERO_RETURN => {
                return Err(OpenSslTlsError::SessionClosed);
            }
            Err(error) => return Err(OpenSslTlsError::InvalidRecordData(error.to_string())),
        }
        let outbound = self.stream.get_mut().take_outbound();
        if outbound.len() > self.maximum_chunk_size {
            return Err(OpenSslTlsError::OutputTooLarge {
                size: outbound.len(),
                maximum: self.maximum_chunk_size,
            });
        }
        Ok(outbound)
    }

    pub fn decrypt_application_data(
        &mut self,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, OpenSslTlsError> {
        if !self.complete {
            return Err(OpenSslTlsError::HandshakeNotComplete);
        }
        if ciphertext.len() > self.maximum_chunk_size {
            return Err(OpenSslTlsError::InputTooLarge {
                size: ciphertext.len(),
                maximum: self.maximum_chunk_size,
            });
        }
        self.stream.get_mut().feed(ciphertext);

        let mut plaintext = Vec::new();
        let mut chunk = vec![0_u8; self.maximum_chunk_size];
        loop {
            match self.stream.ssl_read(&mut chunk) {
                Ok(0) => {
                    if plaintext.is_empty() {
                        return Err(OpenSslTlsError::SessionClosed);
                    }
                    break;
                }
                Ok(count) => plaintext.extend_from_slice(&chunk[..count]),
                Err(error) if error.code() == ErrorCode::WANT_READ => break,
                Err(error) if error.code() == ErrorCode::WANT_WRITE => {
                    if plaintext.is_empty() {
                        return Err(OpenSslTlsError::PlaintextUnavailable);
                    }
                    break;
                }
                Err(error) if error.code() == ErrorCode::ZERO_RETURN => {
                    return Err(OpenSslTlsError::SessionClosed);
                }
                Err(error) => return Err(OpenSslTlsError::InvalidRecordData(error.to_string())),
            }
            if plaintext.len() > self.maximum_chunk_size {
                return Err(OpenSslTlsError::OutputTooLarge {
                    size: plaintext.len(),
                    maximum: self.maximum_chunk_size,
                });
            }
        }
        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use openssl::asn1::Asn1Time;
    use openssl::bn::{BigNum, MsbOption};
    use openssl::hash::MessageDigest;
    use openssl::nid::Nid;
    use openssl::rsa::Rsa;
    use openssl::ssl::{SslContextBuilder, SslMethod, SslVerifyMode};
    use openssl::x509::{X509, X509NameBuilder};

    use super::*;

    fn credentials() -> (X509, PKey<Private>) {
        let key = PKey::from_rsa(Rsa::generate(2048).expect("RSA key")).expect("private key");
        let mut name = X509NameBuilder::new().expect("name");
        name.append_entry_by_nid(Nid::COMMONNAME, "pi-auto-headunit-test")
            .expect("common name");
        let name = name.build();

        let mut certificate = X509::builder().expect("certificate");
        certificate.set_version(2).expect("version");
        let mut serial = BigNum::new().expect("serial");
        serial
            .rand(64, MsbOption::MAYBE_ZERO, false)
            .expect("random serial");
        let serial = serial.to_asn1_integer().expect("ASN.1 serial");
        certificate.set_serial_number(&serial).expect("serial");
        certificate.set_subject_name(&name).expect("subject");
        certificate.set_issuer_name(&name).expect("issuer");
        certificate.set_pubkey(&key).expect("public key");
        certificate
            .set_not_before(&Asn1Time::days_from_now(0).expect("not before"))
            .expect("not before");
        certificate
            .set_not_after(&Asn1Time::days_from_now(1).expect("not after"))
            .expect("not after");
        certificate
            .sign(&key, MessageDigest::sha256())
            .expect("signature");
        (certificate.build(), key)
    }

    fn accepting_server(
        server_certificate: &X509,
        server_key: &PKey<Private>,
        trusted_client_certificate: &X509,
    ) -> SslStream<MemoryTransport> {
        let mut context = SslContextBuilder::new(SslMethod::tls_server()).expect("server context");
        context
            .set_certificate(server_certificate)
            .expect("server certificate");
        context
            .set_private_key(server_key)
            .expect("server private key");
        context.check_private_key().expect("matching server key");
        context
            .cert_store_mut()
            .add_cert(trusted_client_certificate.clone())
            .expect("trusted client certificate");
        context.set_verify(SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT);

        let mut ssl = Ssl::new(&context.build()).expect("server SSL");
        ssl.set_accept_state();
        SslStream::new(ssl, MemoryTransport::default()).expect("server stream")
    }

    fn progress_server(stream: &mut SslStream<MemoryTransport>) -> (bool, Vec<u8>) {
        let complete = match stream.accept() {
            Ok(()) => true,
            Err(error)
                if error.code() == ErrorCode::WANT_READ
                    || error.code() == ErrorCode::WANT_WRITE =>
            {
                false
            }
            Err(error) => panic!("server handshake failed: {error}"),
        };
        (complete, stream.get_mut().take_outbound())
    }

    /// Feeds `bytes` into an already-complete client and drains whatever
    /// `ssl_read` reports, discarding the content. Used to consume
    /// post-handshake TLS-internal data (e.g. session tickets) that would
    /// otherwise desync the client's read-sequence state for later
    /// application-data decryption.
    fn drain_into_client(client: &mut OpenSslTlsClient, bytes: &[u8]) {
        client.stream.get_mut().feed(bytes);
        let mut sink = [0_u8; 4096];
        loop {
            match client.stream.ssl_read(&mut sink) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if error.code() == ErrorCode::WANT_READ => break,
                Err(error) => panic!("failed to drain post-handshake server data: {error}"),
            }
        }
    }

    /// Drives a real client+server handshake to completion and returns both
    /// sides, mirroring `completes_mutual_tls_when_the_client_identity_is_trusted`.
    /// The returned pair shares no state with the caller's own credentials.
    fn established_pair(
        maximum_chunk_size: usize,
    ) -> (OpenSslTlsClient, SslStream<MemoryTransport>) {
        let (client_certificate, client_key) = credentials();
        let (server_certificate, server_key) = credentials();
        let mut client = OpenSslTlsClient::from_pem(
            &client_certificate.to_pem().expect("client certificate PEM"),
            &client_key
                .private_key_to_pem_pkcs8()
                .expect("client private key PEM"),
            maximum_chunk_size,
        )
        .expect("client");
        let mut server = accepting_server(&server_certificate, &server_key, &client_certificate);

        let mut client_progress = client.start().expect("start client");
        let mut server_complete = false;
        for _ in 0..16 {
            if !client_progress.outbound.is_empty() {
                server.get_mut().feed(&client_progress.outbound);
            }
            let (complete, server_outbound) = progress_server(&mut server);
            server_complete = complete;
            if server_outbound.is_empty() {
                client_progress.outbound.clear();
            } else if client_progress.complete {
                // TLS 1.3 servers may emit post-handshake data (e.g.
                // session tickets) after the client already reports
                // complete. `TlsClient::feed` rejects input once complete,
                // but the bytes must still reach the client's TLS session
                // or its read-sequence state for the server-write direction
                // would desync from what the server actually sent, breaking
                // later application-data decryption. Drain it directly.
                drain_into_client(&mut client, &server_outbound);
            } else {
                client_progress = client.feed(&server_outbound).expect("progress client");
            }
            if client_progress.complete && server_complete {
                break;
            }
        }

        assert!(
            client_progress.complete,
            "client handshake did not complete"
        );
        assert!(server_complete, "server handshake did not complete");
        (client, server)
    }

    #[test]
    fn starts_a_bounded_client_hello_with_injected_credentials() {
        let (certificate, key) = credentials();
        let mut client = OpenSslTlsClient::from_pem(
            &certificate.to_pem().expect("certificate PEM"),
            &key.private_key_to_pem_pkcs8().expect("private key PEM"),
            64 * 1024,
        )
        .expect("client");

        let progress = client.start().expect("start");
        assert!(!progress.complete);
        assert!(!progress.outbound.is_empty());
        assert_eq!(progress.outbound[0], 0x16);
    }

    #[test]
    fn generated_credentials_are_ephemeral_and_usable() {
        let first = generate_ephemeral_credentials().expect("first credentials");
        let second = generate_ephemeral_credentials().expect("second credentials");
        assert_ne!(first.private_key_pem, second.private_key_pem);
        OpenSslTlsClient::from_pem(&first.certificate_pem, &first.private_key_pem, 64 * 1024)
            .expect("generated credentials should load");
    }

    #[test]
    fn completes_mutual_tls_when_the_client_identity_is_trusted() {
        let (client_certificate, client_key) = credentials();
        let (server_certificate, server_key) = credentials();
        let mut client = OpenSslTlsClient::from_pem(
            &client_certificate.to_pem().expect("client certificate PEM"),
            &client_key
                .private_key_to_pem_pkcs8()
                .expect("client private key PEM"),
            64 * 1024,
        )
        .expect("client");
        let mut server = accepting_server(&server_certificate, &server_key, &client_certificate);

        let mut client_progress = client.start().expect("start client");
        let mut server_complete = false;
        for _ in 0..16 {
            if !client_progress.outbound.is_empty() {
                server.get_mut().feed(&client_progress.outbound);
            }
            let (complete, server_outbound) = progress_server(&mut server);
            server_complete = complete;
            if !server_outbound.is_empty() && !client_progress.complete {
                client_progress = client.feed(&server_outbound).expect("progress client");
            } else {
                client_progress.outbound.clear();
            }
            if client_progress.complete && server_complete {
                break;
            }
        }

        assert!(
            client_progress.complete,
            "client handshake did not complete"
        );
        assert!(server_complete, "server handshake did not complete");
        assert!(server.ssl().peer_certificate().is_some());
    }

    #[test]
    fn tls12_compatibility_policy_is_explicit() {
        let credentials = generate_ephemeral_credentials().expect("credentials");
        let mut client = OpenSslTlsClient::from_pem_with_policy(
            &credentials.certificate_pem,
            &credentials.private_key_pem,
            64 * 1024,
            TlsVersionPolicy::Tls12Only,
        )
        .expect("TLS 1.2 client");
        assert_eq!(client.version_policy(), TlsVersionPolicy::Tls12Only);
        assert!(!client.start().expect("client hello").outbound.is_empty());
        assert!(!client.handshake_state().is_empty());
    }

    #[test]
    fn rejects_zero_limits_and_mismatched_credentials() {
        let (certificate, key) = credentials();
        assert!(matches!(
            OpenSslTlsClient::from_pem(
                &certificate.to_pem().expect("certificate PEM"),
                &key.private_key_to_pem_pkcs8().expect("private key PEM"),
                0,
            ),
            Err(OpenSslTlsError::InvalidLimit)
        ));

        let (_, other_key) = credentials();
        assert!(matches!(
            OpenSslTlsClient::from_pem(
                &certificate.to_pem().expect("certificate PEM"),
                &other_key
                    .private_key_to_pem_pkcs8()
                    .expect("other private key PEM"),
                64 * 1024,
            ),
            Err(OpenSslTlsError::Credentials(_))
        ));
    }

    #[test]
    fn rejects_oversized_input_before_open_ssl() {
        let (certificate, key) = credentials();
        let mut client = OpenSslTlsClient::from_pem(
            &certificate.to_pem().expect("certificate PEM"),
            &key.private_key_to_pem_pkcs8().expect("private key PEM"),
            4096,
        )
        .expect("client");
        assert!(matches!(client.feed(&[]), Err(OpenSslTlsError::NotStarted)));
        client.start().expect("start");
        assert!(matches!(
            client.start(),
            Err(OpenSslTlsError::AlreadyStarted)
        ));

        assert!(matches!(
            client.feed(&vec![0; 4097]),
            Err(OpenSslTlsError::InputTooLarge {
                size: 4097,
                maximum: 4096
            })
        ));
    }

    #[test]
    fn round_trips_application_data_both_directions_after_handshake() {
        let (mut client, mut server) = established_pair(64 * 1024);

        let ciphertext = client
            .encrypt_application_data(b"service discovery request")
            .expect("client encrypt");
        assert_ne!(ciphertext, b"service discovery request");
        server.get_mut().feed(&ciphertext);
        let mut received = vec![0_u8; 64];
        let count = server.ssl_read(&mut received).expect("server decrypt");
        assert_eq!(&received[..count], b"service discovery request");

        server
            .ssl_write(b"service discovery response")
            .expect("server encrypt");
        let server_ciphertext = server.get_mut().take_outbound();
        let plaintext = client
            .decrypt_application_data(&server_ciphertext)
            .expect("client decrypt");
        assert_eq!(plaintext, b"service discovery response");
    }

    #[test]
    fn decrypt_handles_a_tls_record_split_across_two_calls() {
        let (mut client, mut server) = established_pair(64 * 1024);

        server.ssl_write(b"fragmented").expect("server encrypt");
        let record = server.get_mut().take_outbound();
        assert!(record.len() > 4, "test needs a splittable record");
        let (first_half, second_half) = record.split_at(record.len() / 2);

        let first_result = client
            .decrypt_application_data(first_half)
            .expect("feed first half");
        assert!(
            first_result.is_empty(),
            "an incomplete record must not yield plaintext yet"
        );

        let second_result = client
            .decrypt_application_data(second_half)
            .expect("feed second half");
        assert_eq!(second_result, b"fragmented");
    }

    #[test]
    fn decrypt_returns_multiple_coalesced_records_from_one_call() {
        let (mut client, mut server) = established_pair(64 * 1024);

        server
            .ssl_write(b"first-record")
            .expect("server encrypt first");
        server
            .ssl_write(b"second-record")
            .expect("server encrypt second");
        let combined = server.get_mut().take_outbound();

        let plaintext = client
            .decrypt_application_data(&combined)
            .expect("client decrypt combined");
        assert_eq!(plaintext, b"first-recordsecond-record");
    }

    #[test]
    fn rejects_invalid_ciphertext() {
        let (mut client, _server) = established_pair(64 * 1024);
        let garbage = vec![0xff_u8; 64];
        assert!(matches!(
            client.decrypt_application_data(&garbage),
            Err(OpenSslTlsError::InvalidRecordData(_))
        ));
    }

    #[test]
    fn rejects_application_data_before_handshake_completion() {
        let (certificate, key) = credentials();
        let mut client = OpenSslTlsClient::from_pem(
            &certificate.to_pem().expect("certificate PEM"),
            &key.private_key_to_pem_pkcs8().expect("private key PEM"),
            64 * 1024,
        )
        .expect("client");

        assert!(matches!(
            client.encrypt_application_data(b"too early"),
            Err(OpenSslTlsError::HandshakeNotComplete)
        ));
        assert!(matches!(
            client.decrypt_application_data(b"too early"),
            Err(OpenSslTlsError::HandshakeNotComplete)
        ));

        client.start().expect("start");
        assert!(matches!(
            client.encrypt_application_data(b"still mid-handshake"),
            Err(OpenSslTlsError::HandshakeNotComplete)
        ));
    }

    #[test]
    fn rejects_oversized_application_data() {
        let (mut client, _server) = established_pair(4096);
        assert!(matches!(
            client.encrypt_application_data(&vec![0; 4097]),
            Err(OpenSslTlsError::InputTooLarge {
                size: 4097,
                maximum: 4096
            })
        ));
        assert!(matches!(
            client.decrypt_application_data(&vec![0; 4097]),
            Err(OpenSslTlsError::InputTooLarge {
                size: 4097,
                maximum: 4096
            })
        ));
    }

    #[test]
    fn reports_session_closed_after_peer_close_notify() {
        let (mut client, mut server) = established_pair(64 * 1024);

        let _ = server.shutdown();
        let close_notify = server.get_mut().take_outbound();
        assert!(!close_notify.is_empty(), "shutdown must emit close-notify");

        assert!(matches!(
            client.decrypt_application_data(&close_notify),
            Err(OpenSslTlsError::SessionClosed)
        ));
    }

    #[test]
    fn application_data_errors_never_contain_payload_bytes() {
        let (mut client, _server) = established_pair(64 * 1024);
        let secret_marker = "sk_live_super_secret_marker";
        let garbage = format!("{secret_marker}-not-a-valid-tls-record").into_bytes();

        let error = client
            .decrypt_application_data(&garbage)
            .expect_err("garbage must be rejected");
        assert!(
            !error.to_string().contains(secret_marker),
            "error text must not echo back input bytes: {error}"
        );
    }
}

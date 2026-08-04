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

        let certificate = X509::from_pem(certificate_pem)
            .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
        let private_key = PKey::<Private>::private_key_from_pem(private_key_pem)
            .map_err(|error| OpenSslTlsError::Credentials(error.to_string()))?;
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
}

#[cfg(test)]
mod tests {
    use openssl::asn1::Asn1Time;
    use openssl::bn::{BigNum, MsbOption};
    use openssl::hash::MessageDigest;
    use openssl::nid::Nid;
    use openssl::rsa::Rsa;
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
}

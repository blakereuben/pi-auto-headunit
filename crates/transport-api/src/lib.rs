//! Transport-neutral contracts for the documented Android Open Accessory transition.

use std::fmt;
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransportError {
    InvalidEndpoint(String),
    TimedOut,
    Closed,
    Io(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint(message) => write!(formatter, "invalid endpoint: {message}"),
            Self::TimedOut => formatter.write_str("transport operation timed out"),
            Self::Closed => formatter.write_str("transport closed"),
            Self::Io(message) => write!(formatter, "transport I/O: {message}"),
        }
    }
}

impl std::error::Error for TransportError {}

pub trait SessionTransport {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, TransportError>;
    fn send_all(&mut self, bytes: &[u8]) -> Result<(), TransportError>;
}

/// Bounded in-memory transport pair for deterministic scripted-peer tests.
///
/// Gated behind the `test-support` feature so it is only ever reachable from
/// test targets, never a release binary.
#[cfg(feature = "test-support")]
pub mod fake;

/// Stable-enough identity for one USB enumeration lifetime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsbDeviceId {
    pub bus: u8,
    pub address: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub port_path: Vec<u8>,
}

impl fmt::Display for UsbDeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{} {:04x}:{:04x}",
            self.bus, self.address, self.vendor_id, self.product_id
        )?;
        if !self.port_path.is_empty() {
            write!(f, " port=")?;
            for (index, port) in self.port_path.iter().enumerate() {
                if index > 0 {
                    write!(f, ".")?;
                }
                write!(f, "{port}")?;
            }
        }
        Ok(())
    }
}

/// Publicly documented AOA identification strings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AoaIdentification {
    pub manufacturer: String,
    pub model: String,
    pub description: String,
    pub version: String,
    pub uri: String,
    pub serial: String,
}

impl AoaIdentification {
    /// Development identity for the generic AOA proof. These values do not
    /// claim to be the undocumented Android Auto production identity.
    #[must_use]
    pub fn milestone_one() -> Self {
        Self {
            manufacturer: "Pi Auto Head Unit Project".into(),
            model: "AOA Milestone 1 Diagnostic".into(),
            description: "Documented Android Open Accessory transport test".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            uri: "https://github.com/OWNER/pi-auto-headunit".into(),
            serial: "development".into(),
        }
    }

    /// Android Auto compatibility strings with project-specific provenance.
    #[must_use]
    pub fn receiver_probe() -> Self {
        Self {
            manufacturer: "Android".into(),
            model: "Android Auto".into(),
            description: "Android Auto".into(),
            version: "2.0.1".into(),
            uri: "https://github.com/blakereuben/pi-auto-headunit".into(),
            serial: "pi-auto-headunit-development".into(),
        }
    }

    /// Compatibility identity from the owner-approved AASDK revision:
    /// these are AOA/USB accessory identification strings only (the same
    /// six fields any AOA host sends before `START_ACCESSORY`), not a TLS
    /// credential. Was first used, once, paired with a temporary
    /// project-generated TLS credential (the now-permanently-disabled
    /// `usb_generated_identity_probe` path) and rejected with Android Auto
    /// error 7 — a TLS-trust rejection fully explained by that untrusted
    /// credential, not by these strings (see
    /// `docs/protocol/tls-credential-policy.md`). Reused here, combined
    /// with a real operator-authorized credential loaded through
    /// `credential_store` (`usb_auth_discovery_probe`), to isolate this
    /// specific AOA-identity variable — this reuse does not touch, weaken,
    /// or reopen that permanent lockout. Real-hardware result (two trials):
    /// still Error 2 both times, and neither trial reached video `Start`
    /// (recent `receiver_probe()` trials had reached it) — inconclusive
    /// given this investigation's own well-documented run-to-run
    /// variability with identical code, but no evidence this identity
    /// helps either. `usb_auth_discovery_probe` has reverted to
    /// `receiver_probe()`; this preset is no longer called from any live
    /// probe path, kept only as a record of what was tried (see
    /// `docs/protocol/error-2-investigation.md`, "AOA identity
    /// experiment").
    #[must_use]
    pub fn aasdk_compatibility_probe() -> Self {
        Self {
            manufacturer: "Android".into(),
            model: "Android Auto".into(),
            description: "Android Auto".into(),
            version: "2.0.1".into(),
            uri: "https://f1xstudio.com".into(),
            serial: "HU-AAAAAA001".into(),
        }
    }

    pub fn validate(&self) -> Result<(), AoaError> {
        for (name, value, required) in [
            ("manufacturer", self.manufacturer.as_str(), true),
            ("model", self.model.as_str(), true),
            ("description", self.description.as_str(), false),
            ("version", self.version.as_str(), true),
            ("uri", self.uri.as_str(), false),
            ("serial", self.serial.as_str(), false),
        ] {
            let wire_len = value.len() + 1;
            if required && value.is_empty() {
                return Err(AoaError::InvalidIdentification(format!(
                    "{name} must not be empty"
                )));
            }
            if value.as_bytes().contains(&0) {
                return Err(AoaError::InvalidIdentification(format!(
                    "{name} contains a NUL byte"
                )));
            }
            if wire_len > 256 {
                return Err(AoaError::InvalidIdentification(format!(
                    "{name} exceeds the documented 256-byte AOA limit"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AoaState {
    Idle,
    CandidateSelected,
    QueryingAoaVersion,
    SendingIdentification,
    RequestingAccessoryMode,
    WaitingForReenumeration,
    OpeningAccessoryInterface,
    BulkTransportReady,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BulkTransportInfo {
    pub device: UsbDeviceId,
    pub interface_number: u8,
    pub bulk_in_endpoint: u8,
    pub bulk_out_endpoint: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AoaOutcome {
    pub protocol_version: u16,
    pub transport: BulkTransportInfo,
    pub transitions: Vec<AoaState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AoaError {
    PermissionDenied(String),
    Unsupported(String),
    Unplugged,
    TimedOut(AoaState),
    InvalidIdentification(String),
    Usb(String),
    Internal(String),
}

impl fmt::Display for AoaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied(message) => write!(f, "permission denied: {message}"),
            Self::Unsupported(message) => write!(f, "unsupported: {message}"),
            Self::Unplugged => write!(f, "USB device unplugged"),
            Self::TimedOut(state) => write!(f, "timed out in state {state:?}"),
            Self::InvalidIdentification(message) => {
                write!(f, "invalid AOA identification: {message}")
            }
            Self::Usb(message) => write!(f, "USB error: {message}"),
            Self::Internal(message) => write!(f, "internal error: {message}"),
        }
    }
}

impl std::error::Error for AoaError {}

pub trait AoaBackend {
    fn query_protocol(&mut self, device: &UsbDeviceId) -> Result<u16, AoaError>;
    fn send_identification(
        &mut self,
        device: &UsbDeviceId,
        identification: &AoaIdentification,
    ) -> Result<(), AoaError>;
    fn request_accessory_mode(&mut self, device: &UsbDeviceId) -> Result<(), AoaError>;
    fn wait_for_accessory(
        &mut self,
        original: &UsbDeviceId,
        timeout: Duration,
    ) -> Result<UsbDeviceId, AoaError>;
    fn open_bulk_transport(&mut self, device: &UsbDeviceId) -> Result<BulkTransportInfo, AoaError>;
    fn is_accessory_mode(&self, device: &UsbDeviceId) -> bool;
}

pub struct AoaMachine<B> {
    backend: B,
    reenumeration_timeout: Duration,
}

impl<B: AoaBackend> AoaMachine<B> {
    #[must_use]
    pub fn new(backend: B, reenumeration_timeout: Duration) -> Self {
        Self {
            backend,
            reenumeration_timeout,
        }
    }

    pub fn run(
        &mut self,
        candidate: UsbDeviceId,
        identification: &AoaIdentification,
    ) -> Result<AoaOutcome, AoaError> {
        identification.validate()?;
        let mut transitions = vec![AoaState::Idle, AoaState::CandidateSelected];

        let (protocol_version, accessory) = if self.backend.is_accessory_mode(&candidate) {
            (0, candidate)
        } else {
            transitions.push(AoaState::QueryingAoaVersion);
            let version = self.backend.query_protocol(&candidate)?;
            if version == 0 {
                return Err(AoaError::Unsupported(
                    "device returned AOA protocol version 0".into(),
                ));
            }

            transitions.push(AoaState::SendingIdentification);
            self.backend
                .send_identification(&candidate, identification)?;
            transitions.push(AoaState::RequestingAccessoryMode);
            self.backend.request_accessory_mode(&candidate)?;
            transitions.push(AoaState::WaitingForReenumeration);
            let accessory = self
                .backend
                .wait_for_accessory(&candidate, self.reenumeration_timeout)?;
            (version, accessory)
        };

        transitions.push(AoaState::OpeningAccessoryInterface);
        let transport = self.backend.open_bulk_transport(&accessory)?;
        transitions.push(AoaState::BulkTransportReady);
        transitions.push(AoaState::Closed);

        Ok(AoaOutcome {
            protocol_version,
            transport,
            transitions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeBackend {
        already_accessory: bool,
        fail_at: Option<AoaState>,
        calls: Vec<AoaState>,
    }

    impl FakeBackend {
        fn check(&mut self, state: AoaState) -> Result<(), AoaError> {
            self.calls.push(state);
            if self.fail_at == Some(state) {
                Err(AoaError::Unplugged)
            } else {
                Ok(())
            }
        }
    }

    impl AoaBackend for FakeBackend {
        fn query_protocol(&mut self, _: &UsbDeviceId) -> Result<u16, AoaError> {
            self.check(AoaState::QueryingAoaVersion)?;
            Ok(2)
        }

        fn send_identification(
            &mut self,
            _: &UsbDeviceId,
            _: &AoaIdentification,
        ) -> Result<(), AoaError> {
            self.check(AoaState::SendingIdentification)
        }

        fn request_accessory_mode(&mut self, _: &UsbDeviceId) -> Result<(), AoaError> {
            self.check(AoaState::RequestingAccessoryMode)
        }

        fn wait_for_accessory(
            &mut self,
            original: &UsbDeviceId,
            _: Duration,
        ) -> Result<UsbDeviceId, AoaError> {
            self.check(AoaState::WaitingForReenumeration)?;
            let mut accessory = original.clone();
            accessory.vendor_id = 0x18d1;
            accessory.product_id = 0x2d00;
            Ok(accessory)
        }

        fn open_bulk_transport(
            &mut self,
            device: &UsbDeviceId,
        ) -> Result<BulkTransportInfo, AoaError> {
            self.check(AoaState::OpeningAccessoryInterface)?;
            Ok(BulkTransportInfo {
                device: device.clone(),
                interface_number: 0,
                bulk_in_endpoint: 0x81,
                bulk_out_endpoint: 0x01,
            })
        }

        fn is_accessory_mode(&self, _: &UsbDeviceId) -> bool {
            self.already_accessory
        }
    }

    fn device() -> UsbDeviceId {
        UsbDeviceId {
            bus: 1,
            address: 2,
            vendor_id: 0x1234,
            product_id: 0x5678,
            port_path: vec![1, 3],
        }
    }

    #[test]
    fn completes_documented_transition() {
        let mut machine = AoaMachine::new(FakeBackend::default(), Duration::from_secs(1));
        let outcome = machine
            .run(device(), &AoaIdentification::milestone_one())
            .expect("transition should complete");
        assert_eq!(outcome.protocol_version, 2);
        assert_eq!(outcome.transport.device.vendor_id, 0x18d1);
        assert_eq!(outcome.transitions.last(), Some(&AoaState::Closed));
    }

    #[test]
    fn opens_already_accessory_mode_device_without_control_sequence() {
        let backend = FakeBackend {
            already_accessory: true,
            ..FakeBackend::default()
        };
        let mut machine = AoaMachine::new(backend, Duration::from_secs(1));
        let outcome = machine
            .run(device(), &AoaIdentification::milestone_one())
            .expect("already-accessory device should open");
        assert_eq!(outcome.protocol_version, 0);
        assert!(
            !outcome
                .transitions
                .contains(&AoaState::SendingIdentification)
        );
    }

    #[test]
    fn propagates_unplug_at_every_backend_state() {
        for state in [
            AoaState::QueryingAoaVersion,
            AoaState::SendingIdentification,
            AoaState::RequestingAccessoryMode,
            AoaState::WaitingForReenumeration,
            AoaState::OpeningAccessoryInterface,
        ] {
            let backend = FakeBackend {
                fail_at: Some(state),
                ..FakeBackend::default()
            };
            let mut machine = AoaMachine::new(backend, Duration::from_secs(1));
            assert_eq!(
                machine.run(device(), &AoaIdentification::milestone_one()),
                Err(AoaError::Unplugged),
                "failed at {state:?}"
            );
        }
    }

    #[test]
    fn rejects_oversized_identification_before_usb_io() {
        let mut identification = AoaIdentification::milestone_one();
        identification.model = "x".repeat(256);
        let mut machine = AoaMachine::new(FakeBackend::default(), Duration::from_secs(1));
        assert!(matches!(
            machine.run(device(), &identification),
            Err(AoaError::InvalidIdentification(_))
        ));
    }

    #[test]
    fn aasdk_probe_identity_is_pinned() {
        let identity = AoaIdentification::aasdk_compatibility_probe();
        assert_eq!(identity.manufacturer, "Android");
        assert_eq!(identity.model, "Android Auto");
        assert_eq!(identity.description, "Android Auto");
        assert_eq!(identity.version, "2.0.1");
        assert_eq!(identity.uri, "https://f1xstudio.com");
        assert_eq!(identity.serial, "HU-AAAAAA001");
    }

    #[test]
    fn receiver_probe_keeps_project_specific_provenance() {
        let identity = AoaIdentification::receiver_probe();
        identity.validate().expect("valid AOA identification");
        assert_eq!(identity.model, "Android Auto");
        assert!(identity.uri.contains("blakereuben/pi-auto-headunit"));
        assert_eq!(identity.serial, "pi-auto-headunit-development");
    }
}

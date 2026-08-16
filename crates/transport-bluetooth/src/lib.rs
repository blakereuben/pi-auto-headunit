//! Bluetooth RFCOMM transport for the Android Auto Wireless (`aaw`)
//! cold-start bootstrap exchange (`protocol_aap::{encode_aaw_message,
//! decode_aaw_message, ...}`) — see `docs/protocol/
//! wireless-source-assessment.md` for the full citation trail.
//!
//! This is the one place in the codebase using an async runtime (`tokio`,
//! required by the `bluer` crate's `rfcomm::Stream`, itself
//! `tokio::io::AsyncRead`/`AsyncWrite`) — every other transport in this
//! workspace (`transport-usb`, `transport-tcp`) is plain synchronous I/O.
//! `BluetoothTransport` owns a dedicated single-purpose
//! `tokio::runtime::Runtime` and drives it via `Runtime::block_on` from
//! ordinary synchronous methods, exactly the pattern `tokio` documents
//! for calling async code from a sync caller that isn't itself running
//! inside another async task — no background thread or channel plumbing
//! needed, unlike this session's earlier GTK4 main-thread bridges (whose
//! problem — bridging *out of* an async/GTK main loop from other threads
//! — is different from this one, bridging *into* one from a single
//! ordinary sync caller).
//!
//! Bluetooth UUIDs: `AA_WIRELESS_PROFILE_UUID` confirmed by reading
//! `manio/aa-proxy-rs`'s actual Rust source directly (`src/bluetooth.rs`,
//! GPL-2.0-only — cited for this one fact only, no code reproduced).
//! `HANDSFREE_AG_PROFILE_UUID` is a public Bluetooth SIG-assigned number
//! (Hands-Free Profile, Audio Gateway role) — no project-specific
//! provenance needed; advertising it is `f-io/LIVI`'s own documented
//! finding (`docs/protocol/wireless-source-assessment.md`) that a phone
//! only starts a wireless Android Auto session over an HFP-advertising
//! adapter.
//!
//! **Highest-uncertainty part of the whole wireless bootstrap plan**:
//! whether this SDP registration shape is actually what a real phone
//! expects is not confirmed by any source reviewed so far — attempt
//! empirically, expect possible failure, matching this project's
//! `docs/protocol/error-2-investigation.md` precedent for reverse-
//! engineered behaviour.

#![cfg(target_os = "linux")]

use std::fmt;
use std::time::Duration;

use bluer::rfcomm::{Profile, Role, Stream};
use bluer::{Session, Uuid};
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use transport_api::{SessionTransport, TransportError};

/// Android-Auto-Wireless RFCOMM profile UUID. Source: `manio/aa-proxy-rs`,
/// `src/bluetooth.rs`, `AAWG_PROFILE_UUID` — read directly, GPL-2.0-only,
/// cited as an operational fact only (see module doc comment).
pub const AA_WIRELESS_PROFILE_UUID: &str = "4de17a00-52cb-11e6-bdf4-0800200c9a66";

/// Standard Bluetooth SIG Hands-Free Profile, Audio Gateway role. Public
/// assigned number, not project-specific.
pub const HANDSFREE_AG_PROFILE_UUID: &str = "0000111f-0000-1000-8000-00805f9b34fb";

/// Fixed RFCOMM channel advertised in the AA-Wireless SDP record. Chosen
/// outside the 0-8 range this Pi's other already-registered Bluetooth
/// profiles (`PipeWire`'s audio/HFP stack) were observed using in a real
/// `btmon` capture; RFCOMM channels are 1-30 and this is otherwise an
/// arbitrary, project-chosen value (no source specifies one).
const AA_WIRELESS_SDP_CHANNEL: u16 = 22;

#[derive(Debug)]
pub enum BluetoothError {
    Session(String),
    Adapter(String),
    ProfileRegistration(String),
    NoIncomingConnection,
    Accept(String),
    Io(String),
}

impl fmt::Display for BluetoothError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(message) => write!(formatter, "bluetooth session error: {message}"),
            Self::Adapter(message) => write!(formatter, "bluetooth adapter error: {message}"),
            Self::ProfileRegistration(message) => {
                write!(
                    formatter,
                    "bluetooth profile registration failed: {message}"
                )
            }
            Self::NoIncomingConnection => {
                formatter.write_str("no incoming bluetooth rfcomm connection within the timeout")
            }
            Self::Accept(message) => {
                write!(
                    formatter,
                    "failed to accept bluetooth rfcomm connection: {message}"
                )
            }
            Self::Io(message) => write!(formatter, "bluetooth rfcomm I/O error: {message}"),
        }
    }
}

impl std::error::Error for BluetoothError {}

/// A live, already-connected Bluetooth RFCOMM session, ready to carry the
/// `aaw` bootstrap exchange. Construct via
/// [`accept_wireless_bootstrap_connection`].
pub struct BluetoothTransport {
    runtime: tokio::runtime::Runtime,
    stream: Stream,
    io_timeout: Duration,
}

impl SessionTransport for BluetoothTransport {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, TransportError> {
        let Self {
            runtime,
            stream,
            io_timeout,
        } = self;
        runtime.block_on(async {
            match tokio::time::timeout(*io_timeout, stream.read(buffer)).await {
                Ok(Ok(0)) => Err(TransportError::Closed),
                Ok(Ok(size)) => Ok(size),
                Ok(Err(error)) => Err(TransportError::Io(error.to_string())),
                Err(_) => Err(TransportError::TimedOut),
            }
        })
    }

    fn send_all(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        let Self {
            runtime,
            stream,
            io_timeout,
        } = self;
        runtime.block_on(async {
            match tokio::time::timeout(*io_timeout, stream.write_all(bytes)).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(TransportError::Io(error.to_string())),
                Err(_) => Err(TransportError::TimedOut),
            }
        })
    }
}

/// Registers this device as Bluetooth-discoverable under both the
/// Android-Auto-Wireless and Handsfree-AG profile UUIDs, waits (bounded
/// by `accept_timeout`) for a phone to connect over the AA-Wireless
/// RFCOMM channel, and returns a ready-to-use [`BluetoothTransport`] once
/// it does. `io_timeout` bounds every later `receive`/`send_all` call on
/// the returned transport.
///
/// # Panics
///
/// Never in practice: the two UUID literals this function parses
/// (`AA_WIRELESS_PROFILE_UUID`/`HANDSFREE_AG_PROFILE_UUID`) are fixed,
/// valid constants confirmed at compile time by this crate's own tests.
pub fn accept_wireless_bootstrap_connection(
    accept_timeout: Duration,
    io_timeout: Duration,
) -> Result<BluetoothTransport, BluetoothError> {
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|error| BluetoothError::Session(error.to_string()))?;
    let stream = runtime.block_on(async {
        let session = Session::new()
            .await
            .map_err(|error| BluetoothError::Session(error.to_string()))?;
        let adapter = session
            .default_adapter()
            .await
            .map_err(|error| BluetoothError::Adapter(error.to_string()))?;
        adapter
            .set_powered(true)
            .await
            .map_err(|error| BluetoothError::Adapter(error.to_string()))?;
        adapter
            .set_pairable(true)
            .await
            .map_err(|error| BluetoothError::Adapter(error.to_string()))?;
        adapter
            .set_discoverable(true)
            .await
            .map_err(|error| BluetoothError::Adapter(error.to_string()))?;

        let aa_wireless_uuid = Uuid::parse_str(AA_WIRELESS_PROFILE_UUID)
            .expect("AA_WIRELESS_PROFILE_UUID is a valid, fixed UUID literal");
        let handsfree_uuid = Uuid::parse_str(HANDSFREE_AG_PROFILE_UUID)
            .expect("HANDSFREE_AG_PROFILE_UUID is a valid, fixed UUID literal");

        let mut aa_wireless_handle = session
            .register_profile(Profile {
                uuid: aa_wireless_uuid,
                name: Some("Android Auto Wireless".to_string()),
                // Without an explicit channel, BlueZ has no RFCOMM channel
                // to put in this record's ProtocolDescriptorList, so a
                // direct SDP attribute query for our UUID comes back with
                // an empty attribute list (confirmed via a real `btmon`
                // HCI capture: the phone's `Service Search Attribute
                // Request` for this exact UUID got a 2-byte/empty
                // response and it gave up, repeatedly, every ~5-6s).
                // AA_WIRELESS_SDP_CHANNEL is a fixed, arbitrarily-chosen
                // RFCOMM channel number outside the range already used by
                // this adapter's other registered profiles.
                channel: Some(AA_WIRELESS_SDP_CHANNEL),
                role: Some(Role::Server),
                require_authentication: Some(false),
                require_authorization: Some(false),
                auto_connect: Some(true),
                ..Default::default()
            })
            .await
            .map_err(|error| BluetoothError::ProfileRegistration(error.to_string()))?;
        // Best-effort, not fatal: real-hardware trial found this UUID
        // already registered on a real Pi desktop session (`org.bluez`
        // rejected it: "UUID already registered") — almost certainly
        // BlueZ's own PipeWire-provided audio/HFP support, already
        // advertising this profile system-wide. This directly confirms,
        // rather than contradicts, LIVI's own finding cited in this
        // file's module doc comment ("PipeWire is what puts HFP into the
        // adapter's service record") — on a system where that's already
        // true, this project registering its own copy would be redundant
        // even if it succeeded, not required. Kept as an attempt (some
        // deployments may not have it already) but its failure never
        // blocks the real bootstrap connection below.
        let _handsfree_handle = session
            .register_profile(Profile {
                uuid: handsfree_uuid,
                role: Some(Role::Server),
                require_authentication: Some(false),
                require_authorization: Some(false),
                ..Default::default()
            })
            .await
            .inspect_err(|error| {
                eprintln!(
                    "wireless_bootstrap_state=handsfree_profile_registration_skipped error={error}"
                );
            })
            .ok();

        let request = tokio::time::timeout(accept_timeout, aa_wireless_handle.next())
            .await
            .map_err(|_| BluetoothError::NoIncomingConnection)?
            .ok_or(BluetoothError::NoIncomingConnection)?;
        request
            .accept()
            .map_err(|error| BluetoothError::Accept(error.to_string()))
    })?;
    Ok(BluetoothTransport {
        runtime,
        stream,
        io_timeout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_uuid_constants_parse() {
        assert!(Uuid::parse_str(AA_WIRELESS_PROFILE_UUID).is_ok());
        assert!(Uuid::parse_str(HANDSFREE_AG_PROFILE_UUID).is_ok());
    }
}

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
//!
//! [`accept_wireless_bootstrap_connection`] also now actively reconnects
//! any already-OS-paired device before falling back to its original
//! passive advertise-and-wait behaviour — real production head units
//! don't just wait for the phone to notice them, they proactively
//! reconnect a known phone the moment they power up, which is what
//! prompts the phone's own Android Auto app to offer wireless
//! projection. Not yet confirmed on real hardware whether this actually
//! has that effect.

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

/// Bound on each individual `Device::connect_profile` call in the
/// active-reconnect loop below. Real-hardware finding, 2026-08-23: with a
/// phone genuinely paired (`Paired: yes`) but not currently connected and
/// in range, this D-Bus call was observed to simply never return —
/// blocking well past a minute with no error and no progress, silently
/// wedging the entire wireless bootstrap (nothing past this point, not
/// even the profile registrations or the passive advertise-and-wait
/// fallback, could ever run). The loop's own doc comment already
/// documents this as meant to be "best-effort... slow to respond just
/// means this has no visible effect" — that promise only holds with an
/// actual bound, since without one a genuine hang here is
/// indistinguishable from every other kind of "AA screen stays black"
/// hang this project has spent real hardware time chasing. Short
/// relative to `accept_timeout` (the passive fallback's own, much longer
/// wait): this is a quick nudge, not the real wait.
const ACTIVE_RECONNECT_TIMEOUT: Duration = Duration::from_secs(10);

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
/// the returned transport. `auto_connect_paired_devices` gates the
/// active-reconnect step below (`HeadUnitSettings::wireless_bluetooth_auto_connect`,
/// an operator-facing setting added 2026-08-23) — `false` skips it
/// entirely and falls straight to the passive advertise-and-wait flow,
/// this function's original behaviour.
///
/// # Panics
///
/// Never in practice: the two UUID literals this function parses
/// (`AA_WIRELESS_PROFILE_UUID`/`HANDSFREE_AG_PROFILE_UUID`) are fixed,
/// valid constants confirmed at compile time by this crate's own tests.
pub fn accept_wireless_bootstrap_connection(
    accept_timeout: Duration,
    io_timeout: Duration,
    auto_connect_paired_devices: bool,
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

        // Actively reconnect any already-paired device, 2026-08-23 —
        // Blake's explicit product requirement, matching how real
        // production wireless-AA head units behave: they don't just sit
        // passively advertising and waiting for the phone to notice them
        // (the SDP-advertise-and-wait flow below, which is all this used
        // to do); once a phone is OS-level paired, the head unit
        // proactively reconnects it the moment it powers up, and it's
        // that reconnection event the phone's own Android Auto app
        // watches for to offer wireless projection. Best-effort and
        // non-fatal per device (a phone that's out of range, already
        // connected, or slow to respond just means this has no visible
        // effect — the existing passive flow below still runs exactly as
        // before as a fallback).
        //
        // Real-hardware finding, first trial: the generic `Device::connect()`
        // (bluer's wrapper around BlueZ's own bearer-auto-selecting
        // `Connect` D-Bus method) failed outright with
        // `le-connection-abort-by-local` — BlueZ picked the LE bearer for
        // this dual-mode phone (its own documented tie-break order favors
        // "latest seen bearer", not always BR/EDR) and the phone wasn't
        // actively LE-connectable at that moment. Switched to
        // `Device::connect_profile(&handsfree_uuid)` instead: Handsfree
        // AG is inherently a classic BR/EDR-only profile (there is no LE
        // variant), so this can never hit the same LE-bearer failure, and
        // it's the exact standard reconnection a real car's Bluetooth
        // stack performs — HFP reconnecting is what a phone's own OS/AA
        // app watches for as the "car is back" signal, matching this
        // whole feature's premise. Standard, publicly-specified Bluetooth
        // SIG behaviour, not an Android-Auto-specific protocol, so no
        // AASDK/LIVI source citation is needed — matches
        // `docs/protocol/wireless-source-assessment.md`'s own reasoning
        // for why the underlying SDP/pairing handshake itself doesn't
        // need one either.
        if auto_connect_paired_devices {
            for address in adapter.device_addresses().await.unwrap_or_default() {
                let Ok(device) = adapter.device(address) else {
                    continue;
                };
                if device.is_paired().await.unwrap_or(false) {
                    match tokio::time::timeout(
                        ACTIVE_RECONNECT_TIMEOUT,
                        device.connect_profile(&handsfree_uuid),
                    )
                    .await
                    {
                        Ok(Ok(())) => {
                            println!("wireless_bootstrap_state=paired_device_reconnected address={address}");
                        }
                        Ok(Err(error)) => {
                            eprintln!(
                                "wireless_bootstrap_state=paired_device_reconnect_failed address={address} error={error}"
                            );
                        }
                        Err(_elapsed) => {
                            eprintln!(
                                "wireless_bootstrap_state=paired_device_reconnect_failed address={address} error=timed out after {ACTIVE_RECONNECT_TIMEOUT:?}"
                            );
                        }
                    }
                }
            }
        }

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

//! Coarse connection-state reporting, layered on top of the many fine-
//! grained `probe_state=`/`supervisor_*` lines `auth_discovery_probe` and
//! `session_supervisor` already emit. Answers
//! `MILESTONE_CHECKLIST.md`'s "Show clear ready, connecting, connected,
//! consent, and error states" item. The checklist's "consent" state is
//! deliberately not represented: Android Auto's consent screen is shown
//! and answered entirely on the phone, and this project has no protocol
//! visibility into it (confirmed on the phone screen throughout the
//! `error-2-investigation.md` work, never inferred from any head-unit-side
//! message).
//!
//! - `Ready`: no phone claimed yet (before device discovery, or between
//!   `session-supervisor` cycles while waiting for the phone to
//!   reconnect).
//! - `Connecting`: a specific device has been claimed and the
//!   AOA/version/TLS/service-discovery/channel-setup handshake is under
//!   way.
//! - `Connected`: channel setup has completed (`auth_discovery_probe`'s
//!   existing `probe_state=channel_setup_complete` — video `Start` plus
//!   the input channel open).
//! - `Error`: the current attempt ended in failure, whether the command
//!   stops immediately or `session-supervisor` is about to retry.
//!
//! This is diagnostics-CLI-only reporting (structured stdout lines), not
//! the `ui-model`/`app` crates `ARCHITECTURE.md` describes for a real
//! on-screen UI — those don't exist yet, since the GTK/GStreamer on-device
//! spike `ARCHITECTURE.md` §4 requires hasn't run.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionState {
    Ready,
    Connecting,
    Connected,
    Error,
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ready => "ready",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Error => "error",
        })
    }
}

pub(crate) fn report(state: ConnectionState) {
    println!("connection_state={state}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_matches_wire_labels() {
        assert_eq!(ConnectionState::Ready.to_string(), "ready");
        assert_eq!(ConnectionState::Connecting.to_string(), "connecting");
        assert_eq!(ConnectionState::Connected.to_string(), "connected");
        assert_eq!(ConnectionState::Error.to_string(), "error");
    }
}

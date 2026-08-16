//! Automatic reconnect loop wrapping the one-shot `auth-discovery-probe`
//! flow (`usb_auth_discovery_probe` in `main.rs`, `auth_discovery_probe`'s
//! own module doc comment for the protocol session itself). The existing
//! `usb auth-discovery-probe` command stays a single attempt: it exits
//! non-zero on any failure, and proving recovery has so far meant an
//! operator manually replugging the phone and re-running it by hand. A
//! real head unit needs to notice a disconnect and retry on its own —
//! `usb session-supervisor` is that outer loop.
//!
//! Split into a pure, unit-tested retry policy (`is_retryable`,
//! `supervise`) and a concrete per-cycle attempt (`SupervisedSession`) that
//! touches real USB hardware and is intentionally left untested here,
//! matching this crate's existing convention: `crates/transport-usb`'s
//! `LibUsbAoaBackend` and every other `usb *` subcommand in `main.rs` have
//! no unit tests either, since they can't run without a real device.
//! Real-hardware validation is a manual multi-cycle unplug/replug trial
//! instead (see `docs/protocol/` trial-record conventions used elsewhere
//! in this project).
//!
//! Each cycle's `attempt` also reports the coarse `connection_state`
//! (`crate::connection_state`) alongside these `probe_state=`/
//! `supervisor_*` lines: `Ready` before device resolution, `Connecting`
//! once the device is found, and `Error` from `supervise` on either
//! outcome — retrying or fatal.

use std::path::Path;
use std::time::Duration;

use transport_api::{AoaError, AoaIdentification, AoaMachine, TransportError, UsbDeviceId};

use crate::CliError;
use crate::cancellation::CancellationFlag;

/// How long `wait_for_reconnect` waits for the phone to reappear at the
/// same physical port after a cycle ends in a retryable failure. A guess,
/// not yet tuned against a real replug; long enough for a human to
/// physically unplug and replug, short enough that `supervisor_cycle_result
/// outcome=retrying` log lines stay reasonably frequent rather than one
/// long silent block.
const REDISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long each cycle's own AOA transition is allowed, matching
/// `usb_auth_discovery_probe`'s existing `PROBE_TIMEOUT`.
const AOA_TRANSITION_TIMEOUT: Duration = Duration::from_secs(10);
/// Pause between cycles after a retryable failure, before attempting again.
/// Not applied after a successful cycle. A fixed backoff, not exponential —
/// simplest option that still avoids spinning the CPU while the phone is
/// genuinely gone.
const RETRY_BACKOFF: Duration = Duration::from_millis(500);
/// How long the physical-replug popup (`replug_prompt::show_until_replugged`)
/// waits for the operator to actually unplug and replug the phone.
/// Generous — this is a human-paced wait, not a protocol timeout — but
/// still bounded, matching this project's "never wait forever silently"
/// discipline.
const PHYSICAL_REPLUG_TIMEOUT: Duration = Duration::from_secs(600);

/// Classifies a `CliError` from one supervised cycle as worth retrying
/// (the phone/session didn't finish as hoped — unplug, timeout, transient
/// USB jitter, a protocol-level failure) or fatal (a static host/config
/// problem retrying can't fix). Deliberately exhaustive, no wildcard arm:
/// a future new `CliError`/`AoaError` variant must be classified here
/// explicitly rather than silently defaulting either way.
const fn is_retryable(error: &CliError) -> bool {
    match error {
        // The phone/session didn't finish as hoped — unplug, timeout,
        // transient USB jitter, or a protocol-level failure. Worth
        // retrying.
        CliError::Aoa(AoaError::Unplugged | AoaError::TimedOut(_) | AoaError::Usb(_))
        | CliError::Transport(
            TransportError::Closed | TransportError::TimedOut | TransportError::Io(_),
        )
        | CliError::Protocol(_) => true,

        // Static host/config problems, a real device capability limit, or
        // an operator-requested stop — retrying changes nothing, and a
        // Ctrl-C during the supervisor loop should stop the whole loop,
        // not just the current cycle.
        CliError::Aoa(
            AoaError::PermissionDenied(_)
            | AoaError::Unsupported(_)
            | AoaError::InvalidIdentification(_)
            | AoaError::Internal(_),
        )
        | CliError::Transport(TransportError::InvalidEndpoint(_))
        | CliError::Usage(_)
        | CliError::UnsupportedPlatform
        | CliError::Io(_)
        | CliError::Media(_)
        | CliError::Credentials(_)
        | CliError::Cancelled => false,
    }
}

/// Runs `attempt` in a loop, retrying on any `is_retryable` failure
/// (sleeping `retry_backoff` first) and stopping immediately on a fatal
/// one. A successful cycle does **not** stop the loop either — each
/// `attempt()` is only a single bounded diagnostic session, not a full
/// drive, so the supervisor keeps watching for the next connection,
/// exactly like a real appliance should. `max_cycles`, when set, bounds
/// the number of *attempts* for testing/soak use (mirroring `usb soak
/// --cycles`'s "run N cycles then report" shape) — reaching it always
/// returns `Ok(())`, even if the final cycle(s) were retries, since it
/// isn't an assertion that every cycle succeeded.
fn supervise<F>(
    mut attempt: F,
    max_cycles: Option<u32>,
    retry_backoff: Duration,
) -> Result<(), CliError>
where
    F: FnMut(u32) -> Result<(), CliError>,
{
    let mut cycle: u32 = 0;
    loop {
        cycle += 1;
        println!("probe_state=supervisor_cycle_start cycle={cycle}");
        match attempt(cycle) {
            Ok(()) => println!("probe_state=supervisor_cycle_result cycle={cycle} outcome=ok"),
            Err(error) if is_retryable(&error) => {
                println!(
                    "probe_state=supervisor_cycle_result cycle={cycle} outcome=retrying reason={error}"
                );
                crate::connection_state::report(crate::connection_state::ConnectionState::Error);
                std::thread::sleep(retry_backoff);
            }
            Err(error) => {
                println!(
                    "probe_state=supervisor_cycle_result cycle={cycle} outcome=fatal reason={error}"
                );
                crate::connection_state::report(crate::connection_state::ConnectionState::Error);
                return Err(error);
            }
        }
        if max_cycles.is_some_and(|limit| cycle >= limit) {
            println!("probe_state=supervisor_complete cycles={cycle}");
            return Ok(());
        }
    }
}

/// One supervised phone connection. `last_known` starts `None` (the phone
/// must already be present at the originally-selected `bus:address` for
/// the first cycle, same fail-fast requirement every other `usb *`
/// subcommand already has) and is then used to re-find the same physical
/// device (`wait_for_reconnect`, matched by USB port, not bus:address —
/// the OS reassigns `address` on every reconnect) for every subsequent
/// cycle.
struct SupervisedSession {
    selector_bus: u8,
    selector_address: u8,
    tls12_compatibility: bool,
    last_known: Option<UsbDeviceId>,
    cancel: CancellationFlag,
    /// How many retryable failures have happened in a row, reset to 0 on
    /// any successful cycle (`run()`'s wrapper around `attempt`, based on
    /// `is_retryable`). Drives `resolve_device`'s escalation: 0 = normal
    /// reconnect, 1 = try a software `soft_reset` first (Blake's explicit
    /// instruction, 2026-08-16), 2+ = ask for a physical replug via
    /// `replug_prompt`.
    consecutive_failures: u32,
}

impl SupervisedSession {
    /// Mirrors `usb_auth_discovery_probe`'s body almost exactly (device
    /// discovery, AOA transition, session transport, protocol session) —
    /// see that function's doc comment in `main.rs` for the parts this
    /// duplicates. Credentials are reloaded from disk on every cycle,
    /// deliberately: `credential_store::CredentialMaterial` has no
    /// `Clone`/`Debug` and zeroizes its buffers on `Drop`, so caching it
    /// across cycles would fight the type rather than reuse it, and a
    /// fresh load is cheap (only happens on reconnect events) and picks up
    /// a credential rotation without restarting the supervisor.
    fn attempt(&mut self, cycle: u32) -> Result<(), CliError> {
        crate::connection_state::report(crate::connection_state::ConnectionState::Ready);
        let paths = credential_store::CredentialPaths::from(
            credential_store::load_config(Path::new("/etc/aa-headunit/config.toml"))
                .map_err(|error| CliError::Credentials(error.to_string()))?,
        );
        let credentials = credential_store::load_credentials(&paths, true)
            .map_err(|error| CliError::Credentials(error.to_string()))?;

        let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
        let candidate = self.resolve_device(&backend, cycle)?;
        println!("probe_state=supervisor_device_resolved cycle={cycle} device={candidate}");
        crate::connection_state::report(crate::connection_state::ConnectionState::Connecting);
        self.last_known = Some(candidate.clone());

        let mut aoa = AoaMachine::new(backend, AOA_TRANSITION_TIMEOUT);
        let outcome = aoa
            .run(candidate, &AoaIdentification::receiver_probe())
            .map_err(CliError::Aoa)?;
        let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
        let mut transport = backend
            .open_claimed_session_transport(&outcome.transport.device)
            .map_err(CliError::Aoa)?;
        crate::auth_discovery_probe::run(
            &mut transport,
            self.tls12_compatibility,
            credentials.material,
            crate::auth_discovery_probe::VideoRenderTarget::Wayland,
            &self.cancel,
        )
    }

    /// Resolves this cycle's device, escalating recovery based on
    /// `self.consecutive_failures` (see that field's doc comment). Split
    /// out of `attempt` purely to keep it under `clippy::too_many_lines`.
    fn resolve_device(
        &self,
        backend: &transport_usb::LibUsbAoaBackend,
        cycle: u32,
    ) -> Result<UsbDeviceId, CliError> {
        let Some(previous) = &self.last_known else {
            return backend
                .list_devices()
                .map_err(CliError::Aoa)?
                .into_iter()
                .find(|device| {
                    device.bus == self.selector_bus && device.address == self.selector_address
                })
                .ok_or(CliError::Aoa(AoaError::Unplugged));
        };
        match self.consecutive_failures {
            0 => backend
                .wait_for_reconnect(previous, REDISCOVERY_TIMEOUT)
                .map_err(CliError::Aoa),
            1 => {
                println!("probe_state=supervisor_soft_reset_attempt cycle={cycle}");
                match backend.soft_reset(previous) {
                    Ok(()) => {
                        println!(
                            "probe_state=supervisor_soft_reset_result cycle={cycle} outcome=ok"
                        );
                    }
                    Err(error) => println!(
                        "probe_state=supervisor_soft_reset_result cycle={cycle} outcome=failed reason={error}"
                    ),
                }
                backend
                    .wait_for_reconnect(previous, REDISCOVERY_TIMEOUT)
                    .map_err(CliError::Aoa)
            }
            _ => {
                println!("probe_state=supervisor_physical_replug_requested cycle={cycle}");
                let previous = previous.clone();
                let wait_backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
                let candidate = crate::replug_prompt::show_until_replugged(move || {
                    wait_backend.wait_for_physical_replug(&previous, PHYSICAL_REPLUG_TIMEOUT)
                })
                .map_err(CliError::Aoa)?;
                println!("probe_state=supervisor_physical_replug_confirmed cycle={cycle}");
                Ok(candidate)
            }
        }
    }
}

/// Entry point for `usb session-supervisor --device <bus:address>
/// --allow-live-aap [--tls12-compat] [--max-cycles N]`. `selector` is
/// parsed once, up front, and used only for the very first cycle's device
/// lookup — every later cycle re-discovers the phone by USB port instead
/// (see `SupervisedSession`'s doc comment).
pub(crate) fn run(
    selector: &str,
    tls12_compatibility: bool,
    max_cycles: Option<u32>,
) -> Result<(), CliError> {
    let (bus, address) = transport_usb::parse_bus_address(selector).map_err(CliError::Aoa)?;
    println!("probe_authorization=operator_confirmed");
    println!("probe_payload_logging=disabled");
    println!("probe_state=supervisor_started device={selector}");
    let cancel = crate::cancellation::install_ctrlc_handler()?;
    let mut session = SupervisedSession {
        selector_bus: bus,
        selector_address: address,
        tls12_compatibility,
        last_known: None,
        cancel,
        consecutive_failures: 0,
    };
    supervise(
        |cycle| {
            let result = session.attempt(cycle);
            match &result {
                Ok(()) => session.consecutive_failures = 0,
                Err(error) if is_retryable(error) => session.consecutive_failures += 1,
                Err(_) => {}
            }
            result
        },
        max_cycles,
        RETRY_BACKOFF,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_errors_are_classified_correctly() {
        assert!(is_retryable(&CliError::Aoa(AoaError::Unplugged)));
        assert!(is_retryable(&CliError::Aoa(AoaError::TimedOut(
            transport_api::AoaState::WaitingForReenumeration
        ))));
        assert!(is_retryable(&CliError::Aoa(AoaError::Usb("jitter".into()))));
        assert!(is_retryable(&CliError::Transport(TransportError::Closed)));
        assert!(is_retryable(&CliError::Transport(TransportError::TimedOut)));
        assert!(is_retryable(&CliError::Transport(TransportError::Io(
            "reset".into()
        ))));
        assert!(is_retryable(&CliError::Protocol("boom".into())));
    }

    #[test]
    fn fatal_errors_are_classified_correctly() {
        assert!(!is_retryable(&CliError::Aoa(AoaError::PermissionDenied(
            "udev".into()
        ))));
        assert!(!is_retryable(&CliError::Aoa(AoaError::Unsupported(
            "aoa v0".into()
        ))));
        assert!(!is_retryable(&CliError::Aoa(
            AoaError::InvalidIdentification("bad".into())
        )));
        assert!(!is_retryable(&CliError::Aoa(AoaError::Internal(
            "bad selector".into()
        ))));
        assert!(!is_retryable(&CliError::Transport(
            TransportError::InvalidEndpoint("bad".into())
        )));
        assert!(!is_retryable(&CliError::Usage("bad flag".into())));
        assert!(!is_retryable(&CliError::UnsupportedPlatform));
        assert!(!is_retryable(&CliError::Io(std::io::Error::other("io"))));
        assert!(!is_retryable(&CliError::Media("gst".into())));
        assert!(!is_retryable(&CliError::Credentials("missing".into())));
        assert!(!is_retryable(&CliError::Cancelled));
    }

    fn scripted(results: Vec<Result<(), CliError>>) -> impl FnMut(u32) -> Result<(), CliError> {
        let mut results = results.into_iter();
        move |_cycle| results.next().expect("no more scripted results")
    }

    #[test]
    fn fatal_error_stops_immediately() {
        let mut calls = 0;
        let attempt = |cycle: u32| {
            calls += 1;
            assert_eq!(cycle, 1);
            Err(CliError::Usage("bad".into()))
        };
        let result = supervise(attempt, None, Duration::ZERO);
        assert!(matches!(result, Err(CliError::Usage(_))));
        assert_eq!(calls, 1);
    }

    #[test]
    fn retries_past_failures_then_succeeds_bounded_by_max_cycles() {
        let attempt = scripted(vec![
            Err(CliError::Aoa(AoaError::Unplugged)),
            Err(CliError::Protocol("timeout".into())),
            Ok(()),
        ]);
        let result = supervise(attempt, Some(3), Duration::ZERO);
        assert!(result.is_ok());
    }

    #[test]
    fn success_does_not_stop_the_loop_before_max_cycles() {
        let mut calls = 0;
        let attempt = |_cycle: u32| {
            calls += 1;
            Ok(())
        };
        let result = supervise(attempt, Some(5), Duration::ZERO);
        assert!(result.is_ok());
        assert_eq!(calls, 5);
    }

    #[test]
    fn fatal_error_partway_through_a_budget_stops_early() {
        let mut calls = 0;
        let attempt = |cycle: u32| {
            calls += 1;
            if cycle < 3 {
                Ok(())
            } else {
                Err(CliError::Credentials("rotated away".into()))
            }
        };
        let result = supervise(attempt, Some(10), Duration::ZERO);
        assert!(matches!(result, Err(CliError::Credentials(_))));
        assert_eq!(calls, 3);
    }
}

//! The one real gap `docs/protocol/m2-session-bounds.md` §4 recorded as
//! open: every probe's only stop mechanism was a wall-clock deadline,
//! checked from inside the loop; an operator hitting Ctrl-C fell through
//! to default Rust/OS SIGINT handling (abrupt process termination, cleanup
//! left to whatever `Drop` impls happen to run). This installs a real
//! `SIGINT` handler once per process and hands back a flag every
//! `auth_discovery_probe::run` call site checks from inside its own loop
//! (`Instant::now() < deadline && !cancel.is_set()`), so a Ctrl-C reaches
//! the same clean, already-tested exit path a deadline does — released
//! transport/TLS resources via existing `Drop` impls, an honest
//! `probe_result=cancelled` line, and a distinct `CliError::Cancelled` exit
//! code, rather than a silent kill.
//!
//! Deliberately just one flag, not `ARCHITECTURE.md` §6's future
//! tree-wide, per-channel cancellation-token design for the `app`
//! orchestration layer — that layer doesn't exist yet. This closes the
//! specific, smaller gap the M2 checklist item actually named: today's
//! CLI probes have no way to be cancelled early at all, cooperatively or
//! otherwise.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::CliError;

/// Cheap to clone (an `Arc` around a single flag) so every `run()` call
/// site — including `session-supervisor`'s retry loop, which reuses one
/// installed handler across many cycles — can hold its own handle.
#[derive(Clone)]
pub(crate) struct CancellationFlag(Arc<AtomicBool>);

impl CancellationFlag {
    pub(crate) fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Sets the flag exactly as a real `SIGINT` would — used by
    /// `gtk_dev_ui.rs`'s window `close-request` handler (2026-08-22) so
    /// closing the GTK window drives the same cooperative-cancellation
    /// path Ctrl-C already does, instead of the OS abruptly killing the
    /// detached background protocol thread when `main()` returns.
    /// Real-hardware finding: without this, closing and relaunching the
    /// app left the USB accessory interface never cleanly released (the
    /// background thread's `Drop` impls never got to run — see
    /// `LibUsbBulkTransport`'s own `Drop`), and the phone needed a
    /// physical unplug/replug before a new process could connect again.
    pub(crate) fn trigger(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Resets the flag back to unset — used by `usb kiosk`/`usb
    /// wireless-kiosk`'s reconnect loop (`gtk_dev_ui.rs`) when a window
    /// `close-request` should only end the *current* session attempt,
    /// not quit the whole reconnect-forever process: the same shared
    /// flag that bounds one attempt's underlying protocol session is
    /// reused for every subsequent attempt too, so it must be cleared
    /// before the next one starts, exactly like a real Ctrl-C never
    /// needs to be (a real `SIGINT` should always mean quit, not just
    /// end one attempt — `end_kiosk_attempt` only calls this when it's
    /// determined the trigger was the window, not the signal handler).
    pub(crate) fn reset(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Installs the process's `SIGINT` handler. Must be called at most once per
/// process — `ctrlc::set_handler` itself enforces that and this returns its
/// error unchanged (mapped to `CliError::Io`) rather than silently ignoring
/// a second install, since a second install means some call site's flag
/// would never be set.
pub(crate) fn install_ctrlc_handler() -> Result<CancellationFlag, CliError> {
    let flag = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&flag);
    ctrlc::set_handler(move || {
        handler_flag.store(true, Ordering::SeqCst);
    })
    .map_err(|error| CliError::Io(std::io::Error::other(error)))?;
    Ok(CancellationFlag(flag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_starts_unset() {
        let flag = CancellationFlag(Arc::new(AtomicBool::new(false)));
        assert!(!flag.is_set());
    }

    #[test]
    fn flag_reflects_the_shared_atomic() {
        let inner = Arc::new(AtomicBool::new(false));
        let flag = CancellationFlag(Arc::clone(&inner));
        inner.store(true, Ordering::SeqCst);
        assert!(flag.is_set());
    }

    #[test]
    fn trigger_sets_the_flag_visibly_through_a_clone() {
        let flag = CancellationFlag(Arc::new(AtomicBool::new(false)));
        let clone = flag.clone();
        assert!(!clone.is_set());
        flag.trigger();
        assert!(clone.is_set());
    }
}

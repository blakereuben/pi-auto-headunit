//! A minimal, one-off GTK4 dialog telling the operator a phone needs a
//! physical replug. Added 2026-08-16 at Blake's explicit request: `usb
//! session-supervisor`'s automatic recovery should always try a
//! software-only fix first (`transport_usb::LibUsbAoaBackend::soft_reset`)
//! and only fall back to asking for a physical replug once that's already
//! failed once for the same failure streak.
//!
//! This project has no persistent head-unit UI yet (`ARCHITECTURE.md` §4;
//! `ui-model`/`ui-gtk` don't exist) and no notification daemon is
//! installed on the reference image (`notify-send` unavailable, nothing
//! like `mako`/`dunst` running) — so this spins up its own tiny GTK
//! `Application`/window for exactly as long as needed, then exits.
//! Mirrors `gtk_dev_ui.rs`'s existing thread/`mpsc`/`glib::timeout_add_local`
//! bridge between a blocking background wait and the GTK main loop.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Label, glib};
use transport_api::{AoaError, UsbDeviceId};

const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Runs `wait` (expected to block on a real physical replug — see
/// `transport_usb::LibUsbAoaBackend::wait_for_physical_replug`) on a
/// background thread while showing a popup window on this thread. Closes
/// the window automatically the moment `wait` finishes and returns its
/// result. If the operator closes the window early (or `activate` never
/// fires), still blocks on the same background wait afterward rather than
/// returning early on a still-stuck device — the popup only ever shortens
/// the wait, never skips it.
pub(crate) fn show_until_replugged(
    wait: impl FnOnce() -> Result<UsbDeviceId, AoaError> + Send + 'static,
) -> Result<UsbDeviceId, AoaError> {
    let (sender, receiver) = mpsc::channel::<Result<UsbDeviceId, AoaError>>();
    thread::spawn(move || {
        let _ = sender.send(wait());
    });
    let receiver = Rc::new(receiver);

    let final_result: Rc<RefCell<Option<Result<UsbDeviceId, AoaError>>>> =
        Rc::new(RefCell::new(None));

    let application = Application::builder()
        .application_id("dev.pi-auto-headunit.replug-prompt")
        .build();

    let receiver_for_activate = Rc::clone(&receiver);
    let final_result_for_activate = Rc::clone(&final_result);
    application.connect_activate(move |application| {
        let window = ApplicationWindow::builder()
            .application(application)
            .title("Phone reconnect needed")
            .default_width(480)
            .default_height(160)
            .child(&Label::new(Some(
                "Automatic recovery didn't work.\n\nPlease unplug and replug the phone.",
            )))
            .build();
        window.present();

        let receiver_for_poll = Rc::clone(&receiver_for_activate);
        let final_result_for_poll = Rc::clone(&final_result_for_activate);
        let application_for_poll = application.clone();
        glib::timeout_add_local(POLL_INTERVAL, move || match receiver_for_poll.try_recv() {
            Ok(result) => {
                *final_result_for_poll.borrow_mut() = Some(result);
                application_for_poll.quit();
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                application_for_poll.quit();
                glib::ControlFlow::Break
            }
        });
    });

    let _exit_code = application.run_with_args::<&str>(&[]);

    if let Some(result) = final_result.borrow_mut().take() {
        return result;
    }
    // The window closed (or activate never ran) before the poll observed
    // a result — block on the same background wait directly, so we never
    // proceed on a device that hasn't actually been replugged.
    receiver.recv().unwrap_or_else(|_| {
        Err(AoaError::Internal(
            "replug-wait background thread ended without a result".into(),
        ))
    })
}

//! `aa-headunit-diagnostics credentials setup` — a guided, Windows-installer-
//! style GTK4 wizard around the exact same [`credential_store`] functions
//! `credentials.rs`'s CLI `check`/`install` commands already use
//! (`validate_credentials`, `install_credentials`), for an operator who
//! isn't comfortable with a terminal. Built at the operator's explicit
//! request, 2026-08-23: "im used to windows so installing a exe will take
//! you thrugh the process" — welcome page, a file picker for the
//! certificate and private key, a validation result, then an install
//! result, each with Back/Next-style navigation via a plain [`Stack`]
//! (this project doesn't otherwise depend on libadwaita, so no
//! `AdwNavigationView`).
//!
//! Deliberately does not offer to replace an already-installed pair —
//! `install_credentials` itself refuses to overwrite
//! (`CredentialError::AlreadyInstalled`), by design
//! (`docs/development/credential-provisioning.md`: "Credential rotation
//! will be a separate explicit operation so an interrupted setup cannot
//! silently replace a working identity"). The wizard surfaces that same
//! refusal up front instead of working around it.
//!
//! Never reads a credential file's contents into anything this module
//! prints or logs beyond what [`credential_store::CredentialStatus`]
//! already exposes (validity dates, key file mode) — the same bounded
//! summary the CLI's own `print_status` shows.

use credential_store::{
    CredentialError, CredentialPaths, install_credentials, validate_credentials,
};
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, FileDialog, Label, Orientation, Stack,
    glib,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use transport_bluetooth::PairingProgress;

use crate::credentials::{DEFAULT_CERTIFICATE_PATH, DEFAULT_PRIVATE_KEY_PATH};

const WINDOW_WIDTH: i32 = 560;
const WINDOW_HEIGHT: i32 = 420;
const PAGE_MARGIN: i32 = 24;
const ROW_SPACING: i32 = 12;

/// The exit code this process reports when the operator hit Cancel
/// (`build_cancel_row`) — arbitrary but distinct from `0`/`1`/`2`, the
/// exit codes a plain window-close or an ordinary error might already
/// produce. `packaging/setup.sh` checks for exactly this value to know
/// its own subsequent `apt install` step must be skipped, not just any
/// nonzero wizard exit — see `build_cancel_row`'s own comment for the
/// real-hardware finding that made this necessary.
const CANCELLED_EXIT_CODE: i32 = 42;

/// How long the pairing page waits for a phone before giving up and
/// letting the operator continue anyway (they can always pair later from
/// Bluetooth settings) — matches `packaging/setup.sh`'s own shell
/// implementation of the same wait.
const BLUETOOTH_PAIRING_TIMEOUT: Duration = Duration::from_secs(180);
/// How often the background pairing thread checks whether a phone has
/// paired yet.
const BLUETOOTH_PAIRING_CHECK_INTERVAL: Duration = Duration::from_secs(2);
/// How often the GTK page itself polls for a progress update from that
/// background thread — matches this app's other GTK/background-thread
/// bridges (`gtk_dev_ui.rs`'s own `POLL_INTERVAL`).
const BLUETOOTH_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Where the wizard installs to — identical to `credentials.rs`'s own
/// `install` command destination, not independently chosen.
fn destination_paths() -> CredentialPaths {
    CredentialPaths {
        certificate: PathBuf::from(DEFAULT_CERTIFICATE_PATH),
        private_key: PathBuf::from(DEFAULT_PRIVATE_KEY_PATH),
    }
}

/// Per-attempt state shared across the file-picker/validate/install pages'
/// closures. `Rc<RefCell<..>>` throughout, matching this project's
/// existing GTK4 shared-state convention (`gtk_dev_ui.rs`).
#[derive(Default)]
struct WizardState {
    certificate: Option<PathBuf>,
    private_key: Option<PathBuf>,
}

// Every error this wizard can hit (a missing/invalid credential file, an
// already-installed pair) is shown on its own page instead of propagated —
// the whole point of a guided setup flow is that failure is something the
// operator sees and can act on inline, not a process exit code. The
// `Result` return type exists only to match `credentials_command`'s other
// arms' uniform signature.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn run() -> Result<(), crate::CliError> {
    let application = Application::builder()
        .application_id("dev.pi-auto-headunit.credentials-setup")
        .build();

    application.connect_activate(|application| {
        build_and_present_wizard(application);
    });

    let _exit_code = application.run_with_args::<&str>(&[]);
    Ok(())
}

fn build_and_present_wizard(application: &Application) {
    install_minimum_touch_target_css();

    let window = ApplicationWindow::builder()
        .application(application)
        .title("Android Auto Head Unit Setup")
        .default_width(WINDOW_WIDTH)
        .default_height(WINDOW_HEIGHT)
        .resizable(true)
        .build();

    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_vexpand(true);

    // A persistent row *outside* the stack, not duplicated into every
    // individual page's own button row — stays visible across every page
    // (including ones added to `stack` dynamically later, like the
    // validate/install result pages) without needing separate wiring in
    // each `build_*_page` function. Operator's explicit direction,
    // 2026-08-26: "a cancel installation button in the wizard on every
    // page so i can cancel at anytime, which will also act as a
    // uninstall."
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&stack);
    root.append(&build_cancel_row());
    window.set_child(Some(&root));

    let state: Rc<RefCell<WizardState>> = Rc::new(RefCell::new(WizardState::default()));

    // Checked once, at launch, before showing anything else — an
    // already-installed pair means there's nothing this wizard can
    // usefully walk the operator through (see the module doc comment for
    // why it doesn't offer to replace one).
    if validate_credentials(&destination_paths(), true).is_ok() {
        stack.add_named(
            &build_already_installed_page(&window),
            Some("already-installed"),
        );
        stack.set_visible_child_name("already-installed");
    } else {
        stack.add_named(&build_welcome_page(&stack), Some("welcome"));
        stack.add_named(&build_pair_bluetooth_page(&stack), Some("pair-bluetooth"));
        stack.add_named(
            &build_choose_files_page(&window, &stack, &state),
            Some("choose-files"),
        );
        stack.set_visible_child_name("welcome");
    }

    window.present();
}

/// The persistent "Cancel Installation" row shown under every page. A
/// separate, fixed row (not part of any individual page's own
/// `button_row`) — see [`build_and_present_wizard`]'s own comment for
/// why.
///
/// Real-hardware finding, 2026-08-26: `cancel_and_uninstall` (the
/// `pkexec`/`apt purge` call) genuinely worked the first time this was
/// tried (confirmed via `journalctl`: polkit authenticated and `apt
/// purge` ran successfully) — but running it synchronously right here in
/// the click handler blocked the GTK main thread for as long as the
/// operator took to answer the polkit password prompt, with the window
/// giving no sign anything was happening. Indistinguishable from
/// actually being broken. Same background-thread-plus-channel-plus-
/// `glib::timeout_add_local` bridge as [`build_pair_bluetooth_page`],
/// so the window stays responsive and visibly says what it's doing
/// instead of silently freezing.
fn build_cancel_row() -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, ROW_SPACING);
    row.set_halign(gtk4::Align::Start);
    row.set_margin_start(PAGE_MARGIN);
    row.set_margin_end(PAGE_MARGIN);
    row.set_margin_bottom(PAGE_MARGIN);

    let cancel = Button::with_label("Cancel Installation");
    cancel.add_css_class("destructive-action");
    row.append(&cancel);

    let status = body_label("");
    row.append(&status);

    cancel.connect_clicked(move |button| {
        button.set_sensitive(false);
        status.set_label("Cancelling and uninstalling — check for a password prompt…");

        let (sender, receiver) = mpsc::channel::<()>();
        thread::spawn(move || {
            cancel_and_uninstall();
            let _ = sender.send(());
        });

        glib::timeout_add_local(BLUETOOTH_STATUS_POLL_INTERVAL, move || {
            match receiver.try_recv() {
                // Real-hardware finding, 2026-08-26: `packaging/setup.sh`'s
                // `"$wizard" credentials setup || true` swallows *any*
                // wizard exit code, so it unconditionally continued on to
                // `apt install --reinstall` right after — reinstalling
                // the very package Cancel had just purged, even though
                // the purge itself genuinely succeeded (confirmed via
                // `journalctl`). `window.close()` alone can't signal
                // "cancelled" to the shell script that launched this
                // process at all — only the process's own exit code can.
                // `std::process::exit` (not a normal `window.close()` and
                // GTK app-loop return) so this exact code reaches
                // `setup.sh` unconditionally, skipping GTK's own cleanup
                // since the process is ending regardless. `setup.sh`
                // checks for exactly this value to skip its own install
                // step instead of treating it like any other nonzero
                // wizard exit (e.g. the window just being closed without
                // finishing, which should still fall through to install
                // in case credentials were already staged).
                Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {
                    std::process::exit(CANCELLED_EXIT_CODE);
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            }
        });
    });

    row
}

/// Operator's explicit direction, 2026-08-26: Cancel means "undo
/// everything" — whatever this run of the wizard has staged, and (if
/// this is a later re-run of `credentials setup` against an
/// already-installed package, not the first-run `packaging/setup.sh`
/// flow) the installed package itself. `pkexec` (not `sudo`, which has
/// no terminal to prompt a password on from a GTK button): the exact
/// same mechanism the packaged "Uninstall Android Auto Head Unit" icon
/// uses (`packaging/labwc/aa-headunit-uninstall.desktop`), so Cancel
/// here and that icon always agree on how the app actually gets
/// removed. Best-effort throughout: if the package was never installed
/// yet, `apt purge` on it is a harmless no-op, not an error worth
/// surfacing — the operator is leaving either way.
fn cancel_and_uninstall() {
    if let Some(staging) = crate::credentials::staging_paths() {
        let _ = std::fs::remove_file(&staging.certificate);
        let _ = std::fs::remove_file(&staging.private_key);
    }
    let _ = std::process::Command::new("pkexec")
        .args(["apt", "purge", "-y", "aa-headunit-diagnostics"])
        .status();
}

fn page_container() -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, ROW_SPACING);
    page.set_margin_top(PAGE_MARGIN);
    page.set_margin_bottom(PAGE_MARGIN);
    page.set_margin_start(PAGE_MARGIN);
    page.set_margin_end(PAGE_MARGIN);
    page.set_valign(gtk4::Align::Center);
    page
}

fn title_label(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("title-2");
    label.set_wrap(true);
    label.set_xalign(0.0);
    label
}

fn body_label(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.set_wrap(true);
    label.set_xalign(0.0);
    label
}

/// A right-aligned row of buttons, the way a Windows installer's
/// Back/Next/Cancel row reads left-to-right in visual position but is
/// built right-to-left in code (`set_halign(End)` on the row, buttons
/// appended in on-screen left-to-right order).
fn button_row(buttons: &[&Button]) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, ROW_SPACING);
    row.set_halign(gtk4::Align::End);
    row.set_margin_top(ROW_SPACING);
    for button in buttons {
        row.append(*button);
    }
    row
}

fn build_welcome_page(stack: &Stack) -> GtkBox {
    let page = page_container();
    page.append(&title_label("Android Auto Head Unit Setup"));
    page.append(&body_label(
        "This installs the certificate and private key your Android Auto \
         receiver identity uses, so this head unit can connect to your \
         phone. You'll need a certificate file and a matching private key \
         file you already have on hand — this project doesn't provide, \
         generate, or download these for you.",
    ));

    let next = Button::with_label("Get Started");
    next.add_css_class("suggested-action");
    let stack_for_next = stack.clone();
    next.connect_clicked(move |_| {
        stack_for_next.set_visible_child_name("pair-bluetooth");
    });
    page.append(&button_row(&[&next]));
    page
}

/// Pairing your phone before picking credential files, not after —
/// operator's explicit direction, 2026-08-26: turn Bluetooth on and make
/// it discoverable itself if it isn't already, as part of this same
/// guided flow, so Android Auto can find an already-paired phone and
/// start right up on first launch instead of the operator hitting a
/// black screen with nothing paired. Never blocks progress on actually
/// pairing — "Continue" is enabled from the start, matching
/// `packaging/setup.sh`'s own "setup continues either way" shell version
/// of the same step.
fn build_pair_bluetooth_page(stack: &Stack) -> GtkBox {
    let page = page_container();
    page.append(&title_label("Connect your phone over Bluetooth"));
    page.append(&body_label(
        "Pairing now means Android Auto can start right up once setup \
         finishes, instead of the first launch having nothing paired to \
         talk to. You can also pair later from Bluetooth settings.",
    ));

    let status = body_label("Turning on Bluetooth…");
    page.append(&status);

    let next = Button::with_label("Continue");
    next.add_css_class("suggested-action");
    let stack_for_next = stack.clone();
    next.connect_clicked(move |_| {
        stack_for_next.set_visible_child_name("choose-files");
    });
    page.append(&button_row(&[&next]));

    let (sender, receiver) = mpsc::channel::<PairingProgress>();
    thread::spawn(move || {
        transport_bluetooth::pair_phone_with_progress(
            BLUETOOTH_PAIRING_TIMEOUT,
            BLUETOOTH_PAIRING_CHECK_INTERVAL,
            &sender,
        );
    });

    let status_for_poll = status.clone();
    glib::timeout_add_local(BLUETOOTH_STATUS_POLL_INTERVAL, move || {
        match receiver.try_recv() {
            Ok(PairingProgress::Discoverable { device_name }) => {
                status_for_poll.set_label(&format!(
                    "On your phone: open Bluetooth settings and pair with \
                     \"{device_name}\". Waiting…"
                ));
                glib::ControlFlow::Continue
            }
            Ok(PairingProgress::Paired) => {
                status_for_poll.set_label("Phone paired.");
                glib::ControlFlow::Break
            }
            Ok(PairingProgress::TimedOut) => {
                status_for_poll
                    .set_label("No phone paired yet — you can pair later from Bluetooth settings.");
                glib::ControlFlow::Break
            }
            Ok(PairingProgress::Error(error)) => {
                status_for_poll.set_label(&format!("Couldn't start Bluetooth pairing: {error}."));
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });

    page
}

fn build_already_installed_page(window: &ApplicationWindow) -> GtkBox {
    let page = page_container();
    page.append(&title_label("Credentials already installed"));

    let detail = match validate_credentials(&destination_paths(), true) {
        Ok(status) => format!(
            "A certificate and private key are already installed at \
             {}, valid from {} to {}.\n\nTo replace them, remove those two \
             files yourself first, then run setup again — this is a \
             deliberate safeguard so setup can never silently overwrite a \
             working identity.",
            destination_paths().certificate.parent().map_or_else(
                || destination_paths().certificate.display().to_string(),
                |parent| parent.display().to_string()
            ),
            status.summary.not_before,
            status.summary.not_after,
        ),
        // Unreachable in practice (this page is only shown after a
        // successful validate_credentials call), but a WizardState-free
        // page still needs a body for the type checker's sake, and
        // silently mismatching the check that gated it here would be a
        // worse bug than an honest fallback message.
        Err(error) => format!("Credentials are already installed. ({error})"),
    };
    page.append(&body_label(&detail));

    let close = Button::with_label("Close");
    let window_for_close = window.clone();
    close.connect_clicked(move |_| {
        window_for_close.close();
    });
    page.append(&button_row(&[&close]));
    page
}

fn build_choose_files_page(
    window: &ApplicationWindow,
    stack: &Stack,
    state: &Rc<RefCell<WizardState>>,
) -> GtkBox {
    let page = page_container();
    page.append(&title_label("Step 1 of 2 — Choose your files"));
    page.append(&body_label(
        "Select the certificate (.crt/.pem) and private key (.key/.pem) \
         files for your Android Auto receiver identity.",
    ));

    let certificate_row = GtkBox::new(Orientation::Horizontal, ROW_SPACING);
    let certificate_label = Label::new(Some("Certificate: not selected"));
    certificate_label.set_hexpand(true);
    certificate_label.set_xalign(0.0);
    certificate_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    let certificate_browse = Button::with_label("Browse…");
    certificate_row.append(&certificate_label);
    certificate_row.append(&certificate_browse);
    page.append(&certificate_row);

    let private_key_row = GtkBox::new(Orientation::Horizontal, ROW_SPACING);
    let private_key_label = Label::new(Some("Private key: not selected"));
    private_key_label.set_hexpand(true);
    private_key_label.set_xalign(0.0);
    private_key_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    let private_key_browse = Button::with_label("Browse…");
    private_key_row.append(&private_key_label);
    private_key_row.append(&private_key_browse);
    page.append(&private_key_row);

    let next = Button::with_label("Next");
    next.add_css_class("suggested-action");
    next.set_sensitive(false);

    {
        let window = window.clone();
        let state = Rc::clone(state);
        let certificate_label = certificate_label.clone();
        let next = next.clone();
        let private_key_label_for_sensitivity = private_key_label.clone();
        certificate_browse.connect_clicked(move |_| {
            let dialog = FileDialog::builder()
                .title("Choose certificate file")
                .build();
            let state = Rc::clone(&state);
            let certificate_label = certificate_label.clone();
            let next = next.clone();
            let private_key_label = private_key_label_for_sensitivity.clone();
            dialog.open(Some(&window), gtk4::gio::Cancellable::NONE, move |result| {
                let Ok(file) = result else { return };
                let Some(path) = file.path() else { return };
                certificate_label.set_text(&format!("Certificate: {}", path.display()));
                let has_key = { state.borrow().private_key.is_some() };
                state.borrow_mut().certificate = Some(path);
                next.set_sensitive(has_key);
                let _ = &private_key_label;
            });
        });
    }

    {
        let window = window.clone();
        let state = Rc::clone(state);
        let private_key_label = private_key_label.clone();
        let next = next.clone();
        private_key_browse.connect_clicked(move |_| {
            let dialog = FileDialog::builder()
                .title("Choose private key file")
                .build();
            let state = Rc::clone(&state);
            let private_key_label = private_key_label.clone();
            let next = next.clone();
            dialog.open(Some(&window), gtk4::gio::Cancellable::NONE, move |result| {
                let Ok(file) = result else { return };
                let Some(path) = file.path() else { return };
                private_key_label.set_text(&format!("Private key: {}", path.display()));
                let has_certificate = { state.borrow().certificate.is_some() };
                state.borrow_mut().private_key = Some(path);
                next.set_sensitive(has_certificate);
            });
        });
    }

    {
        let stack = stack.clone();
        let state = Rc::clone(state);
        next.connect_clicked(move |_| {
            let source = {
                let state = state.borrow();
                CredentialPaths {
                    certificate: state.certificate.clone().unwrap_or_default(),
                    private_key: state.private_key.clone().unwrap_or_default(),
                }
            };
            let page_name = "validate-result";
            if stack.child_by_name(page_name).is_some() {
                stack.remove(&stack.child_by_name(page_name).unwrap());
            }
            stack.add_named(
                &build_validate_result_page(&stack, &source),
                Some(page_name),
            );
            stack.set_visible_child_name(page_name);
        });
    }

    page.append(&button_row(&[&next]));
    page
}

fn build_validate_result_page(stack: &Stack, source: &CredentialPaths) -> GtkBox {
    let page = page_container();

    // `false`: this previews the freshly *picked* source pair, which
    // `install_credentials` (`credential-store/src/linux.rs`) rewrites to
    // the real destination with correct `0600` permissions regardless of
    // the source's own mode — see that function's own comment. Matching
    // its relaxed check here means this preview page doesn't reject a
    // pair the actual install step would accept.
    match validate_credentials(source, false) {
        Ok(status) => {
            page.append(&title_label("Step 2 of 2 — Files look valid"));
            page.append(&body_label(&format!(
                "Certificate valid from {} to {}.",
                status.summary.not_before, status.summary.not_after
            )));

            let back = Button::with_label("Back");
            let stack_for_back = stack.clone();
            back.connect_clicked(move |_| {
                stack_for_back.set_visible_child_name("choose-files");
            });

            let install = Button::with_label("Install");
            install.add_css_class("suggested-action");
            let stack_for_install = stack.clone();
            let source = source.clone();
            install.connect_clicked(move |_| {
                let page_name = "install-result";
                if let Some(existing) = stack_for_install.child_by_name(page_name) {
                    stack_for_install.remove(&existing);
                }
                stack_for_install.add_named(&build_install_result_page(&source), Some(page_name));
                stack_for_install.set_visible_child_name(page_name);
            });

            page.append(&button_row(&[&back, &install]));
        }
        Err(error) => {
            page.append(&title_label(credential_error_title(&error)));
            page.append(&body_label(&describe_credential_error(&error)));

            let back = Button::with_label("Back");
            let stack_for_back = stack.clone();
            back.connect_clicked(move |_| {
                stack_for_back.set_visible_child_name("choose-files");
            });
            page.append(&button_row(&[&back]));
        }
    }

    page
}

fn build_install_result_page(source: &CredentialPaths) -> GtkBox {
    let page = page_container();

    // Always stages rather than installing straight to `destination_paths()`
    // — this wizard can run before the package (and the `aa-headunit`
    // group/directory that owns the real destination) is even installed
    // (`packaging/setup.sh`). `postinst`, or the main app's own startup as
    // a safety net, adopts the staged pair for real afterward
    // (`credentials::adopt_staged_credentials_if_present`).
    let result = match crate::credentials::staging_paths() {
        Some(staging) => install_credentials(source, &staging),
        None => Err(CredentialError::Config(
            "could not determine a home directory to save to".into(),
        )),
    };

    match result {
        Ok(_) => {
            page.append(&title_label("Credentials saved"));
            page.append(&body_label(
                "These will be installed automatically the next time the \
                 head unit app starts.",
            ));
        }
        Err(error) => {
            page.append(&title_label(credential_error_title(&error)));
            page.append(&body_label(&describe_credential_error(&error)));
        }
    }

    let finish = Button::with_label("Finish");
    finish.add_css_class("suggested-action");
    let finish_for_click = finish.clone();
    finish.connect_clicked(move |_| {
        if let Some(window) = finish_for_click
            .root()
            .and_then(|root| root.downcast::<ApplicationWindow>().ok())
        {
            window.close();
        }
    });
    page.append(&button_row(&[&finish]));
    page
}

/// A title specific to *what actually went wrong*, shown above
/// [`describe_credential_error`]'s detail text. Previously every branch of
/// [`build_validate_page`] (and, separately, [`build_install_result_page`])
/// hard-coded "These files aren't a valid pair"/"Installation failed" for
/// any [`CredentialError`] at all — so a pure file-permissions problem (no
/// mismatch between the certificate and key whatsoever) read as if the
/// files didn't pair, misleading an operator who already knows their
/// cert/key really do match. Real-hardware finding, 2026-08-26: a fresh
/// install's picked-from-USB-stick private key hit exactly this — 0644
/// permissions inherited from a FAT-formatted drive, reported as "aren't a
/// valid pair".
fn credential_error_title(error: &CredentialError) -> &'static str {
    match error {
        CredentialError::Missing(_) => "File not found",
        CredentialError::InvalidFile(_) => "That file isn't usable",
        CredentialError::InvalidCredentials(_) => "These files aren't a valid pair",
        CredentialError::InsecurePrivateKeyPermissions(_) => "Private key permissions are too open",
        CredentialError::AlreadyInstalled(_) => "Already installed",
        CredentialError::Io(_) => "Couldn't read that file",
        CredentialError::Config(_) => "Setup problem",
    }
}

/// Plain-language versions of [`CredentialError`] for the wizard's own
/// pages — the CLI's `Display` impl (`credential-store/src/linux.rs`) is
/// already reasonably plain, this just avoids the `Debug`-shaped prefix
/// wording ("configuration:", "invalid credential file:") that reads fine
/// in a terminal but not in a dialog.
fn describe_credential_error(error: &CredentialError) -> String {
    match error {
        CredentialError::Missing(path) => {
            format!("{} doesn't exist or couldn't be read.", path.display())
        }
        CredentialError::InvalidFile(reason) => format!("That file isn't usable: {reason}."),
        CredentialError::InvalidCredentials(reason) => {
            format!("The certificate and private key don't form a valid pair: {reason}.")
        }
        CredentialError::InsecurePrivateKeyPermissions(mode) => format!(
            "The private key file's permissions ({mode:04o}) allow other users to read it — \
             it must be exactly 0600 (readable only by you). Fix this before continuing: \
             open a terminal and run `chmod 600` on the private key file, then try again. \
             If that file is on a USB drive or SD card reader formatted FAT32/exFAT, `chmod` \
             won't take effect there — those filesystems don't support Unix permissions at \
             all — copy the file onto this device first (e.g. into your home folder), `chmod \
             600` the copy, and pick that copy instead."
        ),
        CredentialError::AlreadyInstalled(path) => {
            format!("{} already exists.", path.display())
        }
        CredentialError::Io(io_error) => format!("Couldn't read that file: {io_error}."),
        CredentialError::Config(reason) => format!("Setup configuration problem: {reason}."),
    }
}

fn install_minimum_touch_target_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_data("button { min-height: 48px; min-width: 48px; }");
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

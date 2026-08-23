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
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::credentials::{DEFAULT_CERTIFICATE_PATH, DEFAULT_PRIVATE_KEY_PATH};

const WINDOW_WIDTH: i32 = 560;
const WINDOW_HEIGHT: i32 = 420;
const PAGE_MARGIN: i32 = 24;
const ROW_SPACING: i32 = 12;

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
    window.set_child(Some(&stack));

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
        stack.add_named(
            &build_choose_files_page(&window, &stack, &state),
            Some("choose-files"),
        );
        stack.set_visible_child_name("welcome");
    }

    window.present();
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
        stack_for_next.set_visible_child_name("choose-files");
    });
    page.append(&button_row(&[&next]));
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

    match validate_credentials(source, true) {
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
            page.append(&title_label("These files aren't a valid pair"));
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

    match install_credentials(source, &destination_paths()) {
        Ok(_) => {
            page.append(&title_label("Credentials installed"));
            page.append(&body_label(&format!(
                "Installed to {}. You can now open Android Auto Head Unit \
                 from the desktop — it'll connect over USB or wirelessly \
                 automatically.",
                destination_paths().certificate.parent().map_or_else(
                    || destination_paths().certificate.display().to_string(),
                    |parent| parent.display().to_string()
                )
            )));
        }
        Err(error) => {
            page.append(&title_label("Installation failed"));
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
            "The private key file's permissions ({mode:04o}) allow other users to read it. \
             Fix this before continuing, e.g. `chmod 600` on the file, then try again."
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

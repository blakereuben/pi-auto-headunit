//! Drops this process's Linux file capabilities and restores the
//! kernel's "dumpable" flag for every subcommand except the ones that
//! actually need `cap_net_admin`/`cap_net_bind_service`/`cap_net_raw`
//! (granted via `setcap` in
//! `packaging/debian/aa-headunit-diagnostics.postinst`, for the
//! wireless Wi-Fi access point bring-up in `wireless_bootstrap.rs`).
//!
//! Real-hardware finding, 2026-08-24: gaining capabilities via file
//! capabilities on exec makes the kernel mark the *whole process*
//! non-dumpable (`proc(5)`), which blocks any same-uid process —
//! including `xdg-desktop-portal` — from opening `/proc/<pid>/root`.
//! The portal uses exactly that path to identify the calling app
//! before it will honor a `GtkFileDialog` request, so the credential
//! setup wizard's "Browse..." button silently did nothing: the
//! capabilities meant only for the wireless bootstrap path were
//! leaking into every other subcommand, including this one, which
//! never touches the network at all. Verified directly on real
//! hardware: stripping the binary's file capabilities made
//! `/proc/<pid>/root` readable again (`Permission denied` ->
//! `-> /`); this does the same thing at runtime, scoped to just the
//! subcommands that don't need the capabilities in the first place.

/// The only subcommands that actually use the network capabilities
/// `setcap` grants this binary.
fn needs_network_capabilities(args: &[String]) -> bool {
    matches!(
        (
            args.first().map(String::as_str),
            args.get(1).map(String::as_str)
        ),
        (Some("usb"), Some("kiosk" | "wireless-bootstrap-probe"))
    )
}

/// Best-effort: a failure here just leaves the process with more
/// capabilities than it needs (today's status quo), not a reason to
/// abort startup.
pub(crate) fn drop_unneeded_capabilities(args: &[String]) {
    if needs_network_capabilities(args) {
        return;
    }
    for capability_set in [
        caps::CapSet::Effective,
        caps::CapSet::Permitted,
        caps::CapSet::Inheritable,
    ] {
        if let Err(error) = caps::clear(None, capability_set) {
            eprintln!("privilege_warning=failed to clear {capability_set:?} capabilities: {error}");
        }
    }
    if let Err(error) = nix::sys::prctl::set_dumpable(true) {
        eprintln!("privilege_warning=failed to restore dumpable flag: {error}");
    }
}

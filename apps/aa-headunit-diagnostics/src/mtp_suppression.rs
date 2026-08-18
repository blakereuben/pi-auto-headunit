//! Runtime toggle that masks/unmasks the desktop's
//! `gvfs-mtp-volume-monitor` `systemctl --user` service, matching
//! `settings::HeadUnitSettings::mtp_popup_suppression_enabled` —
//! see that method's doc comment for the real-hardware finding this
//! exists to fix (the desktop popping up "couldn't find matching udev
//! device"/"no MTP devices found" on every Android Auto reconnect).
//!
//! An earlier version of this fix tried to suppress MTP auto-mounting
//! purely at the udev layer (`ENV{MTP_NO_PROBE}`, overriding
//! `ENV{ID_MTP_DEVICE}`). Real-hardware trial, 2026-08-18: confirmed via
//! `udevadm info --query=property` (not just `udevadm test`, which
//! re-reads rule files itself and doesn't reflect the live daemon) that
//! the override genuinely won as the final exported property — and the
//! popups still happened anyway. Root-caused by directly stopping
//! `gvfs-mtp-volume-monitor.service` and re-running the same real-hardware
//! reconnect trial: zero popups across 8 cycles, confirmed directly by
//! the operator watching the screen (`journalctl` also showed the
//! service's own log line "device ... has an identical `ID_SERIAL` value
//! to an existing device", suggesting it tracks phones by hardware serial
//! somewhat independent of per-event udev tagging — udev-level
//! suppression alone can't out-race that). The udev-based approach was
//! removed entirely rather than kept alongside this one, since it added
//! real fragility (racing a shipped hardware database and two other
//! vendor rule files) for no proven benefit once this actually worked.
//!
//! `mask --now` rather than plain `stop`: this service is
//! D-Bus-activatable, so a plain `stop` risks another app's
//! `GVolumeMonitor` call silently restarting it before the next
//! reconnect; masking prevents any activation path, not just a manual
//! one. Disabling is `unmask` followed by a separate `start` —
//! `systemctl`'s `--now` flag only accepts `enable`/`disable`/
//! `reenable`/`mask` as its verb, confirmed the hard way: an earlier
//! version of this tried `unmask --now` and `systemctl` rejected it
//! outright ("--now can only be used with verb enable, disable,
//! reenable, or mask").
//!
//! Runs as `systemctl --user`, so it only affects the desktop session
//! belonging to whichever user this process runs as — true unconditionally
//! today, since the diagnostics CLI/GTK app and the desktop session are
//! the same user. If a later milestone (M6, dedicated unprivileged system
//! user) moves the head unit process to its own account with no desktop
//! session of its own, this will need revisiting.
//!
//! `sync` is called once per session start (`auth_discovery_probe::
//! setup_settings_gesture`) so every entry point — `usb
//! auth-discovery-probe`, `usb session-supervisor`, and `usb gtk-dev-ui` —
//! picks up the persisted setting, plus immediately from the settings
//! panel's toggle (`gtk_dev_ui.rs`) for instant feedback within a live
//! session.

use std::process::Command;

const SERVICE: &str = "gvfs-mtp-volume-monitor.service";

/// Best-effort, matching `HeadUnitSettings`'s own "a setting is a
/// convenience, never allowed to fail a live session" discipline — a
/// failure here (e.g. no user D-Bus session, `systemctl` missing) is
/// logged, not propagated.
pub(crate) fn sync(enabled: bool) {
    if let Err(error) = try_sync(enabled) {
        println!("probe_state=mtp_suppression_sync_failed enabled={enabled} error={error}");
    }
}

fn try_sync(enabled: bool) -> std::io::Result<()> {
    if enabled {
        run(&["--user", "mask", "--now", SERVICE])
    } else {
        run(&["--user", "unmask", SERVICE])?;
        run(&["--user", "start", SERVICE])
    }
}

fn run(args: &[&str]) -> std::io::Result<()> {
    let status = Command::new("systemctl").args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "systemctl {} exited with {status}",
            args.join(" ")
        )))
    }
}

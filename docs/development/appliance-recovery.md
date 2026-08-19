# Appliance recovery and development path

How the M5 appliance boots, what's proven and what isn't about that
transition, and how to get back to a normal development environment if
something goes wrong.

**Design history**: an earlier design (`aa-headunit-kiosk@.service`, now
removed) ran `cage`, a bare Wayland kiosk compositor, as a
separately-managed session on a spare VT, alongside whatever desktop the
operator already had running. Real-hardware testing (2026-08-18/19)
found `cage` itself would repeatedly get stuck and stop responding to a
graceful stop request — **first believed to hard-hang the whole board,
requiring a physical power cycle; that turned out not to be quite
right.** SSH stayed fully responsive through every occurrence; what
actually happened was `cage` becoming unresponsive to `SIGTERM` while
holding the display, so the screen froze and a directly-attached
keyboard stopped responding, which looks exactly like a full system hang
unless you check SSH. The design itself was then dropped for an
unrelated, more fundamental reason: it never gave the operator a real
desktop underneath the kiosk app, which is a hard requirement, not just
a nice-to-have. A second design then briefly considered a dedicated
`aa-headunit` *login* account (its own desktop session, autologin
pointed at it) — also dropped, same day: this is a program installed
onto someone's own already-set-up PiOS desktop, not appliance hardware
shipping with its own dedicated account, so it should autologin as
whoever already uses the machine, not switch to a separate identity.
The current design (below) reflects that. The general recovery guidance
later in this document (SSH-first, don't assume a frozen screen means a
frozen board) remains good practice for any future graphical-boot
problem regardless of which design is active.

## How the appliance boots (current design)

- `postinst` detects whichever real account is running the active
  desktop session (`seat0`) at install time, and for that account:
  - adds it to the `aa-headunit` group (unprivileged access to
    `/etc/aa-headunit`, `/var/lib/aa-headunit`, USB, GPIO — previously a
    manual step, see `docs/development/credential-provisioning.md`);
  - copies `packaging/labwc/aa-headunit-autostart` (shipped at
    `/usr/share/aa-headunit/labwc-autostart`) to that account's own
    `~/.config/labwc/autostart`, overwriting it on every install/upgrade
    — it's package-managed content;
  - points LightDM's `autologin-user` at that account, but **only if
    autologin isn't already configured** — an existing choice (including
    one made before this package was ever installed, as on this
    development Pi) is never overridden.
- `aa-headunit` itself is no longer a login identity — it remains only
  as the permissions-group anchor for the points above. Nothing runs as
  that uid; the app runs as whichever real account was detected.
- The installed autostart script contains ONLY the app-launch gate below
  — it does not repeat the panel/file-manager/output-config lines
  `/etc/xdg/labwc/autostart` already starts. Real-hardware finding
  (2026-08-19): `/usr/bin/labwc-pi` always execs `labwc -m`
  ("merge-config"), which runs BOTH the system default and a per-user
  `~/.config/labwc/autostart` — it does NOT replace one with the other.
  An earlier version of this file assumed the opposite and duplicated
  those lines "to avoid losing them," which instead made every one of
  them run twice — two `wf-panel-pi` processes, a visibly doubled
  taskbar, and periodic white/blank flashes as both instances fought to
  composite the same outputs. `aa-headunit-diagnostics launch-on-boot-enabled`
  checks the operator's own persisted preference (Display settings page)
  — **off by default** (2026-08-19, deliberate operator decision — see
  "Real-hardware confirmed" below: turning this on is an explicit opt-in
  once the "return to desktop" hang is confirmed fixed, not something
  this package should risk unattended). It then runs
  `aa-headunit-diagnostics preflight` (`ARCHITECTURE.md` §9: a connected
  display, a touchscreen input device, a writable `/var/lib/aa-headunit`,
  working USB access, at least one audio device) and only execs the
  fullscreen app (`usb kiosk --allow-live-aap`) if both pass — failing
  open to the plain desktop otherwise, not retrying a broken session on
  every boot. Run preflight by hand any time with
  `aa-headunit-diagnostics preflight`. The desktop shortcut
  (`aa-headunit.desktop`, `~/Desktop` and the application menu) still
  launches the app manually regardless of the "launch on boot" setting.
- The app requests its own window fullscreen via a normal Wayland
  `xdg-shell` call (`window.fullscreen()`,
  `apps/aa-headunit-diagnostics/src/gtk_dev_ui.rs`) — this is not
  compositor-specific, so it works the same under `labwc` as it did
  under `cage`. "Return to desktop" (the fullscreen-toggle action)
  un-fullscreens the window rather than quitting it, revealing the
  operator's own real desktop underneath (panel, file manager, and
  everything else already on it) — this is the actual point of the
  redesign.

**Real-hardware confirmed, 2026-08-19, on this development Pi**: package
install correctly detected the active `blakereuben` session, added it to
the `aa-headunit` group (already a member from earlier work — no-op),
and wrote `/home/blakereuben/.config/labwc/autostart` with correct
ownership; `lightdm.conf`'s pre-existing `autologin-user=blakereuben`
was correctly left untouched (the "don't override an existing choice"
guard); a real reboot picked up the autostart file and reached a fully
working fullscreen app — real video/audio streaming, a clean
operator-initiated disconnect (`ByeByeRequest`), and the auto-reconnect
loop all confirmed. Three real bugs were found and fixed via that same
trial, all now real-hardware-confirmed fixed: the `labwc -m`
merge-config duplicate-autostart bug described above (doubled
panel/taskbar); a `VideoRenderPipeline` thread-safety panic on session
teardown (`crates/media-gstreamer/src/render.rs`'s `Drop` impl called
`set_state(Null)` on the wrong thread, hard-aborting the whole process —
fixed by marshalling it onto the GTK main context); and
`gtk_dev_ui.rs`'s `HANG_SAFETY_NET_SECONDS` (120s), which force-quit
*every* session, healthy or not, because the timer was never cancelled
once a session was confirmed working — every real phone session looked
like a recurring white-screen blip exactly every 2 minutes. Now
cancelled once the video pipeline is confirmed built, matching this
constant's own always-stated ("safety net only") intent, and confirmed
via a real multi-cycle session running well past 2 minutes with no
force-quit.

**A fourth issue remains open and unresolved**: "return to desktop"
(the fullscreen↔windowed toggle from the settings panel) can hang the
*entire* compositor — not just this app's window, confirmed by the
taskbar going blank too — with no recourse but a physical reboot.
Real-hardware-reproducible, not yet root-caused with certainty. One
confirmed, real gap was found and fixed while investigating: the video
sink (`gtk4paintablesink`) exposes `window-width`/`window-height`
properties specifically so the embedding app can keep it informed of
the actual rendering surface size, and this project never set them —
now set at pipeline-build time
(`VideoRenderPipeline::set_window_size`). This is a plausible
contributor (a resize the sink was never told about) but **not
confirmed as the full fix** — doing so would need updating this live on
every resize, which requires reaching into the video pipeline from a
different thread than the one that owns it, exactly the class of bug
that caused the thread-safety panic above; that part was deliberately
not attempted without a way to verify it on real hardware first.
**Consequently, `HeadUnitSettings::launch_on_boot` now defaults to
`false`** (2026-08-19, deliberate operator decision, not an oversight):
an unattended appliance that can silently hang on boot with no way to
intervene is worse than one that boots to a plain, always-usable
desktop. Do not default this back to `true` until the compositor hang
above is confirmed fixed on real hardware.

Separately (from the earlier, now-abandoned dedicated-account design):
real-hardware testing confirmed a `labwc` session only starts correctly
once its VT is actually the foreground/active one — a background VT
produces a silent exit after ~10 seconds with no output, the same
seat/DRM-access pattern `cage` needed a `WLR_DRM_DEVICES` override for
(the real DSI panel is `card2`, not the disconnected `card1` HDMI node
autodetection preferred). Not expected to be relevant to the current
design (a normal LightDM autologin for an already-working account
already brings the session up in the foreground, proven every day by
the operator's own desktop), but noted here in case the same symptom
ever reappears.

The `wayvnc` PAM change made earlier the same day
(`/etc/pam.d/wayvnc`, `pam_allow_desktopuser.so` removed) is unrelated
to this design — it was investigated as a blocker for the abandoned
dedicated-account design, where a separate login identity with no
password could have locked the operator out of VNC. Under the current
design, VNC and the desktop are always the same account, so that
specific risk never applied — the PAM change is a harmless, low-risk
simplification (`wayvnc` already accepts any authenticated local user
and shows whatever's on screen regardless), not a required fix.

## Current known-safe state

As of 2026-08-19, this development Pi's boot configuration is:
`lightdm.service` **enabled**, `autologin-user` **`blakereuben`**
(`/etc/lightdm/lightdm.conf`, pre-existing, unchanged by this package).
`/home/blakereuben/.config/labwc/autostart` now exists and will run the
fullscreen app on the next login/reboot if preflight passes. Confirm
with:

```
grep -i autologin-user /etc/lightdm/lightdm.conf
```

Reboot to actually exercise the new autostart file — not yet done as of
this writing. If it doesn't reach the fullscreen app as expected, log in
normally (SSH or the greeter) and check `~/.config/labwc/autostart` was
written as expected and that `aa-headunit-diagnostics preflight`
succeeds when run by hand.

## Investigating a crash after the fact

Requires persistent journald logging — the package now ships this (see
below); if it's ever missing (a manual/dev-only install without the
package's `postinst`), apply it by hand first, or the crashed boot's
logs are gone the moment the board reboots.

```
journalctl --list-boots
sudo journalctl -b -1 --no-pager | tail -150   # the crashed boot, tail end
sudo journalctl -b -1 -k --no-pager | grep -iE 'BUG:|Oops|panic|hung task'
sudo journalctl -b -1 --no-pager | grep -i 'power key'
sudo journalctl -b -1 --user-unit labwc --no-pager   # the operator's own labwc desktop session
vcgencmd get_throttled   # only reflects the *current* boot, not the crashed one
vcgencmd measure_temp
```

## Persistent, bounded journald logging

Raspberry Pi OS ships its own default that keeps the journal in memory
only (`/usr/lib/systemd/journald.conf.d/40-rpi-volatile-storage.conf`,
`Storage=volatile`) — sensible for reducing SD-card wear on a normal
install, but it means any crash's logs are lost on reboot unless this is
overridden.

**The package now ships this override**
(`packaging/systemd/journald-40-rpi-volatile-storage.conf`, installed to
`/etc/systemd/journald.conf.d/40-rpi-volatile-storage.conf` and applied
by `postinst`), satisfying the M5 checklist's "produce privacy-safe
diagnostics and bounded journald logging" item: `Storage=persistent`
plus a real size bound (`SystemMaxUse=200M`, `SystemKeepFree=100M`,
`MaxRetentionSec=30day` — a reasoned starting point, not measured
against a specific device's storage size) so an unattended appliance can
never let its own logs fill the disk.

A **same-named** drop-in in `/etc` is required to actually take effect —
real-hardware finding: a differently-named `/etc` drop-in setting
`Storage=persistent` did **not** override the vendor default in testing,
despite `systemd-analyze cat-config` showing it as the last-applied
value. This is why the installed filename deliberately matches the
vendor drop-in's exactly, shadowing it, rather than using a
project-specific name.

If applying this by hand (e.g. a manual/dev-only install without
`postinst`):

```
sudo mkdir -p /etc/systemd/journald.conf.d
sudo cp packaging/systemd/journald-40-rpi-volatile-storage.conf \
    /etc/systemd/journald.conf.d/40-rpi-volatile-storage.conf
sudo systemctl restart systemd-journald
sudo journalctl --flush   # migrates the already-open runtime journal immediately;
                           # a plain restart alone was not enough in testing
```

Verify with `sudo journalctl --header | grep -i path` — the active file
path should be under `/var/log/journal/`, not `/run/log/journal/`.

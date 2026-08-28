# PiOS Lite appliance boot path (optional, second install method)

An alternate install method for an operator who wants a dedicated
appliance with no desktop environment underneath at all, in exchange for
a faster, leaner boot. **This is not the recommended default install** —
see the main [`README.md`](../../README.md) and
[`packaging/README.md`](../../packaging/README.md) for that. This method
trades away the operator's own desktop/VNC access entirely; only use it
if that trade-off is genuinely wanted.

## Why this exists, and why it's separate from the default path

`MILESTONE_CHECKLIST.md`'s M8 section has an open item for exactly this:
a "full-PiOS-image-no-desktop install method (fastest boot, labwc
stripped, no 'return to desktop')." The default install method
deliberately assumes an existing PiOS Desktop and autologins as whoever
already uses the machine — see
[`appliance-recovery.md`](appliance-recovery.md)'s "Design history"
section for why two earlier, adjacent designs (a bare `cage` kiosk on a
spare VT, and a dedicated `aa-headunit` login account) were both dropped
for the *default* path specifically: "the operator wants a real desktop
underneath the kiosk app, which is a hard requirement, not just a
nice-to-have" for that path. This document is for the different case
where an operator explicitly does *not* want a desktop at all — a
strictly separate, opt-in second method, not a replacement.

## What's different from the default install

Both methods reuse the exact same app-launch chain unchanged: `labwc`
running `~/.config/labwc/autostart`
(`packaging/labwc/aa-headunit-autostart`, shipped at
`/usr/share/aa-headunit/labwc-autostart`) → `aa-headunit-diagnostics
preflight` → `usb kiosk --allow-live-aap`. Both also end up installed
through the same real, single-file end-user installer
(`packaging/aa-headunit-installer.sh` + `packaging/build-installer.sh` —
self-contained, no separate `.deb` or shortcut alongside it, doesn't
reference any fixed account/path) — an operator never runs
`dpkg`/`apt install` on the `.deb` directly on either path. Only *what
starts labwc in the first place*, and *what has to happen before the
installer can show its wizard*, differ:

| | Default (PiOS Desktop) | This method (PiOS Lite) |
|---|---|---|
| Base image | PiOS with Desktop | PiOS Lite |
| Login | LightDM autologin, graphical session | console (`tty1`) autologin, text boot target |
| Compositor | `labwc` (already installed as part of `rpd-labwc`) | `labwc` (installed at flash time by `appliance-lite-flash.sh` — not present on Lite by default) |
| Desktop shell | full Raspberry Pi desktop (panel, file manager, wallpaper) | none — nothing but the app's own window |
| Remote access / fallback | `wayvnc` (desktop is visible/usable over VNC) | none — SSH only, and only if a `--ssh-key` was given at flash time |
| When the installer runs | operator double-clicks it manually, whenever they're ready | automatically, the first time the appliance boots — no manual step at all |

`cage` (a minimal single-purpose Wayland kiosk compositor) was
considered and specifically **not** used here — a prior session already
tried it and shelved it with real, unresolved problems: it became
unresponsive to `SIGTERM` while holding the display, its DRM output
auto-selection picked a disconnected HDMI node instead of the connected
panel, and it fought `wayvnc` for the same output on VT switch. Reusing
`labwc` avoids reopening any of that, at the cost of `labwc` being a
general-purpose compositor rather than a kiosk-only one — a small,
acceptable overhead compared to those known bugs.

## Setup

Everything happens once, on the dev machine, before the SD card is ever
booted — `appliance-lite-flash.sh` flashes PiOS Lite, provisions a
console-autologin `labwc` session, and stages the real single-file
installer so first boot goes straight into it. No SSH step, no manual
script run on the Pi itself.

1. Download and verify a PiOS Lite (not Desktop) `.img.xz` yourself
   (`sha256sum` against the checksum Raspberry Pi publishes).
2. Build the `.deb` (`dpkg-buildpackage`, same as any other build).
3. Run the flashing tool, targeting the unmounted SD card:

   ```
   packaging/appliance-lite-flash.sh \
       --device /dev/sdX \
       --user <username> \
       --base-image raspios-lite-arm64.img.xz \
       --expected-sha256 <checksum> \
       --deb ../aa-headunit-diagnostics_X.Y.Z_arm64.deb \
       [--ssh-key ~/.ssh/some_key.pub]
   ```

   `--device` is never auto-detected — double-check it before running
   this, it's a full-card overwrite. `--ssh-key` is optional and only
   for remote debugging convenience; the installer itself runs on the
   physical screen regardless.
4. Put the card in the Pi and boot it. `<username>` autologins on
   `tty1`, `labwc` starts, and the staged installer runs automatically —
   Bluetooth pairing, then the credential wizard (touch, or a
   keyboard/mouse plugged into the Pi), then install. `postinst` detects
   the active console session (`loginctl`, not desktop-specific) exactly
   as it would a graphical one, and wires up the real autostart file and
   staged credentials the same way the default install method does.
5. Once the installer's `apt install` succeeds, it reboots the Pi on its
   own. Confirm the app's own autostart chain reaches a live session on
   that reboot (`aa-headunit-diagnostics preflight` →
   `usb kiosk --allow-live-aap`). Only once that's confirmed on real
   hardware, turn on "launch on boot" from the app's own touch-gesture
   Settings panel — it defaults to off, same as the default install
   method, so a broken first boot never strands the operator without a
   way back in.

If the installer is cancelled, or the wizard window is closed without
finishing, nothing is installed and the same temporary autostart simply
runs again on the next boot — the appliance keeps offering the installer
until it actually succeeds, rather than needing manual retry logic.

## Recovery

No desktop, no VNC — recovery is SSH-only. This is already the
documented-first recovery path in
[`appliance-recovery.md`](appliance-recovery.md) regardless of which
install method is active (SSH stays responsive even when a compositor is
stuck holding the display), so no separate recovery document is needed
here — follow that one.

## Status

Not yet real-hardware-confirmed. `MILESTONE_CHECKLIST.md`'s M8 item
stays open until a genuine PiOS Lite boot has been measured end to end
on real hardware (console autologin → `labwc` → the installer running
automatically and rendering on that session → the app reaching a live
session after the automatic reboot) and its boot-to-ready time compared
against the existing ~10s Desktop-based baseline.

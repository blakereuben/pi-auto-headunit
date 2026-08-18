# Appliance recovery and development path

How the M5 appliance boots, what to do if the graphical (kiosk) service
fails or hangs the board, and how to get back to a normal development
environment. Written from a real incident (2026-08-18): enabling and
testing `aa-headunit-kiosk@.service` hard-hung this Pi 5 at least four
times, each requiring a physical power cycle. The root cause is not yet
confirmed — see `packaging/systemd/aa-headunit-kiosk@.service`'s own doc
comment for the full investigation. This document assumes that failure
mode can recur and describes how to work around it, not how to prevent
it.

## How the appliance boots

- `aa-headunit-preflight.service` (a oneshot unit, runs as the dedicated
  `aa-headunit` user) checks the five conditions `ARCHITECTURE.md` §9
  requires before the graphical service should even attempt to start: a
  connected display, a touchscreen input device, a writable
  `/var/lib/aa-headunit`, working USB access, and at least one audio
  device. Run it by hand any time with
  `aa-headunit-diagnostics preflight` or `systemctl start
  aa-headunit-preflight.service`.
- `aa-headunit-kiosk@<VT>.service` (a template unit, e.g.
  `aa-headunit-kiosk@tty2.service`) depends on preflight succeeding
  (`Requires=`/`After=`) and runs `cage` (a minimal Wayland kiosk
  compositor) with this project's own binary as its single fullscreen
  client, on whichever virtual terminal it's instantiated against.
- Neither unit is enabled by the package install itself — `postinst`
  only runs `systemctl daemon-reload` so `systemctl` is aware of them.
  Enabling the kiosk unit, and picking which VT to instantiate it
  against, is a deliberate operator decision (`packaging/README.md`).

## Current known-safe state

As of 2026-08-18, this development Pi's boot configuration is
deliberately left as: `lightdm.service` **enabled** (boots into the
normal interactive desktop), `aa-headunit-kiosk@tty2.service`
**disabled**. A plain reboot boots the desktop, not the kiosk. Confirm
with:

```
systemctl is-enabled lightdm.service aa-headunit-kiosk@tty2.service
```

Do not enable the kiosk unit expecting the hang to be fixed — it hasn't
been confirmed fixed. See the unit file's own doc comment before trying
again.

## If the kiosk service hangs the board

Real-hardware symptom (observed repeatedly): a VNC session watching the
display goes grey and stops updating, and a directly-attached keyboard
stops responding. No kernel panic, oops, or hung-task warning has ever
appeared in the journal for a crashed boot — this looks like a
firmware/GPU-level lockup the OS itself never gets to log, not a
userspace crash with a stack trace to chase.

**SSH access kept working throughout every one of these hangs this
session.** If you have an SSH session open (or can open a new one — the
network stack and `sshd` are unaffected by a hung display), that is the
reliable way to investigate and recover:

```
# Confirm what's actually running/enabled
systemctl is-enabled lightdm.service aa-headunit-kiosk@tty2.service
systemctl status aa-headunit-kiosk@tty2.service

# Disable the kiosk and restore the normal desktop boot
sudo systemctl disable aa-headunit-kiosk@tty2.service
sudo systemctl enable lightdm.service
```

If SSH is also unresponsive (not observed yet, but plan for it): the
board needs a physical power cycle. A short press of the power button
is caught by `systemd-logind` as a normal shutdown request and — if the
kernel and `systemd-logind` are still alive underneath the hung display,
which they were in every occurrence so far — produces a clean, orderly
shutdown (filesystems synced, services stopped in order). This is worth
knowing: **a clean-looking shutdown sequence in the journal after a
hang is evidence the recovery action worked, not evidence nothing was
actually wrong.** Cross-check by searching for `Power key pressed` in
the crashed boot's journal (see below) — its presence confirms a manual
recovery happened at that point, not a spontaneous graceful shutdown.

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

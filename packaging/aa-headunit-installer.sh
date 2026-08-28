#!/bin/sh
# packaging/aa-headunit-installer.sh — the real, single-file end-user
# installer. Operator's explicit direction, 2026-08-28: exactly one file,
# nothing hardcoded to any account/path, since this is what a new user
# downloads/copies and just runs — it must work regardless of where it
# ends up (a Desktop, a USB stick, ~/Downloads, anywhere).
#
# This source file has no payload of its own — `packaging/build-installer.sh`
# appends a real `.deb` after the `exit 0`/marker line below to produce
# the actual distributable single file. Everything below `exit 0` is raw
# binary `.deb` bytes, never shell — nothing after that line is ever
# reached by execution.
#
# Same guided flow as the multi-file `packaging/setup.sh` this supersedes
# for end-user distribution (Bluetooth pairing → credential wizard →
# install, `credentials_setup_wizard.rs`) — collects credentials from an
# *extracted* copy of the embedded package (has to work before the real
# package exists on the system at all), stages them under the operator's
# own home directory (`credentials::staging_paths()`), then installs the
# real package, whose own `postinst` picks the staged credentials up.
set -e

self="$0"
case "$self" in
    /*) ;;
    *) self="$(pwd)/$self" ;;
esac

if ! command -v dpkg-deb >/dev/null 2>&1; then
    echo "dpkg-deb is required (part of dpkg, standard on Raspberry Pi OS)." >&2
    exit 1
fi

marker_line=$(grep -a -n '^#__AA_HEADUNIT_DEB_PAYLOAD_BELOW__$' "$self" | head -n1 | cut -d: -f1)
if [ -z "$marker_line" ]; then
    echo "This file is missing its embedded package — it wasn't built with packaging/build-installer.sh." >&2
    exit 1
fi
payload_line=$((marker_line + 1))

work_dir=$(mktemp -d)
cleanup() {
    rm -rf "$work_dir"
}
trap cleanup EXIT INT TERM

deb="$work_dir/aa-headunit-diagnostics.deb"
tail -n +"$payload_line" "$self" >"$deb"

# On the default install method (an existing PiOS Desktop) this script
# already runs inside that real desktop session, so GTK/Wayland/the
# file-picker portal all just work with no extra effort. On the PiOS
# Lite appliance install method (docs/development/pios-lite-appliance.md)
# there is no desktop for this script to inherit a session from when
# it's run from an SSH login instead of the physical console — this
# looks for the Wayland/D-Bus session already running on the physical
# screen and targets that instead. A real desktop session already has
# these set, so this is a no-op there.
find_lite_session() {
    if [ -n "$WAYLAND_DISPLAY" ]; then
        return 0
    fi
    runtime_dir="/run/user/$(id -u)"
    # shellcheck disable=SC2012 # portable enough for this one-shot glob
    wayland_socket=$(ls "$runtime_dir"/wayland-[0-9]* 2>/dev/null | head -n1)
    if [ -z "$wayland_socket" ]; then
        return 0
    fi
    WAYLAND_DISPLAY=$(basename "$wayland_socket")
    export WAYLAND_DISPLAY
    if [ -z "$DBUS_SESSION_BUS_ADDRESS" ] && [ -S "$runtime_dir/bus" ]; then
        DBUS_SESSION_BUS_ADDRESS="unix:path=$runtime_dir/bus"
        export DBUS_SESSION_BUS_ADDRESS
    fi
    echo "No graphical session in this shell — found the appliance session on the physical screen ($WAYLAND_DISPLAY) and will show the wizard there instead."
}
find_lite_session

echo "Extracting the embedded package to collect your certificate and private key first..."
extract_dir="$work_dir/extract"
dpkg-deb -x "$deb" "$extract_dir"

wizard="$extract_dir/usr/bin/aa-headunit-diagnostics"
if [ ! -x "$wizard" ]; then
    echo "Embedded package doesn't contain usr/bin/aa-headunit-diagnostics — built incorrectly?" >&2
    exit 1
fi

# `credentials setup` leads with a Bluetooth-pairing page before the
# credential file picker (`credentials_setup_wizard.rs`'s
# `build_pair_bluetooth_page`). `set +e`/`$?` instead of `|| true`: a
# plain `|| true` would swallow every wizard exit code indistinguishably
# and barrel on into `apt install` below even after the wizard's own
# "Cancel Installation" button already uninstalled the package.
# `CANCELLED_EXIT_CODE` (42) is a specific, deliberately-chosen exit code
# so that case can be told apart from every other nonzero wizard exit
# (e.g. the window just being closed without finishing), which should
# still fall through to install in case credentials were already staged
# from an earlier run.
set +e
# GTK_A11Y=none: real-hardware finding, 2026-08-28 — GTK auto-starts the
# accessibility bus (at-spi-bus-launcher/at-spi2-registryd) for any GTK4
# app, including this wizard, regardless of whether anything uses it.
# The older NO_AT_BRIDGE=1 was tried first and confirmed to have no
# effect at all on GTK4 (it only ever controlled the GTK2/3-era
# ATK-bridge, which GTK4 dropped for its own native AT-SPI2
# integration); GTK_A11Y=none is the GTK4-specific equivalent and is
# what actually stops it, confirmed real-hardware. Once started it
# stays running for the rest of the session even after the wizard
# exits — setting this only on the later kiosk app launch (which this
# project's shared production autostart already does) is not enough on
# its own; the wizard itself needs it too, or the accessibility bus is
# already up by the time the kiosk starts and this saving never
# actually happens.
GTK_A11Y=none "$wizard" credentials setup
wizard_exit_code=$?
set -e
if [ "$wizard_exit_code" -eq 42 ]; then
    echo "Setup was cancelled — the app has already been uninstalled. Nothing more to do."
    exit 0
fi

echo "Installing..."
# --reinstall: plain `apt install` silently no-ops ("already the newest
# version") whenever this exact package/version is already on the
# machine, which would strand a real operator's just-staged credentials
# with zero error or feedback since `postinst` never gets a chance to
# run again.
sudo apt install --reinstall -y "$deb"

# Remove this installer itself from wherever it was run — its job is
# done, and the app's own `postinst` has already put the real app +
# uninstaller icons in place.
rm -f "$self"

echo "Done. Credentials staged during setup have been installed automatically."
exit 0
#__AA_HEADUNIT_DEB_PAYLOAD_BELOW__

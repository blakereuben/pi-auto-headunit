#!/bin/sh
# packaging/setup.sh — the actual "installer" an operator runs: collects
# the Android Auto certificate/private key first (via the same guided
# GTK4 wizard `credentials setup` shows, `credentials_setup_wizard.rs`),
# then installs the .deb. Operator's explicit direction, 2026-08-24:
# "create a batch file kind of that gets the creds and places them in a
# folder and when thats done it clicks on the .deb file" — a real
# Windows-installer-style single entry point, not two separate manual
# steps to remember.
#
# Runs the wizard from an *extracted* copy of the .deb, not the
# installed one — this has to work before the package exists on the
# system at all. On the default install method (an existing PiOS
# Desktop) this script itself already runs inside that real desktop
# session, so GTK/Wayland/the file-picker portal all just work with no
# extra effort. On the PiOS Lite appliance install method
# (docs/development/pios-lite-appliance.md) there is no desktop for
# this script to inherit a session from — it's normally invoked from a
# plain SSH login instead, after `appliance-lite-setup.sh` has already
# bootstrapped a bare labwc session on the physical console (tty1).
# `find_lite_session` below detects that already-running session (its
# Wayland socket and D-Bus session bus, both under
# /run/user/<uid>/) and exports the same environment variables a real
# desktop login would already have set, so the wizard still renders —
# on the physical screen, not over SSH — with no manual env-var setup
# needed either way. A real desktop session already has these set, so
# this is a no-op there.
#
# The wizard always stages the picked files under the operator's own
# home directory (`credentials::staging_paths()`,
# `~/.local/share/aa-headunit/pending-credentials/`) rather than writing
# anywhere privileged — `postinst` (`packaging/debian/aa-headunit-diagnostics.postinst`)
# picks that staged pair up and installs it for real as its very next
# step, once the actual package (and the `aa-headunit` group/directory
# it creates) exists.
#
# `sudo apt install` (not raw `dpkg -i`) so any missing runtime
# dependency this package declares gets pulled in too.
set -e

usage() {
    echo "Usage: $0 [path/to/aa-headunit-diagnostics_*.deb]" >&2
    exit 2
}

if [ "$#" -gt 1 ]; then
    usage
fi

if [ "$#" -eq 1 ]; then
    deb="$1"
else
    # shellcheck disable=SC2012 # portable enough for this one-shot glob
    deb=$(ls ./aa-headunit-diagnostics_*.deb 2>/dev/null | head -n1)
    if [ -z "$deb" ]; then
        echo "No aa-headunit-diagnostics_*.deb found in the current directory." >&2
        usage
    fi
fi

if [ ! -f "$deb" ]; then
    echo "$deb: not found" >&2
    exit 1
fi

if ! command -v dpkg-deb >/dev/null 2>&1; then
    echo "dpkg-deb is required (part of dpkg, standard on Raspberry Pi OS)." >&2
    exit 1
fi

# PiOS Lite appliance support (docs/development/pios-lite-appliance.md):
# if this shell doesn't already have a graphical session (the normal
# case for an SSH login, as opposed to a real desktop terminal), look
# for the one appliance-lite-setup.sh already bootstrapped on the
# physical console for this same user and target it instead. Only
# touches the environment when $WAYLAND_DISPLAY is unset, so a real
# desktop session (which already has it) is never affected.
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

extract_dir=$(mktemp -d)
cleanup() {
    rm -rf "$extract_dir"
}
trap cleanup EXIT INT TERM

echo "Extracting $deb to collect your certificate and private key first..."
dpkg-deb -x "$deb" "$extract_dir"

wizard="$extract_dir/usr/bin/aa-headunit-diagnostics"
if [ ! -x "$wizard" ]; then
    echo "$deb doesn't contain usr/bin/aa-headunit-diagnostics — wrong package?" >&2
    exit 1
fi

# `credentials setup` itself now leads with a Bluetooth-pairing page
# before the credential file picker (`credentials_setup_wizard.rs`'s
# `build_pair_bluetooth_page`) — operator's explicit direction,
# 2026-08-26: that step belongs in the same GTK wizard as the credential
# setup, not a separate terminal-based step here beforehand.
#
# `set +e`/`$?` instead of the old `|| true`: real-hardware finding,
# 2026-08-26 — a plain `|| true` swallows *every* wizard exit code
# indistinguishably, so this script used to barrel on into `apt install
# --reinstall` below even when the operator had just hit the wizard's own
# "Cancel Installation" button, silently reinstalling the very package
# that button had just uninstalled (confirmed via `journalctl`: the
# `pkexec`/`apt purge` it ran genuinely succeeded — this script simply
# never checked). `CANCELLED_EXIT_CODE` (`credentials_setup_wizard.rs`)
# is a specific, deliberately-chosen exit code so this can tell "the
# operator cancelled and uninstalled" apart from every other nonzero
# wizard exit (e.g. the window just being closed without finishing),
# which should still fall through to install below in case credentials
# were already staged from an earlier run.
set +e
"$wizard" credentials setup
wizard_exit_code=$?
set -e
if [ "$wizard_exit_code" -eq 42 ]; then
    echo "Setup was cancelled — the app has already been uninstalled. Nothing more to do."
    exit 0
fi

echo "Installing $deb..."
# --reinstall: plain `apt install` silently no-ops ("already the newest
# version") whenever this exact package/version is already on the
# machine — real finding, 2026-08-24: that left a real operator's
# staged credentials stranded with zero error or feedback, because
# `postinst` (which adopts them) never got a chance to run again. This
# project's own install history already established `--reinstall` as
# the right fix for exactly this local-.deb-reinstall case.
sudo apt install --reinstall -y "$deb"

# Operator's explicit direction, 2026-08-26: exactly two icons should be
# left on the Desktop after running this installer — the app itself and
# the uninstaller (both just installed by the package's own `postinst`,
# `aa-headunit.desktop`/`aa-headunit-uninstall.desktop`) — not a third,
# lingering "Install" icon whose job is now done. Only ever removes the
# fixed, well-known Desktop-shortcut path this project's own install
# convention uses; a no-op if it's not there (run from a terminal, no
# such shortcut, already removed, etc — including every PiOS Lite
# appliance install, which has no Desktop folder at all).
rm -f "$HOME/Desktop/aa-headunit-install.desktop"

echo "Done. Credentials staged during setup have been installed automatically."

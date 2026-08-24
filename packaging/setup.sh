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
# system at all. It runs as the plain, already-logged-in operator (no
# root, no runuser/session juggling needed: this script itself already
# runs in their real desktop session), so GTK/Wayland/the file-picker
# portal all just work. The wizard always stages the picked files under
# the operator's own home directory (`credentials::staging_paths()`,
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

"$wizard" credentials setup || true

echo "Installing $deb..."
# --reinstall: plain `apt install` silently no-ops ("already the newest
# version") whenever this exact package/version is already on the
# machine — real finding, 2026-08-24: that left a real operator's
# staged credentials stranded with zero error or feedback, because
# `postinst` (which adopts them) never got a chance to run again. This
# project's own install history already established `--reinstall` as
# the right fix for exactly this local-.deb-reinstall case.
sudo apt install --reinstall -y "$deb"

echo "Done. Credentials staged during setup have been installed automatically."

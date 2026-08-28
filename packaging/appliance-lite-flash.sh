#!/bin/sh
# packaging/appliance-lite-flash.sh — dev-machine tool that flashes a
# PiOS Lite SD card for the appliance install method
# (docs/development/pios-lite-appliance.md): writes the image, provisions
# a console-autologin labwc session, and stages the real single-file
# installer (packaging/aa-headunit-installer.sh, built via
# packaging/build-installer.sh) so first boot goes straight into it —
# Bluetooth pairing → credential wizard → install — with no manual SSH
# step, and no network access needed on the appliance itself at any
# point (this Pi runs in a car, not next to a router). Run this on the
# dev machine, targeting the *unmounted* SD card; it is not something
# you run on the Pi being provisioned.
#
# --base-image must be an appliance base image already containing
# labwc/dbus-user-session/udisks2/gvfs/gvfs-backends — produced once (it
# needs network) by packaging/build-appliance-base-image.sh from a
# stock PiOS Lite download, then reused here for as many flashes as you
# want, each one fast and fully offline. This script itself never calls
# apt — see build-appliance-base-image.sh's own header for why that
# split exists (a real-hardware finding: installing packages during
# firstrun.sh, on the appliance's own first boot, is neither reliable —
# no guarantee the network is even up that early — nor acceptable for a
# device meant to work with no network at all).
#
# Reuses the exact `rpi-imager --cli --first-run-script` invocation
# proven real-hardware, 2026-08-27/28: a base image's own SHA256 is
# verified independently before ever touching the device (rpi-imager's
# own post-write verify compares against the *pristine* image's hash
# even though `--first-run-script` deliberately changes what gets
# written, so it always reports a false "corrupt" mismatch once
# customization is used — `--disable-verify` is passed to rpi-imager for
# that reason, not to skip integrity checking altogether).
set -e

usage() {
    cat >&2 <<EOF
Usage: $0 --device /dev/sdX --user NAME --base-image FILE --deb FILE
          [--expected-sha256 HASH] [--ssh-key FILE]...

  --device           target block device for the SD card (required;
                      never auto-detected — get this wrong and you wipe
                      the wrong drive)
  --user             account to create on first boot (required)
  --base-image       appliance base image (.img) produced by
                      packaging/build-appliance-base-image.sh — not a
                      stock PiOS Lite download (required)
  --deb               built aa-headunit-diagnostics .deb to stage as the
                      installer's embedded payload (required)
  --expected-sha256   sha256 of --base-image to verify before flashing
                      (optional but recommended)
  --ssh-key           pubkey file to preseed into authorized_keys, for
                      remote debugging convenience only (optional,
                      repeatable) — not required for the installer
                      experience itself, which runs on-screen
EOF
    exit 2
}

device=""
target_user=""
base_image=""
deb=""
expected_sha256=""
ssh_keys=""

while [ "$#" -gt 0 ]; do
    case "$1" in
        --device) device="$2"; shift 2 ;;
        --user) target_user="$2"; shift 2 ;;
        --base-image) base_image="$2"; shift 2 ;;
        --deb) deb="$2"; shift 2 ;;
        --expected-sha256) expected_sha256="$2"; shift 2 ;;
        --ssh-key) ssh_keys="$ssh_keys $2"; shift 2 ;;
        *) usage ;;
    esac
done

[ -n "$device" ] && [ -n "$target_user" ] && [ -n "$base_image" ] && [ -n "$deb" ] || usage
[ -b "$device" ] || { echo "$device: not a block device" >&2; exit 1; }
[ -f "$base_image" ] || { echo "$base_image: not found" >&2; exit 1; }
[ -f "$deb" ] || { echo "$deb: not found" >&2; exit 1; }
for key in $ssh_keys; do
    [ -f "$key" ] || { echo "$key: not found" >&2; exit 1; }
done

# The single most important safety check this script has: never let
# --device point at whatever this machine itself is currently booted
# from.
current_root_device=$(findmnt -no SOURCE / | sed -E 's/p?[0-9]+$//')
device_resolved=$(readlink -f "$device")
if [ "$(readlink -f "$current_root_device" 2>/dev/null)" = "$device_resolved" ]; then
    echo "$device looks like this machine's own boot device — refusing to touch it." >&2
    exit 1
fi

if [ -n "$expected_sha256" ]; then
    echo "Verifying $base_image against the expected checksum..."
    actual_sha256=$(sha256sum "$base_image" | cut -d' ' -f1)
    if [ "$actual_sha256" != "$expected_sha256" ]; then
        echo "Checksum mismatch for $base_image:" >&2
        echo "  expected: $expected_sha256" >&2
        echo "  actual:   $actual_sha256" >&2
        exit 1
    fi
    echo "Checksum OK."
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
cleanup() {
    rm -rf "$work_dir"
}
trap cleanup EXIT INT TERM

echo "Building the single-file installer from $deb..."
installer_name="Install Android Auto Head Unit.sh"
"$script_dir/build-installer.sh" "$deb" "$work_dir/$installer_name"

firstrun="$work_dir/firstrun.sh"
{
    printf '%s\n' '#!/bin/bash'
    printf '%s\n' 'set -u'
    printf '\n'
    printf 'target_user=%s\n' "$(printf '%s' "$target_user" | sed "s/'/'\\\\''/g; s/^/'/; s/\$/'/")"
    printf '\n'
    cat <<'FIRSTRUN_BODY'
if ! id -u "$target_user" >/dev/null 2>&1; then
    useradd -m -s /bin/bash -G sudo,adm,dialout,cdrom,audio,video,plugdev,games,users,input,render,netdev,spi,i2c,gpio "$target_user"
fi
target_home=$(getent passwd "$target_user" | cut -d: -f6)

FIRSTRUN_BODY

    if [ -n "$ssh_keys" ]; then
        printf '%s\n' 'install -o "$target_user" -g "$target_user" -m 700 -d "$target_home/.ssh"'
        printf '%s\n' 'cat > "$target_home/.ssh/authorized_keys" <<'"'"'PUBKEYS'"'"''
        for key in $ssh_keys; do
            cat "$key"
        done
        printf '%s\n' 'PUBKEYS'
        printf '%s\n' 'chown "$target_user:$target_user" "$target_home/.ssh/authorized_keys"'
        printf '%s\n' 'chmod 600 "$target_home/.ssh/authorized_keys"'
        printf '%s\n' 'systemctl enable ssh || true'
        printf '\n'
    fi

    cat <<'FIRSTRUN_BODY'
# Pubkey-only where a key was preseeded above; otherwise no remote
# access at all until the operator sets it up themselves — either way,
# no password login, since this account exists to run an unattended
# graphical installer, not to be SSH'd into with a guessable password.
passwd -l "$target_user" || true

echo "$target_user ALL=(ALL) NOPASSWD:ALL" > "/etc/sudoers.d/010-$target_user"
chmod 440 "/etc/sudoers.d/010-$target_user"

# labwc/dbus-user-session/udisks2/gvfs/gvfs-backends are already
# installed onto this card's filesystem below, before first boot ever
# happens (see the chroot step after rpi-imager writes the image) — not
# via apt here. The appliance must work with no internet access at all
# on first boot (this Pi runs in a car, not next to a router); the only
# place this project ever needs real network access is once, on the dev
# machine preparing the card, which is what that chroot step uses it
# for. Real-hardware finding, 2026-08-28, is exactly why this rule
# exists: an earlier version of this script called apt-get here, during
# firstrun.sh itself, and on a machine where the network genuinely
# wasn't up yet that early in boot it failed silently (this script only
# has `set -u`, not `set -e`, by design — one failed step must not
# abort the rest of provisioning) while autologin/bash_profile/autostart
# still got wired up regardless — console autologin execing a
# nonexistent `labwc` binary crash-loops `getty@tty1` (systemd's own
# restart-rate-limit trips within seconds) with nothing but a blank
# cursor to show for it.
# Real-hardware finding, 2026-08-28: `raspi-config nonint
# do_boot_behaviour B2` silently failed to create the autologin drop-in
# when run from firstrun.sh's own early-boot context — confirmed on a
# card where `do_hostname` (run right after it, same block) succeeded
# but no `/etc/systemd/system/getty@tty1.service.d/autologin.conf` ever
# existed, even though the identical `raspi-config` command worked fine
# when run manually over SSH after boot had fully settled. Rather than
# depend on raspi-config's own live-systemd-state assumptions this
# early, write the exact same drop-in it would have written, directly —
# deterministic regardless of what's fully up yet at this point in boot.
# Real-hardware finding, 2026-08-28: on some boots the operator still
# saw a plain login prompt on tty1 instead of autologin, even with this
# drop-in written above — reproduced live: a manual write of the exact
# same file, followed immediately by `systemctl daemon-reload`, survived
# a real reboot cleanly every time; this script's own write, with no
# reload, did not always take effect for that same boot (most likely:
# getty@tty1.service can already be starting by the time firstrun.sh
# — itself a fairly late early-boot hook — gets to writing this file, so
# systemd needs telling to notice the new drop-in rather than running
# with whatever it already loaded). Rather than depend on winning that
# race, make it self-healing regardless of the exact cause: reload and
# restart the unit right after writing the drop-in for this boot, and
# also install a tiny oneshot service that re-verifies/recreates it on
# every future boot, before getty.target, so a one-off failure here can
# never turn into a permanent "no autologin" state.
write_autologin_conf() {
    mkdir -p "/etc/systemd/system/getty@tty1.service.d"
    cat > "/etc/systemd/system/getty@tty1.service.d/autologin.conf" <<AUTOLOGIN_CONF
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin $target_user --noclear %I \$TERM
AUTOLOGIN_CONF
}
write_autologin_conf
systemctl daemon-reload
systemctl restart getty@tty1.service || true

ensure_script="/usr/local/sbin/aa-headunit-ensure-autologin"
cat > "$ensure_script" <<ENSURE_SCRIPT
#!/bin/sh
conf="/etc/systemd/system/getty@tty1.service.d/autologin.conf"
if [ ! -f "\$conf" ]; then
    mkdir -p "\$(dirname "\$conf")"
    cat > "\$conf" <<AUTOLOGIN_CONF
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin $target_user --noclear %I \$TERM
AUTOLOGIN_CONF
    systemctl daemon-reload
fi
ENSURE_SCRIPT
chmod +x "$ensure_script"

cat > "/etc/systemd/system/aa-headunit-ensure-autologin.service" <<'ENSURE_UNIT'
[Unit]
Description=Ensure PiOS Lite appliance console autologin is configured
DefaultDependencies=no
Before=getty.target getty@tty1.service
After=local-fs.target

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/aa-headunit-ensure-autologin

[Install]
WantedBy=getty.target
ENSURE_UNIT
systemctl enable aa-headunit-ensure-autologin.service

if command -v raspi-config >/dev/null 2>&1; then
    raspi-config nonint do_hostname aa-headunit-lite || true
fi

bash_profile="$target_home/.bash_profile"
if command -v labwc >/dev/null 2>&1; then
    cat >>"$bash_profile" <<'BASH_PROFILE'

# aa-headunit appliance-lite-flash
if [ -z "$WAYLAND_DISPLAY" ] && [ "$(tty)" = "/dev/tty1" ]; then
    # WLR_DRM_NO_ATOMIC=1: real-hardware finding, 2026-08-28 — plain
    # labwc (not the Desktop install's labwc-pi wrapper, which this
    # appliance deliberately doesn't pull in — it drags in the whole
    # rpd-wayland-core desktop stack) shows harsh screen tearing/
    # flickering on Pi 5's VC4/KMS driver without this. A known wlroots
    # workaround for atomic-modesetting issues on this hardware;
    # confirmed fixed with it set, confirmed broken without it.
    exec env WLR_DRM_NO_ATOMIC=1 dbus-run-session -- labwc
fi
BASH_PROFILE
    chown "$target_user:$target_user" "$bash_profile"
else
    # Fails open, deliberately: a plain login shell on tty1 (not a
    # crash-looping one) still lets the operator SSH in — pubkey if one
    # was preseeded above, otherwise physically at the console — and
    # finish provisioning by hand rather than being locked out by a boot
    # loop with no recourse. Shouldn't happen on a card this script
    # itself prepared (the chroot step below guarantees labwc is
    # present before firstrun.sh ever runs) — this guard is for a card
    # whose rootfs was prepared some other way.
    echo "aa-headunit appliance-lite-flash: labwc is missing from this image — it was not set to autostart. SSH in (or use the console) and finish setup manually: sudo apt-get install labwc dbus-user-session udisks2 gvfs gvfs-backends" > "$target_home/APPLIANCE_SETUP_INCOMPLETE.txt"
    chown "$target_user:$target_user" "$target_home/APPLIANCE_SETUP_INCOMPLETE.txt"
fi

FIRSTRUN_BODY

    printf 'installer_name=%s\n' "$(printf '%s' "$installer_name" | sed "s/'/'\\\\''/g; s/^/'/; s/\$/'/")"
    cat <<'FIRSTRUN_BODY'
# The boot partition (FAT32) cannot carry the executable bit at all
# (confirmed real-hardware, 2026-08-27/28: chmod +x on a vfat mount is
# silently discarded) — copy the staged installer onto the real rootfs
# and chmod it there, where it actually sticks.
install -o "$target_user" -g "$target_user" -m 755 \
    "/boot/firmware/aa-headunit-installer/$installer_name" \
    "$target_home/$installer_name"

if command -v labwc >/dev/null 2>&1; then
    mkdir -p "$target_home/.config/labwc"
    cat > "$target_home/.config/labwc/autostart" <<AUTOSTART
#!/bin/sh
if "\$HOME/$installer_name" \\
    && command -v aa-headunit-diagnostics >/dev/null 2>&1; then
    # This install method's whole point is launching straight into the
    # app with no operator present to flip a switch — unlike the
    # default (Desktop) install, "launch on boot" defaults on here, not
    # off. /var/lib/aa-headunit and the aa-headunit group only exist
    # once the package above has actually installed (postinst creates
    # them), so this has to happen here, not earlier in firstrun.sh.
    printf 'launch_on_boot = true\n' | sudo tee /var/lib/aa-headunit/settings.toml >/dev/null
    sudo chown root:aa-headunit /var/lib/aa-headunit/settings.toml
    sudo chmod 0660 /var/lib/aa-headunit/settings.toml
    # No reboot needed, real-hardware finding 2026-08-28: a reboot here
    # was only ever a blunt way to get a fresh login — postinst just
    # added this account to several groups (aa-headunit, plugdev, video,
    # gpio...) that only take effect for a *new* login, not this
    # already-running one, so the app can't just be exec'd directly from
    # here (it would inherit this session's stale, pre-install group
    # membership and fail permission checks). \`sudo -u\` recomputes the
    # target user's group membership fresh at invocation regardless of
    # the calling shell's own — confirmed real-hardware: this reaches a
    # live session immediately, no reboot wait.
    if sudo -u $target_user env NO_AT_BRIDGE=1 /usr/bin/aa-headunit-diagnostics preflight; then
        exec sudo -u $target_user env NO_AT_BRIDGE=1 \\
            WAYLAND_DISPLAY="\$WAYLAND_DISPLAY" \\
            DBUS_SESSION_BUS_ADDRESS="\$DBUS_SESSION_BUS_ADDRESS" \\
            XDG_RUNTIME_DIR="\$XDG_RUNTIME_DIR" \\
            /usr/bin/aa-headunit-diagnostics usb kiosk --allow-live-aap
    fi
fi
AUTOSTART
    chown -R "$target_user:$target_user" "$target_home/.config/labwc"
fi

rm -f /boot/firmware/firstrun.sh /boot/firstrun.sh
for cmdline in /boot/firmware/cmdline.txt /boot/cmdline.txt; do
    if [ -f "$cmdline" ]; then
        sed -i \
            -e 's| systemd.run=[^ ]*||g' \
            -e 's| systemd.run_success_action=[^ ]*||g' \
            -e 's| systemd.unit=kernel-command-line.target||g' \
            "$cmdline" || true
    fi
done

exit 0
FIRSTRUN_BODY
} > "$firstrun"

echo "Flashing $base_image onto $device (this can take several minutes)..."
rpi-imager --cli \
    --disable-verify \
    --first-run-script "$firstrun" \
    "$base_image" \
    "$device"

echo "Staging the installer onto the boot partition..."
boot_part="${device}1"
[ -b "$boot_part" ] || boot_part="${device}p1"
mount_point="$work_dir/bootfs"
mkdir -p "$mount_point"
udisksctl mount -b "$boot_part" 2>/dev/null || mount "$boot_part" "$mount_point"
actual_mount=$(findmnt -no TARGET "$boot_part")
mkdir -p "$actual_mount/aa-headunit-installer"
cp "$work_dir/$installer_name" "$actual_mount/aa-headunit-installer/$installer_name"
udisksctl unmount -b "$boot_part" 2>/dev/null || umount "$actual_mount"

cat <<EOF

Done. Put the card in the Pi and boot it — no network needed on the
appliance at any point: $target_user autologins on tty1, labwc starts,
and the installer runs automatically — Bluetooth pairing, then the
credential wizard, then install. Once that succeeds it reboots on its
own into the normal kiosk app.
EOF

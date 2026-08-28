#!/bin/sh
# packaging/build-appliance-base-image.sh — dev-machine tool that
# produces a reusable PiOS Lite base image with everything the appliance
# install method (docs/development/pios-lite-appliance.md) needs already
# installed: labwc, dbus-user-session (the compositor + session bus a
# bare Lite install doesn't ship), udisks2/gvfs/gvfs-backends (so
# the credential wizard's GTK file picker can actually see a plugged-in
# USB drive — a bare Lite install has no automount stack at all, real
# hardware finding 2026-08-28: without this the operator has to SSH in
# and mount USB media by hand before they can even pick their
# certificate/key files, defeating the point of an on-screen installer),
# and pipewire/pipewire-pulse/wireplumber/libspa-0.2-bluetooth (a bare
# Lite install only pulls in PipeWire's client libraries as a dependency
# of aa-headunit-diagnostics itself, not an actual running session, and
# not the Bluetooth SPA plugin at all — real hardware finding
# 2026-08-28: without something local registered to handle it, BlueZ has
# nowhere to hand a Handsfree profile connection, so the wireless
# bootstrap's own paired-phone auto-reconnect always fails with
# "br-connection-profile-unavailable"; also needed regardless for real
# Android Auto audio playback, same as the Desktop install method).
#
# Build this once — it needs real network access, to install those
# packages. packaging/appliance-lite-flash.sh then flashes the result to
# as many SD cards as you want afterward, each one fast and completely
# offline: the appliance itself (running in a car, not next to a router)
# must never need network access, on first boot or ever, which is
# exactly why package installation doesn't happen there — an earlier
# design tried installing these packages from firstrun.sh, on the
# appliance's own first boot, and a real-hardware run confirmed that's
# neither reliable (no guarantee the network is even up that early in
# boot — this script only had `set -u`, not `set -e`, so a failed
# install did not abort the rest of provisioning, just silently left
# labwc missing while autologin/autostart still got wired up regardless,
# crash-looping `getty@tty1`) nor acceptable for a device meant to work
# with no network at all.
#
# Only correct when run on the same architecture/release as the target
# image (a Pi preparing an image for another Pi) — a native chroot, no
# qemu binfmt translation. Cross-arch preparation (e.g. from an x86 dev
# machine) is not supported by this script as written.
set -e

usage() {
    cat >&2 <<EOF
Usage: $0 --base-image FILE --output FILE [--expected-sha256 HASH]

  --base-image       local, stock PiOS Lite .img or .img.xz, already
                      downloaded (required; this script does not fetch
                      one)
  --output           where to write the resulting appliance base image
                      (required; always uncompressed .img, regardless of
                      whether --base-image was compressed)
  --expected-sha256  sha256 of the *decompressed* image to verify before
                      installing anything into it (optional but
                      recommended) — this is what Raspberry Pi's own
                      image catalog actually publishes
                      ("extract_sha256"), not a checksum of the .xz
                      download itself, so verification happens after
                      decompression either way, even when --base-image
                      was already a plain .img.
  --extra-size-mb    how much room to add to the rootfs partition before
                      installing anything (default: 4096). A stock Lite
                      image's rootfs partition is sized to just barely
                      fit itself — it's meant to auto-expand to fill the
                      whole SD card on the appliance's own first real
                      boot, not to have anything installed into it
                      beforehand — so without this there usually isn't
                      room to add labwc/gvfs's dependency tree (real
                      finding, 2026-08-28: a first attempt with no grow
                      step ran out of space partway through installing
                      GStreamer/Mesa libraries gvfs-backends pulls in,
                      corrupting the partial dpkg state).
EOF
    exit 2
}

base_image=""
output=""
expected_sha256=""
extra_size_mb=4096

while [ "$#" -gt 0 ]; do
    case "$1" in
        --base-image) base_image="$2"; shift 2 ;;
        --output) output="$2"; shift 2 ;;
        --expected-sha256) expected_sha256="$2"; shift 2 ;;
        --extra-size-mb) extra_size_mb="$2"; shift 2 ;;
        *) usage ;;
    esac
done

[ -n "$base_image" ] && [ -n "$output" ] || usage
[ -f "$base_image" ] || { echo "$base_image: not found" >&2; exit 1; }
if [ "$(id -u)" -ne 0 ]; then
    echo "Must be run as root (loop-mounting and chroot need it)." >&2
    exit 1
fi

work_dir=$(mktemp -d)
loop_dev=""
mount_dir="$work_dir/rootfs"
resolv_backup="$work_dir/resolv.conf.orig"
cleanup() {
    umount "$mount_dir/sys" 2>/dev/null || true
    umount "$mount_dir/proc" 2>/dev/null || true
    umount "$mount_dir/dev" 2>/dev/null || true
    umount "$mount_dir" 2>/dev/null || true
    if [ -n "$loop_dev" ]; then
        losetup -d "$loop_dev" 2>/dev/null || true
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT INT TERM

case "$base_image" in
    *.xz)
        echo "Decompressing $base_image to $output..."
        unxz -k -c "$base_image" > "$output"
        ;;
    *)
        echo "Copying $base_image to $output..."
        cp "$base_image" "$output"
        ;;
esac

if [ -n "$expected_sha256" ]; then
    echo "Verifying $output against the expected (decompressed) checksum..."
    actual_sha256=$(sha256sum "$output" | cut -d' ' -f1)
    if [ "$actual_sha256" != "$expected_sha256" ]; then
        echo "Checksum mismatch for $output:" >&2
        echo "  expected: $expected_sha256" >&2
        echo "  actual:   $actual_sha256" >&2
        exit 1
    fi
    echo "Checksum OK."
fi

echo "Growing $output by ${extra_size_mb}MiB for install headroom..."
truncate -s "+${extra_size_mb}M" "$output"

echo "Loop-mounting $output..."
loop_dev=$(losetup --show -f -P "$output")
rootfs_part="${loop_dev}p2"
[ -b "$rootfs_part" ] || rootfs_part="${loop_dev}2"

echo "Growing the rootfs partition and filesystem into the new space..."
growpart "$loop_dev" 2
# growpart resizes the partition table entry but the kernel's view of
# the loop device's partitions needs telling too, before e2fsck/resize2fs
# can see the new size.
partprobe "$loop_dev" 2>/dev/null || true
e2fsck -f -y "$rootfs_part"
resize2fs "$rootfs_part"

mkdir -p "$mount_dir"
mount "$rootfs_part" "$mount_dir"
mount --bind /dev "$mount_dir/dev"
mount --bind /proc "$mount_dir/proc"
mount --bind /sys "$mount_dir/sys"

# Real network access, briefly, on this dev machine only — the whole
# point of building this image ahead of time is so the appliance itself
# never needs any.
if [ -e "$mount_dir/etc/resolv.conf" ]; then
    cp -a "$mount_dir/etc/resolv.conf" "$resolv_backup" 2>/dev/null || true
fi
cp /etc/resolv.conf "$mount_dir/etc/resolv.conf"

# pipewire/pipewire-pulse/wireplumber/libspa-0.2-bluetooth: real-hardware
# finding, 2026-08-28 — this bare Lite image only pulls in PipeWire's
# client *libraries* as a dependency of aa-headunit-diagnostics itself
# (which links against them for its own audio pipeline), not an actual
# running PipeWire/WirePlumber session, and not libspa-0.2-bluetooth at
# all. Without something local registered to handle it, BlueZ has
# nowhere to hand a Handsfree profile connection — confirmed exactly
# this failure real-hardware (`connect_profile` returning
# "br-connection-profile-unavailable"), and confirmed fixed by
# installing and running these three. This is also not optional for a
# reason beyond Bluetooth: real Android Auto audio playback needs an
# actual running PipeWire session regardless, on this install method
# same as the Desktop one.
echo "Installing labwc, dbus-user-session, udisks2, gvfs, gvfs-backends, pipewire, pipewire-pulse, wireplumber, libspa-0.2-bluetooth into the image..."
chroot "$mount_dir" apt-get update
chroot "$mount_dir" apt-get install -y \
    labwc dbus-user-session udisks2 gvfs gvfs-backends \
    pipewire pipewire-pulse wireplumber libspa-0.2-bluetooth
chroot "$mount_dir" apt-get clean

if [ -f "$resolv_backup" ]; then
    cp -a "$resolv_backup" "$mount_dir/etc/resolv.conf"
else
    rm -f "$mount_dir/etc/resolv.conf"
fi

cat <<EOF

Done. $output is a ready-to-flash appliance base image (labwc, gvfs,
udisks2, and the PipeWire/Bluetooth audio stack already installed,
nothing else changed). Feed it to
packaging/appliance-lite-flash.sh's --base-image as many times as you
want — each flash from here on is fast and needs no network at all,
on this machine or on the appliance.

sha256: $(sha256sum "$output" | cut -d' ' -f1)
EOF

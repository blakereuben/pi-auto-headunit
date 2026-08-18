#!/bin/sh
# Produces the artifacts a release actually needs to be verifiable:
# a SHA256SUMS file listing every given artifact's checksum, and a
# detached, armored GPG signature over that checksums file — not a
# signature on each .deb individually, and not a full signed APT
# repository (Release/InRelease + apt-ftparchive) either. This matches
# what this project's own M8 checklist item asks for ("Publish a Pi 5
# preview .deb, checksums, source..."), the standard shape for a
# GitHub-Releases-style distribution, not a hosted package archive —
# a full APT repo is a real option later if this project ever needs
# `apt install` from a real repository, but is more infrastructure than
# a pre-1.0 project publishing standalone .deb files needs yet.
#
# Verifying a downloaded release (what an end user runs):
#   gpg --import packaging/release-signing-key.asc
#   sha256sum --check SHA256SUMS
#   gpg --verify SHA256SUMS.asc SHA256SUMS
#
# Usage: packaging/sign-release.sh <artifact>...
#   e.g. packaging/sign-release.sh ../aa-headunit-diagnostics_0.1.0-2_arm64.deb

set -eu

if [ "$#" -eq 0 ]; then
    echo "usage: $0 <artifact>..." >&2
    exit 2
fi

sha256sum "$@" > SHA256SUMS
gpg --local-user "Pi Auto Head Unit Release Signing" \
    --armor --detach-sign --output SHA256SUMS.asc SHA256SUMS

echo "Wrote SHA256SUMS and SHA256SUMS.asc"

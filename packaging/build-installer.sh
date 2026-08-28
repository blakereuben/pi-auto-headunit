#!/bin/sh
# packaging/build-installer.sh — appends a built .deb onto
# packaging/aa-headunit-installer.sh to produce the single, self-
# contained, distributable installer end users actually get. Internal
# build tooling, not itself part of what an end user sees.
set -e

usage() {
    echo "Usage: $0 <path/to/aa-headunit-diagnostics_*.deb> <output-path>" >&2
    exit 2
}

if [ "$#" -ne 2 ]; then
    usage
fi
deb="$1"
out="$2"

if [ ! -f "$deb" ]; then
    echo "$deb: not found" >&2
    exit 1
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
template="$script_dir/aa-headunit-installer.sh"
if [ ! -f "$template" ]; then
    echo "$template: not found" >&2
    exit 1
fi

cp "$template" "$out"
cat "$deb" >>"$out"
chmod +x "$out"

echo "Built $out ($(du -h "$out" | cut -f1))"

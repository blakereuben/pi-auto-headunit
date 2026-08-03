# Raspberry Pi Native Development Setup

The recommended first build target is the Raspberry Pi 5 with 8 GB RAM and NVMe.

## Operating system

Use current 64-bit Raspberry Pi OS Trixie. Enable SSH during imaging or through Raspberry Pi configuration. A graphical desktop and VNC are not required for Milestone 1.

## Native build dependencies

Install the distribution build tools, Rust compiler, Cargo, pkg-config, and libusb development package. Exact package versions and commands must be confirmed on the target before they are promoted into the installation guide.

## First validation sequence

From the repository root on the Pi:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p aa-headunit-diagnostics -- preflight
cargo run -p aa-headunit-diagnostics -- wireless
cargo run -p aa-headunit-diagnostics -- usb list
```

Do not run the diagnostic as root. Install/test the development udev rule and reconnect the phone before an AOA transition. Select the phone explicitly using the `BUS:ADDRESS` printed by `usb list`.

```text
cargo run -p aa-headunit-diagnostics -- usb aoa --device BUS:ADDRESS
```

The command performs only the public generic AOA transition. It does not establish Android Auto.


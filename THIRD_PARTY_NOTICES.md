# Third-Party Notices

This file records third-party software currently present in the locked dependency graph. It does not replace the original projects' licence files or notices.

## Runtime and native USB dependencies

| Component | Version | Licence | Use |
|---|---:|---|---|
| [`rusb`](https://github.com/a1ien/rusb) | 0.9.4 | MIT | Safe Rust wrapper used for USB access. |
| [`libusb1-sys`](https://github.com/a1ien/rusb) | 0.7.0 | MIT | Rust FFI/build integration for libusb. |
| [`libc`](https://github.com/rust-lang/libc) | 0.2.189 | MIT OR Apache-2.0 | Rust platform type and C-library bindings. |
| [`libusb`](https://github.com/libusb/libusb) | selected by `libusb1-sys` | LGPL-2.1-or-later | Native USB library built by the enabled `rusb` `vendored` feature. |
| [Rust standard library](https://github.com/rust-lang/rust) | release toolchain | MIT OR Apache-2.0 | Rust runtime and standard-library code linked into project binaries. |

The Cargo configuration enables the `rusb` `vendored` feature, which can build and statically link libusb. Every release must record the actual linkage and include the source, build materials, copyright notices, and licence notices required by GPL-3.0-or-later and the applicable libusb LGPL terms.

## Build-only dependencies

| Component | Version | Licence |
|---|---:|---|
| [`cc`](https://github.com/rust-lang/cc-rs) | 1.4.0 | MIT OR Apache-2.0 |
| [`find-msvc-tools`](https://github.com/rust-lang/cc-rs) | 0.1.9 | MIT OR Apache-2.0 |
| [`pkg-config`](https://github.com/rust-lang/pkg-config-rs) | 0.3.33 | MIT OR Apache-2.0 |
| [`shlex`](https://github.com/comex/rust-shlex) | 2.0.1 | MIT OR Apache-2.0 |
| [`vcpkg`](https://github.com/mcgoo/vcpkg-rs) | 0.2.15 | MIT OR Apache-2.0 |

Versions above match `Cargo.lock`. This notice must be regenerated and reviewed whenever locked dependencies, vendored native code, fonts, icons, media, or other assets change.

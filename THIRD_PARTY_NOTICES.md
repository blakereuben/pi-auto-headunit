# Third-Party Notices

This file records third-party software currently present in the locked dependency graph. It does not replace the original projects' licence files or notices.

## Runtime and native dependencies

| Component | Version | Licence | Use |
|---|---:|---|---|
| [`rusb`](https://github.com/a1ien/rusb) | 0.9.4 | MIT | Safe Rust wrapper used for USB access. |
| [`libusb1-sys`](https://github.com/a1ien/rusb) | 0.7.0 | MIT | Rust FFI/build integration for libusb. |
| [`libc`](https://github.com/rust-lang/libc) | 0.2.189 | MIT OR Apache-2.0 | Rust platform type and C-library bindings. |
| [`libusb`](https://github.com/libusb/libusb) | selected by `libusb1-sys` | LGPL-2.1-or-later | Native USB library built by the enabled `rusb` `vendored` feature. |
| [`gstreamer`](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.24.2 | MIT OR Apache-2.0 | Rust bindings used for media capability discovery and pipeline selection. |
| [`gstreamer-sys`](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.24.5 | MIT OR Apache-2.0 | Rust FFI bindings to the system GStreamer library. |
| [`glib`](https://gtk-rs.org/) | 0.21.5 | MIT | Rust bindings required by GStreamer. |
| [GStreamer](https://gstreamer.freedesktop.org/) | Raspberry Pi OS package version | LGPL-2.1-or-later | Dynamically linked media framework and distribution plugins. |
| [Rust standard library](https://github.com/rust-lang/rust) | release toolchain | MIT OR Apache-2.0 | Rust runtime and standard-library code linked into project binaries. |
| [`openssl`](https://github.com/sfackler/rust-openssl) | 0.10.81 | Apache-2.0 | Safe Rust OpenSSL API used by the replaceable Linux TLS adapter. |
| [`openssl-sys`](https://github.com/sfackler/rust-openssl) | 0.9.117 | MIT | Rust FFI bindings to the system OpenSSL library. |
| [OpenSSL](https://www.openssl.org/) | Raspberry Pi OS package version | Apache-2.0 | Dynamically linked TLS implementation. |
| [`foreign-types`](https://github.com/sfackler/foreign-types) | 0.3.2 | MIT OR Apache-2.0 | Ownership wrappers used by the Rust OpenSSL bindings. |
| [`foreign-types-shared`](https://github.com/sfackler/foreign-types) | 0.1.1 | MIT OR Apache-2.0 | Shared traits used by `foreign-types`. |

The Cargo configuration enables the `rusb` `vendored` feature, which can build and statically link libusb. Every release must record the actual linkage and include the source, build materials, copyright notices, and licence notices required by GPL-3.0-or-later and the applicable libusb LGPL terms.

## Approved source references

These projects are provenance-tracked sources for selected Rust behaviour; they are not linked runtime dependencies and their repositories, binaries, credentials, and assets are not vendored.

| Component | Pinned revision | Licence | Approved use |
|---|---|---|---|
| [AASDK](https://github.com/opencardev/aasdk) | `9bf6adf933665dee26532201719fac14a047ccf1` | GPL-3.0-or-later | Framing, control-handshake, and bounded TLS-engine behaviour listed in `docs/protocol/aasdk-adoption.md`; all shared credentials excluded. |
| [OpenAuto](https://github.com/f1xpl/openauto) | `aa90412bf93b5a5078495ea85ac9270c6297d369` | GPL-3.0-or-later in relevant source headers; README declares GPLv3 | Source for the attributed service-discovery event transition and approved candidate for later file-attributed service behaviour listed in `docs/protocol/openauto-adoption.md`. Credentials, identities, trademarks, proprietary material, and assets excluded. |

## Build-only dependencies

| Component | Version | Licence |
|---|---:|---|
| [`cc`](https://github.com/rust-lang/cc-rs) | 1.4.0 | MIT OR Apache-2.0 |
| [`find-msvc-tools`](https://github.com/rust-lang/cc-rs) | 0.1.9 | MIT OR Apache-2.0 |
| [`pkg-config`](https://github.com/rust-lang/pkg-config-rs) | 0.3.33 | MIT OR Apache-2.0 |
| [`openssl-macros`](https://github.com/sfackler/rust-openssl) | 0.1.1 | Apache-2.0 |
| [`system-deps`](https://github.com/gdesmott/system-deps) | 7.0.8 | MIT OR Apache-2.0 |
| [`shlex`](https://github.com/comex/rust-shlex) | 2.0.1 | MIT OR Apache-2.0 |
| [`vcpkg`](https://github.com/mcgoo/vcpkg-rs) | 0.2.15 | MIT OR Apache-2.0 |

Versions above match `Cargo.lock`. This notice must be regenerated and reviewed whenever locked dependencies, vendored native code, fonts, icons, media, or other assets change.

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

## Additional transitive Rust dependencies

Every other crate in `Cargo.lock`'s locked *normal* (non-dev, non-build) dependency graph — pulled in transitively by the direct dependencies above (the `gstreamer`/`glib` bindings' async, numeric, macro, and TOML-parsing infrastructure; `syn`/`quote`/`proc-macro2` and similar compile-time code-generation tooling used by several of those bindings) — not referenced directly by this project's own code. Verified against each crate's own published `Cargo.toml` licence declaration (crates.io registry metadata) on 2026-08-14. All are permissively licensed (MIT, Apache-2.0, Unlicense, or the Unicode-3.0 data licence) and compatible with this project's GPL-3.0-or-later.

| Component | Version | Licence |
|---|---:|---|
| [`bitflags`](https://github.com/bitflags/bitflags) | 2.13.1 | MIT OR Apache-2.0 |
| [`cfg-if`](https://github.com/rust-lang/cfg-if) | 1.0.4 | MIT OR Apache-2.0 |
| [`either`](https://github.com/rayon-rs/either) | 1.17.0 | MIT OR Apache-2.0 |
| [`equivalent`](https://github.com/indexmap-rs/equivalent) | 1.0.2 | Apache-2.0 OR MIT |
| [`futures-channel`](https://github.com/rust-lang/futures-rs) | 0.3.33 | MIT OR Apache-2.0 |
| [`futures-core`](https://github.com/rust-lang/futures-rs) | 0.3.33 | MIT OR Apache-2.0 |
| [`futures-executor`](https://github.com/rust-lang/futures-rs) | 0.3.33 | MIT OR Apache-2.0 |
| [`futures-macro`](https://github.com/rust-lang/futures-rs) | 0.3.33 | MIT OR Apache-2.0 |
| [`futures-task`](https://github.com/rust-lang/futures-rs) | 0.3.33 | MIT OR Apache-2.0 |
| [`futures-util`](https://github.com/rust-lang/futures-rs) | 0.3.33 | MIT OR Apache-2.0 |
| [`gio-sys`](https://gtk-rs.org/) | 0.21.5 | MIT |
| [`glib-macros`](https://gtk-rs.org/) | 0.21.5 | MIT |
| [`glib-sys`](https://gtk-rs.org/) | 0.21.5 | MIT |
| [`gobject-sys`](https://gtk-rs.org/) | 0.21.5 | MIT |
| [`hashbrown`](https://github.com/rust-lang/hashbrown) | 0.17.1 | MIT OR Apache-2.0 |
| [`heck`](https://github.com/withoutboats/heck) | 0.5.0 | MIT OR Apache-2.0 |
| [`indexmap`](https://github.com/indexmap-rs/indexmap) | 2.14.0 | Apache-2.0 OR MIT |
| [`itertools`](https://github.com/rust-itertools/itertools) | 0.14.0 | MIT OR Apache-2.0 |
| [`kstring`](https://github.com/cobalt-org/kstring) | 2.0.2 | MIT OR Apache-2.0 |
| [`memchr`](https://github.com/BurntSushi/memchr) | 2.8.3 | Unlicense OR MIT |
| [`muldiv`](https://github.com/sdroege/muldiv) | 1.0.1 | MIT |
| [`num-integer`](https://github.com/rust-num/num-integer) | 0.1.46 | MIT OR Apache-2.0 |
| [`num-rational`](https://github.com/rust-num/num-rational) | 0.4.2 | MIT OR Apache-2.0 |
| [`num-traits`](https://github.com/rust-num/num-traits) | 0.2.19 | MIT OR Apache-2.0 |
| [`option-operations`](https://github.com/danielhenrymantilla/option-operations-rs) | 0.6.1 | MIT OR Apache-2.0 |
| [`pastey`](https://github.com/dtolnay/pastey) | 0.1.1 | MIT OR Apache-2.0 |
| [`pastey`](https://github.com/dtolnay/pastey) | 0.2.3 | MIT OR Apache-2.0 |
| [`pin-project-lite`](https://github.com/taiki-e/pin-project-lite) | 0.2.17 | Apache-2.0 OR MIT |
| [`proc-macro2`](https://github.com/dtolnay/proc-macro2) | 1.0.107 | MIT OR Apache-2.0 |
| [`proc-macro-crate`](https://github.com/bkchr/proc-macro-crate) | 3.5.0 | MIT OR Apache-2.0 |
| [`quote`](https://github.com/dtolnay/quote) | 1.0.47 | MIT OR Apache-2.0 |
| [`serde`](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [`serde_core`](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [`serde_derive`](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [`serde_spanned`](https://github.com/toml-rs/toml) | 1.1.1 | MIT OR Apache-2.0 |
| [`slab`](https://github.com/tokio-rs/slab) | 0.4.12 | MIT |
| [`smallvec`](https://github.com/servo/rust-smallvec) | 1.15.2 | MIT OR Apache-2.0 |
| [`static_assertions`](https://github.com/nvzqz/static-assertions) | 1.1.0 | MIT OR Apache-2.0 |
| [`syn`](https://github.com/dtolnay/syn) | 2.0.119 | MIT OR Apache-2.0 |
| [`syn`](https://github.com/dtolnay/syn) | 3.0.3 | MIT OR Apache-2.0 |
| [`thiserror`](https://github.com/dtolnay/thiserror) | 2.0.19 | MIT OR Apache-2.0 |
| [`thiserror-impl`](https://github.com/dtolnay/thiserror) | 2.0.19 | MIT OR Apache-2.0 |
| [`toml`](https://github.com/toml-rs/toml) | 0.9.12+spec-1.1.0 | MIT OR Apache-2.0 |
| [`toml_datetime`](https://github.com/toml-rs/toml) | 0.7.5+spec-1.1.0 | MIT OR Apache-2.0 |
| [`toml_datetime`](https://github.com/toml-rs/toml) | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 |
| [`toml_edit`](https://github.com/toml-rs/toml) | 0.25.13+spec-1.1.0 | MIT OR Apache-2.0 |
| [`toml_parser`](https://github.com/toml-rs/toml) | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |
| [`toml_writer`](https://github.com/toml-rs/toml) | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| [`unicode-ident`](https://github.com/dtolnay/unicode-ident) | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| [`winnow`](https://github.com/winnow-rs/winnow) | 0.7.15 | MIT |
| [`winnow`](https://github.com/winnow-rs/winnow) | 1.0.4 | MIT |

## Approved source references

These projects are provenance-tracked sources for selected Rust behaviour; they are not linked runtime dependencies and their repositories, binaries, credentials, and assets are not vendored.

| Component | Pinned revision | Licence | Approved use |
|---|---|---|---|
| [AASDK](https://github.com/opencardev/aasdk) | `9bf6adf933665dee26532201719fac14a047ccf1` | GPL-3.0-or-later | Framing, control-handshake, and bounded TLS-engine behaviour listed in `docs/protocol/aasdk-adoption.md`; all shared credentials excluded. |
| [OpenAuto](https://github.com/f1xpl/openauto) | `aa90412bf93b5a5078495ea85ac9270c6297d369` | GPL-3.0-or-later in relevant source headers; README declares GPLv3 | Source for the attributed service-discovery event transition and bounded internal service catalogue, and an approved candidate for later file-attributed service behaviour listed in `docs/protocol/openauto-adoption.md`. Credentials, identities, trademarks, proprietary material, and assets excluded. |
| [LIVI](https://github.com/f-io/LIVI) | `9000f308eec423c5c56ac0a14491a7c95ce5762d` | GPL-3.0-or-later (`package.json`, `README.md`, per-file SPDX headers) | Source for video-focus timing, per-frame media ack, unconditional key-binding response, ping cadence advertisement, ping arm-timing/watchdog behaviour, and small/recycled touch pointer-id allocation listed in `docs/protocol/livi-adoption.md`. Credentials (including `native/crypto/**`), branding, assets, and unmodeled channels excluded. |

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
| [`autocfg`](https://github.com/cuviper/autocfg) | 1.5.1 | Apache-2.0 OR MIT |
| [`cfg-expr`](https://github.com/EmbarkStudios/cfg-expr) | 0.20.8 | MIT OR Apache-2.0 |
| [`target-lexicon`](https://github.com/bytecodealliance/target-lexicon) | 0.13.5 | Apache-2.0 WITH LLVM-exception |
| [`toml`](https://github.com/toml-rs/toml) | 1.1.4+spec-1.1.0 | MIT OR Apache-2.0 |
| [`version-compare`](https://github.com/timvisee/version-compare) | 0.2.1 | MIT |

Versions above match `Cargo.lock` as of 2026-08-14. This notice must be regenerated and reviewed whenever locked dependencies, vendored native code, fonts, icons, media, or other assets change.

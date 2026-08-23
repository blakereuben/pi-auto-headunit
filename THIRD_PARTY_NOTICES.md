# Third-Party Notices

This file records third-party software currently present in the locked dependency graph, both the native libraries dynamically or statically linked at runtime and every Rust crate compiled into the binary. It does not replace the original projects' licence files or notices.

Regenerated 2026-08-23 from `cargo license` (`~/.cargo/bin/cargo-license`, installed for this pass — not a project dependency) against `Cargo.lock`, filtered to the `aarch64-unknown-linux-gnu` target this project actually ships for, and cross-checked against `ldd` on the built `aa-headunit-diagnostics` binary for the native-library section below. Superseded a 2026-08-14 snapshot that predated the Bluetooth/netlink/GTK4 work.

## Native libraries dynamically linked at runtime

Confirmed via `ldd` on the built binary — not visible to `cargo license`, which only inspects Rust crate metadata. Versions are whatever Raspberry Pi OS's own package repository currently provides; this project does not pin or vendor these.

| Library | Licence | Linked via |
|---|---|---|
| GTK 4 (`libgtk-4`) | LGPL-2.1-or-later | `gtk4`/`gtk4-sys` |
| GLib (`libglib-2.0`), GObject | LGPL-2.1-or-later | `glib`/`glib-sys`, `gobject-sys` |
| GDK Pixbuf (`libgdk_pixbuf-2.0`) | LGPL-2.1-or-later | `gdk-pixbuf`/`gdk-pixbuf-sys` |
| Pango, PangoCairo, PangoFT2 | LGPL-2.1-or-later | `pango`/`pango-sys` |
| Graphene (`libgraphene-1.0`) | MIT | `graphene-rs`/`graphene-sys` |
| Cairo (`libcairo`, `libcairo-gobject`, `libcairo-script-interpreter`) | LGPL-2.1-or-later OR MPL-1.1 | `cairo-rs`/`cairo-sys-rs` |
| GStreamer (`libgstreamer-1.0`) | LGPL-2.1-or-later | `gstreamer`/`gstreamer-sys` and the `gstreamer-app`/`gstreamer-base` bindings |
| OpenSSL (`libssl`, via OpenSSL 3.x) | Apache-2.0 | `openssl`/`openssl-sys` |
| D-Bus (`libdbus-1`) | AFL-2.1 OR GPL-2.0-or-later | `dbus`/`libdbus-sys` (used by `bluer` and this project's own Bluetooth/wireless-bootstrap code) |

## Statically-linked native libraries

| Library | Licence | Note |
|---|---|---|
| libusb | LGPL-2.1-or-later | Built and statically linked by `rusb`'s `vendored` Cargo feature (confirmed via `ldd` showing no dynamic `libusb` entry). Every release must record the actual linkage and include the source, build materials, copyright notices, and licence notices required by GPL-3.0-or-later and the applicable libusb LGPL terms. |

## Rust crate dependencies compiled into the binary

Every crate in `Cargo.lock`'s locked *normal* (non-dev, non-build) dependency graph for the `aarch64-unknown-linux-gnu` target, excluding this project's own workspace crates (`aa-headunit-diagnostics`, `credential-store`, `media-api`, `media-gstreamer`, `platform-api`, `platform-linux`, `protocol-aap`, `security-openssl`, `transport-api`, `transport-bluetooth`, `transport-tcp`, `transport-usb`, all `GPL-3.0-or-later`, matching this project's own licence). All are permissively licensed (MIT, Apache-2.0, BSD-2-Clause, Unlicense, or the Unicode-3.0 data licence, alone or in an OR combination) and compatible with this project's GPL-3.0-or-later — confirmed with `cargo audit` reporting zero known vulnerabilities across the same 208-crate locked graph.

| Component | Version | Licence |
|---|---:|---|
| [`atomic_refcell`](https://github.com/mozilla/atomic_refcell) | 0.1.14 | Apache-2.0 OR MIT |
| [`bitflags`](https://github.com/bitflags/bitflags) | 1.3.2 | Apache-2.0 OR MIT |
| [`bitflags`](https://github.com/bitflags/bitflags) | 2.13.1 | Apache-2.0 OR MIT |
| [`bitvec`](https://github.com/bitvecto-rs/bitvec) | 1.1.1 | MIT |
| [`bluer`](https://github.com/bluez/bluer) | 0.17.4 | BSD-2-Clause |
| [`bytes`](https://github.com/tokio-rs/bytes) | 1.12.1 | MIT |
| [`cairo-rs`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`cairo-sys-rs`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`cfg-if`](https://github.com/rust-lang/cfg-if) | 1.0.4 | Apache-2.0 OR MIT |
| [`ctrlc`](https://github.com/Detegr/rust-ctrlc.git) | 3.5.2 | Apache-2.0 OR MIT |
| [`custom_debug`](https://github.com/panicbit/custom_debug) | 0.6.2 | Apache-2.0 OR MIT |
| [`custom_debug_derive`](https://github.com/panicbit/custom_debug) | 0.6.2 | Apache-2.0 OR MIT |
| [`darling`](https://github.com/TedDriggs/darling) | 0.20.11 | MIT |
| [`darling_core`](https://github.com/TedDriggs/darling) | 0.20.11 | MIT |
| [`darling_macro`](https://github.com/TedDriggs/darling) | 0.20.11 | MIT |
| [`dbus`](https://github.com/diwic/dbus-rs) | 0.9.12 | Apache-2.0 OR MIT |
| [`dbus-crossroads`](https://github.com/diwic/dbus-rs/) | 0.5.3 | Apache-2.0 OR MIT |
| [`dbus-tokio`](https://github.com/diwic/dbus-rs) | 0.7.6 | Apache-2.0 OR MIT |
| [`displaydoc`](https://github.com/yaahc/displaydoc) | 0.2.7 | Apache-2.0 OR MIT |
| [`either`](https://github.com/rayon-rs/either) | 1.17.0 | Apache-2.0 OR MIT |
| [`equivalent`](https://github.com/indexmap-rs/equivalent) | 1.0.2 | Apache-2.0 OR MIT |
| [`evdev`](https://github.com/cmr/evdev) | 0.13.2 | Apache-2.0 OR MIT |
| [`field-offset`](https://github.com/Diggsey/rust-field-offset) | 0.3.6 | Apache-2.0 OR MIT |
| [`fnv`](https://github.com/servo/rust-fnv) | 1.0.7 | Apache-2.0 OR MIT |
| [`foreign-types`](https://github.com/sfackler/foreign-types) | 0.3.2 | Apache-2.0 OR MIT |
| [`foreign-types-shared`](https://github.com/sfackler/foreign-types) | 0.1.1 | Apache-2.0 OR MIT |
| [`funty`](https://github.com/myrrlyn/funty) | 2.0.0 | MIT |
| [`futures`](https://github.com/rust-lang/futures-rs) | 0.3.33 | Apache-2.0 OR MIT |
| [`futures-channel`](https://github.com/rust-lang/futures-rs) | 0.3.33 | Apache-2.0 OR MIT |
| [`futures-core`](https://github.com/rust-lang/futures-rs) | 0.3.33 | Apache-2.0 OR MIT |
| [`futures-executor`](https://github.com/rust-lang/futures-rs) | 0.3.33 | Apache-2.0 OR MIT |
| [`futures-io`](https://github.com/rust-lang/futures-rs) | 0.3.34 | Apache-2.0 OR MIT |
| [`futures-macro`](https://github.com/rust-lang/futures-rs) | 0.3.33 | Apache-2.0 OR MIT |
| [`futures-sink`](https://github.com/rust-lang/futures-rs) | 0.3.34 | Apache-2.0 OR MIT |
| [`futures-task`](https://github.com/rust-lang/futures-rs) | 0.3.33 | Apache-2.0 OR MIT |
| [`futures-util`](https://github.com/rust-lang/futures-rs) | 0.3.33 | Apache-2.0 OR MIT |
| [`gdk-pixbuf`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`gdk-pixbuf-sys`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`gdk4`](https://github.com/gtk-rs/gtk4-rs) | 0.10.3 | MIT |
| [`gdk4-sys`](https://github.com/gtk-rs/gtk4-rs) | 0.10.3 | MIT |
| [`getrandom`](https://github.com/rust-random/getrandom) | 0.4.3 | Apache-2.0 OR MIT |
| [`gio`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`gio-sys`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`glib`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`glib-macros`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`glib-sys`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`gobject-sys`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`gpiod`](https://github.com/katyo/gpiod-rs) | 0.3.0 | MIT |
| [`gpiod-core`](https://github.com/katyo/gpiod-rs) | 0.3.0 | MIT |
| [`graphene-rs`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`graphene-sys`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`gsk4`](https://github.com/gtk-rs/gtk4-rs) | 0.10.3 | MIT |
| [`gsk4-sys`](https://github.com/gtk-rs/gtk4-rs) | 0.10.3 | MIT |
| [`gstreamer`](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.24.2 | Apache-2.0 OR MIT |
| [`gstreamer-app`](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.24.2 | Apache-2.0 OR MIT |
| [`gstreamer-app-sys`](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.24.5 | MIT |
| [`gstreamer-base`](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.24.5 | Apache-2.0 OR MIT |
| [`gstreamer-base-sys`](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.24.5 | MIT |
| [`gstreamer-sys`](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.24.5 | MIT |
| [`gtk4`](https://github.com/gtk-rs/gtk4-rs) | 0.10.3 | MIT |
| [`gtk4-macros`](https://github.com/gtk-rs/gtk4-rs) | 0.10.3 | MIT |
| [`gtk4-sys`](https://github.com/gtk-rs/gtk4-rs) | 0.10.3 | MIT |
| [`hashbrown`](https://github.com/rust-lang/hashbrown) | 0.17.1 | Apache-2.0 OR MIT |
| [`heck`](https://github.com/withoutboats/heck) | 0.5.0 | Apache-2.0 OR MIT |
| [`hex`](https://github.com/KokaKiwi/rust-hex) | 0.4.3 | Apache-2.0 OR MIT |
| [`ident_case`](https://github.com/TedDriggs/ident_case) | 1.0.1 | Apache-2.0 OR MIT |
| [`indexmap`](https://github.com/indexmap-rs/indexmap) | 2.14.0 | Apache-2.0 OR MIT |
| [`itertools`](https://github.com/rust-itertools/itertools) | 0.14.0 | Apache-2.0 OR MIT |
| [`kstring`](https://github.com/cobalt-org/kstring) | 2.0.2 | Apache-2.0 OR MIT |
| [`lazy_static`](https://github.com/rust-lang-nursery/lazy-static.rs) | 1.5.0 | Apache-2.0 OR MIT |
| [`libc`](https://github.com/rust-lang/libc) | 0.2.189 | Apache-2.0 OR MIT |
| [`libdbus-sys`](https://github.com/diwic/dbus-rs) | 0.2.7 | Apache-2.0 OR MIT |
| [`libusb1-sys`](https://github.com/a1ien/rusb.git) | 0.7.0 | MIT |
| [`log`](https://github.com/rust-lang/log) | 0.4.33 | Apache-2.0 OR MIT |
| [`macaddr`](https://github.com/svartalf/rust-macaddr) | 1.0.1 | Apache-2.0 OR MIT |
| [`memchr`](https://github.com/BurntSushi/memchr) | 2.8.3 | MIT OR Unlicense |
| [`memoffset`](https://github.com/Gilnaa/memoffset) | 0.9.1 | MIT |
| [`mio`](https://github.com/tokio-rs/mio) | 1.2.2 | MIT |
| [`muldiv`](https://github.com/sdroege/rust-muldiv) | 1.0.1 | MIT |
| [`netlink-packet-core`](https://github.com/rust-netlink/netlink-packet-core) | 0.9.0 | MIT |
| [`netlink-packet-route`](https://github.com/rust-netlink/netlink-packet-route) | 0.33.0 | MIT |
| [`netlink-proto`](https://github.com/rust-netlink/netlink-proto) | 0.13.0 | MIT |
| [`netlink-sys`](https://github.com/rust-netlink/netlink-sys) | 0.9.0 | MIT |
| [`nix`](https://github.com/nix-rust/nix) | 0.26.4 | MIT |
| [`nix`](https://github.com/nix-rust/nix) | 0.29.0 | MIT |
| [`nix`](https://github.com/nix-rust/nix) | 0.30.1 | MIT |
| [`nix`](https://github.com/nix-rust/nix) | 0.31.3 | MIT |
| [`num-derive`](https://github.com/rust-num/num-derive) | 0.4.2 | Apache-2.0 OR MIT |
| [`num-integer`](https://github.com/rust-num/num-integer) | 0.1.46 | Apache-2.0 OR MIT |
| [`num-rational`](https://github.com/rust-num/num-rational) | 0.4.2 | Apache-2.0 OR MIT |
| [`num-traits`](https://github.com/rust-num/num-traits) | 0.2.19 | Apache-2.0 OR MIT |
| [`openssl`](https://github.com/rust-openssl/rust-openssl) | 0.10.81 | Apache-2.0 |
| `openssl-macros` | 0.1.1 | Apache-2.0 OR MIT |
| [`openssl-sys`](https://github.com/rust-openssl/rust-openssl) | 0.9.117 | MIT |
| [`option-operations`](https://github.com/fengalin/option-operations) | 0.6.1 | Apache-2.0 OR MIT |
| [`pango`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`pango-sys`](https://github.com/gtk-rs/gtk-rs-core) | 0.21.5 | MIT |
| [`pastey`](https://github.com/as1100k/pastey) | 0.1.1 | Apache-2.0 OR MIT |
| [`pastey`](https://github.com/as1100k/pastey) | 0.2.3 | Apache-2.0 OR MIT |
| [`pin-project`](https://github.com/taiki-e/pin-project) | 1.1.13 | Apache-2.0 OR MIT |
| [`pin-project-internal`](https://github.com/taiki-e/pin-project) | 1.1.13 | Apache-2.0 OR MIT |
| [`pin-project-lite`](https://github.com/taiki-e/pin-project-lite) | 0.2.17 | Apache-2.0 OR MIT |
| [`proc-macro-crate`](https://github.com/bkchr/proc-macro-crate) | 3.5.0 | Apache-2.0 OR MIT |
| [`proc-macro2`](https://github.com/dtolnay/proc-macro2) | 1.0.107 | Apache-2.0 OR MIT |
| [`quote`](https://github.com/dtolnay/quote) | 1.0.47 | Apache-2.0 OR MIT |
| [`radium`](https://github.com/bitvecto-rs/radium) | 0.7.0 | MIT |
| [`rtnetlink`](https://github.com/rust-netlink/rtnetlink) | 0.23.0 | MIT |
| [`rusb`](https://github.com/a1ien/rusb.git) | 0.9.4 | MIT |
| [`rustversion`](https://github.com/dtolnay/rustversion) | 1.0.23 | Apache-2.0 OR MIT |
| [`serde`](https://github.com/serde-rs/serde) | 1.0.229 | Apache-2.0 OR MIT |
| [`serde_core`](https://github.com/serde-rs/serde) | 1.0.229 | Apache-2.0 OR MIT |
| [`serde_derive`](https://github.com/serde-rs/serde) | 1.0.229 | Apache-2.0 OR MIT |
| [`serde_spanned`](https://github.com/toml-rs/toml) | 1.1.1 | Apache-2.0 OR MIT |
| [`slab`](https://github.com/tokio-rs/slab) | 0.4.12 | MIT |
| [`smallvec`](https://github.com/servo/rust-smallvec) | 1.15.2 | Apache-2.0 OR MIT |
| [`socket2`](https://github.com/rust-lang/socket2) | 0.6.5 | Apache-2.0 OR MIT |
| [`static_assertions`](https://github.com/nvzqz/static-assertions-rs) | 1.1.0 | Apache-2.0 OR MIT |
| [`strsim`](https://github.com/rapidfuzz/strsim-rs) | 0.11.1 | MIT |
| [`strum`](https://github.com/Peternator7/strum) | 0.26.3 | MIT |
| [`strum_macros`](https://github.com/Peternator7/strum) | 0.26.4 | MIT |
| [`syn`](https://github.com/dtolnay/syn) | 2.0.119 | Apache-2.0 OR MIT |
| [`syn`](https://github.com/dtolnay/syn) | 3.0.3 | Apache-2.0 OR MIT |
| [`synstructure`](https://github.com/mystor/synstructure) | 0.13.2 | MIT |
| [`tap`](https://github.com/myrrlyn/tap) | 1.0.1 | MIT |
| [`thiserror`](https://github.com/dtolnay/thiserror) | 2.0.19 | Apache-2.0 OR MIT |
| [`thiserror-impl`](https://github.com/dtolnay/thiserror) | 2.0.19 | Apache-2.0 OR MIT |
| [`tokio`](https://github.com/tokio-rs/tokio) | 1.53.1 | MIT |
| [`tokio-macros`](https://github.com/tokio-rs/tokio) | 2.7.2 | MIT |
| [`tokio-stream`](https://github.com/tokio-rs/tokio) | 0.1.19 | MIT |
| [`toml`](https://github.com/toml-rs/toml) | 0.9.12+spec-1.1.0 | Apache-2.0 OR MIT |
| [`toml_datetime`](https://github.com/toml-rs/toml) | 0.7.5+spec-1.1.0 | Apache-2.0 OR MIT |
| [`toml_datetime`](https://github.com/toml-rs/toml) | 1.1.1+spec-1.1.0 | Apache-2.0 OR MIT |
| [`toml_edit`](https://github.com/toml-rs/toml) | 0.25.13+spec-1.1.0 | Apache-2.0 OR MIT |
| [`toml_parser`](https://github.com/toml-rs/toml) | 1.1.3+spec-1.1.0 | Apache-2.0 OR MIT |
| [`toml_writer`](https://github.com/toml-rs/toml) | 1.1.2+spec-1.1.0 | Apache-2.0 OR MIT |
| [`unicode-ident`](https://github.com/dtolnay/unicode-ident) | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| [`uuid`](https://github.com/uuid-rs/uuid) | 1.24.1 | Apache-2.0 OR MIT |
| [`winnow`](https://github.com/winnow-rs/winnow) | 0.7.15 | MIT |
| [`winnow`](https://github.com/winnow-rs/winnow) | 1.0.4 | MIT |
| [`wyz`](https://github.com/myrrlyn/wyz) | 0.5.1 | MIT |
| [`zerocopy`](https://github.com/google/zerocopy) | 0.8.56 | Apache-2.0 OR BSD-2-Clause OR MIT |
| [`zerocopy-derive`](https://github.com/google/zerocopy) | 0.8.56 | Apache-2.0 OR BSD-2-Clause OR MIT |

## Approved source references

These projects are provenance-tracked sources for selected Rust behaviour; they are not linked runtime dependencies and their repositories, binaries, credentials, and assets are not vendored.

| Component | Pinned revision | Licence | Approved use |
|---|---|---|---|
| [AASDK](https://github.com/opencardev/aasdk) | `9bf6adf933665dee26532201719fac14a047ccf1` | GPL-3.0-or-later | Framing, control-handshake, and bounded TLS-engine behaviour listed in `docs/protocol/aasdk-adoption.md`; all shared credentials excluded. |
| [OpenAuto](https://github.com/f1xpl/openauto) | `aa90412bf93b5a5078495ea85ac9270c6297d369` | GPL-3.0-or-later in relevant source headers; README declares GPLv3 | Source for the attributed service-discovery event transition and bounded internal service catalogue, and an approved candidate for later file-attributed service behaviour listed in `docs/protocol/openauto-adoption.md`. Credentials, identities, trademarks, proprietary material, and assets excluded. |
| [LIVI](https://github.com/f-io/LIVI) | `9000f308eec423c5c56ac0a14491a7c95ce5762d` | GPL-3.0-or-later (`package.json`, `README.md`, per-file SPDX headers) | Source for video-focus timing, per-frame media ack, unconditional key-binding response, ping cadence advertisement, ping arm-timing/watchdog behaviour, and small/recycled touch pointer-id allocation listed in `docs/protocol/livi-adoption.md`. Credentials (including `native/crypto/**`), branding, assets, and unmodeled channels excluded. |

## Build- and test-only dependencies (not distributed)

Present in `Cargo.lock` only for `cargo build`/`cargo test` (property-fuzz testing's `proptest`/`rand`/`rusty-fork`, native-library build scripts' `cc`/`pkg-config`/`system-deps`, and their own transitive dependencies) — never compiled into the shipped binary or `.deb` package.

| Component | Version | Licence |
|---|---:|---|
| [`autocfg`](https://github.com/cuviper/autocfg) | 1.5.1 | Apache-2.0 OR MIT |
| [`bit-set`](https://github.com/contain-rs/bit-set) | 0.8.0 | Apache-2.0 OR MIT |
| [`bit-vec`](https://github.com/contain-rs/bit-vec) | 0.8.0 | Apache-2.0 OR MIT |
| [`cc`](https://github.com/rust-lang/cc-rs) | 1.4.0 | Apache-2.0 OR MIT |
| [`cfg-expr`](https://github.com/EmbarkStudios/cfg-expr) | 0.20.8 | Apache-2.0 OR MIT |
| [`cfg_aliases`](https://github.com/katharostech/cfg_aliases) | 0.2.2 | MIT |
| [`errno`](https://github.com/lambda-fairy/rust-errno) | 0.3.14 | Apache-2.0 OR MIT |
| [`fastrand`](https://github.com/smol-rs/fastrand) | 2.5.0 | Apache-2.0 OR MIT |
| [`find-msvc-tools`](https://github.com/rust-lang/cc-rs) | 0.1.9 | Apache-2.0 OR MIT |
| [`getrandom`](https://github.com/rust-random/getrandom) | 0.3.4 | Apache-2.0 OR MIT |
| [`itoa`](https://github.com/dtolnay/itoa) | 1.0.18 | Apache-2.0 OR MIT |
| [`linux-raw-sys`](https://github.com/sunfishcode/linux-raw-sys) | 0.12.1 | Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT |
| [`once_cell`](https://github.com/matklad/once_cell) | 1.21.4 | Apache-2.0 OR MIT |
| [`pkg-config`](https://github.com/rust-lang/pkg-config-rs) | 0.3.33 | Apache-2.0 OR MIT |
| [`ppv-lite86`](https://github.com/cryptocorrosion/cryptocorrosion) | 0.2.21 | Apache-2.0 OR MIT |
| [`proptest`](https://github.com/proptest-rs/proptest) | 1.11.0 | Apache-2.0 OR MIT |
| [`quick-error`](http://github.com/tailhook/quick-error) | 1.2.3 | Apache-2.0 OR MIT |
| [`rand`](https://github.com/rust-random/rand) | 0.9.5 | Apache-2.0 OR MIT |
| [`rand_chacha`](https://github.com/rust-random/rand) | 0.9.0 | Apache-2.0 OR MIT |
| [`rand_core`](https://github.com/rust-random/rand) | 0.9.5 | Apache-2.0 OR MIT |
| [`rand_xorshift`](https://github.com/rust-random/rngs) | 0.4.0 | Apache-2.0 OR MIT |
| [`regex-syntax`](https://github.com/rust-lang/regex) | 0.8.11 | Apache-2.0 OR MIT |
| [`rustc_version`](https://github.com/djc/rustc-version-rs) | 0.4.1 | Apache-2.0 OR MIT |
| [`rustix`](https://github.com/bytecodealliance/rustix) | 1.1.4 | Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT |
| [`rusty-fork`](https://github.com/altsysrq/rusty-fork) | 0.3.1 | Apache-2.0 OR MIT |
| [`semver`](https://github.com/dtolnay/semver) | 1.0.28 | Apache-2.0 OR MIT |
| [`serde_json`](https://github.com/serde-rs/json) | 1.0.151 | Apache-2.0 OR MIT |
| [`shlex`](https://github.com/comex/rust-shlex) | 2.0.1 | Apache-2.0 OR MIT |
| [`system-deps`](https://github.com/gdesmott/system-deps) | 7.0.8 | Apache-2.0 OR MIT |
| [`target-lexicon`](https://github.com/bytecodealliance/target-lexicon) | 0.13.5 | Apache-2.0 WITH LLVM-exception |
| [`tempfile`](https://github.com/Stebalien/tempfile) | 3.27.0 | Apache-2.0 OR MIT |
| [`toml`](https://github.com/toml-rs/toml) | 1.1.4+spec-1.1.0 | Apache-2.0 OR MIT |
| [`unarray`](https://github.com/cameron1024/unarray) | 0.1.4 | Apache-2.0 OR MIT |
| [`vcpkg`](https://github.com/mcgoo/vcpkg-rs) | 0.2.15 | Apache-2.0 OR MIT |
| [`version-compare`](https://gitlab.com/timvisee/version-compare) | 0.2.1 | MIT |
| [`wait-timeout`](https://github.com/alexcrichton/wait-timeout) | 0.2.1 | Apache-2.0 OR MIT |
| [`zmij`](https://github.com/dtolnay/zmij) | 1.0.23 | MIT |

This notice must be regenerated and reviewed whenever locked dependencies, vendored native code, fonts, icons, media, or other assets change — `cargo license --json --avoid-build-deps --avoid-dev-deps --filter-platform aarch64-unknown-linux-gnu` reproduces the runtime table above; drop `--avoid-build-deps --avoid-dev-deps` and diff against it for the build/test-only table.

# Working in this repository

Work only inside this repository.

Do not scan the entire repository at the start of every task. First inspect
only `git status`, the latest five commits, `ARCHITECTURE.md`, the relevant
section of `MILESTONE_CHECKLIST.md`, and files explicitly named in the task.
Use targeted search to locate additional symbols.

Never request, access, reproduce, generate, store, or commit certificates,
private keys, passwords, SSH keys, credential identities, NDA information,
phone identifiers, or user content. Tests must use freshly generated
synthetic credentials.

Do not redesign the project or repeat completed research. Maintain the
existing Rust architecture and separation between protocol, transport, TLS,
credentials, media, UI, and hardware services (see `ARCHITECTURE.md`).

Development remains on the Pi 5 until wired and wireless Android Auto,
packaging, appliance startup, and stability are complete. Native Pi
formatting, Clippy, and test results are authoritative; results from other
hosts are informative only.

The proven handoff state is commit `f1207d2`: USB accessory transition,
protocol version negotiation, and TLS completion succeeded with an
operator-authorised external identity. Full authentication, service
discovery, video, audio, microphone, touch, and reconnect are not yet
complete.

The current milestone is a new, explicitly gated authentication/
service-discovery probe. Preserve the existing TLS-only `credential-probe`
(`apps/aa-headunit-diagnostics/src/live_probe.rs`) unchanged. Test against a
deterministic fake phone first, enforce strict bounds and timeouts, log no
raw payloads, and stop before any unreviewed service response or media
setup.

Before editing, state which files you intend to inspect. After editing, run
formatting, compile checks, strict Clippy, tests, secret-marker scanning,
ARM64 packaging, and GitHub CI. Explain outcomes in plain English and
clearly distinguish proven behaviour from assumptions.

## Verification commands

These match `.github/workflows/ci.yml` exactly — run them as-is, no
`--all-features` flag needed (the new `test-support` feature is wired
through `crates/protocol-aap/Cargo.toml`'s `[dev-dependencies]` entry for
`transport-api`, so `--workspace`/`--all-targets` already picks it up):

```
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Also run a secret-marker scan before committing (grep the diff for PEM
headers / private-key markers) and the project's ARM64 `.deb` packaging
step.

## Status as of this note

Verified on native Pi 5 Rust tooling (fmt, check, strict clippy, full
workspace test suite, secret-marker scan, ARM64 `.deb` packaging all pass):
the fake in-memory transport (`crates/transport-api/src/fake.rs`), the
fake-phone handshake integration test
(`crates/protocol-aap/tests/fake_phone_transport.rs`), parser fuzz/property
tests for untrusted phone input (`crates/protocol-aap/tests/property_fuzz.rs`),
and the gated `auth-discovery-probe` CLI subcommand in
`apps/aa-headunit-diagnostics` are implemented and committed.

`crates/protocol-aap/src/service_discovery.rs`'s `ServiceDiscoveryRequestSummary`
field naming is now confirmed directly against the pinned primary AASDK
source (`docs/protocol/aasdk-adoption.md`, revision `9bf6adf`): field 4 is
`label_text_bytes` and field 5 is `device_name_bytes`. An earlier revision of
this file incorrectly named them `device_name_bytes`/`device_brand_bytes`
(sourced from a secondary reference instead of the pinned primary source);
there is no `device_brand` field in the schema. This was a naming-only
defect — the summarizer only ever recorded byte length and validated UTF-8,
never the field's semantic name.

The newer AASDK `Service`/`ServiceDiscoveryResponse` schema is now mapped
field by field in `docs/protocol/aasdk-adoption.md` for the five service
kinds the current catalogue models (sensor source, media sink, input
source, media source, Bluetooth) and explicitly contrasted against
OpenAuto's older `ChannelDescriptor` schema. The remaining eight nested
service types and their leaf enum/config messages are recorded as not yet
mapped. No `Service`/`ServiceDiscoveryResponse` Rust wire encoder exists;
response encoding remains gated.

`auth-discovery-probe` previously rejected any AAP frame with the
`Encrypted` flag outright, which would have hard-failed against a real
phone's post-handshake `AuthComplete`/`ServiceDiscoveryRequest` traffic
(sent as TLS-encrypted application data, not more `EncapsulatedTls`
handshake messages). `TlsClient` (`crates/protocol-aap/src/tls.rs`) now has
`encrypt_application_data`/`decrypt_application_data`, implemented on
`OpenSslTlsClient` using the same live post-handshake `SslStream` the
handshake completed on (`crates/security-openssl/src/linux.rs`, no session
reconstruction). `auth-discovery-probe`'s receive loop decrypts each
`Encrypted` frame's payload before it reaches bounded reassembly, and
rejects an encrypted frame only if it arrives before TLS has completed.
Verified with real OpenSSL crypto, not fakes: client/server round-trip,
split/coalesced TLS records, invalid ciphertext, premature use before
handshake completion, session closure, and sanitized errors
(`crates/security-openssl/src/linux.rs`'s test module), plus a real TLS 1.2
handshake and a possibly-fragmented encrypted `ServiceDiscoveryRequest`
reassembled end to end (`crates/protocol-aap/tests/encrypted_service_discovery.rs`).
This work also surfaced and fixed a latent defect in the frame codec itself
(`crates/protocol-aap/src/lib.rs`): `decode_frame`/`encode_frame` compared
a first frame's declared total against that frame's on-wire length
unconditionally, which is only valid when both are plaintext-domain (true
for plain frames) — for encrypted frames the wire length is ciphertext,
which can exceed a small plaintext total by TLS per-record overhead. The
check is now skipped only for `Encryption::Encrypted`; the `Plain` path is
unchanged and still strictly enforced. Confirmed against the pinned primary
AASDK source (`docs/protocol/aasdk-adoption.md`, "Encrypted-message
framing"): the declared total is plaintext-domain, matching what the fix
assumes.

`usb auth-discovery-probe --device <bus:address> --allow-live-aap` has now
been run on Pi 5 against a real phone (USB accessory transition, the phone
re-enumerated as the documented Google AOA accessory ID) using the
operator-authorised external identity, requiring `sudo` since
`/etc/aa-headunit/credentials` is root-only (`0700`). It reached
`probe_result=service_discovery_summary_received`: version negotiated, TLS
handshake completed, `AuthComplete` sent, and — the specific behaviour this
session's TLS application-data work targeted — the phone's real
TLS-encrypted `ServiceDiscoveryRequest` was decrypted and reassembled
(`probe_state=encrypted_frame_received`) into a bounded, byte-count-only
summary. The probe stopped cleanly before any response or media setup, and
the USB interface was released cleanly. The installed `/usr/bin/aa-headunit-diagnostics`
package binary was stale at the time (missing `auth-discovery-probe`
entirely), so the freshly built `target/release/aa-headunit-diagnostics`
was used instead. The `.deb` has since been rebuilt from current source and
reinstalled (`packaging/debian`, via a temporary `debian` symlink at the
repo root removed after the build); `/usr/bin/aa-headunit-diagnostics` now
includes `auth-discovery-probe` under both `usb` and `developer`, confirmed
by re-running `usb auth-discovery-probe --device <bus:address>
--allow-live-aap` from the installed binary directly against the real
phone — same clean result (`probe_result=service_discovery_summary_received`,
stopped before response/media setup). The package binary is no longer
stale.

Clean timeout, malformed-message, unplug, and reconnect recovery are now
proven, all on Pi 5 against the real phone (see `MILESTONE_CHECKLIST.md`
M2 for full detail): a timeout was captured naturally (a stale-app-state
phone left `auth-discovery-probe` waiting past its 10s `PROBE_TIMEOUT`,
failing closed with no hang); malformed-message recovery is proven at the
parser boundary the real probe actually calls, via
`property_fuzz.rs`/`encrypted_service_discovery.rs` (a real phone sending
genuinely malformed bytes isn't reproducible from the head-unit side, so
fuzzing the exact same parser code path is the correct proof, not a
compromise); `usb hold --device <bus:address> --seconds N` proved unplug
detection with a real physical unplug (`hold_result=unplug_detected`, no
hang) — this also surfaced and fixed a pre-existing bug in `usb hold`
(`apps/aa-headunit-diagnostics/src/main.rs`) where two mutually-exclusive
accessory-mode checks made the command fail unconditionally regardless of
device state; reconnect recovery was proven by physically replugging after
that unplug and immediately re-running `auth-discovery-probe`
successfully. `usb auth-discovery-probe --device <bus:address>
--allow-live-aap` needs `sudo` (credentials are root-only `0700`); the
installed `/usr/bin/aa-headunit-diagnostics` package is current and
includes `auth-discovery-probe`, so either it or a freshly built
`target/release/aa-headunit-diagnostics` works.

Since the note above, the remaining `Service`/`ServiceDiscoveryResponse`
schema mapping (all 13 nested kinds, every leaf enum/config message, and
`DriverPosition`/`ConnectionConfiguration`/`HeadUnitInfo`) was completed in
`docs/protocol/aasdk-adoption.md`, and a full implementation was built on
top of it, scoped to `Video`/`Input`/`MediaAudio`:
`crates/protocol-aap/src/{protobuf,service_discovery_response,channel_open,
media_message,video_setup}.rs`, wired into `auth-discovery-probe`
(`apps/aa-headunit-diagnostics/src/auth_discovery_probe.rs`). This sends
`ServiceDiscoveryResponse` the instant the phone's request summary is
received, drives `ChannelOpenRequest`/`ChannelOpenResponse` for all three
channels, and drives the video channel through `Setup`→`Config`→`Start`.
It's proven correct end to end with real TLS crypto and real frame
reassembly across three concurrently-fragmenting channels
(`crates/protocol-aap/tests/full_channel_setup.rs`), and the frame codec,
message assembler, and every new state machine have their own unit tests.

**This is blocked on real hardware, not a natural stopping point.** Running
`usb auth-discovery-probe --allow-live-aap` against a real phone reaches
`probe_state=service_discovery_response_sent` cleanly (TLS-encrypted, no
local error) and then the phone shows Android Auto's **"Error 2: phone and
car are running incompatible software"** — no `ChannelOpenRequest` ever
arrives. Three independent, minimal, reversible hypotheses were tested
against the real phone and each refuted: a missing audio service, the
offered protocol version (the phone negotiates `1.7`, undocumented in any
known open-source AASDK fork — offering `1.7` instead of the pinned `1.6`
made no difference and was reverted), and missing head-unit identity
(`HeadUnitInfo`, now populated and kept). The `MediaAudio` channel and
`HeadUnitInfo` both remain in the code as genuine completeness
improvements, independent of the fact neither alone fixed Error 2.

Full investigation writeup, confirmed facts, what's ruled out, what isn't,
and research leads for whoever picks this up:
**`docs/protocol/error-2-investigation.md`**. **First step in a new
session: read that file, then run the verification commands above and
report the actual results before writing any more code.**

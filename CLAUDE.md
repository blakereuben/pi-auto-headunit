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
package binary was stale (missing `auth-discovery-probe` entirely); the
freshly built `target/release/aa-headunit-diagnostics` was used instead —
worth re-installing the package before the next real-phone run.

**First step in a new session: run the verification commands above and
report the actual results before writing any more code.** The next M2
milestone step is proving clean timeout, malformed-message, unplug, and
reconnect recovery against a real phone — not yet attempted.

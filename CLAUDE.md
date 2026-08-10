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

Working tree has uncommitted groundwork for the new probe, not yet verified
on real Rust tooling (written from a sandbox with no Rust toolchain and no
network access — logic was traced against existing compiling patterns in
the repo, not compiled):

- `crates/protocol-aap/src/service_discovery.rs`: fixed a field-mapping bug
  in `ServiceDiscoveryRequestSummary` — field 4 is `device_name_bytes` and
  field 5 is `device_brand_bytes` (previously reversed/mislabeled as
  `label_text_bytes`). Confirmed against the `aap_protobuf` schema.
- `crates/transport-api/src/fake.rs` (new, behind the `test-support`
  feature): a bounded in-memory duplex `SessionTransport` pair
  (`fake::Transport` / `fake::Peer`) for deterministic scripted-peer tests,
  with its own unit tests.
- `crates/protocol-aap/tests/fake_phone_transport.rs` (new): an integration
  test that drives the real frame codec + `MessageAssembler` +
  `HandshakeStateMachine` over the fake transport against a scripted
  deterministic phone (version accept → fake TLS → `AuthComplete` sent →
  synthetic `ServiceDiscoveryRequest` → asserts only a bounded summary is
  produced and nothing further is sent).

**First step in a new session: run the verification commands above and
report the actual results before writing any more code.** If everything is
green, the next milestone step is the gated `auth-discovery-probe` CLI
subcommand in `apps/aa-headunit-diagnostics`, reusing this logic against
real USB/TCP transports behind an explicit `--allow-live-aap`-style opt-in,
mirroring `credential-probe`'s pattern without modifying it.

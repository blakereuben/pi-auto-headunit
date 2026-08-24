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

## Status as of this note (corrected 2026-08-23 — see note below)

**The "blocked on real hardware" framing and the file pointer this section
used to end with were stale and have been removed.** They described the
Android Auto "Error 2" investigation while it was still open. That
investigation is long since closed: `docs/protocol/error-2-investigation.md`
itself is headed **"FULLY RESOLVED"** (2026-08-15) — the phone was confirmed,
on its own screen, actively driving-mode navigating with Google Maps over a
real, live session, and real H.265 video was confirmed on the head unit's
own physical display. Full authentication, service discovery, video, audio,
microphone, touch, and reconnect — everything this section previously
listed as incomplete — have all since been built and real-hardware-confirmed
(milestones M2 through M7).

`MILESTONE_CHECKLIST.md` is the authoritative, actively-maintained record of
what's done and what's open — this file will not attempt to duplicate it,
since a second copy of that status is exactly how this section went stale
last time. As of this correction, M0–M7 are substantially complete and
**M8 (Pi 5 completion gate)** is the current milestone in progress: several
substantial items remain (cold-boot and wired/wireless-switching
determinism, no-VNC operation, failure-mode and soak testing, the
compatibility matrix, and publish-readiness), plus a full whole-application
security pass beyond the wireless-specific one already done. Read
`MILESTONE_CHECKLIST.md`'s M8 section directly for the current, precise
state of each item before starting work.

`docs/protocol/error-2-investigation.md` remains valuable as a detailed
historical record of how that investigation was actually resolved (useful
if a similar protocol-level symptom ever recurs), not as a pointer to an
open blocker.

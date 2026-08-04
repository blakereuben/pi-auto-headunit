# Exact First Implementation Milestone: Documented USB/AOA Vertical Slice

## Implementation status (3 August 2026)

Implemented in the workspace:

- Rust workspace and transport/platform boundaries.
- Documented AOA state machine and Linux/libusb adapter.
- Explicit `BUS:ADDRESS` selection before any AOA control request.
- Linux Raspberry Pi/OS inventory and sysfs radio discovery.
- Independent Wi-Fi/Bluetooth `Auto`, `Onboard`, and stable USB-provider selection policy.
- Command-line preflight, wireless, USB listing, and AOA diagnostic entry points.
- Development Debian metadata, narrow udev rules, protocol certainty matrix, hardware evidence template, Linux CI workflow, formatting, strict linting, and portable unit tests.

Validated locally and on the Pi 5 reference system:

- `cargo check --workspace --all-targets --locked` passes.
- strict workspace Clippy passes with warnings denied.
- 15 native tests pass on the Pi 5, including deterministic unplug injection at every backend-driven AOA state.

Pending Pi 5 reference evidence:

- finish the repeated physical cable soak, USB-radio fallback, and older-version package upgrade test;
- run the documented AOA transition against additional phones when available (first Samsung/Pi 5 transition and reconnect passed);
- retain the passing controlled physical unplug test while the bulk interface is actively claimed; faster control-stage unplug paths are covered deterministically with the fake backend;
- complete USB-radio fallback tests (the native `.deb` build/install/remove/purge/reinstall lifecycle passes);
- defer Pi 4, CM4/Waveshare, and CM5 physical evidence to the later cross-board validation phase.

Milestone 1 is therefore **Pi 5 reference in progress** and **product in progress**. Pi 4, CM4/Waveshare, and CM5 physical checks are intentionally deferred to the later cross-board validation phase by product-owner decision.

## Objective

Build the smallest native Rust vertical slice that proves reliable USB host access on all target boards and performs only the publicly documented Android Open Accessory discovery/transition. It ends after the accessory interface and bulk endpoints are identified. It does **not** initiate or claim an Android Auto session.

## Why this is first

USB topology, permissions, re-enumeration, cancellation, and carrier behavior are prerequisites for every wired feature. This slice produces useful hardware evidence while avoiding undocumented post-AOA protocol assumptions.

## In scope

### Repository foundation

- Cargo workspace and minimum crates/apps needed for this slice: `transport-api`, `transport-usb`, `platform-api`, `platform-linux`, `platform-rpi`, `diagnostics`, and `aa-headunit-diagnostics`.
- Formatting, lint, unit tests, dependency/license policy, security policy, contribution basics, and `arm64` build CI.
- ADR-0001 (modular monolith) and an initial ADR-0002 that marks post-AOA protocol behavior as unresolved.

### USB behavior

- Enumerate USB devices through a maintained Rust/libusb binding.
- Recognize AOA support using the public AOA protocol/version request.
- Read and validate the AOA version.
- Send project-approved accessory identification strings with their provenance documented.
- Request accessory mode, detect disconnect/re-enumeration, find the resulting interface/endpoints, and open the bulk transport.
- Handle already-in-accessory-mode devices.
- Handle unplug during every state without panic, leaked handle, or stuck process.
- Handle multiple candidate phones deterministically: list them and require an explicit selector in the diagnostic; do not attach arbitrarily.
- Apply bounded per-state timeouts and cancellation.

### Platform/preflight behavior

- Report board/SoC family from reliable system data while keeping USB logic model-independent.
- Report kernel/OS/architecture and fail clearly outside the approved Trixie `arm64` baseline.
- Report USB bus/port path, negotiated USB speed, permissions, interface class/endpoints, and power/undervoltage indicators that Linux exposes reliably.
- Report Wi-Fi and Bluetooth independently as `Absent`, `Ready`, `Disabled`, or `Degraded`, including all onboard and external providers and which provider would be selected. This is detection only; wireless Android Auto remains out of scope.
- Define and test the provider-selection policy and configuration schema (`Auto`, `Onboard`, or stable USB adapter identity). A graphical settings control is not part of this diagnostic milestone.
- Redact serial numbers and stable phone identifiers by default.
- Provide actionable exit codes/categories: no device, permission denied, unsupported AOA, transition timeout, endpoint error, unplugged, unsupported platform, internal failure.

### Tests

- Fake USB backend unit tests for every state, timeout, cancellation point, duplicate device, multiple devices, already-accessory state, malformed control response, and unplug.
- State-machine/property tests asserting no invalid transitions and terminal cleanup.
- A parser fuzz target for USB descriptors/control responses owned by this slice.
- Opt-in physical integration test on the Pi 5 reference system; Pi 4, CM4, and CM5 run during the later cross-board validation gate.
- Use the Pi 5 8 GB/NVMe as the reference build and hardware target. Preserve the Pi 4 8 GB and CM4 4 GB/eMMC on Waveshare CM4-IO-BASE-B revision 3.1 as later compatibility targets; add CM5 when available.
- Wireless-capability tests on at least one wireless and one non-wireless Compute Module variant, supported USB Wi-Fi/Bluetooth adapters, mixed onboard/USB arrangements, and injected rfkill, hot-unplug, stopped-service, and missing-firmware cases where safe.
- Manual matrix with at least two current Android phones from different vendors if available; record Android OS and Android Auto version.
- 100 connect/disconnect cycles on one Pi 4-family and one Pi 5-family reference system.

### Packaging preview

- Produce an unsigned development `arm64` `.deb` in CI containing only the diagnostic and the minimum udev rule needed for the test.
- The development package installs no always-on service and modifies no boot configuration.
- Verify clean install/remove and that unprivileged access works only for the intended device/interface scope.

## Explicitly out of scope

- Android Auto version negotiation, TLS/security handshake, service discovery, channel setup, video, audio, microphone, touch return, Bluetooth, Wi-Fi, GTK UI, compositor changes, auto-start service, or boot optimization.
- Copying AASDK/OpenAuto protocol definitions or constants without the approved provenance/license decision.
- Raw USB/session capture committed to the repository.
- Claiming that accessory mode proves Android Auto compatibility.

## State model to implement

```text
Idle
  -> CandidateSelected
  -> QueryingAoaVersion
  -> SendingIdentification
  -> RequestingAccessoryMode
  -> WaitingForReenumeration
  -> OpeningAccessoryInterface
  -> BulkTransportReady
  -> Closed
```

Every non-terminal state can transition to `Cancelled`, `Unplugged`, `TimedOut`, or a typed `Failed` result and must release owned resources.

## Deliverables

1. Native diagnostic executable and the slice crates above.
2. Fake backend and automated tests.
3. Development `arm64` `.deb` artifact.
4. Four-board USB evidence report and phone matrix.
5. Wireless capability report demonstrating correct detection on wireless and non-wireless CM variants.
6. Sanitized state-transition logs from successful and unplug cases.
7. Updated protocol certainty matrix showing exactly which operations are P0 and what remains PX.
8. Short maintainer note identifying all unsafe/FFI boundaries and dependencies/licenses.

## Definition of done

- A normal user with the package-granted permissions can run the diagnostic; root is not required.
- Each target board reaches `BulkTransportReady` with at least one test phone on a documented USB-host-capable port/carrier.
- The process returns to `Idle` or exits cleanly after unplug at every injected state.
- The 100-cycle test shows no increasing open-handle count and no statistically meaningful unbounded resident-memory trend.
- Unit/property/fuzz smoke tests and `arm64` package build pass in CI.
- Logs contain no raw serial, media, microphone, contact, navigation, or protocol payload data.
- A non-wireless CM starts normally in wired mode, reports both onboard radio capabilities as absent, and identifies supported attached USB providers; disabled, degraded, or unplugged radios do not prevent wired operation.
- Reviewers can map every AOA control operation to public AOSP documentation.
- A repository search and dependency audit show no post-AOA Android Auto session implementation.

## Stop conditions

Stop and request a decision if:

- target carrier hardware cannot expose a stable host port;
- required device access appears to require running the application as root;
- accessory identification values needed for Android Auto cannot be justified by an approved source;
- a dependency introduces an incompatible license; or
- tests require storing sensitive captures without an approved handling process.

## Approval required to begin

Implementation starts only after the user approves this plan and approves or amends M0’s Trixie baseline, license/provenance strategy, and reference hardware assumptions.

Public AOA reference for this milestone: https://source.android.com/docs/core/interaction/accessories/aoa

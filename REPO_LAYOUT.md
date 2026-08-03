# Proposed Repository Layout

The repository starts as a Cargo workspace with documentation and Debian packaging beside the code. This is a plan; the directories are not to be created until implementation is approved.

```text
pi-auto-headunit/
├── README.md
├── LICENSES/
├── LICENSE
├── CONTRIBUTING.md
├── CODE_OF_CONDUCT.md
├── SECURITY.md
├── CHANGELOG.md
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── deny.toml
├── PRD.md
├── ARCHITECTURE.md
├── REPO_LAYOUT.md
├── MILESTONES.md
├── RISK_REGISTER.md
├── MILESTONE_01.md
├── PACKAGING.md
├── docs/
│   ├── adr/
│   │   ├── 0001-modular-monolith.md
│   │   └── 0002-protocol-provenance-and-license.md
│   ├── protocol/
│   │   ├── provenance.md
│   │   ├── certainty-matrix.md
│   │   └── interoperability-lab.md
│   ├── hardware/
│   │   ├── support-matrix.md
│   │   ├── reference-setups.md
│   │   └── touchscreen-calibration.md
│   ├── operations/
│   │   ├── diagnostics.md
│   │   ├── privacy.md
│   │   └── troubleshooting.md
│   └── testing/
│       ├── phone-matrix.md
│       └── performance-methods.md
├── crates/
│   ├── transport-api/
│   ├── transport-usb/
│   ├── protocol-types/
│   ├── protocol/
│   ├── session/
│   ├── media-api/
│   ├── media-gstreamer/
│   ├── ui-model/
│   ├── ui-gtk/
│   ├── platform-api/
│   ├── platform-linux/
│   ├── platform-rpi/
│   ├── diagnostics/
│   ├── config/
│   └── app/
├── apps/
│   ├── aa-headunit/
│   ├── aa-headunit-diagnostics/
│   └── aa-headunit-setup/
├── assets/
│   ├── icons/
│   ├── themes/
│   └── schemas/
├── config/
│   └── default.toml
├── packaging/
│   ├── debian/
│   │   ├── control
│   │   ├── rules
│   │   ├── changelog
│   │   ├── copyright
│   │   ├── aa-headunit.install
│   │   ├── aa-headunit.conffiles
│   │   └── source/
│   │       └── format
│   ├── systemd/
│   │   ├── aa-headunit.service
│   │   └── aa-headunit-preflight.service
│   ├── udev/
│   │   └── 70-aa-headunit.rules
│   ├── polkit/
│   └── tmpfiles/
├── tests/
│   ├── protocol-vectors/
│   ├── scripted-peer/
│   ├── package/
│   └── hardware/
├── fuzz/
│   ├── framing/
│   └── messages/
├── tools/
│   ├── dev/
│   ├── fixture-redaction/
│   └── release/
└── .github/
    ├── ISSUE_TEMPLATE/
    ├── pull_request_template.md
    └── workflows/
        ├── ci.yml
        ├── arm64-package.yml
        ├── security.yml
        └── release.yml
```

## Boundary notes

- `protocol-types` is separate so generated or provenance-sensitive definitions have an obvious audit boundary.
- `transport-usb` knows AOA and USB, but not GTK/GStreamer or board models.
- Planned post-wired crates are `network-api`, `network-networkmanager`, `bluetooth-api`, `bluetooth-bluez`, and `transport-wireless`. The wireless transport depends on those contracts, not directly on NetworkManager, BlueZ, or a carrier model.
- A planned `carrier-profile` crate owns the schema and profile-matching policy; platform adapters perform the Linux operations.
- Hardware documentation will include `carrier-profiles.md`, the exact Waveshare reference, custom-carrier requirements, and the CM4/CM5 pin/peripheral compatibility matrix.
- `session` knows protocol states, but not concrete USB or media implementations.
- `media-gstreamer`, `ui-gtk`, `platform-linux`, and `platform-rpi` are replaceable adapters.
- `app` is composition and lifecycle logic, not a dumping ground for protocol or hardware code.
- `apps` contains thin executable entry points only.
- Hardware tests are skipped unless explicitly selected; they never silently run in ordinary contributor CI.
- Fixtures must be synthetic or sanitized and must carry provenance metadata. Raw captures are never committed.
- Packaging lives in the same repository and is tested from the first vertical slice, not added after feature completion.

## Ownership proposal

Use `CODEOWNERS` only after maintainers exist. Suggested review domains are protocol/provenance, media/platform, UI/accessibility, packaging/release, and security/privacy. A protocol change should require both a protocol reviewer and a provenance check.

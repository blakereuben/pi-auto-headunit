# Milestone Plan

Milestones are exit-gated. Dates are intentionally omitted until the protocol source, team capacity, and hardware lab are known. A milestone is complete only when its evidence is checked into the repository or linked from the release record.

## Execution order

Development is Pi 5 reference-first. M1 through the first complete wired projection are implemented and stabilized on the Pi 5/NVMe reference system before physical Pi 4, CM4/Waveshare, and CM5 validation. Cross-board work remains mandatory before product 1.0, but it is grouped into the later hardware-validation gate rather than blocking forward feature development after each Pi 5 milestone.

Each milestone records two statuses where relevant:

- **Reference complete:** passed on the Pi 5 reference system.
- **Product complete:** passed on the full required board/carrier matrix.

## M0 — Decisions and lab readiness

Deliverables:

- Approve PRD, architecture, target license strategy, naming placeholder, and Trixie baseline.
- Acquire/reference one Pi 4, CM4 carrier, Pi 5, CM5 carrier, two supported touch displays, audio output, USB microphone, and representative Android phones.
- Record USB topology for each carrier and confirm host-mode ports.
- Create the first carrier-profile requirements sheet for the Waveshare CM4-IO-BASE-B Mini Base Board (B), revision 3.1, including its USB 2.0 hub/FFC expansion, NVMe, DSI, HDMI, RTC, fan, and power topology.
- Inventory wireless and non-wireless CM4/CM5 variants and define observable `Absent`, `Ready`, `Disabled`, and `Degraded` states.
- Record the original official 7-inch DSI Touch Display as the first 800 × 480 profile and define the requirements for larger HDMI/USB-touch and DSI replacements.
- Identify the exact CarPiHAT model/revision before relying on any of its audio, power, CAN, or vehicle-integration functions.
- Prepare the Pi 5 8 GB/NVMe as the primary native build machine and confirm SSH-based development from Windows 11.
- Obtain legal guidance on protocol provenance, reverse engineering, Google trademarks, and distribution.
- Define protocol P0/P1/P2/PX evidence rules and capture/privacy rules.
- Decide whether the project can use, adapt, or only study GPL-3.0 projects such as AASDK/OpenAuto.

Exit gate: no unresolved legal/provenance ambiguity about the source permitted for Milestone 1. Hardware lab owners and reference configurations are named.

## M1 — Documented USB/AOA vertical slice

This is the exact first implementation milestone. Its complete scope and acceptance criteria are in `MILESTONE_01.md`.

Outcome: a native Rust diagnostic detects a phone, performs the publicly documented AOA transition, survives re-enumeration and unplug, and reports the accessory bulk interface without claiming an Android Auto session.

Reference exit gate: all Pi 5 tests in `MILESTONE_01.md` pass and no post-AOA undocumented behavior is present. Product completion remains pending until the later Pi 4, CM4/Waveshare, and CM5 validation gate.

## M2 — Protocol feasibility and session skeleton

Deliverables:

- Implement framing and the minimum approved negotiation/session states from accepted P1/P2 sources.
- Add fake transport, scripted peer, limits, timeouts, cancellation, sanitized state-transition logs, parser fuzzing, and version/capability rejection behavior.
- Demonstrate a repeatable session reaching a precisely named state. Do not label it “connected” unless service/channel setup has actually completed.
- Produce ADR-0002 with protocol source, license impact, clean-room procedure if applicable, and publication constraints.

Exit gate: the team can show a lawful, maintainable route to full session interoperability. If not, stop/go decision is escalated before media/UI investment.

## M3 — Media/UI architecture spike

Deliverables:

- Validate GTK 4 + GStreamer + Wayland/DRM on Pi 4 and Pi 5 families.
- Render synthetic and approved captured encoded streams; measure startup, decode, presentation, memory, CPU, dropped frames, and end-to-end latency.
- Validate H.264 hardware decode on Pi 4/CM4 and the Pi 5/CM5 fallback; probe HEVC without assuming the phone can negotiate it.
- Validate ALSA playback/capture, stream-role policy, touchscreen geometry/rotation, and full-screen compositor startup.
- Decide ADR-0003/0005/0006.

Exit gate: 720p30, touch, audio output, and microphone meet provisional targets on one board from each SoC family, with documented fallbacks.

## M4 — First complete wired projection

Deliverables:

- Integrate session, video, media/system/speech audio, microphone, and touch.
- Implement explicit session lifecycle, phone consent guidance, hot-unplug, reconnect, and user-facing error categories.
- Add capability negotiation derived only from approved protocol evidence.
- Publish initial phone compatibility matrix and known limitations.

Exit gate: a 30-minute interactive drive-bench scenario passes on Pi 4 and Pi 5 reference setups without crash, unbounded memory growth, or private payload logging.

## M5 — Appliance integration and package beta

Deliverables:

- Dedicated user, udev permissions, systemd supervision, minimal compositor/session, configuration schema, preflight, diagnostics, and safe shutdown boundary.
- Debian source packaging and signed `arm64` `.deb` artifacts.
- Install/upgrade/remove/purge tests on fresh Trixie Lite and documented migration rules.
- Boot profiling and safe reductions; no boot optimization may hide recovery or diagnostics.

Exit gate: fresh install to usable appliance by following the published steps, service recovery works, and package tests pass.

## M6 — Wired beta hardening

Deliverables:

- Port and validate the Pi 5 reference implementation on Pi 4, CM4/Waveshare, and CM5, resolving differences through existing platform/media/carrier interfaces or reviewed architecture changes.

- Full four-board and carrier matrix, multiple phones/Android versions, 100-cycle reconnect, multi-hour soak, power interruption, thermal, low-storage, and missing-device tests.
- Fuzz/property testing and external security/license review.
- Accessibility and touch-target review, redacted support bundles, contributor documentation, issue templates, and release process.
- Performance budgets enforced in hardware CI where stable.

Exit gate: no open release-blocking defect, protocol provenance risk, or critical/high security issue.

## M7 — Wired 1.0

Deliverables:

- Versioned compatibility contract, release notes, signed package/checksums, SBOM/source offer as required, and rollback instructions.
- Stable configuration migration and support policy.
- Published limitations: experimental/uncertified status, Pi 5 codec fallback, supported displays/audio, and phone matrix.

Exit gate: all PRD 1.0 acceptance criteria pass.

## M8 — Wireless research and implementation (post-1.0)

Research first: discovery/pairing, Bluetooth role, Wi-Fi AP/client policy, credentials, encryption, coexistence, reconnect, regulatory/user experience, protocol provenance, BlueZ, and NetworkManager integration.

The wireless transport must implement the existing transport contract. It must not alter the wired state machine unless the protocol evidence requires an explicitly reviewed change.

The reference implementation prefers onboard CM4/CM5 Wi-Fi and Bluetooth when `Ready` and uses supported external USB adapters for either missing capability. It must validate onboard/onboard, onboard/USB, USB/onboard, and USB/USB provider combinations where the hardware permits them. It must also prove that the user interface and wired path remain correct while an external adapter is unplugged or a radio is disabled or degraded.

The first wireless release publishes a short adapter compatibility list by chipset and USB ID rather than claiming support for every Wi-Fi/Bluetooth dongle.

The settings UI must demonstrate independent `Auto`/`Onboard`/USB selection for Wi-Fi and Bluetooth, persistence across reboot, a clear missing-adapter state, and return to operation when the selected adapter is reattached.

## Parallel hardware track — production carrier PCB

This track begins after the Waveshare reference setup is stable and does not block early wired software work.

1. **H0 requirements:** exact phone USB role/power, display connector, touch, audio codec/amplifier/microphone, regulated automotive power, ignition/shutdown, eMMC flashing, cooling, antenna, service ports, and enclosure constraints.
2. **H1 compatibility matrix:** compare every used CM4/CM5 pin and peripheral against both datasheets and transition guidance; select only a verified common path or add explicit straps/muxes/assembly options.
3. **H2 schematic review:** electrical, USB signal/power integrity, RF keep-out/antenna choice, ESD, reverse polarity, surge/load-dump strategy, thermal, and test points.
4. **H3 prototype:** fabrication-neutral KiCad sources plus Gerbers, drill, BOM, and placement files suitable for PCBWay or another assembler; bench bring-up on both CM4 and CM5.
5. **H4 validation:** carrier identity/profile, wired/wireless soak, power interruption, thermal/RF testing, manufacturing test procedure, and revision-controlled errata.

Automotive electrical protection and regulatory/RF compliance require qualified hardware review; a successful development-board prototype is not sufficient evidence.

# Milestone Plan

Milestones are exit-gated. Dates are intentionally omitted until the protocol source, team capacity, and hardware lab are known. A milestone is complete only when its evidence is checked into the repository or linked from the release record.

## Execution order

Development is Pi 5 reference-first. Wired projection, wireless projection, appliance startup, packaging, and stability are implemented and completed on the Pi 5/NVMe reference system before any physical Pi 4, CM4/Waveshare, or CM5 validation. Cross-board work remains mandatory before product 1.0, but begins only after the Pi 5 completion gate.

`MILESTONE_CHECKLIST.md` is the operational source of truth for progress and the exact gate between Pi 5 development and other-board testing.

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

- Validate GTK 4 + GStreamer + Wayland/DRM on the Pi 5 reference system.
- Render synthetic and approved captured encoded streams; measure startup, decode, presentation, memory, CPU, dropped frames, and end-to-end latency.
- Validate the Pi 5 H.264 decode path and software fallback; probe HEVC without assuming the phone can negotiate it. Pi 4/CM4 media validation is deferred until M9.
- Validate ALSA playback/capture, stream-role policy, touchscreen geometry/rotation, and full-screen compositor startup.
- Decide ADR-0003/0005/0006.

Reference exit gate: 720p30, touch, audio output, and microphone meet provisional targets on the Pi 5, with documented fallbacks. Cross-SoC validation is deferred until the Pi 5 completion gate.

## M4 — First complete wired projection

Deliverables:

- Integrate session, video, media/system/speech audio, microphone, and touch.
- Implement explicit session lifecycle, phone consent guidance, hot-unplug, reconnect, and user-facing error categories.
- Add capability negotiation derived only from approved protocol evidence.
- Publish initial phone compatibility matrix and known limitations.

Reference exit gate: a 30-minute interactive drive-bench scenario passes on the Pi 5 without crash, unbounded memory growth, or private payload logging.

## M5 — Appliance integration and package beta

Deliverables:

- Dedicated user, udev permissions, systemd supervision, minimal compositor/session, configuration schema, preflight, diagnostics, and safe shutdown boundary.
- Debian source packaging and signed `arm64` `.deb` artifacts.
- Install/upgrade/remove/purge tests on fresh Trixie Lite and documented migration rules.
- Boot profiling and safe reductions; no boot optimization may hide recovery or diagnostics.

Exit gate: fresh install to usable appliance by following the published steps, service recovery works, and package tests pass.

## M6 — Pi 5 wired beta hardening

Deliverables:

- Multiple phones/Android versions, 100-cycle reconnect, multi-hour soak, power interruption, thermal, low-storage, and missing-device tests on Pi 5.
- Fuzz/property testing and external security/license review.
- Accessibility and touch-target review, redacted support bundles, contributor documentation, issue templates, and release process.
- Performance budgets enforced against the Pi 5 reference where stable.

Reference exit gate: no open Pi 5 wired release-blocking defect, protocol provenance risk, or critical/high security issue.

## M7 — Pi 5 wireless Android Auto

Deliverables:

- Complete the approved wireless protocol and security research before implementation.
- Support onboard and tested external USB Wi-Fi/Bluetooth providers through the existing transport/platform boundaries.
- Add independent `Auto`/`Onboard`/USB settings, persistence, actionable failures, and hotplug recovery.
- Complete wireless video, audio, microphone, touch, reconnect, and multi-hour soak testing on Pi 5.
- Prove wired operation is unaffected when wireless hardware is missing or degraded.

Reference exit gate: wireless Android Auto passes the Pi 5 acceptance checklist and has no unresolved protocol, security, privacy, or licence blocker.

## M8 — Pi 5 completion gate

Deliverables:

- Complete wired and wireless operation from cold boot without VNC.
- Complete Pi 5 package, recovery, power, thermal, missing-device, low-storage, and sustained-use validation.
- Publish a Pi 5 preview package and compatibility/limitations matrix.

Exit gate: every required Pi 5 item in `MILESTONE_CHECKLIST.md` passes. Only after this gate may physical work begin on other supported boards.

## Extras and install-method delivery (after M8, before M9/M10)

Starts only once M8's Pi 5 completion gate passes (wired and wireless
Android Auto both fully working). Runs before any cross-board (M9) or
1.0 (M10) work, since none of it needs more than one board. Sequence,
agreed 2026-08-21:

1. **Build the install methods (first pass).** The `.deb` package
   already exists (M5). Add the two new image variants: a full PiOS
   image with desktop (same app, tweaked for boot speed, keeps "return
   to desktop"), and a full PiOS image with no desktop at all (fastest
   boot; reuses `labwc` with LightDM and the desktop chrome stripped out
   rather than a new compositor; "return to desktop" is unavailable
   since there is nothing to return to). Both images are built by a
   one-off script that customizes an officially published PiOS base
   image (mount, chroot, install the project's own `.deb`, apply a
   couple of config drops) rather than a `pi-gen` build — rerun by hand
   against a new PiOS release, expected every few years, not months.
2. **Implement extras** — features layered on top of a fully working AA
   session, not required for AA itself to work:
   - Restructure navigation so Android Auto is one entry in the existing
     settings menu (alongside Gestures/Display/Themes/Equalizer/etc.)
     rather than the fixed backdrop everything else sits on top of. The
     live AA video/audio keeps running in the background regardless of
     which page is showing — this is a navigation change, not a
     rendering change. Needed before/alongside the install-method work
     above so all three install methods ship the same structure.
   - Dashcam GPS/speed overlay: request `SENSOR_LOCATION`/`SENSOR_SPEED`
     from the phone over the existing `SensorSourceService` (already
     partially implemented for night-mode/driving-status), and render
     them as an overlay on dashcam recordings.
   - Accurate equalizer: GStreamer's own `equalizer-10bands` element
     inserted into each of the three audio pipelines
     (`crates/media-gstreamer/src/audio.rs`), adjustable live via the
     same set-property-on-a-named-element pattern already used for the
     video sink. Stretch goal, not required for a first pass: use the
     project's existing microphone capture to play a calibration
     sweep/pink noise and compute a real measured room-correction curve
     instead of a flat starting point.
   - Samsung DeX (wired and wireless) — genuinely optional, "cool to
     include" rather than a real priority. Wired DeX cannot use either
     of the Pi's onboard HDMI ports (both are output-only; receiving
     video needs a capture chain: phone's DisplayPort-Alt-Mode over
     USB-C → a USB-C-to-HDMI adapter → a USB HDMI-capture device →
     the Pi). That capture capability is the same underlying feature as
     the existing empty `RearCamera`/`ScreenMirroring` stub pages and
     should be built once, generically, not DeX-specific. Wireless DeX
     needs the same research-before-code step as wireless AA (confirm
     whether it negotiates as standard Miracast/WFD or a proprietary
     Samsung layer) before any implementation is attempted. Selecting
     DeX tears down any active AA session first — not a Pi-side
     limitation, a real phone can't be in AA accessory mode and DeX
     desktop mode at the same time.
3. **Rebuild the install methods (second pass)** so the published
   package and both images include every extra from step 2, not just
   the core AA functionality from step 1.

Exit gate: all three install artifacts are published on GitHub and each
includes every extra implemented in step 2.

## M9 — Pi 4, CM4, and CM5 validation

Deliverables:

- Port and validate the frozen Pi 5 reference behaviour on Pi 4, CM4/Waveshare, and CM5.
- Test the non-wireless CM4 with external USB Wi-Fi/Bluetooth providers.
- Repeat wired, wireless, appliance, package, reconnect, soak, power, thermal, display, touch, and audio tests.
- Resolve board differences through platform/media/carrier interfaces or reviewed architecture changes.
- Publish the full board/carrier and peripheral compatibility matrix.

Exit gate: every supported target meets the published compatibility contract with no release-blocking defect.

## M10 — Project 1.0

Deliverables:

- Versioned compatibility contract, release notes, signed package/checksums, SBOM/source offer as required, and rollback instructions.
- Stable configuration migration and support policy.
- Published limitations: experimental/uncertified status, codec fallbacks, supported displays/audio/radios, and phone matrix.

Exit gate: all PRD 1.0 acceptance criteria pass.

## Wireless implementation policy

Research first: discovery/pairing, Bluetooth role, Wi-Fi AP/client policy, credentials, encryption, coexistence, reconnect, regulatory/user experience, protocol provenance, BlueZ, and NetworkManager integration.

The wireless transport must implement the existing transport contract. It must not alter the wired state machine unless the protocol evidence requires an explicitly reviewed change.

The reference implementation prefers onboard CM4/CM5 Wi-Fi and Bluetooth when `Ready` and uses supported external USB adapters for either missing capability. It must validate onboard/onboard, onboard/USB, USB/onboard, and USB/USB provider combinations where the hardware permits them. It must also prove that the user interface and wired path remain correct while an external adapter is unplugged or a radio is disabled or degraded.

The first wireless release publishes a short adapter compatibility list by chipset and USB ID rather than claiming support for every Wi-Fi/Bluetooth dongle.

The settings UI must demonstrate independent `Auto`/`Onboard`/USB selection for Wi-Fi and Bluetooth, persistence across reboot, a clear missing-adapter state, and return to operation when the selected adapter is reattached.

## Parallel hardware track — production carrier PCB

Requirements research may proceed in parallel, but physical carrier-board validation begins only after the Pi 5 completion gate. This track does not block Pi 5 software work.

1. **H0 requirements:** exact phone USB role/power, display connector, touch, audio codec/amplifier/microphone, regulated automotive power, ignition/shutdown, eMMC flashing, cooling, antenna, service ports, and enclosure constraints.
2. **H1 compatibility matrix:** compare every used CM4/CM5 pin and peripheral against both datasheets and transition guidance; select only a verified common path or add explicit straps/muxes/assembly options.
3. **H2 schematic review:** electrical, USB signal/power integrity, RF keep-out/antenna choice, ESD, reverse polarity, surge/load-dump strategy, thermal, and test points.
4. **H3 prototype:** fabrication-neutral KiCad sources plus Gerbers, drill, BOM, and placement files suitable for PCBWay or another assembler; bench bring-up on both CM4 and CM5.
5. **H4 validation:** carrier identity/profile, wired/wireless soak, power interruption, thermal/RF testing, manufacturing test procedure, and revision-controlled errata.

Automotive electrical protection and regulatory/RF compliance require qualified hardware review; a successful development-board prototype is not sufficient evidence.

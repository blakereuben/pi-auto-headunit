# Real-phone investigation: touch input (head unit → phone)

## Status: **FULLY RESOLVED — real-hardware-confirmed, including continuous drag (pan) and multitouch (pinch).**

Touch is architecturally the reverse direction from every other channel this
project has implemented so far: the head unit is the `InputSourceService`,
so it *sends* `InputReport` to the phone rather than receiving `Data`. The
full pipeline — real evdev capture of the official DSI touchscreen
(`ft5x06`), a portable multitouch state tracker
(`platform_api::touch::MultiTouchTracker`), wire-exact `InputReport`
encoding (`protocol_aap::encode_touch_report`), and wiring into the live
probe (`auth_discovery_probe.rs`'s `service_touch_input`) — was built and
verified (`cargo fmt`/`check`/`clippy`/`test`, secret-marker scan, ARM64
`.deb` packaging) before any real-hardware trial. See
`apps/aa-headunit-diagnostics/src/auth_discovery_probe.rs`'s module doc
comment for the implementation-level detail; this document is the
real-hardware trial record.

## Trial 1: wire-level crash on `MEDIA_MESSAGE_STOP`

The first real-hardware trial (fresh phone replug, `usb
auth-discovery-probe --allow-live-aap`) reached a genuinely live session —
input channel opened almost immediately, video streamed continuously and
was acked cleanly — but ended after roughly 5-8 seconds on a previously
unhandled message: wire id `32770` on the `MediaAudio` channel, immediately
after the phone requested audio-focus ducking
(`audio_focus_requested type=GainTransientMayDuck`). This project's
`MediaMessageId` enum had a gap at that exact value (`Setup`=32768,
`Start`=32769, `Config`=32771 — 32770 unmapped), and any unrecognized
message on an already-`Ready` media channel was treated as a hard,
probe-ending protocol error.

Confirmed against the pinned `aasdk` source
(`protobuf/aap_protobuf/service/media/sink/MediaMessageId.proto`): `32770`
is `MEDIA_MESSAGE_STOP`, an empty
(`aap_protobuf.service.media.shared.message.Stop` has no fields),
**unacknowledged** notification — confirmed from
`VideoMediaSinkService::handleStopIndication`
(`src/Channel/MediaSink/Video/VideoMediaSinkService.cpp`), which parses and
forwards it to the app layer but never calls `send()`. This is almost
certainly a normal part of audio-focus ducking, not something specific to
touch, and would have cut short *any* sufficiently long real trial, not
just this one.

**Fixed**: `MediaMessageId::Stop` added; `VideoSetupAction::StopReceived`/
`AudioSetupAction::StopReceived` handle it as a no-op, no-reply
notification in both `video_setup.rs` and `audio_setup.rs` (mirroring
`Data`/`CodecConfig`'s existing shape but without the `Ack`). Unit-tested
in both files (`stop_while_ready_is_a_no_reply_notification`).

## Trial 2: coordinate-space fix — taps confirmed working

With the `Stop` fix in place, a second trial ran the full session cleanly
to `observation_window_complete` and sent 93 well-formed `InputReport`s
(real touchscreen taps/drags, correct `Down`/`Moved`/`Up` sequencing) with
zero protocol errors — but touching the screen produced **no visible
reaction** on the phone, despite video genuinely rendering on the head
unit's own display throughout. `InputReport` has no wire-level ack, so this
couldn't be diagnosed from protocol success alone.

Leading hypothesis: `TouchCapability` advertised the DSI panel's native
800×480 resolution while `VideoConfiguration` advertises 1280×720 (a
pre-existing mismatch, dating to the 1280×720 experiment in
`docs/protocol/error-2-investigation.md`), and touch reports were scaled
into that same 800×480 space. Read the pinned `f1xpl/openauto` source
(`docs/protocol/openauto-adoption.md`) for comparison:
`InputDevice::handleTouchEvent`
(`src/autoapp/Projection/InputDevice.cpp`) explicitly rescales raw
touchscreen pixels into `displayGeometry_` — the video output resolution —
before ever building an outgoing touch event, suggesting the phone maps
touch against the frame buffer it renders, not the physical digitizer's
own resolution.

**Fixed**: `TOUCH_COORDINATE_SPACE_WIDTH`/`_HEIGHT` (1280×720) now used for
both `TouchCapability` advertisement and raw-coordinate scaling
(`EvdevTouchSource`), replacing the panel-native 800×480.

**Trial 3 (real hardware): confirmed.** The operator directly confirmed
"pressing on the screen worked" — single taps now land correctly. This
proves the wire-level pipeline, message framing, and coordinate scaling
are all genuinely correct end to end; it is not a base-level protocol
defect. Swipe (pan) and pinch still did not register, despite the same
trial sending 226 well-formed reports including 197 `Moved` and several
`PointerDown`/`PointerUp` multitouch transitions — the data was genuinely
being captured and forwarded, just not acted on.

## Trial 4: `action_index` always-required — inconclusive negative

Second hypothesis: `action_index` was left unset (`None`) for `Down` and
`Moved` phases in `MultiTouchTracker`/`encode_touch_report` (only
`PointerDown`/`PointerUp`/`Up` set it). Investigated
`opencardev/openauto` (same GitHub org as this project's pinned `aasdk`
fork, and — unlike the separately pinned `f1xpl/openauto` — already
updated to this project's exact `InputReport`/`TouchEvent`/
`sendInputReport` wire schema; **not yet formally adopted as a licensed
source**, this is read-only provenance for a protocol-behavior fact, per
the same posture already used elsewhere in this project for
not-yet-adopted reference reading):

- `InputSourceService::onTouchEvent`
  (`src/autoapp/Service/InputSource/InputSourceService.cpp`, commit
  `4cc739b813622739b09352655581072fc4d39280`, `main` branch as of
  2026-08-15) unconditionally calls
  `touchEvent->set_action_index(event.actionIndex)` for every touch event
  — never left unset.
- `InputDevice::handleMultiTouchEvent`
  (`src/autoapp/Projection/InputDevice.cpp`, same commit) sets
  `event.actionIndex = 0` explicitly for `TouchBegin` (down) and plain
  `TouchUpdate` movement, and to the changed pointer's index for
  `POINTER_DOWN`/`POINTER_UP`/`UP` — matching Android's own
  `MotionEvent.getActionIndex()` convention, but always present.

**Fixed**: `TouchFrame.action_index`/`encode_touch_report`'s `action_index`
and `action` parameters changed from `Option<u32>`/`Option<PointerAction>`
to plain `u32`/`PointerAction` — always sent, closing off the whole class
of "forgot to set it" bug rather than patching the two known instances.

**Trial 4 result (real hardware): no change.** 108 well-formed reports
(7 `Down`, 92 `Moved`, 2 `PointerDown`, 7 `Up`) sent cleanly, zero protocol
errors, clean `observation_window_complete` — but the operator confirmed
swipe/pan and pinch still produced no visible effect. This refutes
`action_index` omission as *the* (or *a*) blocking cause, though the fix is
independently correct (matches the reference implementation exactly) and
stays in the code.

## Trial 5: pointer-id allocation strategy — RESOLVED

Third hypothesis, following the suggested-next-steps path of comparing
against a known-working client instead of continuing to guess: LIVI
(`docs/protocol/livi-adoption.md`, formally adopted GPL-3.0-or-later
revision `9000f308eec423c5c56ac0a14491a7c95ce5762d`) was read directly.
`useProjectionTouch.ts`'s `alloc()`/`free()` deliberately maps the
browser's own arbitrary, large `PointerEvent.pointerId` down to the
smallest free non-negative integer per active contact, recycled on lift,
before `AaSession.ts`'s `SendMultiTouch` handler ever calls
`InputChannel.sendTouch` — LIVI never sends the browser's raw pointer id to
the phone.

This project's `MultiTouchTracker::to_point`
(`crates/platform-api/src/touch.rs`) did the opposite: `pointer_id:
state.tracking_id.unwrap_or(slot)` passed the Linux kernel driver's raw
`ABS_MT_TRACKING_ID` straight through — an ever-incrementing counter across
the touchscreen's whole session lifetime (every new contact gets a new,
larger id, never reused), unlike LIVI's deliberately small, reused ids. A
session with several taps followed by a two-finger pinch could hand the
phone pointer ids well past single digits, plausibly overflowing whatever
small fixed-size internal representation the phone's own input pipeline
uses to track concurrent pointers — this is exactly the "`pointer_id`
values grow unbounded" theory this document had already flagged as
worth hardening but not yet attempted.

**Fixed**: `to_point` now uses the kernel's own `ABS_MT_SLOT` index as
`pointer_id` instead of `ABS_MT_TRACKING_ID`. No manual allocate/free
bookkeeping was needed on the Rust side (unlike LIVI's JS layer, which only
has access to the browser's own arbitrary pointer id) — `ABS_MT_SLOT` is
already small, bounded by the touchscreen's simultaneous-touch capability,
and reused by the kernel driver itself as soon as a contact lifts, so using
it directly achieves the same property LIVI achieves manually. Formally
added to `docs/protocol/livi-adoption.md`'s adopted scope (item 7).

**Trial 5 result (real hardware): CONFIRMED.** 257 single-finger `Moved`
reports, 232 two-finger `Moved` reports, plus correct `Down`/`Up`/
`PointerDown`/`PointerUp` transitions, zero protocol errors, clean
`observation_window_complete` — and the operator directly confirmed
**drag/swipe and pinch now work** on the real phone screen. This closes the
investigation: the full touch pipeline (evdev capture, multitouch state
tracking, wire encoding, live TLS-encrypted send) is confirmed correct end
to end for taps, continuous drag, and multitouch gestures alike.

## Confirmed facts

- Real evdev multitouch capture of the DSI touchscreen (`ft5x06`,
  `/dev/input/event6` on the reference Pi 5), the `MultiTouchTracker`
  state machine, and `InputReport` wire encoding are all correct — proven
  by taps registering correctly on a real phone (trial 3).
- `TouchCapability`'s advertised coordinate space must match the video
  resolution actually negotiated, not the touchscreen's native panel
  resolution — the first confirmed root cause found this session.
- `MEDIA_MESSAGE_STOP` is a real, unhandled-until-now message a real phone
  sends during ordinary audio-focus ducking; unrelated to touch, but
  discovered by touch trials and now fixed for every AV channel.
- `pointer_id` must be a small, contact-lifetime-scoped id (the kernel's
  own `ABS_MT_SLOT` index), not the driver's raw, ever-incrementing
  `ABS_MT_TRACKING_ID` — the second confirmed root cause, found by direct
  comparison against LIVI's own pointer-id allocation strategy
  (`docs/protocol/livi-adoption.md`, adopted scope item 7) and
  real-hardware-confirmed to unblock continuous drag/pinch (trial 5).

## Refuted (or not the blocking cause, on their own)

- `action_index` being omitted for `Down`/`Moved` (trial 4) — fixed,
  independently correct, but not sufficient on its own to unblock
  swipe/pinch; needed alongside the trial 5 `pointer_id` fix.
- Something rate- or timestamp-sensitive, or specific to the app under test
  — never had supporting evidence, and are moot now that the actual cause
  (trial 5) is confirmed and gestures work end to end.

## Suggested follow-up (optional, not blocking)

The investigation is closed — touch input (taps, drag, pinch) is fully
functional. If anyone wants to hunt for further edge cases:

1. `opencardev/openauto` is still only read-only investigative provenance,
   not formally adopted (unlike LIVI, which now is, per
   `docs/protocol/livi-adoption.md`'s adopted scope). Formal adoption would
   need the project owner's explicit decision, not something to
   self-declare.
2. `MILESTONE_CHECKLIST.md`'s M4 "Return calibrated touch input to the
   phone" can now be checked off — continuous gestures are confirmed
   working, not just taps.

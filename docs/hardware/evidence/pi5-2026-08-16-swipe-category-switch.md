# Pi 5 swipe-direction app-category switching evidence — 16 August 2026

## Scope

`MILESTONE_CHECKLIST.md` M3's touch-rotation/gesture-settings item. At the
operator's explicit request, the three-finger-tap follow-up gesture was
renamed to a two-finger tap (three fingers weren't used anywhere else),
and four new single-finger follow-up gestures were added —
`GestureId::SwipeUp`/`SwipeDown`/`SwipeLeft`/`SwipeRight` — defaulting to
`Action::SwitchToNavigation`/`SwitchToMedia`/`SwitchToPhone`/`SwitchToRadio`.
Each `SwitchTo*` action sends a real, sourced car-specific `KeyCode`
(`Navigation`=65538/`Media`=65537/`Tel`=65540/`Radio`=65539, confirmed
against the pinned AASDK source, `docs/protocol/aasdk-adoption.md`) as a
down-then-up `InputReport`, dispatched from the background protocol
thread the instant the gesture completes.

Before this trial, whether a real phone does anything meaningful with
these key codes was an explicit, stated assumption — no approved source
describes their effect, only that they are the documented values.

## Trial

`sudo /usr/bin/aa-headunit-diagnostics usb gtk-dev-ui --device 001:031
--allow-live-aap` against a real phone, freshly rebuilt `.deb` installed
beforehand (full verification sweep — fmt, check, clippy strict, full
workspace test suite including the new gesture/key-event/keycode unit
tests, secret-marker scan — all passed first). Confirmed directly by the
operator on the physical screen and the phone, not inferred from logs:

- Settings panel's gesture-assignment dropdowns showed the four new swipe
  directions with their new default actions, as expected.
- Two-finger tap still opened settings correctly — the three-finger→
  two-finger rename didn't regress the existing gesture.
- Swipe up, swipe down, and swipe left each armed and fired correctly
  (four-finger arm swipe, then the directional swipe), and **the phone's
  Android Auto screen actually visibly switched app/category** for all
  three — confirming the `SwitchToNavigation`/`SwitchToMedia`/
  `SwitchToPhone` key codes work as intended on this phone.
- Swipe right (`SwitchToRadio`, `KeyCode::Radio`=65539) fired correctly
  on the head-unit side (armed, completed, key event sent — no local
  error), but the phone responded with **"AA was not available"**. The
  video/audio session was not otherwise disrupted; this was specific to
  the radio action.

### Follow-up trial: radio app installed

The operator installed RadioPlayer (an Android Auto-compatible radio
app) on the test phone and confirmed it works correctly *inside* Android
Auto (opened and used it directly). Re-ran the same swipe-right gesture
afterward, same 001:034 device, `AA_HEADUNIT_OBSERVATION_WINDOW_SECONDS=120`
for a longer trial window: **swipe-right still produced "AA not
available"**, confirmed directly by the operator. This rules out the
original "no radio app registered" theory — the app exists, is
AA-registered, and works. The current best guess (see
`docs/protocol/aasdk-adoption.md`'s `KeyCode` section) is that
`KEYCODE_RADIO` targets a real broadcast-radio tuner/HAL this
software-only head unit doesn't provide, distinct from the generic
app-category switching the other three car-specific codes perform — not
confirmed by any approved source, not investigated further this session.

### Follow-up trial: isolating the gesture from the action

To rule out a swipe-right-specific bug (as opposed to `KeyCode::Radio`
itself), the operator reassigned the gesture-to-action mapping live in
the settings panel — moving `SwitchToRadio` off swipe-right onto a
different swipe direction — and tried again. Same result: the phone
responds "AA was not available" for `SwitchToRadio` regardless of which
gesture triggers it. This confirms the failure is specific to the
`KEYCODE_RADIO` action itself, not swipe-right's gesture detection —
the gesture layer is not implicated.

### Settings-panel UI: five real-hardware iterations to a reliable design

The gesture-reassignment control itself surfaced a long real-hardware
debugging thread, independent of the radio question. Each gesture's
action list had grown from 3 to 7 options, and the original popup
`DropDown` no longer let every option be reached by touch on the 800x480
panel. In order, each of the following was built, packaged, installed,
and real-hardware-trialled — most were rejected or found broken, not
assumed:

1. Every `Action` as an always-expanded `ToggleButton` row, one row per
   gesture — rejected on sight as visually "awful" (too much permanent
   vertical space for a control touched only occasionally).
2. A compact `MenuButton` per gesture whose popover held every `Action`
   in a `ListBox` inside its own `ScrolledWindow` — real-hardware trial:
   touch-drag inside the popover did not scroll at all (only the
   scrollbar's own thumb did), and a hand-rolled `GestureDrag`-based
   scroll-vs-click arbitration, added to fix that, instead **silently
   mis-selected rows while the operator tried to scroll**, corrupting the
   persisted gesture→action mapping (all four swipe directions ended up
   set to `switch_to_radio`, `/var/lib/aa-headunit/settings.toml`) — a
   real, confirmed defect, not a false alarm.
3. Replaced the popover entirely with a full-panel action picker
   (`ActionPicker`, `gtk_dev_ui.rs`): tapping a gesture's button swaps the
   whole settings panel to a page of seven plain `Button`s, no popover, no
   competing gesture to arbitrate. Made the whole panel fill the window
   instead of floating as a small centered box, since a fixed-size panel
   didn't leave room for seven rows either way.
4. Even full-screen, one column of seven big buttons still needed
   scrolling, and the plain scrollbar thumb was explicitly rejected as
   unusable ("it MUST be a touch scroll inside") — added a manual
   `GestureDrag`-driven scroll (`enable_touch_drag_scroll`), safe this
   time since there's no `ListBox`/row-click gesture left to conflict
   with, only ordinary button clicks.
5. At the operator's explicit request, replaced scrolling with a
   two-column layout instead. A `FlowBox` (auto-wrapping) was tried first
   and, despite correct `min`/`max-children-per-line` configuration,
   real-hardware-confirmed to still render as one column. A `GtkGrid`
   with explicit `attach(widget, column, row, 1, 1)` placement (no
   wrapping heuristic to misbehave) finally produced real two-column
   layout, confirmed on both the action-picker page and — at a further
   explicit request — the main gesture-assignment list itself.

### Root cause of the "radio never works" question, sourced

Re-reading this project's own pinned-source mapping
(`docs/protocol/aasdk-adoption.md`) surfaced the actual answer, already
recorded but not previously connected: `AudioStreamType` (the enum
Android Auto uses for every audio content type it actually plays) has no
`RADIO` value at all — only `GUIDANCE`/`SYSTEM_AUDIO`/`MEDIA`/`TELEPHONY`
— and the pinned schema's real radio support is a wholly separate
`RadioService` (`aap_protobuf.service.radio.RadioService`), with its own
capability advertisement (`RadioProperties`: radio ID, AM/FM/AM-HD/FM-HD/
DAB/XM type) and ~25 further runtime tuning/scanning/preset messages —
none of which this project had implemented at the time
(`radio_service` had no `ServiceKind` in `service_catalogue.rs`).
`KEYCODE_RADIO` was never a software app-category switch the way
`MEDIA`/`NAVIGATION`/`TEL` are — real "radio" in Android Auto means a
car's own physical AM/FM/DAB/XM tuner, exposed through that separate
service.

### Follow-up: advertising `RadioService`, real-hardware-confirmed

Implemented the discovery-time-only side of that separate service —
`RadioCapability`/`RadioType` (`service_discovery_response.rs`),
`ServiceKind::Radio` (`service_catalogue.rs`), a new `RADIO_CHANNEL_ID`
using this project's already-proven generic channel-open path (no new
runtime message decoding — the ~25 tuning/scanning/preset messages stay
unimplemented, deliberately, with no real tuner hardware to drive them),
and a placeholder `RadioCapability` (`radio_id=0`, `FmRadio`,
`channel_spacing=100`) advertised in the real session
(`auth_discovery_probe.rs`). Real-hardware trial: the phone accepted the
new `ServiceDiscoveryResponse` cleanly (handshake, video, and audio all
continued to work normally), and swipe-right (`SwitchToRadio`) now
**navigates to Android Auto's own native radio screen** instead of
returning "AA not available" — confirmed directly by the operator. The
screen is empty, since no real tuning/station backend exists behind it,
but this conclusively confirms the actual mechanism: radio is a
first-class *native* Android Auto UI category, not a switch to a
third-party app like the RadioPlayer app installed earlier — the missing
piece really was the head unit never advertising the service at all.

`Action::SwitchToRadio`'s *default* mapping was moved off swipe-right
(now `ToggleFullscreen`, a redundant-but-functional choice, since a
fresh install shouldn't default to a screen with no real data behind
it) — `SwitchToRadio` itself is confirmed functional (navigates
correctly) and stays fully selectable; a real audio-capable radio
backend would need the ~25 unmapped runtime messages implemented against
real tuner hardware, which remains out of scope. The corrupted
`settings.toml` from the earlier popover bug (all four swipe directions
mapped to radio) was deleted directly on the Pi so sessions regenerate it
from the fixed defaults.

## Outcome

Four of four category-switch actions are real-hardware-confirmed
working end to end: gesture → key event → real, correct navigation on
the phone. Navigation, media, and phone switch to real third-party
apps; radio switches to Android Auto's own native (currently empty,
since no tuner backend is implemented) radio screen — a different but
equally real and correct mechanism, confirmed only after advertising the
previously-missing `RadioService` capability. The settings panel's
gesture-reassignment UI is now a two-column, no-popover, `Grid`-based
design, real-hardware-confirmed after five iterations — the third
iteration (popover + hand-rolled scroll gesture)
is the one that corrupted live settings data and should not be revisited
without addressing the scroll-vs-click arbitration bug that caused it.

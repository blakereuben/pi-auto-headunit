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
  the radio action. Given the other three category codes worked
  correctly using the same mechanism, the most likely explanation is that
  this phone simply has no Android-Auto-compatible radio app registered
  as the default handler for that category, not a defect in the encoding
  or dispatch path — genuinely unconfirmed, not investigated further this
  session.

## Outcome

Three of four category-switch actions (navigation, media, phone) are now
real-hardware-confirmed working end to end: gesture → key event → real
app switch on the phone. The radio action is real-hardware-tested but
inconclusive (phone-side "not available" response, not a local failure)
— left as a known, documented open question rather than treated as a
proven success or a bug to chase blindly.

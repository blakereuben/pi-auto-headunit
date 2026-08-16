# Pi 5 touch rotation / settings gesture evidence — 16 August 2026

## Scope

`MILESTONE_CHECKLIST.md` M3's "Verify touch rotation and calibration in
every supported screen orientation." No rotation-handling code existed
before this session. Built: a live-adjustable touch rotation mechanism
(`platform_linux::touch::Rotation`/`SharedRotation`), and, at the
operator's explicit request mid-session, a real settings UI in
`usb gtk-dev-ui` reachable via a new four-finger-swipe-then-follow-up
gesture (`platform_api::ArmedGestureDetector`), since rotation needed
somewhere live to be exposed and controlled.

## Trials

Nine separate real-hardware trials against a real phone over one session,
each preceded by a clean physical unplug/replug. Several found and fixed
real bugs; the final two trials passed cleanly with no new issues.

### Trial 1–2: initial gesture wiring

First attempt: the operator only performed the four-finger arming swipe,
never the required follow-up gesture (a genuine two-part sequence:
swipe-and-lift, then separately double-tap/three-finger-tap/long-press).
Nothing fired, as expected — not a bug.

### Trial 3: settings panel reachable, but no way back to fullscreen

The four-finger-swipe-then-double-tap sequence correctly opened the
settings panel. Pressing "Return to desktop" left fullscreen with no
gesture or button anywhere to get back — a real, genuine gap. Fixed by
adding a "Close" button (hides the panel only, doesn't touch window
state — previously conflated) and turning the desktop action into a real
bidirectional toggle, `Action::ToggleFullscreen`.

### Trial 4–5: hung-window bug

A deliberately long test window
(`AA_HEADUNIT_OBSERVATION_WINDOW_SECONDS=300`) was silently cut off at
120 seconds by `gtk_dev_ui.rs`'s hang-safety-net timer, which was a fixed
constant unaware of that override — the whole window and process died
mid-session, which looked identical to "the gesture stopped working."
Fixed by deriving the safety net from the same override plus a margin.

### Trial 6–7: the real rotation bug

With the hang-safety-net fixed, a real, reproducible pattern emerged: the
arming swipe fired once successfully, but never again after the operator
changed live rotation via the settings panel's "Cycle rotation" control,
even though the touch log showed real, correctly-shaped four-finger
swipes continuing to arrive. Root cause: the swipe-arm distance check
measured "moved down" using already-rotation-adjusted target-space
coordinates — once rotation remapped which raw axis fed target X vs Y, a
real physical downward swipe no longer produced an increasing target Y
at all. Fixed by switching to straight-line displacement *magnitude*,
which all four rotations preserve (they're isometries — axis swaps and
reflections, never scaling). Confirmed with a dedicated unit test
(`a_purely_horizontal_swipe_still_arms`) and, more importantly, a real
trial: 13 arm events and 10 completions (mixed `DoubleTap`/
`ThreeFingerTap`) over a full 5-minute session including a real live
rotation change mid-session.

### Trial 8: cross-contamination between recognizers

The operator reported the "direct to fullscreen" three-finger-tap path
didn't work reliably — it seemed to route through the settings panel
(the `DoubleTap`-mapped action) instead. Root cause: a three-finger tap's
fingers never land or lift in perfect lockstep on real hardware, so the
lone first-landing finger is, for one frame, indistinguishable from the
start of a real single-finger tap. The independent `DoubleTapRecognizer`
picked up that leading frame, and a later, unrelated single-finger touch
could complete a spurious `DoubleTap`. A second issue in the same area:
`ThreeFingerTapRecognizer`'s own release detection required an exact
"still 3 fingers, now lifting" frame, which `MultiTouchTracker`'s
documented one-transition-per-frame behavior (see that type's own doc
comment) often skips on real releases, silently failing the gesture.
Fixed both: track each touch episode's peak simultaneous finger count,
gate/reset the single-finger recognizers the instant an episode reveals
itself as multi-finger; and fire the three-finger tap as soon as the
count drops below three rather than waiting for an exact frame shape.
Two new regression tests cover the staggered-landing scenario directly.

### Trial 9: clean confirmation

Real Pi 5 hardware, real phone, `AA_HEADUNIT_OBSERVATION_WINDOW_SECONDS=300`:

```
usb gtk-dev-ui --device BUS:ADDRESS --allow-live-aap
```

The operator directly confirmed (not inferred from logs): swipe +
three-finger-tap went directly from fullscreen to desktop, and swipe +
three-finger-tap again went directly back to fullscreen, with no need to
ever open Settings. Log confirms 12 arm events and 11 clean completions
(mixed `DoubleTap`/`ThreeFingerTap`) across the full session, one clean
timeout-disarm, zero errors, process still healthy when the operator
chose to stop it (not a crash or hang).

## What remains open

Whether a touch actually lands in the geometrically correct position on
the phone's video once a non-zero rotation is active was not verified —
that needs either the DSI panel physically mounted rotated (not available
on this project's reference rig) or a software-only check (`wlr-randr
--transform` to rotate the Wayland output digitally, matched against the
same rotation setting, then confirming taps land correctly on real
video). The rotation geometry itself is unit-tested and believed correct
by construction (`crates/platform-linux/src/touch.rs`), not yet
real-hardware-proven end to end. `MILESTONE_CHECKLIST.md`'s item stays
unchecked until that's done.

Native Pi 5 formatting, strict Clippy, the full workspace test suite, a
secret-marker scan, and an ARM64 `.deb` packaging rebuild all passed
before every trial in this session.

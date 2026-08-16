# Pi 5 microphone input evidence — 16 August 2026

## Scope

`MILESTONE_CHECKLIST.md` M3's "Select and test a microphone input." Local
hardware selection/health check only: a new `media mic-probe [--seconds N]`
diagnostic command (`crates/media-gstreamer/src/capture.rs`,
`apps/aa-headunit-diagnostics/src/main.rs`) measures signal level (peak/RMS
dBFS, via `GStreamer`'s `level` element) from the `PipeWire`-Pulse default
input device, without ever reading a raw sample buffer in this project's
own code. This is not wired into the AAP microphone channel — that remains
the separate, not-yet-started M4 item ("Capture microphone audio for voice
interaction").

## Device

Same USB sound card already selected as the Pi 5 reference audio-output
fallback (`docs/hardware/evidence/pi5-2026-08-04.md`): PipeWire device
"Audio Adapter (Unitek Y-247A)", confirmed active default source ("Audio
Adapter (Unitek Y-247A) Mono") via `wpctl status` immediately before this
trial.

## Result

An earlier trial with this same physical device and PipeWire routing
(`docs/hardware/evidence/pi5-2026-08-04.md`) found the microphone input
produced only static, no intelligible speech, despite a reasonable
measured peak level. This trial found the opposite:

- Idle baseline (`media mic-probe --seconds 8`, no one speaking):
  peak `-15.4 dB`, RMS `-27.9 dB`, 40 `level` messages over 8s (real,
  non-silent room noise, not digital silence).
- Live loopback (`pulsesrc ! audioconvert ! audioresample ! pulsesink`,
  ad hoc, not committed as project code): the operator spoke into the mic
  and **directly confirmed hearing their own voice clearly** through the
  speakers in real time — not inferred from levels.
- Speaking trial (`media mic-probe --seconds 8`, operator confirmed
  speaking continuously for the full window): peak `-1.1 dB`, RMS
  `-17.5 dB`, 40 `level` messages — a clear, non-clipping rise from the
  idle baseline consistent with real speech.

Microphone input is confirmed working and intelligible with this device
and PipeWire routing today. The earlier static-only failure is not
reproduced; no root cause investigation was needed since the current
result is a pass, not a regression to explain. If it recurs, the earlier
evidence's hypothesis (microphone/sound-card electrical compatibility)
remains the first thing to check.

Native Pi 5 formatting, strict Clippy, and the full workspace test suite
(including two new pure-logic `capture.rs` tests using a synthetic
`audiotestsrc`, no real hardware required) passed before this trial. A
secret-marker scan and an ARM64 `.deb` packaging rebuild also passed; the
built package was not installed for this trial (`target/release` binary
used directly).

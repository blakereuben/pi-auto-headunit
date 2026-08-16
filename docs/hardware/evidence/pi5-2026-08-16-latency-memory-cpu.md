# Pi 5 video/audio/memory/CPU latency and resource evidence — 16 August 2026

## Scope

`MILESTONE_CHECKLIST.md` M3's "Measure video, audio, memory, CPU, and touch
latency against provisional targets." The provisional targets themselves
are `PRD.md`'s "Performance targets" section. Not every named quantity has
the same kind of target, and this trial covers what's actually measurable
on this hardware today:

- **Touch**: `PRD.md` explicitly requires "a repeatable camera-based
  procedure" (p95 below 120ms). Genuinely blocked — neither the operator
  nor this session has a way to record and frame-count a slow-motion
  touch/screen video. Left unmeasured; not silently dropped from the
  checklist item, which stays unchecked pending that tooling.
- **Video**: `PRD.md` gives no per-frame latency number for video — its
  target is "sustained 720p30 on all supported boards," a throughput
  target, already substantially evidenced by prior real-hardware trials
  (1,462-frame streaming trial, the 30-minute bench). This trial adds one
  more clean data point.
- **Audio**: `PRD.md`'s "audio start latency below 150 ms." Measured via a
  new instrumented metric.
- **Memory**: `PRD.md`'s idle-below-250 MiB / connected-steady-state-below-
  600 MiB. Measured directly.
- **CPU**: no specific target in `PRD.md`, just listed as something to
  measure and record.

## New instrumentation

`apps/aa-headunit-diagnostics/src/auth_discovery_probe.rs` gained
`RunningAudioPipeline` (wraps each audio channel's `AudioPlaybackPipeline`
with a `started_at` timestamp and a one-shot `first_frame_latency_logged`
flag). On the first real `MediaDataReceived` push after a channel's
`Start`, it prints
`probe_metric=<channel>_audio_start_latency_ms=<elapsed>` once. This
measures software dispatch latency — time from the pipeline reaching
`Playing` to the first real audio frame being pushed into it — not true
glass-to-glass audible time, which no in-process timestamp can observe
(the same limitation that makes touch latency require a camera).

## Trial

Real Pi 5 hardware, real phone (Samsung, `04e8:6860` before AOA
transition), `AA_HEADUNIT_OBSERVATION_WINDOW_SECONDS=25`:

```
usb auth-discovery-probe --device 1:5 --allow-live-aap
```

Result: `probe_result=observation_window_complete`, no errors other than
the known pre-existing benign `GStreamer`-Wayland startup warning already
documented in the 30-minute bench evidence. The operator directly
confirmed the phone showed a normal, working Android Auto session
throughout — not inferred from logs.

- **Video**: 1,271 real video `Data` frames received and rendered, zero
  render/pipeline errors.
- **Audio**: 1,794 real `MediaAudio` `Data` frames received and played,
  zero playback/pipeline errors. `probe_metric=media_audio_audio_start_latency_ms=0`
  — well under the 150ms target. This is software dispatch latency only
  (see above); it does not include `PipeWire`/`PulseAudio`'s own buffering
  latency, which was not separately measured this trial.
- **Memory**: RSS sampled every 2 seconds via `ps -o rss=` against the
  running process for the full 25-second window. Flat at exactly
  `86768 KiB` (~84.7 MiB) at every sample, from shortly after startup
  through the end of an active session with video render, three audio
  playback pipelines, and touch input service all running concurrently.
  Comfortably under both the 250 MiB idle and 600 MiB connected-steady-
  state targets — a single number satisfies both, since this session
  never grew from its startup baseline.
- **CPU**: sampled via `ps -o pcpu=`, which reports cumulative average CPU
  utilization since process start; the reading at the last sample before
  clean exit (~62% of one core) is the correct whole-session average by
  that metric's own definition. No target exists to compare against;
  recorded for future reference.

## What this does not prove

This is one trial, one phone, one session length. It is not a soak test
(that's the separate, still-open "60-minute wired media/audio soak"
M4 item) and it does not establish a distribution (single-sample, not
p95). The audio-start-latency metric measures software dispatch only, not
full audible latency. Touch latency remains entirely unmeasured, blocked
on camera/frame-counting tooling neither the operator nor this session
currently has.

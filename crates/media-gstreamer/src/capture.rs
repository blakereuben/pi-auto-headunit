//! Real (non-probing) microphone/audio-input level metering.
//!
//! Owns a `gst::Pipeline` shaped `<source> ! audioconvert ! audioresample
//! ! level ! fakesink`. This is deliberately a *level meter*, not a
//! recorder: the `level` element reports peak/RMS in dBFS per channel as
//! bus messages, and this module never reads a raw sample buffer, so
//! captured audio content never reaches this project's own code —
//! satisfying `CLAUDE.md`'s no-user-content rule by construction, not by
//! choosing not to log it. This is `MILESTONE_CHECKLIST.md` M3's "select
//! and test a microphone input": local hardware selection/health check
//! only, mirroring how `audio.rs`'s `AudioPlaybackPipeline` was built and
//! tested before ever being wired into a live phone session. Wiring
//! captured audio into the AAP microphone channel is the separate,
//! not-yet-started M4 item ("Capture microphone audio for voice
//! interaction").

use std::time::Duration;

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;

use crate::{AudioFormat, GstreamerError};

/// Where captured audio comes from. `Pulse` reads the `PipeWire`-Pulse
/// default input device (mirrors `AudioSink::Pulse` reading the default
/// output) — the same "let the OS/PipeWire pick the active device"
/// approach already proven for playback (`docs/hardware/evidence/`).
/// `Test` generates a synthetic sine wave via `audiotestsrc`, used only
/// by automated tests so they never depend on real capture hardware.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioCaptureSource {
    Pulse,
    Test,
}

/// One `level` element measurement window: peak and RMS in dBFS per
/// channel (`0.0` is full scale, more negative is quieter,
/// `f64::NEG_INFINITY` is true digital silence). Never contains raw
/// sample data.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureLevel {
    pub peak_db: Vec<f64>,
    pub rms_db: Vec<f64>,
}

/// A running (or not-yet-started) audio capture/level-metering pipeline.
/// Not `Clone`/`Copy` — owns live `GStreamer` resources.
pub struct AudioCapturePipeline {
    pipeline: gst::Pipeline,
}

impl AudioCapturePipeline {
    pub(crate) fn new(
        format: AudioFormat,
        source: AudioCaptureSource,
        interval: Duration,
    ) -> Result<Self, GstreamerError> {
        let source_element = match source {
            AudioCaptureSource::Pulse => "pulsesrc".to_string(),
            AudioCaptureSource::Test => "audiotestsrc is-live=true".to_string(),
        };
        let description = format!(
            "{source_element} ! audioconvert ! audioresample \
             ! audio/x-raw,format=S16LE,rate={},channels={},layout=interleaved \
             ! level name=lvl interval={} \
             ! fakesink sync=false",
            format.sampling_rate,
            format.channels,
            interval.as_nanos(),
        );
        let element = gst::parse::launch(&description)
            .map_err(|error| GstreamerError::PipelineConstruction(error.to_string()))?;
        let pipeline = element.downcast::<gst::Pipeline>().map_err(|_| {
            GstreamerError::PipelineConstruction(
                "parsed audio-capture graph was not a top-level Pipeline".into(),
            )
        })?;
        Ok(Self { pipeline })
    }

    /// Starts the pipeline (`Playing`). For `AudioCaptureSource::Pulse`,
    /// this is where an unreachable `PipeWire`/`PulseAudio` session
    /// surfaces as a recoverable `Err`, matching
    /// `AudioPlaybackPipeline::start`'s discipline.
    pub fn start(&self) -> Result<(), GstreamerError> {
        self.pipeline
            .set_state(gst::State::Playing)
            .map(|_| ())
            .map_err(|error| GstreamerError::StateChange(error.to_string()))
    }

    /// Blocking wait, bounded by `timeout`, for the next `level` bus
    /// message. Returns `Ok(None)` on timeout: silence still produces
    /// `level` messages at the configured interval, so a timeout here
    /// means the pipeline itself stalled, not that the input was quiet
    /// (quiet input reports real, very negative dB values instead).
    pub fn next_level(&self, timeout: Duration) -> Result<Option<CaptureLevel>, GstreamerError> {
        let bus = self
            .pipeline
            .bus()
            .ok_or_else(|| GstreamerError::Pipeline("capture pipeline has no bus".into()))?;
        let timeout =
            gst::ClockTime::from_nseconds(u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX));
        loop {
            let Some(message) = bus.timed_pop_filtered(
                timeout,
                &[gst::MessageType::Element, gst::MessageType::Error],
            ) else {
                return Ok(None);
            };
            if let gst::MessageView::Error(error) = message.view() {
                return Err(GstreamerError::Pipeline(error.error().to_string()));
            }
            let Some(structure) = message.structure() else {
                continue;
            };
            if structure.name() != "level" {
                continue;
            }
            return Ok(Some(CaptureLevel {
                peak_db: read_db_array(structure, "peak")?,
                rms_db: read_db_array(structure, "rms")?,
            }));
        }
    }

    /// Clean shutdown to `Null`. Consumes `self`; `Drop` below is the
    /// unconditional safety net for every other exit path.
    pub fn shutdown(self) -> Result<(), GstreamerError> {
        self.pipeline
            .set_state(gst::State::Null)
            .map(|_| ())
            .map_err(|error| GstreamerError::StateChange(error.to_string()))
    }
}

impl Drop for AudioCapturePipeline {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

fn read_db_array(structure: &gst::StructureRef, field: &str) -> Result<Vec<f64>, GstreamerError> {
    let array = structure.get::<glib::ValueArray>(field).map_err(|error| {
        GstreamerError::Pipeline(format!("level message missing \"{field}\": {error}"))
    })?;
    array
        .iter()
        .map(|value| {
            value.get::<f64>().map_err(|error| {
                GstreamerError::Pipeline(format!("level \"{field}\" entry not f64: {error}"))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GstreamerBackend;

    fn capture_format() -> AudioFormat {
        AudioFormat {
            sampling_rate: 48_000,
            channels: 1,
        }
    }

    #[test]
    fn reports_level_messages_from_a_synthetic_test_source() {
        let backend = GstreamerBackend::new().expect("gstreamer available on this host");
        let pipeline = backend
            .build_audio_capture_pipeline(
                capture_format(),
                AudioCaptureSource::Test,
                Duration::from_millis(50),
            )
            .expect("pipeline builds");
        pipeline.start().expect("pipeline starts");

        let level = pipeline
            .next_level(Duration::from_secs(5))
            .expect("no pipeline error")
            .expect("a level message arrives before the timeout");
        assert_eq!(level.peak_db.len(), 1, "one channel requested");
        assert_eq!(level.rms_db.len(), 1, "one channel requested");
        // audiotestsrc's default sine wave is not silence.
        assert!(level.peak_db[0] > -100.0);

        pipeline.shutdown().expect("clean shutdown");
    }

    #[test]
    fn next_level_times_out_cleanly_without_a_running_pipeline() {
        let backend = GstreamerBackend::new().expect("gstreamer available on this host");
        let pipeline = backend
            .build_audio_capture_pipeline(
                capture_format(),
                AudioCaptureSource::Test,
                Duration::from_secs(5),
            )
            .expect("pipeline builds");
        // Never started (still Null): no level messages will ever arrive.
        let result = pipeline
            .next_level(Duration::from_millis(200))
            .expect("no pipeline error");
        assert!(result.is_none(), "timeout reported as None, not an error");
    }
}

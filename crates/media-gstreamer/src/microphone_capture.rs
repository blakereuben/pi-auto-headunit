//! Real (non-probing) microphone audio capture and relay.
//!
//! Owns a `gst::Pipeline` shaped `<source> ! audioconvert ! audioresample
//! ! appsink`, the mirror image of `audio.rs`'s `AudioPlaybackPipeline`
//! (`appsrc ! audioconvert ! audioresample ! <sink>`) — same raw-`S16LE`,
//! no-codec assumption, real-hardware-confirmed correct for playback and
//! reused here unchanged. Unlike `capture.rs`'s level-meter pipeline
//! (`... ! level ! fakesink`, which deliberately never reads a raw sample
//! buffer), this module *does* read real captured audio — that is the
//! entire point of `MILESTONE_CHECKLIST.md` M4's "Capture microphone
//! audio for voice interaction": relaying real-time audio to the phone
//! for a legitimate, user-initiated voice-assistant session. This is not
//! a `CLAUDE.md` no-user-content violation, any more than this crate's
//! existing inbound video/audio `Data` handling is — the same discipline
//! applies in the other direction: process it, never log it whole, never
//! persist it to disk. Every caller in this module and in
//! `apps/aa-headunit-diagnostics` logs only byte counts, frame counts,
//! timestamps, and drop counts.
//!
//! Pulls samples via a dedicated background thread (`AppSink::pull_sample`,
//! blocking) feeding a bounded channel, mirroring
//! `platform_linux::touch::EvdevTouchSource`'s proven thread-plus-channel
//! shape rather than `AppSink::set_callbacks` — this crate has never used
//! `GStreamer`'s callback dispatch anywhere (every existing bus/sample read
//! is polled), and `EvdevTouchSource` is a real-hardware-validated pattern
//! that fits directly. The channel is deliberately bounded and drops the
//! newest sample on overflow rather than growing unboundedly — a live
//! voice session needs live audio, not a backlog; see
//! `protocol_aap::microphone_setup`'s matching flow-control design for the
//! same policy applied one layer up, at the AAP credit-window level.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::{AudioCaptureSource, AudioFormat, GstreamerError};

/// Bounds the reader thread's backlog independently of the AAP-level
/// credit window — protects the capture thread/memory from an
/// unresponsive main loop even before wire-level flow control ever sees a
/// frame. Deliberately small, matching this project's "smallest correct
/// slice, no unbounded growth" bias.
const CAPTURE_CHANNEL_CAPACITY: usize = 8;

/// One captured PCM buffer. `payload` must never be logged whole by
/// callers — only its length — matching every other real-media type in
/// this crate.
pub struct CapturedPcmFrame {
    pub timestamp_micros: u64,
    pub payload: Vec<u8>,
}

/// A running (or not-yet-started) microphone capture pipeline. Not
/// `Clone`/`Copy` — owns live `GStreamer` resources and a background
/// reader thread.
pub struct MicrophoneCapturePipeline {
    pipeline: gst::Pipeline,
    receiver: mpsc::Receiver<CapturedPcmFrame>,
    dropped: Arc<AtomicU64>,
}

impl MicrophoneCapturePipeline {
    /// `device`, when set, names a specific `PulseAudio` source to
    /// capture from instead of the system default — ignored for
    /// `AudioCaptureSource::Test`. M5's persisted `microphone_input_device`
    /// setting (`crate::settings`) is this project's only source for it;
    /// see `AudioPlaybackPipeline::new`'s matching doc comment for why no
    /// validation happens here.
    pub(crate) fn new(
        format: AudioFormat,
        source: AudioCaptureSource,
        device: Option<&str>,
    ) -> Result<Self, GstreamerError> {
        let source_element = match source {
            AudioCaptureSource::Pulse => "pulsesrc".to_string(),
            AudioCaptureSource::Test => "audiotestsrc is-live=true".to_string(),
        };
        let device_property = match (source, device) {
            (AudioCaptureSource::Pulse, Some(device)) => format!(" device=\"{device}\""),
            _ => String::new(),
        };
        let description = format!(
            "{source_element}{device_property} ! audioconvert ! audioresample \
             ! audio/x-raw,format=S16LE,rate={},channels={},layout=interleaved \
             ! appsink name=sink emit-signals=false sync=false",
            format.sampling_rate, format.channels,
        );
        let element = gst::parse::launch(&description)
            .map_err(|error| GstreamerError::PipelineConstruction(error.to_string()))?;
        let pipeline = element.downcast::<gst::Pipeline>().map_err(|_| {
            GstreamerError::PipelineConstruction(
                "parsed microphone-capture graph was not a top-level Pipeline".into(),
            )
        })?;
        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| {
                GstreamerError::PipelineConstruction(
                    "appsink element \"sink\" missing after parse".into(),
                )
            })?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| {
                GstreamerError::PipelineConstruction("\"sink\" was not an AppSink".into())
            })?;

        // Blocking `pull_sample()` before the pipeline reaches `Playing`
        // is harmless (it simply blocks until the first buffer arrives),
        // so spawning here — before `start()` — is safe and keeps
        // `try_recv`'s non-blocking-drain shape available immediately,
        // mirroring `EvdevTouchSource::open`'s spawn-and-go shape.
        let (sender, receiver) = mpsc::sync_channel(CAPTURE_CHANNEL_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        let reader_dropped = Arc::clone(&dropped);
        thread::spawn(move || run_reader(&appsink, &sender, &reader_dropped));

        Ok(Self {
            pipeline,
            receiver,
            dropped,
        })
    }

    /// Starts the pipeline (`Playing`). For `AudioCaptureSource::Pulse`,
    /// this is where an unreachable `PipeWire`/`PulseAudio` session
    /// surfaces as a recoverable `Err`, matching every other pipeline in
    /// this crate.
    pub fn start(&self) -> Result<(), GstreamerError> {
        self.pipeline
            .set_state(gst::State::Playing)
            .map(|_| ())
            .map_err(|error| GstreamerError::StateChange(error.to_string()))
    }

    /// Drains one queued frame, if any, without blocking — safe to call
    /// once per probe-loop iteration, matching
    /// `EvdevTouchSource::try_recv`'s exact contract.
    #[must_use]
    pub fn try_recv(&self) -> Option<CapturedPcmFrame> {
        self.receiver.try_recv().ok()
    }

    /// Non-blocking drain of any bus-reported element error since the
    /// last call, matching `AudioPlaybackPipeline::poll_bus_error`.
    #[must_use]
    pub fn poll_bus_error(&self) -> Option<GstreamerError> {
        let bus = self.pipeline.bus()?;
        while let Some(message) = bus.pop_filtered(&[gst::MessageType::Error]) {
            if let gst::MessageView::Error(error) = message.view() {
                return Some(GstreamerError::Pipeline(error.error().to_string()));
            }
        }
        None
    }

    /// Cumulative count of captured buffers dropped because the bounded
    /// channel was full — never logged per-frame by callers, only as an
    /// aggregate count.
    #[must_use]
    pub fn dropped_frame_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
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

impl Drop for MicrophoneCapturePipeline {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// Background reader thread body: blocks on `pull_sample()`, forwards
/// each buffer's raw bytes (never logged) and PTS-derived timestamp
/// through the bounded channel, dropping (and counting, never logging)
/// the newest sample on overflow. Returns on any pull error (EOS,
/// pipeline stopped, or torn down) — mirrors
/// `platform_linux::touch::run_reader`'s `Err => return` shape exactly.
fn run_reader(
    appsink: &gst_app::AppSink,
    sender: &mpsc::SyncSender<CapturedPcmFrame>,
    dropped: &Arc<AtomicU64>,
) {
    loop {
        let Ok(sample) = appsink.pull_sample() else {
            return;
        };
        let Some(buffer) = sample.buffer() else {
            continue;
        };
        let Ok(map) = buffer.map_readable() else {
            continue;
        };
        let timestamp_micros = buffer.pts().map_or(0, gst::ClockTime::useconds);
        let frame = CapturedPcmFrame {
            timestamp_micros,
            payload: map.as_slice().to_vec(),
        };
        drop(map);
        if sender.try_send(frame).is_err() {
            dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::GstreamerBackend;

    fn capture_format() -> AudioFormat {
        AudioFormat {
            sampling_rate: 16_000,
            channels: 1,
        }
    }

    #[test]
    fn captures_real_pcm_buffers_from_a_synthetic_test_source() {
        let backend = GstreamerBackend::new().expect("gstreamer available on this host");
        let pipeline = backend
            .build_microphone_capture_pipeline(capture_format(), AudioCaptureSource::Test, None)
            .expect("pipeline builds");
        pipeline.start().expect("pipeline starts");

        let frame = pipeline
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("a captured frame arrives before the timeout");
        assert!(!frame.payload.is_empty(), "captured a non-empty buffer");
        assert_eq!(pipeline.dropped_frame_count(), 0);

        pipeline.shutdown().expect("clean shutdown");
    }

    #[test]
    fn try_recv_is_none_without_a_running_pipeline() {
        let backend = GstreamerBackend::new().expect("gstreamer available on this host");
        let pipeline = backend
            .build_microphone_capture_pipeline(capture_format(), AudioCaptureSource::Test, None)
            .expect("pipeline builds");
        // Never started (still Null): no samples will ever arrive.
        assert!(pipeline.try_recv().is_none());
    }
}

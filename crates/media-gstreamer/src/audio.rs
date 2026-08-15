//! Real (non-probing) raw PCM audio playback pipeline execution.
//!
//! Owns a `gst::Pipeline` shaped `appsrc ! audioconvert ! audioresample !
//! <sink>` and pushes raw PCM payload buffers (`Data` bytes, already
//! stripped of AAP framing by `protocol_aap`) directly into it — there is
//! no decoder stage, since `MEDIA_CODEC_AUDIO_PCM` is the only audio codec
//! this project's `ServiceDiscoveryResponse` ever advertises. Assumes
//! signed 16-bit little-endian interleaved samples (`S16LE`) — the
//! platform-standard raw layout, but this project has never observed real
//! phone audio bytes to confirm it. Unlike the video pipeline's
//! H.264/H.265 parsers, raw PCM has no in-stream parser to reject a wrong
//! assumption: a wrong sample format would produce audible noise or
//! garbled playback, not a clean pipeline `Error`, so this must be
//! confirmed by ear on real hardware, not inferred from a clean bus alone.
//! `Data` frames' PTS is derived from the AAP timestamp field assuming
//! microseconds, matching the video pipeline's identical, equally
//! unconfirmed assumption. `CodecConfig` messages on an audio channel are
//! not pushed into this pipeline — raw PCM has no parameter-set payload
//! for a parser to extract, unlike H.264/H.265 SPS/PPS.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::GstreamerError;

/// The two pieces of `AudioConfiguration` that actually shape the pipeline
/// caps (`ServiceDiscoveryResponse`'s `sampling_rate`/`number_of_channels`
/// — `number_of_bits` is always 16 for `MEDIA_CODEC_AUDIO_PCM` in this
/// project's own advertised configurations, so it isn't threaded through
/// separately; see the module doc comment for the `S16LE` assumption).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioFormat {
    pub sampling_rate: u32,
    pub channels: u32,
}

/// Where decoded audio is presented. `Fake` never touches a real sink and
/// is the only one used by automated tests; `Pulse` is the production
/// sink, routing through the `PipeWire`-provided `PulseAudio` compatibility
/// layer confirmed present on the reference Pi 5 setup (`pipewire-pulse`;
/// no native `gstreamer1.0-pipewire` plugin is installed on this project's
/// reference image).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioSink {
    Pulse,
    Fake,
}

/// A running (or not-yet-started) audio playback pipeline for one AV
/// sink channel (`MediaAudio`, `SystemAudio`, or `SpeechAudio` — each gets
/// its own independent instance and its own independent `appsrc`, so the
/// three channels' audio mixes at the OS level like any other set of
/// concurrent `PipeWire` streams). Not `Clone`/`Copy` — owns live
/// `GStreamer` resources.
pub struct AudioPlaybackPipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
}

impl AudioPlaybackPipeline {
    pub(crate) fn new(format: AudioFormat, sink: AudioSink) -> Result<Self, GstreamerError> {
        let sink_element = match sink {
            AudioSink::Pulse => "pulsesink",
            AudioSink::Fake => "fakesink",
        };
        let description = format!(
            "appsrc name=src is-live=true format=time \
             caps=\"audio/x-raw,format=S16LE,rate={},channels={},layout=interleaved\" \
             ! audioconvert ! audioresample ! {sink_element} sync=false",
            format.sampling_rate, format.channels,
        );
        let element = gst::parse::launch(&description)
            .map_err(|error| GstreamerError::PipelineConstruction(error.to_string()))?;
        let pipeline = element.downcast::<gst::Pipeline>().map_err(|_| {
            GstreamerError::PipelineConstruction(
                "parsed audio-playback graph was not a top-level Pipeline".into(),
            )
        })?;
        let appsrc = pipeline
            .by_name("src")
            .ok_or_else(|| {
                GstreamerError::PipelineConstruction(
                    "appsrc element \"src\" missing after parse".into(),
                )
            })?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| {
                GstreamerError::PipelineConstruction("\"src\" was not an AppSrc".into())
            })?;
        Ok(Self { pipeline, appsrc })
    }

    /// Starts the pipeline (`Playing`). For `AudioSink::Pulse`, this is
    /// where an unreachable PipeWire/PulseAudio session (e.g. no
    /// `XDG_RUNTIME_DIR` — an unprivileged `sudo` session without `-E`)
    /// surfaces as a recoverable `Err`, never a panic or hang. Callers must
    /// treat this as recoverable and keep the rest of the session running.
    pub fn start(&self) -> Result<(), GstreamerError> {
        self.pipeline
            .set_state(gst::State::Playing)
            .map(|_| ())
            .map_err(|error| GstreamerError::StateChange(error.to_string()))
    }

    /// Pushes one `Data` frame's raw PCM payload, with PTS derived from the
    /// AAP `Data` message's 8-byte timestamp (assumed microseconds — see
    /// module doc comment; unconfirmed against real phone bytes).
    pub fn push_frame(&self, payload: &[u8], timestamp: u64) -> Result<(), GstreamerError> {
        self.push_buffer(payload, gst::ClockTime::from_useconds(timestamp))
    }

    fn push_buffer(&self, payload: &[u8], pts: gst::ClockTime) -> Result<(), GstreamerError> {
        let mut buffer = gst::Buffer::from_mut_slice(payload.to_vec());
        {
            let buffer_mut = buffer.get_mut().expect("uniquely owned, just created");
            buffer_mut.set_pts(Some(pts));
        }
        self.appsrc
            .push_buffer(buffer)
            .map(|_| ())
            .map_err(|flow_error| GstreamerError::PushBuffer(flow_error.to_string()))
    }

    /// Non-blocking drain of any bus-reported element error since the last
    /// call. Returns at most one error per call; callers loop or call once
    /// per push, per their own tolerance.
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

    /// Graceful shutdown: EOS, bounded wait for it to propagate, then
    /// `Null`. Consumes `self`; `Drop` below is the unconditional safety
    /// net for every other exit path (including error/unwind), so callers
    /// that don't need EOS confirmation may simply drop the value instead.
    pub fn shutdown(self) -> Result<(), GstreamerError> {
        self.appsrc
            .end_of_stream()
            .map_err(|flow_error| GstreamerError::PushBuffer(flow_error.to_string()))?;
        if let Some(bus) = self.pipeline.bus() {
            let _ = bus.timed_pop_filtered(
                gst::ClockTime::from_seconds(5),
                &[gst::MessageType::Eos, gst::MessageType::Error],
            );
        }
        self.pipeline
            .set_state(gst::State::Null)
            .map(|_| ())
            .map_err(|error| GstreamerError::StateChange(error.to_string()))
    }
}

impl Drop for AudioPlaybackPipeline {
    fn drop(&mut self) {
        // Best-effort; Drop cannot propagate errors, and EOS is not
        // required for a safe teardown (Null is always sufficient).
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GstreamerBackend;

    fn media_audio_format() -> AudioFormat {
        AudioFormat {
            sampling_rate: 48_000,
            channels: 2,
        }
    }

    /// Builds a tiny, fully self-generated S16LE PCM buffer via
    /// `audiotestsrc ! audioconvert ! appsink`, constructed and run
    /// directly as a `gst::Pipeline` in Rust (no `gst-launch-1.0`
    /// subprocess). Never derived from a real phone capture — see
    /// `CLAUDE.md`'s user-content rule.
    fn synthetic_pcm_frames(format: AudioFormat, buffer_count: u32) -> Vec<Vec<u8>> {
        gst::init().expect("gstreamer available on this host");
        let description = format!(
            "audiotestsrc num-buffers={buffer_count} samplesperbuffer=480 \
             ! audio/x-raw,format=S16LE,rate={},channels={},layout=interleaved \
             ! appsink name=sink emit-signals=false sync=false",
            format.sampling_rate, format.channels,
        );
        let pipeline = gst::parse::launch(&description)
            .expect("fixture pipeline parses")
            .downcast::<gst::Pipeline>()
            .expect("fixture graph is a Pipeline");
        let appsink = pipeline
            .by_name("sink")
            .expect("named appsink present")
            .downcast::<gst_app::AppSink>()
            .expect("sink is an AppSink");
        pipeline
            .set_state(gst::State::Playing)
            .expect("fixture pipeline starts");
        let mut frames = Vec::new();
        while let Ok(sample) = appsink.pull_sample() {
            let buffer = sample.buffer().expect("sample has a buffer");
            let map = buffer.map_readable().expect("buffer is readable");
            frames.push(map.as_slice().to_vec());
        }
        let _ = pipeline.set_state(gst::State::Null);
        assert!(!frames.is_empty(), "fixture produced no PCM frames");
        frames
    }

    #[test]
    fn plays_a_synthetic_pcm_stream_end_to_end_with_fakesink() {
        let backend = GstreamerBackend::new().expect("gstreamer available on this host");
        let format = media_audio_format();
        let pipeline = backend
            .build_audio_playback_pipeline(format, AudioSink::Fake)
            .expect("pipeline builds");
        pipeline.start().expect("pipeline starts");

        let frames = synthetic_pcm_frames(format, 10);
        for (index, frame) in frames.iter().enumerate() {
            pipeline
                .push_frame(frame, (index as u64) * 10_000)
                .expect("frame pushes");
            assert!(pipeline.poll_bus_error().is_none(), "no pipeline errors");
        }
        pipeline.shutdown().expect("clean EOS shutdown");
    }
}

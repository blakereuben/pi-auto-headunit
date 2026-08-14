//! Real (non-probing) H.264 decode-and-render pipeline execution.
//!
//! Owns a `gst::Pipeline` shaped `appsrc ! h264parse ! avdec_h264 !
//! videoconvert ! <sink>` and pushes raw encoded payload buffers (H.264
//! `Data`/`CodecConfig` bytes, already stripped of AAP framing by
//! `protocol_aap`) directly into it. Assumes Annex-B byte-stream H.264
//! framing (start-code-delimited NAL units) — this project has never
//! observed real phone `Data`/`CodecConfig` bytes to confirm this; `Data`
//! frames' PTS is derived from the AAP timestamp field assuming
//! microseconds, also unconfirmed. Both are the least-assumption defaults
//! available and fail closed (a bus `Error`, not corrupted output) if
//! wrong; see `docs/protocol` for what has and hasn't been confirmed about
//! `Data`/`CodecConfig` wire framing. `CodecConfig` is pushed through the
//! same `appsrc` ahead of frame data, relying on `h264parse`'s in-band
//! SPS/PPS extraction rather than out-of-band `codec_data` caps.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::{GstreamerError, PipelineElements};

/// Where decoded frames are presented. `Fake` never touches a display and
/// is the only sink used by automated tests; `Wayland` is the production
/// sink, reusing `PipelineElements::sink` unmodified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderSink {
    Wayland,
    Fake,
}

/// A running (or not-yet-started) decode/render pipeline for one video
/// session. Not `Clone`/`Copy` — owns live `GStreamer` resources.
pub struct VideoRenderPipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
}

impl VideoRenderPipeline {
    pub(crate) fn new(
        elements: PipelineElements,
        sink: RenderSink,
    ) -> Result<Self, GstreamerError> {
        let sink_element = match sink {
            RenderSink::Wayland => elements.sink,
            RenderSink::Fake => "fakesink",
        };
        let description = format!(
            "appsrc name=src is-live=true format=time \
             caps=\"video/x-h264,stream-format=byte-stream,alignment=au\" \
             ! {} ! {} ! {} ! {sink_element} sync=false",
            elements.parser, elements.decoder, elements.converter,
        );
        let element = gst::parse::launch(&description)
            .map_err(|error| GstreamerError::PipelineConstruction(error.to_string()))?;
        let pipeline = element.downcast::<gst::Pipeline>().map_err(|_| {
            GstreamerError::PipelineConstruction(
                "parsed video-render graph was not a top-level Pipeline".into(),
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

    /// Starts the pipeline (`Playing`). For `RenderSink::Wayland`, this is
    /// where a missing/unreachable Wayland compositor (e.g. no
    /// `WAYLAND_DISPLAY` — an SSH session without display forwarding)
    /// surfaces as a recoverable `Err`, never a panic or hang. Callers
    /// must treat this as recoverable and keep the rest of the session
    /// running.
    pub fn start(&self) -> Result<(), GstreamerError> {
        self.pipeline
            .set_state(gst::State::Playing)
            .map(|_| ())
            .map_err(|error| GstreamerError::StateChange(error.to_string()))
    }

    /// Pushes `CodecConfig` bytes (out-of-band SPS/PPS, no AAP timestamp)
    /// through the same `appsrc` as frame data — see module doc comment.
    pub fn push_codec_config(&self, payload: &[u8]) -> Result<(), GstreamerError> {
        self.push_buffer(payload, Some(gst::ClockTime::ZERO))
    }

    /// Pushes one `Data` frame's payload, with PTS derived from the AAP
    /// `Data` message's 8-byte timestamp (assumed microseconds — see
    /// module doc comment; unconfirmed against real phone bytes).
    pub fn push_frame(&self, payload: &[u8], timestamp: u64) -> Result<(), GstreamerError> {
        self.push_buffer(payload, Some(gst::ClockTime::from_useconds(timestamp)))
    }

    fn push_buffer(
        &self,
        payload: &[u8],
        pts: Option<gst::ClockTime>,
    ) -> Result<(), GstreamerError> {
        let mut buffer = gst::Buffer::from_mut_slice(payload.to_vec());
        {
            let buffer_mut = buffer.get_mut().expect("uniquely owned, just created");
            buffer_mut.set_pts(pts);
        }
        self.appsrc
            .push_buffer(buffer)
            .map(|_| ())
            .map_err(|flow_error| GstreamerError::PushBuffer(flow_error.to_string()))
    }

    /// Non-blocking drain of any bus-reported element error since the last
    /// call (e.g. a mid-stream decode failure, or an async compositor
    /// failure that didn't surface synchronously from `start`). Returns at
    /// most one error per call; callers loop or call once per push, per
    /// their own tolerance.
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

impl Drop for VideoRenderPipeline {
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
    use media_api::{DecoderCapability, DecoderKind, VideoCodec};

    fn h264_capability() -> DecoderCapability {
        DecoderCapability {
            id: "gstreamer:avdec_h264".into(),
            codec: VideoCodec::H264,
            kind: DecoderKind::Software,
            maximum_width: 800,
            maximum_height: 480,
            maximum_frames_per_second: 30,
        }
    }

    /// Builds a tiny, fully self-generated H.264 Annex-B byte stream via
    /// `videotestsrc ! openh264enc ! h264parse ! appsink`, constructed and
    /// run directly as a `gst::Pipeline` in Rust (no `gst-launch-1.0`
    /// subprocess). Never derived from a real phone capture — see
    /// `CLAUDE.md`'s user-content rule. Returns one `Vec<u8>` per encoded
    /// access unit; with `config-interval=-1`, every access unit already
    /// carries in-band SPS/PPS, so the first element also doubles as this
    /// test's `CodecConfig` stand-in.
    fn synthetic_h264_access_units(count: u32) -> Vec<Vec<u8>> {
        gst::init().expect("gstreamer available on this host");
        let description = format!(
            "videotestsrc num-buffers={count} \
             ! video/x-raw,width=64,height=48,framerate=30/1 \
             ! openh264enc \
             ! h264parse config-interval=-1 \
             ! video/x-h264,stream-format=byte-stream,alignment=au \
             ! appsink name=sink emit-signals=false sync=false"
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
        let mut access_units = Vec::new();
        while let Ok(sample) = appsink.pull_sample() {
            let buffer = sample.buffer().expect("sample has a buffer");
            let map = buffer.map_readable().expect("buffer is readable");
            access_units.push(map.as_slice().to_vec());
        }
        let _ = pipeline.set_state(gst::State::Null);
        assert!(!access_units.is_empty(), "fixture produced no access units");
        access_units
    }

    #[test]
    fn decodes_and_runs_a_synthetic_h264_stream_end_to_end_with_fakesink() {
        let backend = GstreamerBackend::new().expect("gstreamer available on this host");
        let pipeline = backend
            .build_video_render_pipeline(&h264_capability(), RenderSink::Fake)
            .expect("pipeline builds");
        pipeline.start().expect("pipeline starts");

        let access_units = synthetic_h264_access_units(10);
        let (codec_config, frames) = access_units
            .split_first()
            .expect("fixture produced at least one access unit");
        pipeline
            .push_codec_config(codec_config)
            .expect("codec config pushes");
        for (index, frame) in frames.iter().enumerate() {
            pipeline
                .push_frame(frame, (index as u64) * 33_333)
                .expect("frame pushes");
            assert!(pipeline.poll_bus_error().is_none(), "no pipeline errors");
        }
        pipeline.shutdown().expect("clean EOS shutdown");
    }

    #[test]
    #[ignore = "requires a reachable Wayland compositor (WAYLAND_DISPLAY); run \
                manually on Pi hardware for a visually-confirmed check, not part \
                of the standard cargo test sweep"]
    fn renders_a_synthetic_h264_stream_to_the_physical_display() {
        let backend = GstreamerBackend::new().expect("gstreamer available on this host");
        let pipeline = backend
            .build_video_render_pipeline(&h264_capability(), RenderSink::Wayland)
            .expect("pipeline builds");
        pipeline
            .start()
            .expect("pipeline starts (needs a compositor)");
        let access_units = synthetic_h264_access_units(90);
        let (codec_config, frames) = access_units.split_first().expect("access units");
        pipeline
            .push_codec_config(codec_config)
            .expect("codec config pushes");
        for (index, frame) in frames.iter().enumerate() {
            pipeline
                .push_frame(frame, (index as u64) * 33_333)
                .expect("frame pushes");
        }
        pipeline.shutdown().expect("clean EOS shutdown");
    }
}

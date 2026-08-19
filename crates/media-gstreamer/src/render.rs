//! Real (non-probing) H.264/H.265 decode-and-render pipeline execution.
//!
//! Owns a `gst::Pipeline` shaped `appsrc ! {parser} ! {decoder} !
//! videoconvert ! <sink>` (`h264parse`/`avdec_h264` or `h265parse`/
//! `avdec_h265`, chosen by `PipelineElements`, which also picks the
//! matching `appsrc` caps) and pushes raw encoded payload buffers
//! (`Data`/`CodecConfig` bytes, already stripped of AAP framing by
//! `protocol_aap`) directly into it. Assumes Annex-B byte-stream framing
//! (start-code-delimited NAL units) for both codecs and that `Data`
//! frames' PTS is the AAP timestamp field in microseconds — both were
//! least-assumption defaults when first written, and are now real-
//! hardware-confirmed correct against a live phone: 1,462 real H.265
//! `Data` frames decoded and rendered with zero pipeline errors, with the
//! operator directly confirming correct video on the head unit's own
//! physical display (`MILESTONE_CHECKLIST.md` M4, "Display projected
//! video..."). Both assumptions still fail closed (a bus `Error`, not
//! corrupted output) if a future codec/phone combination violates them.
//! `CodecConfig` is pushed through the same `appsrc` ahead of frame data,
//! relying on the parser's in-band SPS/PPS (or VPS/SPS/PPS) extraction
//! rather than out-of-band `codec_data` caps — implicitly confirmed by the
//! same successful real decode, since H.264/H.265 decoding cannot succeed
//! without a correctly-parsed parameter set.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::{GstreamerError, PipelineElements};

/// Where decoded frames are presented. `Fake` never touches a display and
/// is the only sink used by automated tests; `Wayland` is the production
/// sink, reusing `PipelineElements::sink` unmodified. `Gtk4Paintable` is a
/// third, spike-only option: `gtk4paintablesink`, whose `paintable`
/// `GObject` property (retrievable via `VideoRenderPipeline::
/// gtk4_paintable_property`) bridges decoded video into a `gtk::Picture`
/// widget. Used only by `examples/gtk_fullscreen_spike.rs`, answering
/// `ARCHITECTURE.md` §4's "GTK/GStreamer integration... must pass an
/// on-device architecture spike" gate — the already-proven `Wayland` path
/// in `auth_discovery_probe.rs` is untouched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderSink {
    Wayland,
    Fake,
    Gtk4Paintable,
}

/// A running (or not-yet-started) decode/render pipeline for one video
/// session. Not `Clone`/`Copy` — owns live `GStreamer` resources.
pub struct VideoRenderPipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    flip: gst::Element,
}

impl VideoRenderPipeline {
    pub(crate) fn new(
        elements: PipelineElements,
        sink: RenderSink,
    ) -> Result<Self, GstreamerError> {
        // `waylandsink` defaults `fullscreen` to `false` (confirmed via
        // `gst-inspect-1.0 waylandsink`) and creates a plain toplevel window
        // sized to the negotiated video resolution — never actually
        // requesting fullscreen from the compositor. On the reference DSI
        // display (800x480) this happened to look full-screen only because
        // the negotiated 1280x720 video exceeds the panel size, an
        // accident of resolution, not a real request; a real-hardware
        // trial showed a session where the same 1280x720 negotiation did
        // *not* visually cover the screen, confirming this was never
        // reliable. `elements.sink` stays the plain "waylandsink" name
        // (still required for `ElementFactory::find` lookups elsewhere,
        // e.g. `GstreamerBackend::verify_pipeline_elements`); the
        // `fullscreen=true` property is only added to this parse-launch
        // description string.
        let sink_element = match sink {
            RenderSink::Wayland => format!("{} fullscreen=true", elements.sink),
            RenderSink::Fake => "fakesink".to_string(),
            RenderSink::Gtk4Paintable => "gtk4paintablesink name=gtk_paintable_sink".to_string(),
        };
        // `videoflip name=flip` sits between the decoder's raw output and
        // the sink — real-hardware-required (2026-08-18): the existing
        // touch-rotation setting (`platform_linux::touch::Rotation`,
        // `settings::HeadUnitSettings::rotation`) only ever remapped touch
        // coordinates, never the video image itself, which the operator
        // correctly identified as pointless on its own ("if the video
        // cannot rotate then there is no point in rotating touch") — a
        // physically-rotated screen mount needs both to move together.
        // `video-direction` (not the deprecated `method` property) is
        // `controllable, changeable in NULL, READY, PAUSED or PLAYING
        // state` (`gst-inspect-1.0 videoflip`), so `set_rotation_degrees`
        // can retune it on an already-`Playing` pipeline with no rebuild —
        // matching `SharedRotation`'s existing live-adjustable touch
        // rotation exactly.
        let description = format!(
            "appsrc name=src is-live=true format=time \
             caps=\"{}\" \
             ! {} ! {} ! {} ! videoflip name=flip video-direction=identity \
             ! {sink_element} sync=false",
            elements.caps, elements.parser, elements.decoder, elements.converter,
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
        let flip = pipeline.by_name("flip").ok_or_else(|| {
            GstreamerError::PipelineConstruction(
                "videoflip element \"flip\" missing after parse".into(),
            )
        })?;
        Ok(Self {
            pipeline,
            appsrc,
            flip,
        })
    }

    /// Rotates the rendered video to match [`Rotation`]'s own two states
    /// (`platform_linux::touch::Rotation` — this crate doesn't depend on
    /// `platform-linux`, so the caller converts to plain degrees rather
    /// than this crate taking that type on directly; only `0`/`180` are
    /// ever passed — `Rotation` itself has no 90°/270° variants to send
    /// since 2026-08-18, see its own doc comment). Safe to call at any
    /// pipeline state, including `Playing` — see the constructor's doc
    /// comment on `video-direction`. Any value other than `0`/`180` is a
    /// caller bug, so this silently falls back to `"identity"` rather than
    /// erroring — matching this crate's existing "hardware side effects
    /// never abort a live session" discipline, not a case worth a
    /// `Result`.
    pub fn set_rotation_degrees(&self, degrees: u16) {
        let value = if degrees == 180 { "180" } else { "identity" };
        self.flip.set_property_from_str("video-direction", value);
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
    /// `Data` message's 8-byte timestamp (microseconds — see module doc
    /// comment; real-hardware-confirmed, not just assumed).
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

    /// The `gtk4paintablesink` element's `paintable` `GObject` property, as a
    /// raw `glib::Value` — kept untyped so GTK stays confined to the
    /// example that actually needs it (`examples/gtk_fullscreen_spike.rs`);
    /// this crate's own dependency graph is unchanged. `None` unless built
    /// with `RenderSink::Gtk4Paintable`.
    #[must_use]
    pub fn gtk4_paintable_property(&self) -> Option<gst::glib::Value> {
        self.pipeline
            .by_name("gtk_paintable_sink")
            .map(|element| element.property_value("paintable"))
    }

    /// Real-hardware finding (2026-08-19): `gtk4paintablesink` exposes
    /// `window-width`/`window-height` ("the width/height of the main
    /// widget rendering the paintable" — `gst-inspect-1.0
    /// gtk4paintablesink`, "changeable in NULL, READY, PAUSED or PLAYING
    /// state") specifically so the embedding application can keep it
    /// informed of the actual rendering surface's size; this crate never
    /// set them, so the sink operated with stale/zero dimensions the
    /// whole session. Investigated while chasing a real-hardware
    /// compositor hang triggered by the fullscreen↔windowed transition
    /// ("return to desktop") — a confirmed, real, and plausible
    /// contributor (a resize the sink was never told about), though not
    /// independently confirmed as the *sole* cause; a caller should set
    /// this at pipeline build time and again whenever the rendering
    /// widget's actual size changes, matching the property's own
    /// documented intent. Safe to call from any thread — a plain
    /// `GObject` property, unlike the `paintable`/`GdkPaintable` value
    /// itself, which glib's `ThreadGuard` restricts to the creating
    /// thread (see `VideoRenderPipeline`'s own `Drop` impl).
    pub fn set_window_size(&self, width: u32, height: u32) {
        if let Some(sink) = self.pipeline.by_name("gtk_paintable_sink") {
            sink.set_property("window-width", width);
            sink.set_property("window-height", height);
        }
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
        // Real-hardware finding (2026-08-19): `RenderSink::Gtk4Paintable`'s
        // `gtk4paintablesink` wraps a GTK-widget-bound `GObject` that
        // `glib`'s `ThreadGuard` only allows the creating thread to touch
        // — the GTK main thread, since `gtk_dev_ui.rs` always builds this
        // pipeline from its `capability_receiver` poll (a
        // `glib::timeout_add_local` callback). But the built pipeline is
        // then handed across an `mpsc` channel to a background session
        // thread (`auth_discovery_probe.rs`'s `VideoRenderState::Running`),
        // which owns it for the rest of the session and is the thread
        // that actually drops it once the session ends. A plain
        // `set_state(Null)` here running on that thread panics
        // ("Value accessed from different thread than where it was
        // created") inside `gst_element_change_state` — and since that's
        // a panic in a non-unwind-safe FFI callback, it hard-aborts the
        // whole process rather than returning an error, confirmed via a
        // real boot-to-kiosk session that crashed exactly this way right
        // after a clean session end. Marshal the actual teardown onto the
        // main `GLib` context instead (safe to call from any thread) —
        // best-effort and fire-and-forget, matching this Drop impl's
        // existing contract: Drop cannot propagate errors, and EOS is not
        // required for a safe teardown (Null is always sufficient).
        let pipeline = self.pipeline.clone();
        gst::glib::MainContext::default().invoke(move || {
            let _ = pipeline.set_state(gst::State::Null);
        });
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

    fn hevc_capability() -> DecoderCapability {
        DecoderCapability {
            id: "gstreamer:avdec_h265".into(),
            codec: VideoCodec::Hevc,
            kind: DecoderKind::Software,
            maximum_width: 1280,
            maximum_height: 720,
            maximum_frames_per_second: 60,
        }
    }

    /// Builds a tiny, fully self-generated Annex-B byte stream via
    /// `videotestsrc ! {encoder} ! {parser} ! appsink`, constructed and run
    /// directly as a `gst::Pipeline` in Rust (no `gst-launch-1.0`
    /// subprocess). Never derived from a real phone capture — see
    /// `CLAUDE.md`'s user-content rule. Returns one `Vec<u8>` per encoded
    /// access unit; with `config-interval=-1`, every access unit already
    /// carries in-band parameter sets, so the first element also doubles as
    /// this test's `CodecConfig` stand-in.
    fn synthetic_access_units(count: u32, encoder: &str, parser: &str, caps: &str) -> Vec<Vec<u8>> {
        gst::init().expect("gstreamer available on this host");
        let description = format!(
            "videotestsrc num-buffers={count} \
             ! video/x-raw,width=64,height=48,framerate=30/1 \
             ! {encoder} \
             ! {parser} config-interval=-1 \
             ! {caps} \
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

    fn synthetic_h264_access_units(count: u32) -> Vec<Vec<u8>> {
        synthetic_access_units(
            count,
            "openh264enc",
            "h264parse",
            "video/x-h264,stream-format=byte-stream,alignment=au",
        )
    }

    fn synthetic_hevc_access_units(count: u32) -> Vec<Vec<u8>> {
        synthetic_access_units(
            count,
            "x265enc",
            "h265parse",
            "video/x-h265,stream-format=byte-stream,alignment=au",
        )
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
    fn decodes_and_runs_a_synthetic_hevc_stream_end_to_end_with_fakesink() {
        let backend = GstreamerBackend::new().expect("gstreamer available on this host");
        let pipeline = backend
            .build_video_render_pipeline(&hevc_capability(), RenderSink::Fake)
            .expect("pipeline builds");
        pipeline.start().expect("pipeline starts");

        let access_units = synthetic_hevc_access_units(10);
        let (codec_config, frames) = access_units
            .split_first()
            .expect("fixture produced at least one access unit");
        pipeline
            .push_codec_config(codec_config)
            .expect("codec config pushes");
        for (index, frame) in frames.iter().enumerate() {
            pipeline
                .push_frame(frame, (index as u64) * 16_667)
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

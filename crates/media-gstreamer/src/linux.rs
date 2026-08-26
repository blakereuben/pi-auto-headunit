use gstreamer as gst;
use media_api::{DecoderCapability, DecoderKind, VideoRequest};
use std::fmt;
use std::sync::Once;

use crate::{PipelineElements, capability_for_request, decoder_element, pipeline_elements};

static EQUALIZER_PLUGIN_WARMUP: Once = Once::new();

/// Real CI/test finding, 2026-08-26: `cargo test`'s default parallel test
/// threads can each independently try to load the `equalizer` `GStreamer`
/// plugin (via `equalizer-10bands` in a parsed pipeline string,
/// `audio.rs`) for the first time simultaneously. Observed directly as an
/// intermittent SIGSEGV — `cannot register existing type
/// 'GstIirEqualizerBand'` from `GLib`'s own type system — a genuine
/// double-registration race in `GStreamer`/`GLib`'s plugin loading, not in
/// this project's own code (nothing here calls `g_type_register_static`
/// directly, and `gst::init()` itself is already documented safe to call
/// concurrently). Forcing exactly one thread to construct (and
/// immediately drop, never started) a throwaway pipeline containing the
/// element, gated by a process-wide `Once`, serializes that first load;
/// every other concurrent caller of [`GstreamerBackend::new`] just waits
/// for it rather than racing it — after the plugin's types are
/// registered once, further lookups are read-only and safe. Reproduced
/// directly: `cargo test -p media-gstreamer --lib` failed 2 of 3 runs
/// before this fix (and failed CI, `RUST_TEST_THREADS=1` sidestepping it
/// locally without fixing it), passed reliably after.
fn warm_up_equalizer_plugin() {
    EQUALIZER_PLUGIN_WARMUP.call_once(|| {
        let _ = gst::parse::launch("equalizer-10bands name=warmup ! fakesink");
    });
}

#[derive(Debug)]
pub enum GstreamerError {
    Initialization(String),
    MissingElement(&'static str),
    PipelineConstruction(String),
    StateChange(String),
    PushBuffer(String),
    Pipeline(String),
}

impl fmt::Display for GstreamerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialization(message) => {
                write!(formatter, "failed to initialize GStreamer: {message}")
            }
            Self::MissingElement(element) => {
                write!(
                    formatter,
                    "required GStreamer element is missing: {element}"
                )
            }
            Self::PipelineConstruction(message) => {
                write!(formatter, "failed to construct render pipeline: {message}")
            }
            Self::StateChange(message) => {
                write!(formatter, "render pipeline state change failed: {message}")
            }
            Self::PushBuffer(message) => {
                write!(
                    formatter,
                    "failed to push buffer into render pipeline: {message}"
                )
            }
            Self::Pipeline(message) => {
                write!(formatter, "render pipeline reported an error: {message}")
            }
        }
    }
}

impl std::error::Error for GstreamerError {}

pub struct GstreamerBackend;

impl GstreamerBackend {
    pub fn new() -> Result<Self, GstreamerError> {
        gst::init().map_err(|error| GstreamerError::Initialization(error.to_string()))?;
        warm_up_equalizer_plugin();
        Ok(Self)
    }

    #[must_use]
    pub fn available_decoders(&self, request: &VideoRequest) -> Vec<DecoderCapability> {
        [DecoderKind::Hardware, DecoderKind::Software]
            .into_iter()
            .filter_map(|kind| {
                let element = decoder_element(request.codec, kind);
                gst::ElementFactory::find(element)
                    .map(|_| capability_for_request(*request, kind, element))
            })
            .collect()
    }

    pub fn verify_pipeline_elements(
        &self,
        capability: &DecoderCapability,
    ) -> Result<PipelineElements, GstreamerError> {
        let elements = pipeline_elements(capability);
        for element in [
            elements.parser,
            elements.decoder,
            elements.converter,
            elements.sink,
        ] {
            if gst::ElementFactory::find(element).is_none() {
                return Err(GstreamerError::MissingElement(element));
            }
        }
        Ok(elements)
    }

    pub fn build_video_render_pipeline(
        &self,
        capability: &DecoderCapability,
        sink: crate::RenderSink,
    ) -> Result<crate::VideoRenderPipeline, GstreamerError> {
        crate::VideoRenderPipeline::new(pipeline_elements(capability), sink)
    }

    pub fn build_audio_playback_pipeline(
        &self,
        format: crate::AudioFormat,
        sink: crate::AudioSink,
        device: Option<&str>,
        eq_bands: Option<&[f64]>,
        volume_percent: Option<u8>,
    ) -> Result<crate::AudioPlaybackPipeline, GstreamerError> {
        crate::AudioPlaybackPipeline::new(format, sink, device, eq_bands, volume_percent)
    }

    pub fn build_audio_capture_pipeline(
        &self,
        format: crate::AudioFormat,
        source: crate::AudioCaptureSource,
        interval: std::time::Duration,
    ) -> Result<crate::AudioCapturePipeline, GstreamerError> {
        crate::AudioCapturePipeline::new(format, source, interval)
    }

    pub fn build_microphone_capture_pipeline(
        &self,
        format: crate::AudioFormat,
        source: crate::AudioCaptureSource,
        device: Option<&str>,
    ) -> Result<crate::MicrophoneCapturePipeline, GstreamerError> {
        crate::MicrophoneCapturePipeline::new(format, source, device)
    }
}

use gstreamer as gst;
use media_api::{DecoderCapability, DecoderKind, VideoRequest};
use std::fmt;

use crate::{PipelineElements, capability_for_request, decoder_element, pipeline_elements};

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
    ) -> Result<crate::AudioPlaybackPipeline, GstreamerError> {
        crate::AudioPlaybackPipeline::new(format, sink, device)
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

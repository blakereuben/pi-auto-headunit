use gstreamer as gst;
use media_api::{DecoderCapability, DecoderKind, VideoRequest};
use std::fmt;

use crate::{PipelineElements, capability_for_request, decoder_element, pipeline_elements};

#[derive(Debug)]
pub enum GstreamerError {
    Initialization(String),
    MissingElement(&'static str),
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
}

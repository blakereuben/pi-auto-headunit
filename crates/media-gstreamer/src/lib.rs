//! `GStreamer` capability adapter for the board-independent media contracts.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod render;

#[cfg(target_os = "linux")]
pub use linux::{GstreamerBackend, GstreamerError};
#[cfg(target_os = "linux")]
pub use render::{RenderSink, VideoRenderPipeline};

#[cfg(any(target_os = "linux", test))]
use media_api::VideoRequest;
use media_api::{DecoderCapability, DecoderKind, VideoCodec};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PipelineElements {
    pub parser: &'static str,
    pub decoder: &'static str,
    pub converter: &'static str,
    pub sink: &'static str,
    /// `appsrc`'s caps string for this codec's Annex-B byte-stream framing
    /// (`video/x-h264` or `video/x-h265`) — must match `parser`/`decoder`,
    /// since an H.264-typed `appsrc` feeding `h265parse` (or vice versa) is
    /// a caps mismatch, not a real sink.
    pub caps: &'static str,
}

#[must_use]
pub fn pipeline_elements(capability: &DecoderCapability) -> PipelineElements {
    let (parser, caps) = match capability.codec {
        VideoCodec::H264 => (
            "h264parse",
            "video/x-h264,stream-format=byte-stream,alignment=au",
        ),
        VideoCodec::Hevc => (
            "h265parse",
            "video/x-h265,stream-format=byte-stream,alignment=au",
        ),
    };
    let decoder = decoder_element(capability.codec, capability.kind);
    PipelineElements {
        parser,
        decoder,
        converter: "videoconvert",
        sink: "waylandsink",
        caps,
    }
}

#[must_use]
pub fn decoder_element(codec: VideoCodec, kind: DecoderKind) -> &'static str {
    match (codec, kind) {
        (VideoCodec::H264, DecoderKind::Hardware) => "v4l2slh264dec",
        (VideoCodec::H264, DecoderKind::Software) => "avdec_h264",
        (VideoCodec::Hevc, DecoderKind::Hardware) => "v4l2slh265dec",
        (VideoCodec::Hevc, DecoderKind::Software) => "avdec_h265",
    }
}

#[cfg(any(target_os = "linux", test))]
fn capability_for_request(
    request: VideoRequest,
    kind: DecoderKind,
    element: &str,
) -> DecoderCapability {
    DecoderCapability {
        id: format!("gstreamer:{element}"),
        codec: request.codec,
        kind,
        maximum_width: request.mode.width,
        maximum_height: request.mode.height,
        maximum_frames_per_second: request.mode.frames_per_second,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use media_api::VideoMode;

    #[test]
    fn pi_five_h264_software_chain_is_explicit() {
        let capability = capability_for_request(
            VideoRequest {
                codec: VideoCodec::H264,
                mode: VideoMode {
                    width: 800,
                    height: 480,
                    frames_per_second: 30,
                },
            },
            DecoderKind::Software,
            "avdec_h264",
        );
        assert_eq!(
            pipeline_elements(&capability),
            PipelineElements {
                parser: "h264parse",
                decoder: "avdec_h264",
                converter: "videoconvert",
                sink: "waylandsink",
                caps: "video/x-h264,stream-format=byte-stream,alignment=au",
            }
        );
    }

    #[test]
    fn hevc_hardware_never_maps_to_h264() {
        assert_eq!(
            decoder_element(VideoCodec::Hevc, DecoderKind::Hardware),
            "v4l2slh265dec"
        );
    }
}

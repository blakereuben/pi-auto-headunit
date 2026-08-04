//! Board-independent media capability and decoder-selection contracts.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoCodec {
    H264,
    Hevc,
}

impl fmt::Display for VideoCodec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::H264 => formatter.write_str("H.264"),
            Self::Hevc => formatter.write_str("HEVC"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoMode {
    pub width: u16,
    pub height: u16,
    pub frames_per_second: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoRequest {
    pub codec: VideoCodec,
    pub mode: VideoMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DecoderKind {
    Hardware,
    Software,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecoderCapability {
    pub id: String,
    pub codec: VideoCodec,
    pub kind: DecoderKind,
    pub maximum_width: u16,
    pub maximum_height: u16,
    pub maximum_frames_per_second: u8,
}

impl DecoderCapability {
    #[must_use]
    pub fn supports(&self, request: &VideoRequest) -> bool {
        self.codec == request.codec
            && request.mode.width > 0
            && request.mode.height > 0
            && request.mode.frames_per_second > 0
            && request.mode.width <= self.maximum_width
            && request.mode.height <= self.maximum_height
            && request.mode.frames_per_second <= self.maximum_frames_per_second
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecoderPolicy {
    pub allow_software: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecoderSelectionError {
    InvalidMode(VideoMode),
    NoSupportedDecoder(VideoRequest),
}

impl fmt::Display for DecoderSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMode(mode) => write!(
                formatter,
                "invalid video mode {}x{} at {} fps",
                mode.width, mode.height, mode.frames_per_second
            ),
            Self::NoSupportedDecoder(request) => write!(
                formatter,
                "no permitted decoder supports {} at {}x{} and {} fps",
                request.codec,
                request.mode.width,
                request.mode.height,
                request.mode.frames_per_second
            ),
        }
    }
}

impl std::error::Error for DecoderSelectionError {}

/// Selects an exact-codec decoder, preferring hardware without inventing codec
/// substitution or negotiation that the caller did not request.
pub fn select_decoder<'a>(
    request: &VideoRequest,
    capabilities: &'a [DecoderCapability],
    policy: DecoderPolicy,
) -> Result<&'a DecoderCapability, DecoderSelectionError> {
    if request.mode.width == 0 || request.mode.height == 0 || request.mode.frames_per_second == 0 {
        return Err(DecoderSelectionError::InvalidMode(request.mode));
    }

    capabilities
        .iter()
        .filter(|capability| capability.supports(request))
        .filter(|capability| policy.allow_software || capability.kind == DecoderKind::Hardware)
        .min_by_key(|capability| capability.kind)
        .ok_or(DecoderSelectionError::NoSupportedDecoder(*request))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(codec: VideoCodec) -> VideoRequest {
        VideoRequest {
            codec,
            mode: VideoMode {
                width: 800,
                height: 480,
                frames_per_second: 30,
            },
        }
    }

    fn decoder(id: &str, codec: VideoCodec, kind: DecoderKind) -> DecoderCapability {
        DecoderCapability {
            id: id.into(),
            codec,
            kind,
            maximum_width: 1920,
            maximum_height: 1080,
            maximum_frames_per_second: 60,
        }
    }

    #[test]
    fn hardware_is_preferred_for_the_exact_codec() {
        let capabilities = [
            decoder("software-h264", VideoCodec::H264, DecoderKind::Software),
            decoder("hardware-h264", VideoCodec::H264, DecoderKind::Hardware),
        ];
        let selected = select_decoder(
            &request(VideoCodec::H264),
            &capabilities,
            DecoderPolicy {
                allow_software: true,
            },
        )
        .expect("supported decoder");
        assert_eq!(selected.id, "hardware-h264");
    }

    #[test]
    fn pi_five_style_h264_falls_back_to_software() {
        let capabilities = [
            decoder("hardware-hevc", VideoCodec::Hevc, DecoderKind::Hardware),
            decoder("software-h264", VideoCodec::H264, DecoderKind::Software),
        ];
        let selected = select_decoder(
            &request(VideoCodec::H264),
            &capabilities,
            DecoderPolicy {
                allow_software: true,
            },
        )
        .expect("measured software fallback");
        assert_eq!(selected.id, "software-h264");
    }

    #[test]
    fn never_substitutes_a_different_codec() {
        let capabilities = [decoder(
            "hardware-hevc",
            VideoCodec::Hevc,
            DecoderKind::Hardware,
        )];
        assert!(matches!(
            select_decoder(
                &request(VideoCodec::H264),
                &capabilities,
                DecoderPolicy {
                    allow_software: true,
                }
            ),
            Err(DecoderSelectionError::NoSupportedDecoder(_))
        ));
    }

    #[test]
    fn rejects_software_when_policy_disallows_it() {
        let capabilities = [decoder(
            "software-h264",
            VideoCodec::H264,
            DecoderKind::Software,
        )];
        assert!(
            select_decoder(
                &request(VideoCodec::H264),
                &capabilities,
                DecoderPolicy {
                    allow_software: false,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_zero_sized_or_zero_rate_requests() {
        let capabilities = [decoder(
            "software-h264",
            VideoCodec::H264,
            DecoderKind::Software,
        )];
        let invalid = VideoRequest {
            codec: VideoCodec::H264,
            mode: VideoMode {
                width: 0,
                height: 480,
                frames_per_second: 30,
            },
        };
        assert!(matches!(
            select_decoder(
                &invalid,
                &capabilities,
                DecoderPolicy {
                    allow_software: true,
                }
            ),
            Err(DecoderSelectionError::InvalidMode(_))
        ));
    }
}

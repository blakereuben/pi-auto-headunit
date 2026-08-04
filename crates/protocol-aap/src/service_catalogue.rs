use std::collections::BTreeSet;
use std::fmt;

// Portions derived from OpenAuto's service factory and service feature boundary
// at revision aa90412bf93b5a5078495ea85ac9270c6297d369.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// SPDX-License-Identifier: GPL-3.0-or-later

pub const DEFAULT_MAX_SERVICE_CANDIDATES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ServiceKind {
    Microphone,
    MediaAudio,
    SpeechAudio,
    SystemAudio,
    Sensors,
    Video,
    Bluetooth,
    Input,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceAvailability {
    Ready,
    Disabled,
    HardwareUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceCandidate {
    pub channel_id: u8,
    pub kind: ServiceKind,
    pub availability: ServiceAvailability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceDescriptor {
    pub channel_id: u8,
    pub kind: ServiceKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceCatalogue {
    services: Vec<ServiceDescriptor>,
}

impl ServiceCatalogue {
    pub fn build(
        candidates: &[ServiceCandidate],
        maximum_candidates: usize,
    ) -> Result<Self, ServiceCatalogueError> {
        if maximum_candidates == 0 {
            return Err(ServiceCatalogueError::InvalidLimit);
        }
        if candidates.len() > maximum_candidates {
            return Err(ServiceCatalogueError::TooManyCandidates {
                count: candidates.len(),
                maximum: maximum_candidates,
            });
        }

        let mut channel_ids = BTreeSet::new();
        let mut kinds = BTreeSet::new();
        let mut services = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            if candidate.channel_id == 0 {
                return Err(ServiceCatalogueError::ControlChannelReserved);
            }
            if !channel_ids.insert(candidate.channel_id) {
                return Err(ServiceCatalogueError::DuplicateChannel(
                    candidate.channel_id,
                ));
            }
            if !kinds.insert(candidate.kind) {
                return Err(ServiceCatalogueError::DuplicateKind(candidate.kind));
            }
            if candidate.availability == ServiceAvailability::Ready {
                services.push(ServiceDescriptor {
                    channel_id: candidate.channel_id,
                    kind: candidate.kind,
                });
            }
        }

        Ok(Self { services })
    }

    #[must_use]
    pub fn services(&self) -> &[ServiceDescriptor] {
        &self.services
    }

    #[must_use]
    pub fn service_for_channel(&self, channel_id: u8) -> Option<ServiceDescriptor> {
        self.services
            .iter()
            .copied()
            .find(|service| service.channel_id == channel_id)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceCatalogueError {
    InvalidLimit,
    TooManyCandidates { count: usize, maximum: usize },
    ControlChannelReserved,
    DuplicateChannel(u8),
    DuplicateKind(ServiceKind),
}

impl fmt::Display for ServiceCatalogueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit => formatter.write_str("service candidate limit must be non-zero"),
            Self::TooManyCandidates { count, maximum } => {
                write!(
                    formatter,
                    "service candidate count {count} exceeds limit {maximum}"
                )
            }
            Self::ControlChannelReserved => {
                formatter.write_str("channel zero is reserved for control messages")
            }
            Self::DuplicateChannel(channel_id) => {
                write!(formatter, "service channel {channel_id} is duplicated")
            }
            Self::DuplicateKind(kind) => write!(formatter, "service kind {kind:?} is duplicated"),
        }
    }
}

impl std::error::Error for ServiceCatalogueError {}

#[cfg(test)]
mod tests {
    use super::*;

    const CANDIDATES: [ServiceCandidate; 5] = [
        ServiceCandidate {
            channel_id: 1,
            kind: ServiceKind::Video,
            availability: ServiceAvailability::Ready,
        },
        ServiceCandidate {
            channel_id: 2,
            kind: ServiceKind::Input,
            availability: ServiceAvailability::Ready,
        },
        ServiceCandidate {
            channel_id: 3,
            kind: ServiceKind::MediaAudio,
            availability: ServiceAvailability::Disabled,
        },
        ServiceCandidate {
            channel_id: 4,
            kind: ServiceKind::Microphone,
            availability: ServiceAvailability::HardwareUnavailable,
        },
        ServiceCandidate {
            channel_id: 5,
            kind: ServiceKind::SystemAudio,
            availability: ServiceAvailability::Ready,
        },
    ];

    #[test]
    fn advertises_only_ready_services_in_declared_order() {
        let catalogue = ServiceCatalogue::build(&CANDIDATES, DEFAULT_MAX_SERVICE_CANDIDATES)
            .expect("catalogue");
        assert_eq!(
            catalogue.services(),
            [
                ServiceDescriptor {
                    channel_id: 1,
                    kind: ServiceKind::Video,
                },
                ServiceDescriptor {
                    channel_id: 2,
                    kind: ServiceKind::Input,
                },
                ServiceDescriptor {
                    channel_id: 5,
                    kind: ServiceKind::SystemAudio,
                },
            ]
        );
        assert_eq!(
            catalogue.service_for_channel(2),
            Some(ServiceDescriptor {
                channel_id: 2,
                kind: ServiceKind::Input,
            })
        );
        assert_eq!(catalogue.service_for_channel(3), None);
    }

    #[test]
    fn rejects_control_and_duplicate_channels() {
        let control = [ServiceCandidate {
            channel_id: 0,
            kind: ServiceKind::Video,
            availability: ServiceAvailability::Ready,
        }];
        assert_eq!(
            ServiceCatalogue::build(&control, 1),
            Err(ServiceCatalogueError::ControlChannelReserved)
        );

        let mut duplicate = CANDIDATES;
        duplicate[1].channel_id = duplicate[0].channel_id;
        assert_eq!(
            ServiceCatalogue::build(&duplicate, DEFAULT_MAX_SERVICE_CANDIDATES),
            Err(ServiceCatalogueError::DuplicateChannel(1))
        );
    }

    #[test]
    fn rejects_duplicate_service_roles_even_when_disabled() {
        let duplicate = [
            ServiceCandidate {
                channel_id: 1,
                kind: ServiceKind::Bluetooth,
                availability: ServiceAvailability::HardwareUnavailable,
            },
            ServiceCandidate {
                channel_id: 2,
                kind: ServiceKind::Bluetooth,
                availability: ServiceAvailability::Ready,
            },
        ];
        assert_eq!(
            ServiceCatalogue::build(&duplicate, 2),
            Err(ServiceCatalogueError::DuplicateKind(ServiceKind::Bluetooth))
        );
    }

    #[test]
    fn enforces_candidate_limit() {
        assert_eq!(
            ServiceCatalogue::build(&[], 0),
            Err(ServiceCatalogueError::InvalidLimit)
        );
        assert_eq!(
            ServiceCatalogue::build(&CANDIDATES, 4),
            Err(ServiceCatalogueError::TooManyCandidates {
                count: 5,
                maximum: 4,
            })
        );
    }
}

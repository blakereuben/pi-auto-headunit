//! Platform inventory and radio-provider contracts.

use std::fmt;

mod gesture;
mod touch;
pub use gesture::{ArmedGestureDetector, GestureEvent, GestureId, SharedArmWindow};
pub use touch::{MultiTouchTracker, RawTouchEvent, TouchFrame, TouchPhase, TouchPoint};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityState {
    Absent,
    Ready,
    Disabled,
    Degraded,
}

impl fmt::Display for CapabilityState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Absent => "absent",
            Self::Ready => "ready",
            Self::Disabled => "disabled",
            Self::Degraded => "degraded",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderKind {
    Onboard,
    Usb,
    Other,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Onboard => "onboard",
            Self::Usb => "usb",
            Self::Other => "other",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioKind {
    Wifi,
    Bluetooth,
}

impl fmt::Display for RadioKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wifi => f.write_str("wifi"),
            Self::Bluetooth => f.write_str("bluetooth"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RadioProvider {
    /// Stable physical identity when Linux exposes one. Never use `wlan1` or
    /// `hci1` alone as persisted identity.
    pub stable_id: String,
    pub interface_name: String,
    pub radio: RadioKind,
    pub provider: ProviderKind,
    pub state: CapabilityState,
    pub driver: Option<String>,
    pub usb_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderPreference {
    Auto,
    Onboard,
    StableId(String),
}

impl ProviderPreference {
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "onboard" => Self::Onboard,
            _ => Self::StableId(value.trim().to_owned()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderChoice {
    pub selected: Option<RadioProvider>,
    pub explanation: String,
}

#[must_use]
pub fn choose_provider(
    radio: RadioKind,
    providers: &[RadioProvider],
    preference: &ProviderPreference,
) -> ProviderChoice {
    let matching = providers.iter().filter(|provider| provider.radio == radio);
    let selected = match preference {
        ProviderPreference::Auto => matching
            .clone()
            .find(|provider| {
                provider.provider == ProviderKind::Onboard
                    && provider.state == CapabilityState::Ready
            })
            .or_else(|| {
                matching.clone().find(|provider| {
                    provider.provider == ProviderKind::Usb
                        && provider.state == CapabilityState::Ready
                })
            }),
        ProviderPreference::Onboard => matching.clone().find(|provider| {
            provider.provider == ProviderKind::Onboard && provider.state == CapabilityState::Ready
        }),
        ProviderPreference::StableId(stable_id) => matching.clone().find(|provider| {
            provider.stable_id == *stable_id && provider.state == CapabilityState::Ready
        }),
    };

    let explanation = if let Some(provider) = selected {
        format!(
            "selected {} {} provider {}",
            provider.provider, radio, provider.stable_id
        )
    } else {
        match preference {
            ProviderPreference::Auto => format!("no ready {radio} provider found"),
            ProviderPreference::Onboard => {
                format!("requested onboard {radio} provider is not ready")
            }
            ProviderPreference::StableId(stable_id) => {
                format!("requested {radio} provider {stable_id} is missing or not ready")
            }
        }
    };

    ProviderChoice {
        selected: selected.cloned(),
        explanation,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemInventory {
    pub model: String,
    pub os_name: String,
    pub os_version: String,
    pub architecture: String,
    pub kernel: String,
    pub supported_baseline: bool,
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str, kind: ProviderKind, state: CapabilityState) -> RadioProvider {
        RadioProvider {
            stable_id: id.into(),
            interface_name: id.into(),
            radio: RadioKind::Wifi,
            provider: kind,
            state,
            driver: None,
            usb_id: None,
            reason: None,
        }
    }

    #[test]
    fn auto_prefers_ready_onboard() {
        let providers = vec![
            provider("usb", ProviderKind::Usb, CapabilityState::Ready),
            provider("onboard", ProviderKind::Onboard, CapabilityState::Ready),
        ];
        let choice = choose_provider(RadioKind::Wifi, &providers, &ProviderPreference::Auto);
        assert_eq!(choice.selected.expect("provider").stable_id, "onboard");
    }

    #[test]
    fn auto_falls_back_to_usb_when_onboard_is_absent_or_degraded() {
        let providers = vec![
            provider("onboard", ProviderKind::Onboard, CapabilityState::Degraded),
            provider("usb", ProviderKind::Usb, CapabilityState::Ready),
        ];
        let choice = choose_provider(RadioKind::Wifi, &providers, &ProviderPreference::Auto);
        assert_eq!(choice.selected.expect("provider").stable_id, "usb");
    }

    #[test]
    fn manual_selection_never_silently_falls_back() {
        let providers = vec![provider(
            "onboard",
            ProviderKind::Onboard,
            CapabilityState::Ready,
        )];
        let choice = choose_provider(
            RadioKind::Wifi,
            &providers,
            &ProviderPreference::StableId("missing-usb".into()),
        );
        assert!(choice.selected.is_none());
    }
}

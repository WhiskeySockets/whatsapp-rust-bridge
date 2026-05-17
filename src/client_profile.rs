//! TS-facing input shapes for `Client::set_client_profile`.
//!
//! Mirrors the preset constructors in `wacore::client_profile::ClientProfile`
//! 1:1. Defaulting (device name, manufacturer, `include_web_info`) lives in
//! wacore — exposing presets here avoids drift if upstream changes the
//! defaults for any platform.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wacore::client_profile::ClientProfile;

/// Optional Noise-payload overrides applied on top of every preset.
/// Leaving any field `None` preserves wacore's default, which matches
/// WA Web (notably `phoneId` stays unset on the wire).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(default, rename_all = "camelCase")]
pub struct ClientProfileOverrides {
    pub phone_id: Option<String>,
    pub locale_language: Option<String>,
    pub locale_country: Option<String>,
    pub passive_login: Option<bool>,
}

impl ClientProfileOverrides {
    fn apply(self, mut profile: ClientProfile) -> ClientProfile {
        if self.phone_id.is_some() {
            profile.phone_id = self.phone_id;
        }
        if let Some(lang) = self.locale_language {
            profile.locale_language = lang;
        }
        if let Some(country) = self.locale_country {
            profile.locale_country = country;
        }
        if let Some(passive) = self.passive_login {
            profile.passive_login = passive;
        }
        profile
    }
}

/// Selects which `ClientProfile` preset to use for the noise-handshake
/// `ClientPayload.UserAgent`. Independent of `DeviceProps`: `setDeviceProps`
/// controls the "Linked Devices" display on the phone, this controls what
/// the server sees in the noise layer.
///
/// Use `{ preset: 'android', osVersion: '13' }` to mirror the upstream
/// Baileys `Browsers.android('13')` behavior (UserAgent.platform = ANDROID,
/// `web_info` omitted).
///
/// Every variant flattens the [`ClientProfileOverrides`] fields, so the
/// JS literal is flat (e.g. `{ preset: 'web', phoneId: 'fixed-id' }`).
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(
    tag = "preset",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClientProfileInput {
    Web {
        #[serde(flatten)]
        overrides: ClientProfileOverrides,
    },
    Android {
        os_version: String,
        #[serde(flatten)]
        overrides: ClientProfileOverrides,
    },
    SmbAndroid {
        os_version: String,
        #[serde(flatten)]
        overrides: ClientProfileOverrides,
    },
    Ios {
        os_version: String,
        #[serde(flatten)]
        overrides: ClientProfileOverrides,
    },
    Macos {
        os_version: String,
        #[serde(flatten)]
        overrides: ClientProfileOverrides,
    },
    Windows {
        os_version: String,
        #[serde(flatten)]
        overrides: ClientProfileOverrides,
    },
}

impl From<ClientProfileInput> for ClientProfile {
    fn from(input: ClientProfileInput) -> Self {
        match input {
            ClientProfileInput::Web { overrides } => overrides.apply(ClientProfile::web()),
            ClientProfileInput::Android {
                os_version,
                overrides,
            } => overrides.apply(ClientProfile::android(os_version)),
            ClientProfileInput::SmbAndroid {
                os_version,
                overrides,
            } => overrides.apply(ClientProfile::smb_android(os_version)),
            ClientProfileInput::Ios {
                os_version,
                overrides,
            } => overrides.apply(ClientProfile::ios(os_version)),
            ClientProfileInput::Macos {
                os_version,
                overrides,
            } => overrides.apply(ClientProfile::macos(os_version)),
            ClientProfileInput::Windows {
                os_version,
                overrides,
            } => overrides.apply(ClientProfile::windows(os_version)),
        }
    }
}

//! TS-facing input shapes for `Client::set_client_profile`.
//!
//! Mirrors the preset constructors in `wacore::client_profile::ClientProfile`
//! 1:1. Defaulting (device name, manufacturer, `include_web_info`) lives in
//! wacore — exposing presets here avoids drift if upstream changes the
//! defaults for any platform.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wacore::client_profile::ClientProfile;

/// Selects which `ClientProfile` preset to use for the noise-handshake
/// `ClientPayload.UserAgent`. Independent of `DeviceProps`: `setDeviceProps`
/// controls the "Linked Devices" display on the phone, this controls what
/// the server sees in the noise layer.
///
/// Use `{ preset: 'android', osVersion: '13' }` to mirror the upstream
/// Baileys `Browsers.android('13')` behavior (UserAgent.platform = ANDROID,
/// `web_info` omitted).
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(
    tag = "preset",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClientProfileInput {
    Web,
    Android { os_version: String },
    SmbAndroid { os_version: String },
    Ios { os_version: String },
    Macos { os_version: String },
    Windows { os_version: String },
}

impl From<ClientProfileInput> for ClientProfile {
    fn from(input: ClientProfileInput) -> Self {
        match input {
            ClientProfileInput::Web => ClientProfile::web(),
            ClientProfileInput::Android { os_version } => ClientProfile::android(os_version),
            ClientProfileInput::SmbAndroid { os_version } => ClientProfile::smb_android(os_version),
            ClientProfileInput::Ios { os_version } => ClientProfile::ios(os_version),
            ClientProfileInput::Macos { os_version } => ClientProfile::macos(os_version),
            ClientProfileInput::Windows { os_version } => ClientProfile::windows(os_version),
        }
    }
}

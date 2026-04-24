//! TS-facing input shapes for `Client::set_device_props`. Defined here (not
//! reused from prost) because prost types don't derive `Tsify`/`Deserialize`,
//! and because we want a stable typed surface that doesn't churn whenever the
//! underlying proto adds a field.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wacore::store::DevicePropsOverride;
use waproto::whatsapp::device_props as wa_dp;

/// Mirrors `device_props.PlatformType`. The display value the phone shows in
/// "Linked Devices" — and the type WhatsApp's server uses to decide whether
/// features like view-once are deliverable as payload or as `absent` stub.
/// Variant names render as `SCREAMING_SNAKE_CASE` in TS to match the proto
/// enum identifiers callers see in WhatsApp documentation / wire dumps.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DevicePlatformType {
    Unknown,
    Chrome,
    Firefox,
    Ie,
    Opera,
    Safari,
    Edge,
    Desktop,
    Ipad,
    AndroidTablet,
    Ohana,
    Aloha,
    Catalina,
    TclTv,
    IosPhone,
    IosCatalyst,
    AndroidPhone,
    AndroidAmbiguous,
    WearOs,
    ArWrist,
    ArDevice,
    Uwp,
    Vr,
    CloudApi,
    Smartglasses,
}

impl From<DevicePlatformType> for wa_dp::PlatformType {
    fn from(value: DevicePlatformType) -> Self {
        match value {
            DevicePlatformType::Unknown => Self::Unknown,
            DevicePlatformType::Chrome => Self::Chrome,
            DevicePlatformType::Firefox => Self::Firefox,
            DevicePlatformType::Ie => Self::Ie,
            DevicePlatformType::Opera => Self::Opera,
            DevicePlatformType::Safari => Self::Safari,
            DevicePlatformType::Edge => Self::Edge,
            DevicePlatformType::Desktop => Self::Desktop,
            DevicePlatformType::Ipad => Self::Ipad,
            DevicePlatformType::AndroidTablet => Self::AndroidTablet,
            DevicePlatformType::Ohana => Self::Ohana,
            DevicePlatformType::Aloha => Self::Aloha,
            DevicePlatformType::Catalina => Self::Catalina,
            DevicePlatformType::TclTv => Self::TclTv,
            DevicePlatformType::IosPhone => Self::IosPhone,
            DevicePlatformType::IosCatalyst => Self::IosCatalyst,
            DevicePlatformType::AndroidPhone => Self::AndroidPhone,
            DevicePlatformType::AndroidAmbiguous => Self::AndroidAmbiguous,
            DevicePlatformType::WearOs => Self::WearOs,
            DevicePlatformType::ArWrist => Self::ArWrist,
            DevicePlatformType::ArDevice => Self::ArDevice,
            DevicePlatformType::Uwp => Self::Uwp,
            DevicePlatformType::Vr => Self::Vr,
            DevicePlatformType::CloudApi => Self::CloudApi,
            DevicePlatformType::Smartglasses => Self::Smartglasses,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAppVersion {
    #[tsify(optional)]
    #[serde(default)]
    pub primary: Option<u32>,
    #[tsify(optional)]
    #[serde(default)]
    pub secondary: Option<u32>,
    #[tsify(optional)]
    #[serde(default)]
    pub tertiary: Option<u32>,
    #[tsify(optional)]
    #[serde(default)]
    pub quaternary: Option<u32>,
}

impl From<DeviceAppVersion> for wa_dp::AppVersion {
    fn from(v: DeviceAppVersion) -> Self {
        Self {
            primary: v.primary,
            secondary: v.secondary,
            tertiary: v.tertiary,
            quaternary: v.quaternary,
            ..Default::default()
        }
    }
}

/// Mirrors `device_props.HistorySyncConfig`. Only fields a consumer would
/// realistically tune are exposed individually; partial overrides merge into
/// `wacore::store::default_history_sync_config()` so callers don't accidentally
/// drop the WA-Web-aligned support_* claims by setting just one field.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct DeviceHistorySyncConfig {
    #[tsify(optional)]
    #[serde(default)]
    pub full_sync_days_limit: Option<u32>,
    #[tsify(optional)]
    #[serde(default)]
    pub full_sync_size_mb_limit: Option<u32>,
    #[tsify(optional)]
    #[serde(default)]
    pub storage_quota_mb: Option<u32>,
    #[tsify(optional)]
    #[serde(default)]
    pub recent_sync_days_limit: Option<u32>,
    #[tsify(optional)]
    #[serde(default)]
    pub support_call_log_history: Option<bool>,
    #[tsify(optional)]
    #[serde(default)]
    pub support_group_history: Option<bool>,
    #[tsify(optional)]
    #[serde(default)]
    pub on_demand_ready: Option<bool>,
    #[tsify(optional)]
    #[serde(default)]
    pub thumbnail_sync_days_limit: Option<u32>,
    #[tsify(optional)]
    #[serde(default)]
    pub initial_sync_max_messages_per_chat: Option<u32>,
}

impl DeviceHistorySyncConfig {
    fn merge_into(self, mut base: wa_dp::HistorySyncConfig) -> wa_dp::HistorySyncConfig {
        if self.full_sync_days_limit.is_some() {
            base.full_sync_days_limit = self.full_sync_days_limit;
        }
        if self.full_sync_size_mb_limit.is_some() {
            base.full_sync_size_mb_limit = self.full_sync_size_mb_limit;
        }
        if self.storage_quota_mb.is_some() {
            base.storage_quota_mb = self.storage_quota_mb;
        }
        if self.recent_sync_days_limit.is_some() {
            base.recent_sync_days_limit = self.recent_sync_days_limit;
        }
        if self.support_call_log_history.is_some() {
            base.support_call_log_history = self.support_call_log_history;
        }
        if self.support_group_history.is_some() {
            base.support_group_history = self.support_group_history;
        }
        if self.on_demand_ready.is_some() {
            base.on_demand_ready = self.on_demand_ready;
        }
        if self.thumbnail_sync_days_limit.is_some() {
            base.thumbnail_sync_days_limit = self.thumbnail_sync_days_limit;
        }
        if self.initial_sync_max_messages_per_chat.is_some() {
            base.initial_sync_max_messages_per_chat = self.initial_sync_max_messages_per_chat;
        }
        base
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct DevicePropsInput {
    #[tsify(optional)]
    #[serde(default)]
    pub os: Option<String>,
    #[tsify(optional)]
    #[serde(default)]
    pub platform_type: Option<DevicePlatformType>,
    #[tsify(optional)]
    #[serde(default)]
    pub version: Option<DeviceAppVersion>,
    #[tsify(optional)]
    #[serde(default)]
    pub history_sync_config: Option<DeviceHistorySyncConfig>,
}

impl From<DevicePropsInput> for DevicePropsOverride {
    fn from(input: DevicePropsInput) -> Self {
        let mut o = DevicePropsOverride::new();
        if let Some(os) = input.os {
            o = o.with_os(os);
        }
        if let Some(pt) = input.platform_type {
            o = o.with_platform_type(pt.into());
        }
        if let Some(v) = input.version {
            o = o.with_version(v.into());
        }
        if let Some(hsc) = input.history_sync_config {
            o = o.with_history_sync_config(
                hsc.merge_into(wacore::store::device::default_history_sync_config()),
            );
        }
        o
    }
}

// Signal protocol primitives exposed to JS — only needed if JS handles
// sessions directly. When using WasmWhatsAppClient (which handles Signal
// internally), these are dead code and excluded by default to reduce WASM size.
#[cfg(feature = "signal-js")]
pub mod appstate;
#[cfg(feature = "signal-js")]
pub mod binary;
#[cfg(feature = "signal-js")]
pub mod group_cipher;
#[cfg(feature = "signal-js")]
pub mod group_types;
#[cfg(feature = "signal-js")]
pub mod key_helper;
#[cfg(feature = "signal-js")]
pub mod noise_session;
#[cfg(feature = "signal-js")]
pub mod protocol_address;
#[cfg(feature = "signal-js")]
pub mod sender_key_name;
#[cfg(feature = "signal-js")]
pub mod session_builder;
#[cfg(feature = "signal-js")]
pub mod session_cipher;
#[cfg(feature = "signal-js")]
pub mod session_record;
#[cfg(feature = "signal-js")]
pub mod storage_adapter;

#[cfg(feature = "audio")]
pub mod audio;
#[cfg(feature = "image")]
pub mod image_utils;
#[cfg(feature = "sticker")]
pub mod sticker_metadata;

pub mod camel_serializer;
pub mod crypto;
pub mod curve;
pub mod js_backend;
pub mod js_cache_store;
pub mod js_http;
pub mod js_time;
pub mod js_transport;
pub mod logger;
pub mod proto;
pub mod result_types;
pub mod runtime;
pub mod wasm_client;

/// SAFETY: WASM is single-threaded — Send + Sync are trivially satisfied.
/// This macro reduces boilerplate for types that hold JS values.
macro_rules! wasm_send_sync {
    ($($t:ty),+ $(,)?) => {
        $(
            unsafe impl Send for $t {}
            unsafe impl Sync for $t {}
        )+
    };
}
pub(crate) use wasm_send_sync;

use serde::Serialize;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

#[cfg(feature = "signal-js")]
use js_sys::Uint8Array;

/// Returns the WhatsApp connection header (WA_CONN_HEADER).
/// This is the 4-byte header sent at the start of a WebSocket connection.
/// Only needed when JS handles the noise handshake directly.
#[cfg(feature = "signal-js")]
#[wasm_bindgen(js_name = getWAConnHeader)]
pub fn get_wa_conn_header() -> Uint8Array {
    let result = Uint8Array::new_with_length(4);
    result.copy_from(&wacore_binary::consts::WA_CONN_HEADER);
    result
}

/// Enabled features in this build.
/// Use this to check feature availability at runtime before calling feature-gated functions.
#[derive(Debug, Clone, Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct EnabledFeatures {
    /// Audio processing support (waveform generation, duration detection)
    pub audio: bool,
    /// Image processing support (thumbnails, profile pictures, format conversion)
    pub image: bool,
    /// Sticker metadata support (WebP EXIF for WhatsApp stickers)
    pub sticker: bool,
}

/// Returns which optional features are enabled in this build.
/// Use this to conditionally call feature-gated functions.
#[wasm_bindgen(js_name = getEnabledFeatures)]
pub fn get_enabled_features() -> EnabledFeatures {
    EnabledFeatures {
        audio: cfg!(feature = "audio"),
        image: cfg!(feature = "image"),
        sticker: cfg!(feature = "sticker"),
    }
}

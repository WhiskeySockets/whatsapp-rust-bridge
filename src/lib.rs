// Talc allocator — ~2x faster than dlmalloc (default), ~63% smaller binary overhead.
// Dynamic mode with grow-in-place for reduced fragmentation in long-running clients.
// `WasmDynamicTalc` is the single-threaded, zero-overhead variant (talc 5.x).
#[cfg(all(not(target_feature = "atomics"), target_family = "wasm"))]
#[global_allocator]
static ALLOCATOR: talc::wasm::WasmDynamicTalc = talc::wasm::new_wasm_dynamic_allocator();

#[cfg(feature = "audio")]
pub mod audio;
#[cfg(feature = "image")]
pub mod image_utils;
#[cfg(feature = "sticker")]
pub mod sticker_metadata;

pub mod camel_serializer;
mod generated_types;
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

/// Convert a JsValue error to JsError. Shared across WASM wrapper modules.
pub(crate) fn js_val_to_error(e: JsValue) -> JsError {
    if let Some(s) = e.as_string() {
        JsError::new(&s)
    } else {
        JsError::new(&format!("{e:?}"))
    }
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

/// Returns current WASM linear memory usage in bytes.
///
/// This is the total memory reserved by the WASM instance (pages × 64KB).
/// Useful for monitoring memory pressure during media operations.
///
/// Note: this includes free space managed by the allocator — it's the
/// total memory footprint, not the amount currently in use.
#[wasm_bindgen(js_name = getWasmMemoryBytes)]
pub fn get_wasm_memory_bytes() -> f64 {
    let pages = core::arch::wasm32::memory_size::<0>();
    (pages * 65536) as f64
}

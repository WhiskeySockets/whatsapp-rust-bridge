pub mod appstate;
pub mod audio;
pub mod binary;
pub mod curve;
pub mod group_cipher;
pub mod group_types;
pub mod image_utils;
pub mod key_helper;
pub mod logger;
pub mod noise_session;
pub mod protocol_address;
pub mod sender_key_name;
pub mod session_builder;
pub mod session_cipher;
pub mod session_record;
pub mod sticker_metadata;
pub mod storage_adapter;

// Re-export WhatsApp protocol constants for JS usage
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

/// Returns the WhatsApp connection header (WA_CONN_HEADER).
/// This is the 4-byte header sent at the start of a WebSocket connection.
#[wasm_bindgen(js_name = getWAConnHeader)]
pub fn get_wa_conn_header() -> Uint8Array {
    let result = Uint8Array::new_with_length(4);
    result.copy_from(&wacore_binary::consts::WA_CONN_HEADER);
    result
}

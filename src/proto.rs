use base64::Engine;
use prost::Message;
use wasm_bindgen::prelude::*;

use waproto::whatsapp as wa;

/// Serialize a value to JsValue. Uses BigInt for large integer types
/// (u64, i64) to avoid precision loss, unlike protobufjs which truncates to f64.
pub fn to_js_value<T: serde::Serialize>(val: &T) -> Result<JsValue, JsValue> {
    let serializer =
        serde_wasm_bindgen::Serializer::new().serialize_large_number_types_as_bigints(true);
    val.serialize(&serializer)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Recursively convert camelCase keys to snake_case on a JS object.
/// Handles nested objects and arrays. Leaves non-objects unchanged.
pub fn to_snake_case_js(val: &JsValue) -> JsValue {
    if val.is_null() || val.is_undefined() {
        return val.clone();
    }

    if let Ok(arr) = val.clone().dyn_into::<js_sys::Array>() {
        let result = js_sys::Array::new_with_length(arr.length());
        for i in 0..arr.length() {
            result.set(i, to_snake_case_js(&arr.get(i)));
        }
        return result.into();
    }

    if val.is_object() {
        // Skip typed arrays and ArrayBuffer (Uint8Array, Buffer, DataView, etc.)
        if val.is_instance_of::<js_sys::Uint8Array>()
            || val.is_instance_of::<js_sys::ArrayBuffer>()
            || val.is_instance_of::<js_sys::DataView>()
            || val.is_instance_of::<js_sys::Int8Array>()
        {
            return val.clone();
        }

        // Skip if it has a `buffer` property (Node Buffer) — it's array-like, not a plain object
        if js_sys::Reflect::has(val, &JsValue::from_str("buffer")).unwrap_or(false)
            && js_sys::Reflect::has(val, &JsValue::from_str("byteOffset")).unwrap_or(false)
        {
            return val.clone();
        }

        let Ok(obj) = val.clone().dyn_into::<js_sys::Object>() else {
            return val.clone();
        };
        let result = js_sys::Object::new();
        let entries = js_sys::Object::entries(&obj);
        for i in 0..entries.length() {
            let pair = js_sys::Array::from(&entries.get(i));
            let key = pair.get(0).as_string().unwrap_or_default();
            let value = pair.get(1);

            let snake_key = camel_to_snake(&key);
            let converted_value = if let Some(s) = value.as_string() {
                // Detect base64 strings for known bytes fields → convert to Uint8Array
                if !s.is_empty() && is_likely_bytes_field(&snake_key) && looks_like_base64(&s) {
                    // Use js_sys::global atob or manual decode
                    match base64_decode(&s) {
                        Some(bytes) => js_sys::Uint8Array::from(bytes.as_slice()).into(),
                        None => JsValue::from_str(&s),
                    }
                } else {
                    JsValue::from_str(&s)
                }
            } else {
                to_snake_case_js(&value)
            };
            let _ = js_sys::Reflect::set(&result, &JsValue::from_str(&snake_key), &converted_value);
        }
        return result.into();
    }

    // Truncate floats to integers for proto integer fields
    if let Some(n) = val.as_f64() {
        if n != n.trunc() {
            return JsValue::from_f64(n.trunc());
        }
    }

    val.clone()
}

const BYTES_SUFFIXES: &[&str] = &[
    "thumbnail",
    "key",
    "hash",
    "sha256",
    "enc",
    "mac",
    "iv",
    "signature",
    "ciphertext",
    "payload",
    "token",
    "secret",
    "identity",
    "ephemeral",
    "hmac",
];

fn is_likely_bytes_field(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    BYTES_SUFFIXES.iter().any(|s| lower.contains(s))
}

fn looks_like_base64(s: &str) -> bool {
    s.len() >= 4
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

fn camel_to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_uppercase() {
            result.push('_');
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

macro_rules! proto_types {
    ($($name:literal => $type:ty),* $(,)?) => {
        /// Generic protobuf encode: takes a type name and camelCase JS object, returns binary.
        /// Converts camelCase→snake_case and truncates floats in Rust before deserialization.
        #[wasm_bindgen(js_name = encodeProto)]
        pub fn encode_proto(type_name: &str, json: JsValue) -> Result<Vec<u8>, JsValue> {
            let snake = to_snake_case_js(&json);
            match type_name {
                $($name => {
                    let msg: $type = serde_wasm_bindgen::from_value(snake)
                        .map_err(|e| JsValue::from_str(&e.to_string()))?;
                    Ok(msg.encode_to_vec())
                })*
                _ => Err(JsValue::from_str(&format!("unknown proto type: {type_name}")))
            }
        }

        /// Generic protobuf decode: takes a type name and binary, returns camelCase JS object.
        #[wasm_bindgen(js_name = decodeProto)]
        pub fn decode_proto(type_name: &str, data: &[u8]) -> Result<JsValue, JsValue> {
            match type_name {
                $($name => {
                    let msg = <$type>::decode(data)
                        .map_err(|e| JsValue::from_str(&e.to_string()))?;
                    crate::camel_serializer::to_js_value_camel(&msg)
                })*
                _ => Err(JsValue::from_str(&format!("unknown proto type: {type_name}")))
            }
        }
    }
}

proto_types! {
    "Message" => wa::Message,
    "WebMessageInfo" => wa::WebMessageInfo,
    "HistorySync" => wa::HistorySync,
    "SyncActionData" => wa::SyncActionData,
    "ClientPayload" => wa::ClientPayload,
    "AdvSignedDeviceIdentity" => wa::AdvSignedDeviceIdentity,
    "AdvSignedKeyIndexList" => wa::AdvSignedKeyIndexList,
    "AdvDeviceIdentity" => wa::AdvDeviceIdentity,
    "AdvSignedDeviceIdentityHmac" => wa::AdvSignedDeviceIdentityHmac,
    "HandshakeMessage" => wa::HandshakeMessage,
    "SyncdRecord" => wa::SyncdRecord,
    "SyncdMutation" => wa::SyncdMutation,
    "SyncdMutations" => wa::SyncdMutations,
    "SyncdPatch" => wa::SyncdPatch,
    "SyncdSnapshot" => wa::SyncdSnapshot,
    "ExitCode" => wa::ExitCode,
    "SyncActionValue" => wa::SyncActionValue,
    "DeviceProps" => wa::DeviceProps,
    "SenderKeyDistributionMessage" => wa::SenderKeyDistributionMessage,
    "SenderKeyMessage" => wa::SenderKeyMessage,
    "ServerErrorReceipt" => wa::ServerErrorReceipt,
    "CertChain" => wa::CertChain,
    "CertChain.NoiseCertificate" => wa::cert_chain::NoiseCertificate,
    "CertChain.NoiseCertificate.Details" => wa::cert_chain::noise_certificate::Details,
    "ExternalBlobReference" => wa::ExternalBlobReference,
    "LidMigrationMappingSyncPayload" => wa::LidMigrationMappingSyncPayload,
    "MediaRetryNotification" => wa::MediaRetryNotification,
    "VerifiedNameCertificate" => wa::VerifiedNameCertificate,
    "VerifiedNameCertificate.Details" => wa::verified_name_certificate::Details,
    "Message.PollVoteMessage" => wa::message::PollVoteMessage,
    "Message.EventResponseMessage" => wa::message::EventResponseMessage,
}

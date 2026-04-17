use wasm_bindgen::prelude::*;

/// Serialize a value to JsValue. Uses BigInt for large integer types
/// (u64, i64) to avoid precision loss, unlike protobufjs which truncates to f64.
pub fn to_js_value<T: serde::Serialize>(val: &T) -> Result<JsValue, JsValue> {
    let serializer =
        serde_wasm_bindgen::Serializer::new().serialize_large_number_types_as_bigints(true);
    val.serialize(&serializer)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

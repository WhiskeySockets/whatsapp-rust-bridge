use wasm_bindgen::prelude::*;

/// Serialize a value to JsValue.
///
/// - BigInt for large integer types (u64, i64) avoids precision loss
///   that protobufjs's f64 fallback would introduce.
/// - `serialize_maps_as_objects(true)` makes any rust `serialize_map` (used
///   by hand-written `Serialize` impls — notably the `WireEnum` derive's
///   internally-tagged enum output) emit a plain JS `Object` instead of a
///   native JS `Map`. Plain objects round-trip through `JSON.stringify`,
///   support `obj.key` property access, and match what every downstream
///   adapter expects.
pub fn to_js_value<T: serde::Serialize>(val: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_large_number_types_as_bigints(true)
        .serialize_maps_as_objects(true);
    val.serialize(&serializer)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

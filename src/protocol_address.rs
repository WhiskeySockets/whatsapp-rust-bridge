use js_sys::Reflect;
use std::fmt;
use wacore_libsignal::core::DeviceId;
use wacore_libsignal::core::ProtocolAddress as CoreProtocolAddress;
use wasm_bindgen::prelude::*;

// This is our Rust struct. We'll use wasm-bindgen to export it.
// It's a wrapper around the core libsignal struct.
#[wasm_bindgen(js_name = ProtocolAddress)]
pub struct ProtocolAddress(CoreProtocolAddress);

// Implement Display for idiomatic Rust `to_string()`.
impl fmt::Display for ProtocolAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[wasm_bindgen(js_class = ProtocolAddress)]
impl ProtocolAddress {
    #[wasm_bindgen(constructor)]
    pub fn new(id: JsValue, device_id: JsValue) -> Result<ProtocolAddress, JsValue> {
        let id_str = id
            .as_string()
            .ok_or_else(|| JsValue::from_str("id required for addr"))?;

        let device_id_num = device_id
            .as_f64()
            .map(|num| num as u32)
            .ok_or_else(|| JsValue::from_str("number required for deviceId"))?;

        // Use wacore-libsignal's validation logic.
        if id_str.contains('.') {
            return Err(JsValue::from_str("encoded addr detected"));
        }

        Ok(ProtocolAddress(CoreProtocolAddress::new(
            id_str,
            DeviceId::from(device_id_num),
        )))
    }

    #[wasm_bindgen(js_name = from)]
    pub fn from_string(encoded: JsValue) -> Result<ProtocolAddress, JsValue> {
        let encoded_str = encoded
            .as_string()
            .ok_or_else(|| JsValue::from_str("Invalid address encoding"))?;

        // This is a simplified parser to pass the tests. `wacore-libsignal` doesn't have a from_str.
        let parts: Vec<&str> = encoded_str.split('.').collect();
        if parts.len() < 2 {
            return Err(JsValue::from_str("Invalid address encoding"));
        }
        let id_str = parts[0].to_string();
        let device_id_num = parts[1]
            .parse::<u32>()
            .map_err(|_| JsValue::from_str("Invalid address encoding"))?;

        Ok(ProtocolAddress(CoreProtocolAddress::new(
            id_str,
            DeviceId::from(device_id_num),
        )))
    }

    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.0.name().to_string()
    }

    // JS expects `deviceId`, so we map the getter.
    #[wasm_bindgen(getter, js_name=deviceId)]
    pub fn device_id(&self) -> u32 {
        self.0.device_id().into()
    }

    #[wasm_bindgen(js_name = toString)]
    pub fn js_to_string(&self) -> String {
        self.0.to_string()
    }

    pub fn is(&self, other: &JsValue) -> bool {
        if other.is_null() || other.is_undefined() || !other.is_object() {
            return false;
        }

        let has_wasm_ptr = Reflect::get(other, &JsValue::from_str("__wbg_ptr"))
            .ok()
            .and_then(|ptr| ptr.as_f64())
            .is_some();
        if !has_wasm_ptr {
            return false;
        }

        let other_id = match Reflect::get(other, &JsValue::from_str("id")) {
            Ok(val) => match val.as_string() {
                Some(s) => s,
                None => return false,
            },
            Err(_) => return false,
        };

        let device_id_val = match Reflect::get(other, &JsValue::from_str("deviceId")) {
            Ok(val) => val,
            Err(_) => return false,
        };

        let other_device_id = match device_id_val.as_f64() {
            Some(num) => num as u32,
            None => return false,
        };

        self.0.name() == other_id && self.device_id() == other_device_id
    }
}

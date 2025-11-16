use js_sys::JsString;
use js_sys::Number;
use std::fmt;
use wacore_libsignal::core::DeviceId;
use wacore_libsignal::core::ProtocolAddress as CoreProtocolAddress;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = ProtocolAddress)]
pub struct ProtocolAddress(pub(crate) CoreProtocolAddress);

impl fmt::Display for ProtocolAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[wasm_bindgen(js_class = ProtocolAddress)]
impl ProtocolAddress {
    #[wasm_bindgen(constructor)]
    pub fn new(id: JsString, device_id: Number) -> Result<ProtocolAddress, JsValue> {
        let id_str = id
            .as_string()
            .ok_or_else(|| JsValue::from_str("id required for addr"))?;

        let device_id_num = device_id
            .as_f64()
            .map(|num| num as u32)
            .ok_or_else(|| JsValue::from_str("number required for deviceId"))?;

        if id_str.contains('.') {
            return Err(JsValue::from_str("encoded addr detected"));
        }

        Ok(ProtocolAddress(CoreProtocolAddress::new(
            id_str,
            DeviceId::from(device_id_num),
        )))
    }

    #[wasm_bindgen(js_name = from)]
    pub fn from_string(encoded: JsString) -> Result<ProtocolAddress, JsValue> {
        let encoded_str = encoded
            .as_string()
            .ok_or_else(|| JsValue::from_str("Invalid address encoding"))?;

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

    #[wasm_bindgen(getter, js_name=deviceId)]
    pub fn device_id(&self) -> u32 {
        self.0.device_id().into()
    }

    #[wasm_bindgen(js_name = toString)]
    pub fn js_to_string(&self) -> String {
        self.0.to_string()
    }

    pub fn is(&self, other: &ProtocolAddress) -> bool {
        self.0.name() == other.0.name() && self.device_id() == other.device_id()
    }
}

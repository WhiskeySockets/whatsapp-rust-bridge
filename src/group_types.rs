use js_sys::Uint8Array;
use wacore_libsignal::protocol::{
    SenderKeyDistributionMessage as CoreSenderKeyDistributionMessage,
    SenderKeyRecord as CoreSenderKeyRecord,
};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

#[wasm_bindgen(js_name = SenderKeyRecord)]
pub struct SenderKeyRecord {
    pub(crate) core: CoreSenderKeyRecord,
}

#[wasm_bindgen(js_class = SenderKeyRecord)]
impl SenderKeyRecord {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    #[wasm_bindgen(js_name = deserialize)]
    pub fn deserialize(serialized: &[u8]) -> Result<SenderKeyRecord, JsValue> {
        let core = CoreSenderKeyRecord::deserialize(serialized)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Self { core })
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsValue> {
        let bytes = self
            .core
            .serialize()
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Uint8Array::from(bytes.as_slice()))
    }

    #[wasm_bindgen(js_name = isEmpty)]
    pub fn is_empty(&self) -> bool {
        // This logic is based on Baileys' SenderKeyRecord
        self.core.sender_key_state().is_err()
    }
}

impl Default for SenderKeyRecord {
    fn default() -> Self {
        Self {
            core: CoreSenderKeyRecord::new_empty(),
        }
    }
}

#[wasm_bindgen(js_name = SenderKeyDistributionMessage)]
pub struct SenderKeyDistributionMessage(pub(crate) CoreSenderKeyDistributionMessage);

#[wasm_bindgen(js_class = SenderKeyDistributionMessage)]
impl SenderKeyDistributionMessage {
    #[wasm_bindgen(js_name = deserialize)]
    pub fn deserialize(serialized: &[u8]) -> Result<SenderKeyDistributionMessage, JsValue> {
        let core = CoreSenderKeyDistributionMessage::try_from(serialized)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Self(core))
    }

    pub fn serialize(&self) -> Uint8Array {
        Uint8Array::from(self.0.serialized())
    }
}

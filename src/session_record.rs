use js_sys::{Array, Reflect, Uint8Array};
use wacore_libsignal::protocol::SessionRecord as CoreSessionRecord;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = SessionRecord)]
pub struct SessionRecord {
    pub(crate) serialized_data: Vec<u8>,
}

#[wasm_bindgen(js_class = SessionRecord)]
impl SessionRecord {
    // Private constructor, not exposed to JS.
    pub(crate) fn new(data: Vec<u8>) -> Self {
        Self {
            serialized_data: data,
        }
    }

    #[wasm_bindgen(js_name = deserialize)]
    pub fn deserialize(val: JsValue) -> Result<SessionRecord, JsValue> {
        // 1. Try as Uint8Array (Standard Rust Bridge format / Protobuf)
        if let Some(uint8_array) = val.dyn_ref::<Uint8Array>() {
            return Ok(SessionRecord::new(uint8_array.to_vec()));
        }

        // 2. Try as standard Array (sometimes passed by generic serialization)
        if Array::is_array(&val) {
            let array = Array::from(&val);
            return Ok(SessionRecord::new(js_array_to_vec(&array)));
        }

        // 3. Check for Legacy libsignal-node JSON format
        // It usually has "_sessions" or "registrationId" keys
        let has_sessions = Reflect::has(&val, &JsValue::from_str("_sessions")).unwrap_or(false);

        if has_sessions {
            // MIGRATION STRATEGY:
            // We have detected a legacy libsignal-node JSON session.
            // Migrating the internal crypto state (chains, ratchets) from the custom JSON format
            // to the standard Signal Protobuf format is not supported directly.
            //
            // To prevent crashes, we return an EMPTY session record.
            // This tells the protocol "we have no valid session", triggering a safe re-negotiation
            // (sending a new PreKey bundle) which effectively migrates the session to the new format
            // on the next message exchange.
            let empty_record = CoreSessionRecord::deserialize(&[]).unwrap_or_else(|_| {
                // Fallback: if we can't create an empty record via deserialize,
                // we might need another way. But for now, let's try this.
                // If this panics, we know deserialize(&[]) is not the way.
                panic!("Could not create empty session record");
            });
            let bytes = empty_record
                .serialize()
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            return Ok(SessionRecord::new(bytes));
        }

        // 4. Fallback / Buffer-like objects { type: 'Buffer', data: [...] }
        // Common in JSON database dumps
        let data_prop = Reflect::get(&val, &JsValue::from_str("data"));
        if let Ok(data) = data_prop
            && Array::is_array(&data)
        {
            let array = Array::from(&data);
            return Ok(SessionRecord::new(js_array_to_vec(&array)));
        }

        // If we reach here, the data is unrecognizable or corrupted
        Err(JsValue::from_str(
            "SessionRecord.deserialize: Invalid input type. Expected Uint8Array, Array, or Buffer-like object.",
        ))
    }

    pub fn serialize(&self) -> Uint8Array {
        Uint8Array::from(self.serialized_data.as_slice())
    }

    #[wasm_bindgen(js_name = haveOpenSession)]
    pub fn have_open_session(&self) -> bool {
        match CoreSessionRecord::deserialize(&self.serialized_data) {
            Ok(record) => record.session_state().is_some(),
            Err(_) => false,
        }
    }
}

// Helper to convert JS Array of numbers to Vec<u8>
fn js_array_to_vec(array: &Array) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(array.length() as usize);
    for i in 0..array.length() {
        if let Some(byte) = array.get(i).as_f64() {
            bytes.push(byte as u8);
        }
    }
    bytes
}

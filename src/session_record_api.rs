use js_sys::Uint8Array;
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
    pub fn deserialize(serialized: &Uint8Array) -> SessionRecord {
        SessionRecord::new(serialized.to_vec())
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

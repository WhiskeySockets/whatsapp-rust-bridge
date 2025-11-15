use js_sys::{Date, Object, Reflect, Uint8Array};
use rand::{TryRngCore, rngs::OsRng};
use std::time::{Duration, UNIX_EPOCH};
use wasm_bindgen::prelude::*;

use crate::{protocol_address::ProtocolAddress, storage_adapter::JsStorageAdapter};
use wacore_libsignal::protocol::{self as libsignal, UsePQRatchet};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Object, typescript_type = "{ type: number; body: Uint8Array }")]
    pub type EncryptResult;
}

#[wasm_bindgen(js_name = SessionCipher)]
pub struct SessionCipher {
    storage_adapter: JsStorageAdapter,
    remote_address: ProtocolAddress,
}

#[wasm_bindgen(js_class = SessionCipher)]
impl SessionCipher {
    #[wasm_bindgen(constructor)]
    pub fn new(storage: JsValue, remote_address: &ProtocolAddress) -> Self {
        Self {
            storage_adapter: JsStorageAdapter {
                js_storage: storage,
            },
            remote_address: ProtocolAddress(remote_address.0.clone()),
        }
    }

    pub async fn encrypt(&mut self, plaintext: &[u8]) -> Result<EncryptResult, JsValue> {
        #[cfg(debug_assertions)]
        console_error_panic_hook::set_once();

        let storage_adapter = JsStorageAdapter {
            js_storage: self.storage_adapter.js_storage.clone(),
        };

        let now_millis = Date::now();
        let now_sys_time = UNIX_EPOCH + Duration::from_millis(now_millis as u64);

        let mut session_store = storage_adapter.clone();
        let mut identity_store = storage_adapter.clone();

        let ciphertext_message = libsignal::message_encrypt(
            plaintext,
            &self.remote_address.0,
            &mut session_store,
            &mut identity_store,
            now_sys_time,
        )
        .await
        .map_err(|e| e.to_string())?;

        let body = ciphertext_message.serialize();
        let type_id = ciphertext_message.message_type() as u8;

        let result = Object::new();
        Reflect::set(&result, &"type".into(), &(type_id as u32).into())?;
        Reflect::set(&result, &"body".into(), &Uint8Array::from(body).into())?;

        Ok(result.unchecked_into())
    }

    #[wasm_bindgen(js_name = decryptPreKeyWhisperMessage)]
    pub async fn decrypt_prekey_whisper_message(
        &mut self,
        ciphertext: &[u8],
    ) -> Result<Uint8Array, JsValue> {
        #[cfg(debug_assertions)]
        console_error_panic_hook::set_once();

        let prekey_message =
            libsignal::PreKeySignalMessage::try_from(ciphertext).map_err(|e| e.to_string())?;

        let storage_adapter = JsStorageAdapter {
            js_storage: self.storage_adapter.js_storage.clone(),
        };

        let mut session_store = storage_adapter.clone();
        let mut identity_store = storage_adapter.clone();
        let mut prekey_store = storage_adapter.clone();
        let signed_prekey_store = storage_adapter;

        let plaintext = libsignal::message_decrypt_prekey(
            &prekey_message,
            &self.remote_address.0,
            &mut session_store,
            &mut identity_store,
            &mut prekey_store,
            &signed_prekey_store,
            &mut OsRng.unwrap_err(),
            UsePQRatchet::No,
        )
        .await
        .map_err(|e| e.to_string())?;

        Ok(Uint8Array::from(plaintext.as_slice()))
    }

    #[wasm_bindgen(js_name = decryptWhisperMessage)]
    pub async fn decrypt_whisper_message(
        &mut self,
        ciphertext: &[u8],
    ) -> Result<Uint8Array, JsValue> {
        #[cfg(debug_assertions)]
        console_error_panic_hook::set_once();

        let storage_adapter = JsStorageAdapter {
            js_storage: self.storage_adapter.js_storage.clone(),
        };

        let signal_message =
            libsignal::SignalMessage::try_from(ciphertext).map_err(|e| e.to_string())?;

        let mut session_store = storage_adapter.clone();
        let mut identity_store = storage_adapter.clone();

        let plaintext = libsignal::message_decrypt_signal(
            &signal_message,
            &self.remote_address.0,
            &mut session_store,
            &mut identity_store,
            &mut OsRng.unwrap_err(),
        )
        .await
        .map_err(|e| e.to_string())?;

        Ok(Uint8Array::from(plaintext.as_slice()))
    }
}

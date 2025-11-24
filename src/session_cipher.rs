use js_sys::{Object, Reflect, Uint8Array};
use rand::{TryRngCore, rngs::OsRng};
use wasm_bindgen::prelude::*;

use crate::{
    protocol_address::ProtocolAddress,
    storage_adapter::{JsStorageAdapter, SignalStorage},
};
use wacore_libsignal::protocol::{self as libsignal, SessionStore, UsePQRatchet};

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
    pub fn new(storage: SignalStorage, remote_address: &ProtocolAddress) -> Self {
        Self {
            storage_adapter: JsStorageAdapter::new(storage.into()),
            remote_address: ProtocolAddress(remote_address.0.clone()),
        }
    }

    pub async fn encrypt(&mut self, plaintext: &[u8]) -> Result<EncryptResult, JsValue> {
        #[cfg(debug_assertions)]
        console_error_panic_hook::set_once();

        let mut session_store = self.storage_adapter.clone();
        let mut identity_store = session_store.clone();

        let ciphertext_message = libsignal::message_encrypt(
            plaintext,
            &self.remote_address.0,
            &mut session_store,
            &mut identity_store,
        )
        .await
        .map_err(|e| {
            let msg = format!("SessionCipher.encrypt error: {:?}", e);
            JsValue::from_str(&msg)
        })?;

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

        let prekey_message = libsignal::PreKeySignalMessage::try_from(ciphertext)
            .map_err(|e| {
                let msg = format!("SessionCipher.decryptPreKeyWhisperMessage failed: Invalid PreKeyMessage format: {}", e);
                JsValue::from_str(&msg)
            })?;

        let mut session_store = self.storage_adapter.clone();
        let mut identity_store = session_store.clone();
        let mut prekey_store = session_store.clone();
        let signed_prekey_store = session_store.clone();

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
        .map_err(|e| {
            let msg = format!("SessionCipher.decryptPreKeyWhisperMessage failed: {:?}", e);
            JsValue::from_str(&msg)
        })?;

        Ok(Uint8Array::from(plaintext.as_slice()))
    }

    #[wasm_bindgen(js_name = decryptWhisperMessage)]
    pub async fn decrypt_whisper_message(
        &mut self,
        ciphertext: &[u8],
    ) -> Result<Uint8Array, JsValue> {
        #[cfg(debug_assertions)]
        console_error_panic_hook::set_once();

        let signal_message = libsignal::SignalMessage::try_from(ciphertext).map_err(|e| {
            let msg = format!(
                "SessionCipher.decryptWhisperMessage failed: Invalid WhisperMessage format: {}",
                e
            );
            JsValue::from_str(&msg)
        })?;

        let mut session_store = self.storage_adapter.clone();
        let mut identity_store = session_store.clone();

        let plaintext = libsignal::message_decrypt_signal(
            &signal_message,
            &self.remote_address.0,
            &mut session_store,
            &mut identity_store,
            &mut OsRng.unwrap_err(),
        )
        .await
        .map_err(|e| {
            let msg = format!("SessionCipher.decryptWhisperMessage failed: {:?}", e);
            JsValue::from_str(&msg)
        })?;

        Ok(Uint8Array::from(plaintext.as_slice()))
    }

    #[wasm_bindgen(js_name = hasOpenSession)]
    pub async fn has_open_session(&self) -> Result<bool, JsValue> {
        let record = self
            .storage_adapter
            .load_session(&self.remote_address.0)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        match record {
            Some(r) => Ok(r.session_state().is_some()),
            None => Ok(false),
        }
    }
}

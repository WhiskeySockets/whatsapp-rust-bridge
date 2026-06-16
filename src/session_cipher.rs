use js_sys::{Object, Reflect, Uint8Array};
use rand::rngs::StdRng;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

use crate::{
    protocol_address::ProtocolAddress,
    storage_adapter::{JsStorageAdapter, SignalStorage},
};
use wacore_libsignal::protocol::{self as libsignal, PreKeyStore, SessionStore, UsePQRatchet};

#[inline]
fn bytes_to_uint8array(bytes: &[u8]) -> Uint8Array {
    Uint8Array::from(bytes)
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Object, typescript_type = "{ type: number; body: Uint8Array }")]
    pub type EncryptResult;
}

thread_local! {
    static TYPE_KEY: RefCell<JsValue> = RefCell::new(JsValue::from_str("type"));
    static BODY_KEY: RefCell<JsValue> = RefCell::new(JsValue::from_str("body"));
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
            storage_adapter: JsStorageAdapter::new(storage),
            remote_address: ProtocolAddress(remote_address.0.clone()),
        }
    }

    pub async fn encrypt(&mut self, plaintext: &[u8]) -> Result<EncryptResult, JsValue> {
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

        let body_array = bytes_to_uint8array(ciphertext_message.serialize());
        let type_id = ciphertext_message.message_type() as u8;

        let result = Object::new();
        TYPE_KEY.with(|k| Reflect::set(&result, &k.borrow(), &(type_id as u32).into()))?;
        BODY_KEY.with(|k| Reflect::set(&result, &k.borrow(), &body_array.into()))?;

        Ok(result.unchecked_into())
    }

    #[wasm_bindgen(js_name = decryptPreKeyWhisperMessage)]
    pub async fn decrypt_prekey_whisper_message(
        &mut self,
        ciphertext: &[u8],
    ) -> Result<Uint8Array, JsValue> {
        let prekey_message = libsignal::PreKeySignalMessage::try_from(ciphertext)
            .map_err(|e| {
                let msg = format!("SessionCipher.decryptPreKeyWhisperMessage failed: Invalid PreKeyMessage format: {}", e);
                JsValue::from_str(&msg)
            })?;

        // Snapshot pre-decrypt state for skipped-key seed capture (cheap; reused
        // session only — a pkmsg that creates a fresh session has no chain yet).
        let inner = prekey_message.message();
        let skip_snapshot = self
            .storage_adapter
            .snapshot_skip(
                &self.remote_address.0,
                inner.sender_ratchet_key(),
                inner.counter(),
            )
            .await;

        let mut session_store = self.storage_adapter.clone();
        let mut identity_store = session_store.clone();
        let mut prekey_store = session_store.clone();
        let signed_prekey_store = session_store.clone();

        let result = libsignal::message_decrypt_prekey(
            &prekey_message,
            &self.remote_address.0,
            &mut session_store,
            &mut identity_store,
            &mut prekey_store,
            &signed_prekey_store,
            &mut rand::make_rng::<StdRng>(),
            UsePQRatchet::No,
        )
        .await
        .map_err(|e| {
            let msg = format!("SessionCipher.decryptPreKeyWhisperMessage failed: {:?}", e);
            JsValue::from_str(&msg)
        })?;

        // The promoted session is already durable (message_decrypt_prekey stored it
        // through to JS), so it's now safe to remove the consumed one-time prekey —
        // the v0.6 API reports it instead of deleting it internally.
        if let Some(prekey_id) = result.consumed_prekey_id {
            prekey_store
                .remove_pre_key(prekey_id)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
        }

        // Message authenticated → safe to compute the skipped seeds and rewrite
        // the JSON wacore just stored (without them) so they survive a revert.
        let mut captured = false;
        for snapshot in skip_snapshot {
            captured |= self
                .storage_adapter
                .commit_skip_snapshot(&self.remote_address.0, snapshot);
        }
        if captured {
            self.storage_adapter
                .repersist_session_json(&self.remote_address.0)
                .await;
        }

        Ok(bytes_to_uint8array(&result.plaintext))
    }

    #[wasm_bindgen(js_name = decryptWhisperMessage)]
    pub async fn decrypt_whisper_message(
        &mut self,
        ciphertext: &[u8],
    ) -> Result<Uint8Array, JsValue> {
        let signal_message = libsignal::SignalMessage::try_from(ciphertext).map_err(|e| {
            let msg = format!(
                "SessionCipher.decryptWhisperMessage failed: Invalid WhisperMessage format: {}",
                e
            );
            JsValue::from_str(&msg)
        })?;

        // Phase 1 (cheap, pre-decrypt): snapshot the state needed to compute the
        // seeds for keys this decrypt will skip, before wacore advances the chain.
        let skip_snapshot = self
            .storage_adapter
            .snapshot_skip(
                &self.remote_address.0,
                signal_message.sender_ratchet_key(),
                signal_message.counter(),
            )
            .await;

        let mut session_store = self.storage_adapter.clone();
        let mut identity_store = session_store.clone();

        let result = libsignal::message_decrypt_signal(
            &signal_message,
            &self.remote_address.0,
            &mut session_store,
            &mut identity_store,
            &mut rand::make_rng::<StdRng>(),
        )
        .await
        .map_err(|e| {
            let msg = format!("SessionCipher.decryptWhisperMessage failed: {:?}", e);
            JsValue::from_str(&msg)
        })?;

        // Phase 2 (post-auth): now that the MAC checked out, derive the skipped
        // seeds and rewrite the JSON wacore stored without them.
        let mut captured = false;
        for snapshot in skip_snapshot {
            captured |= self
                .storage_adapter
                .commit_skip_snapshot(&self.remote_address.0, snapshot);
        }
        if captured {
            self.storage_adapter
                .repersist_session_json(&self.remote_address.0)
                .await;
        }

        Ok(bytes_to_uint8array(&result.plaintext))
    }

    #[wasm_bindgen(js_name = hasOpenSession)]
    pub async fn has_open_session(&self) -> Result<bool, JsValue> {
        let record = SessionStore::load_session(&self.storage_adapter, &self.remote_address.0)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        match record {
            Some(r) => Ok(r.session_state().is_some()),
            None => Ok(false),
        }
    }
}

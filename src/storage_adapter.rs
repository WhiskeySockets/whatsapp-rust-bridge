use async_trait::async_trait;
use js_sys::{Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use wacore_libsignal::protocol::{
    self as libsignal, Direction as StoreDirection, GenericSignedPreKey as _, IdentityChange,
    IdentityKeyPair, IdentityKeyStore, PreKeyId, PreKeyRecord, PreKeyStore, SessionStore,
    SignedPreKeyId, SignedPreKeyRecord, SignedPreKeyStore,
};
type SignalResult<T> = wacore_libsignal::protocol::error::Result<T>;

use wacore_libsignal::protocol::SessionRecord as CoreSessionRecord;
use wacore_libsignal::protocol::SignalProtocolError;

#[wasm_bindgen(module = "/ts/libsignal_storage_adapter.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = isTrustedIdentity)]
    fn is_trusted_identity(
        storage: &JsValue,
        address: String,
        identity_key: &[u8],
        direction: u32,
    ) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = loadSession)]
    fn load_session(storage: &JsValue, address: String) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = storeSession)]
    fn store_session(
        storage: &JsValue,
        address: String,
        record_data: &[u8],
    ) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = getIdentityKeyPair)]
    fn get_identity_key_pair(storage: &JsValue) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = getLocalRegistrationId)]
    fn get_local_registration_id(storage: &JsValue) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = saveIdentity)]
    fn save_identity(
        storage: &JsValue,
        address: String,
        identity_key: &[u8],
    ) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = loadPreKey)]
    fn load_pre_key(storage: &JsValue, prekey_id: u32) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = removePreKey)]
    fn remove_pre_key(storage: &JsValue, prekey_id: u32) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = loadSignedPreKey)]
    fn load_signed_pre_key(storage: &JsValue, signed_prekey_id: u32) -> Result<Promise, JsValue>;
}

#[derive(Clone)]
pub struct JsStorageAdapter {
    pub js_storage: JsValue,
}

fn js_to_signal_error(e: JsValue) -> libsignal::SignalProtocolError {
    libsignal::SignalProtocolError::FfiBindingError(format!("{:?}", e))
}

macro_rules! js_promise_to_bytes {
    ($promise:expr) => {{
        let value = JsFuture::from($promise).await.map_err(js_to_signal_error)?;
        if value.is_null() || value.is_undefined() {
            Ok::<Option<Vec<u8>>, libsignal::SignalProtocolError>(None)
        } else {
            let arr: Uint8Array = value.dyn_into().map_err(|e| js_to_signal_error(e.into()))?;
            Ok::<Option<Vec<u8>>, libsignal::SignalProtocolError>(Some(arr.to_vec()))
        }
    }};
}

#[async_trait(?Send)]
impl SessionStore for JsStorageAdapter {
    async fn load_session(
        &self,
        address: &libsignal::ProtocolAddress,
    ) -> SignalResult<Option<CoreSessionRecord>> {
        console_error_panic_hook::set_once();
        let promise =
            load_session(&self.js_storage, address.to_string()).map_err(js_to_signal_error)?;
        let bytes = js_promise_to_bytes!(promise)?;
        match bytes {
            Some(data) => Ok(Some(CoreSessionRecord::deserialize(&data)?)),
            None => Ok(None),
        }
    }

    async fn store_session(
        &mut self,
        address: &libsignal::ProtocolAddress,
        record: &CoreSessionRecord,
    ) -> SignalResult<()> {
        console_error_panic_hook::set_once();
        let bytes = record.serialize()?;
        let promise = store_session(&self.js_storage, address.to_string(), &bytes)
            .map_err(js_to_signal_error)?;
        JsFuture::from(promise).await.map_err(js_to_signal_error)?;
        Ok(())
    }
}

#[async_trait(?Send)]
impl IdentityKeyStore for JsStorageAdapter {
    async fn get_identity_key_pair(&self) -> SignalResult<IdentityKeyPair> {
        let promise = get_identity_key_pair(&self.js_storage).map_err(js_to_signal_error)?;
        let bytes = js_promise_to_bytes!(promise)?;
        IdentityKeyPair::try_from(
            bytes
                .ok_or_else(|| {
                    SignalProtocolError::InvalidState(
                        "get_identity_key_pair",
                        "JS returned null".into(),
                    )
                })?
                .as_slice(),
        )
    }
    async fn get_local_registration_id(&self) -> SignalResult<u32> {
        let promise = get_local_registration_id(&self.js_storage).map_err(js_to_signal_error)?;
        let result = JsFuture::from(promise).await.map_err(js_to_signal_error)?;
        Ok(result.as_f64().ok_or_else(|| {
            SignalProtocolError::InvalidState(
                "get_local_registration_id",
                "JS did not return a number".into(),
            )
        })? as u32)
    }
    async fn is_trusted_identity(
        &self,
        address: &libsignal::ProtocolAddress,
        identity: &libsignal::IdentityKey,
        direction: StoreDirection,
    ) -> SignalResult<bool> {
        let direction_val = match direction {
            StoreDirection::Sending => 0,
            StoreDirection::Receiving => 1,
        };

        let promise = is_trusted_identity(
            &self.js_storage,
            address.name().to_string(),
            &identity.serialize(),
            direction_val,
        )
        .map_err(js_to_signal_error)?;
        let result = JsFuture::from(promise).await.map_err(js_to_signal_error)?;
        Ok(result.as_bool().unwrap_or(false))
    }

    async fn save_identity(
        &mut self,
        address: &libsignal::ProtocolAddress,
        identity: &libsignal::IdentityKey,
    ) -> SignalResult<IdentityChange> {
        let promise = save_identity(
            &self.js_storage,
            address.name().to_string(),
            &identity.serialize(),
        )
        .map_err(js_to_signal_error)?;
        let result = JsFuture::from(promise).await.map_err(js_to_signal_error)?;
        let changed = result.as_bool().unwrap_or(false);
        Ok(IdentityChange::from_changed(changed))
    }
    async fn get_identity(
        &self,
        _address: &libsignal::ProtocolAddress,
    ) -> SignalResult<Option<libsignal::IdentityKey>> {
        Ok(None)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl PreKeyStore for JsStorageAdapter {
    async fn get_pre_key(&self, prekey_id: PreKeyId) -> SignalResult<PreKeyRecord> {
        let promise =
            load_pre_key(&self.js_storage, prekey_id.into()).map_err(js_to_signal_error)?;
        let bytes = js_promise_to_bytes!(promise)?; // Re-using our macro
        PreKeyRecord::deserialize(&bytes.ok_or(SignalProtocolError::InvalidPreKeyId)?)
    }

    async fn save_pre_key(
        &mut self,
        _prekey_id: PreKeyId,
        _record: &PreKeyRecord,
    ) -> SignalResult<()> {
        Ok(())
    }

    async fn remove_pre_key(&mut self, prekey_id: PreKeyId) -> SignalResult<()> {
        let promise =
            remove_pre_key(&self.js_storage, prekey_id.into()).map_err(js_to_signal_error)?;
        JsFuture::from(promise).await.map_err(js_to_signal_error)?;
        Ok(())
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl SignedPreKeyStore for JsStorageAdapter {
    async fn get_signed_pre_key(
        &self,
        signed_prekey_id: SignedPreKeyId,
    ) -> SignalResult<SignedPreKeyRecord> {
        let promise = load_signed_pre_key(&self.js_storage, signed_prekey_id.into())
            .map_err(js_to_signal_error)?;
        let bytes = js_promise_to_bytes!(promise)?;
        SignedPreKeyRecord::deserialize(&bytes.ok_or(SignalProtocolError::InvalidSignedPreKeyId)?)
    }

    async fn save_signed_pre_key(
        &mut self,
        _id: SignedPreKeyId,
        _record: &SignedPreKeyRecord,
    ) -> SignalResult<()> {
        Ok(())
    }
}

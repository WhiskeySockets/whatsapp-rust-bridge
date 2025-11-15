use async_trait::async_trait;
use js_sys::{Date, Promise, Uint8Array};
use rand::TryRngCore;
use rand::rngs::OsRng;
use serde::Deserialize;
use std::time::{Duration, UNIX_EPOCH};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::protocol_address::ProtocolAddress;
use crate::session_record_api::SessionRecord;
use wacore_libsignal::core::curve::PublicKey as CorePublicKey;
use wacore_libsignal::protocol::{
    self as libsignal, Direction as StoreDirection, IdentityChange, IdentityKeyPair,
    IdentityKeyStore, PreKeyBundle, SessionStore, UsePQRatchet,
};
type SignalResult<T> = wacore_libsignal::protocol::error::Result<T>;

use wacore_libsignal::protocol::SessionRecord as CoreSessionRecord;
use wacore_libsignal::protocol::SignalProtocolError;

impl SessionRecord {
    pub fn from_core(core_record: &CoreSessionRecord) -> Result<Self, SignalProtocolError> {
        Ok(Self::new(core_record.serialize()?))
    }

    pub fn to_core(&self) -> Result<CoreSessionRecord, SignalProtocolError> {
        CoreSessionRecord::deserialize(&self.serialized_data)
    }
}

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
}

#[derive(Clone)]
struct JsStorageAdapter {
    js_storage: JsValue,
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

#[wasm_bindgen(js_name = SessionBuilder)]
pub struct SessionBuilder {
    storage: JsValue,
    remote_address: ProtocolAddress,
}

#[wasm_bindgen(js_class = SessionBuilder)]
impl SessionBuilder {
    #[wasm_bindgen(constructor)]
    pub fn new(storage: JsValue, remote_address: &ProtocolAddress) -> Self {
        Self {
            storage,
            // We need to clone the inner data, as ProtocolAddress is passed by reference
            remote_address: ProtocolAddress(remote_address.0.clone()),
        }
    }

    #[wasm_bindgen(js_name = processPreKeyBundle)]
    pub async fn process_prekey_bundle(&mut self, bundle_val: JsValue) -> Result<(), JsValue> {
        console_error_panic_hook::set_once();

        let storage_adapter = JsStorageAdapter {
            js_storage: self.storage.clone(),
        };

        let js_bundle: JsPreKeyBundle = serde_wasm_bindgen::from_value(bundle_val)?;

        let pre_key = match js_bundle.pre_key {
            Some(pk) => {
                let key = CorePublicKey::deserialize(&pk.public_key).map_err(|e| e.to_string())?;
                Some((pk.key_id.into(), key))
            }
            None => None,
        };
        let signed_pre_key_public =
            CorePublicKey::deserialize(&js_bundle.signed_pre_key.public_key)
                .map_err(|e| e.to_string())?;
        let identity_key =
            libsignal::IdentityKey::decode(&js_bundle.identity_key).map_err(|e| e.to_string())?;

        let bundle = PreKeyBundle::new(
            js_bundle.registration_id,
            self.remote_address.0.device_id(),
            pre_key,
            js_bundle.signed_pre_key.key_id.into(),
            signed_pre_key_public,
            js_bundle.signed_pre_key.signature,
            identity_key,
        )
        .map_err(|e| e.to_string())?;

        let mut session_store = storage_adapter.clone();
        let mut identity_store = storage_adapter.clone();

        let now_millis = Date::now();
        let now_sys_time = UNIX_EPOCH + Duration::from_millis(now_millis as u64);

        libsignal::process_prekey_bundle(
            &self.remote_address.0,
            &mut session_store,
            &mut identity_store,
            &bundle,
            now_sys_time,
            &mut OsRng.unwrap_err(),
            UsePQRatchet::No,
        )
        .await
        .map_err(|e| e.to_string())?;

        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsPreKeyBundle {
    identity_key: Vec<u8>,
    registration_id: u32,
    pre_key: Option<JsPreKey>,
    signed_pre_key: JsSignedPreKey,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsPreKey {
    key_id: u32,
    public_key: Vec<u8>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsSignedPreKey {
    key_id: u32,
    public_key: Vec<u8>,
    signature: Vec<u8>,
}

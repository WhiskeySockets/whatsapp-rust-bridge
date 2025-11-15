use js_sys::Date;
use rand::TryRngCore;
use rand::rngs::OsRng;
use serde::Deserialize;
use std::time::{Duration, UNIX_EPOCH};
use wasm_bindgen::prelude::*;

use crate::protocol_address::ProtocolAddress;
use crate::session_record::SessionRecord;
use crate::storage_adapter::JsStorageAdapter;
use wacore_libsignal::core::curve::PublicKey as CorePublicKey;
use wacore_libsignal::protocol::{self as libsignal, PreKeyBundle, UsePQRatchet};

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

#[wasm_bindgen(js_name = SessionBuilder)]
pub struct SessionBuilder {
    storage_adapter: JsStorageAdapter,
    remote_address: ProtocolAddress,
}

#[wasm_bindgen(js_class = SessionBuilder)]
impl SessionBuilder {
    #[wasm_bindgen(constructor)]
    pub fn new(storage: JsValue, remote_address: &ProtocolAddress) -> Self {
        Self {
            storage_adapter: JsStorageAdapter::new(storage),
            // We need to clone the inner data, as ProtocolAddress is passed by reference
            remote_address: ProtocolAddress(remote_address.0.clone()),
        }
    }

    #[wasm_bindgen(js_name = processPreKeyBundle)]
    pub async fn process_prekey_bundle(&mut self, bundle_val: JsValue) -> Result<(), JsValue> {
        console_error_panic_hook::set_once();

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

        let mut session_store = self.storage_adapter.clone();
        let mut identity_store = session_store.clone();

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

    #[wasm_bindgen(js_name = initOutgoing)]
    pub async fn init_outgoing(&mut self, bundle_val: JsValue) -> Result<(), JsValue> {
        self.process_prekey_bundle(bundle_val).await
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

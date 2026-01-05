use rand::TryRngCore;
use rand::rngs::OsRng;
use serde::Deserialize;
use tsify_next::Tsify;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;

use crate::protocol_address::ProtocolAddress;
use crate::session_record::SessionRecord;
use crate::storage_adapter::{JsStorageAdapter, SignalStorage};
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

/// Public key for a pre-key
#[derive(Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PreKeyPublicKey {
    pub key_id: u32,
    #[tsify(type = "Uint8Array")]
    pub public_key: Vec<u8>,
}

/// Signed pre-key with signature
#[derive(Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct SignedPreKeyPublicKey {
    pub key_id: u32,
    #[tsify(type = "Uint8Array")]
    pub public_key: Vec<u8>,
    #[tsify(type = "Uint8Array")]
    pub signature: Vec<u8>,
}

/// Input bundle for establishing a session
#[derive(Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PreKeyBundleInput {
    pub registration_id: u32,
    #[tsify(type = "Uint8Array")]
    pub identity_key: Vec<u8>,
    #[serde(default)]
    pub pre_key: Option<PreKeyPublicKey>,
    pub signed_pre_key: SignedPreKeyPublicKey,
}

#[wasm_bindgen(js_name = SessionBuilder)]
pub struct SessionBuilder {
    storage_adapter: JsStorageAdapter,
    remote_address: ProtocolAddress,
}

#[wasm_bindgen(js_class = SessionBuilder)]
impl SessionBuilder {
    #[wasm_bindgen(constructor)]
    pub fn new(storage: SignalStorage, remote_address: &ProtocolAddress) -> Self {
        Self {
            storage_adapter: JsStorageAdapter::new(storage),
            remote_address: ProtocolAddress(remote_address.0.clone()),
        }
    }

    #[wasm_bindgen(js_name = processPreKeyBundle)]
    pub async fn process_prekey_bundle(
        &mut self,
        bundle_input: PreKeyBundleInput,
    ) -> Result<(), JsValue> {
        let pre_key = match bundle_input.pre_key {
            Some(pk) => {
                let key = CorePublicKey::deserialize(&pk.public_key).map_err(|e| e.to_string())?;
                Some((pk.key_id.into(), key))
            }
            None => None,
        };
        let signed_pre_key_public =
            CorePublicKey::deserialize(&bundle_input.signed_pre_key.public_key)
                .map_err(|e| e.to_string())?;
        let identity_key = libsignal::IdentityKey::decode(&bundle_input.identity_key)
            .map_err(|e| e.to_string())?;

        let bundle = PreKeyBundle::new(
            bundle_input.registration_id,
            self.remote_address.0.device_id(),
            pre_key,
            bundle_input.signed_pre_key.key_id.into(),
            signed_pre_key_public,
            bundle_input.signed_pre_key.signature,
            identity_key,
        )
        .map_err(|e| e.to_string())?;

        let mut session_store = self.storage_adapter.clone();
        let mut identity_store = session_store.clone();

        libsignal::process_prekey_bundle(
            &self.remote_address.0,
            &mut session_store,
            &mut identity_store,
            &bundle,
            &mut OsRng.unwrap_err(),
            UsePQRatchet::No,
        )
        .await
        .map_err(|e| e.to_string())?;

        Ok(())
    }

    #[wasm_bindgen(js_name = initOutgoing)]
    pub async fn init_outgoing(&mut self, bundle_input: PreKeyBundleInput) -> Result<(), JsValue> {
        self.process_prekey_bundle(bundle_input).await
    }
}

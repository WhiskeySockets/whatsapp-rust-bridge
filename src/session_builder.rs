use rand::TryRngCore;
use rand::rngs::OsRng;
use serde::Deserialize;
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

#[wasm_bindgen(typescript_custom_section)]
const PREKEY_BUNDLE_TS: &str = r#"
export interface PreKeyPublicKey {
    keyId: number;
    publicKey: Uint8Array;
}

export interface SignedPreKeyPublicKey extends PreKeyPublicKey {
    signature: Uint8Array;
}

export interface PreKeyBundleInput {
    registrationId: number;
    identityKey: Uint8Array;
    preKey?: PreKeyPublicKey;
    signedPreKey: SignedPreKeyPublicKey;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "PreKeyBundleInput")]
    pub type PreKeyBundleInput;
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
        bundle_val: PreKeyBundleInput,
    ) -> Result<(), JsValue> {
        console_error_panic_hook::set_once();

        let js_value = JsValue::from(bundle_val);
        let js_bundle: JsPreKeyBundle = serde_wasm_bindgen::from_value(js_value)?;

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
    pub async fn init_outgoing(&mut self, bundle_val: PreKeyBundleInput) -> Result<(), JsValue> {
        let address_str = self.remote_address.0.to_string();
        let existing_session = self.storage_adapter.load_session(&address_str).await;

        if let Ok(Some(session_bytes)) = existing_session
            && let Ok(record) = CoreSessionRecord::deserialize(&session_bytes)
            && record.has_usable_sender_chain().unwrap_or(false)
        {
            log::debug!(
                "initOutgoing: Session already exists for {}, skipping injection",
                address_str
            );
            return Ok(());
        }

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

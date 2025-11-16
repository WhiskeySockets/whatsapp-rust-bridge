use async_trait::async_trait;
use js_sys::{Promise, Uint8Array};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_bytes::ByteBuf;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use wacore_libsignal::protocol::{
    self as libsignal, Direction as StoreDirection, GenericSignedPreKey as _, IdentityChange,
    IdentityKey, IdentityKeyPair, IdentityKeyStore, KeyPair, PreKeyId, PreKeyRecord, PreKeyStore,
    PrivateKey, SenderKeyStore, SessionStore, SignedPreKeyId, SignedPreKeyRecord,
    SignedPreKeyStore,
};
type SignalResult<T> = wacore_libsignal::protocol::error::Result<T>;

use wacore_libsignal::protocol::SessionRecord as CoreSessionRecord;
use wacore_libsignal::protocol::SignalProtocolError;

use wacore_libsignal::protocol::SenderKeyRecord as CoreSenderKeyRecord;
use wacore_libsignal::protocol::Timestamp;
use wacore_libsignal::store::sender_key_name::SenderKeyName as CoreSenderKeyName;

#[wasm_bindgen(typescript_custom_section)]
const SIGNAL_STORAGE_TS: &str = r#"
export type MaybePromise<T> = T | Promise<T>;

export interface SignalStorage {
    loadSession(address: string): MaybePromise<Uint8Array | null | undefined>;
    storeSession(address: string, record: SessionRecord): MaybePromise<void>;
    getOurIdentity(): MaybePromise<KeyPairType>;
    getOurRegistrationId(): MaybePromise<number>;
    isTrustedIdentity(name: string, identityKey: Uint8Array, direction: number): MaybePromise<boolean>;
    loadPreKey(id: number): MaybePromise<KeyPairType | null | undefined>;
    removePreKey(id: number): MaybePromise<void>;
    loadSignedPreKey(id: number): MaybePromise<SignedPreKeyType | null | undefined>;
    loadSenderKey(keyId: string): MaybePromise<Uint8Array | null | undefined>;
    storeSenderKey(keyId: string, record: Uint8Array): MaybePromise<void>;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "SignalStorage")]
    pub type SignalStorage;
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
    #[wasm_bindgen(catch, js_name = loadPreKey)]
    fn load_pre_key(storage: &JsValue, prekey_id: u32) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = removePreKey)]
    fn remove_pre_key(storage: &JsValue, prekey_id: u32) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = loadSignedPreKey)]
    fn load_signed_pre_key(storage: &JsValue, signed_prekey_id: u32) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = loadSenderKey)]
    fn load_sender_key(storage: &JsValue, key_id: String) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = storeSenderKey)]
    fn store_sender_key(
        storage: &JsValue,
        key_id: String,
        record_data: &[u8],
    ) -> Result<Promise, JsValue>;
}

#[derive(Clone)]
pub struct JsStorageAdapter {
    pub js_storage: JsValue,
    cached_identity_key_pair: Rc<RefCell<Option<IdentityKeyPair>>>,
    cached_registration_id: Rc<RefCell<Option<u32>>>,
    cached_sessions: Rc<RefCell<HashMap<String, CoreSessionRecord>>>,
    cached_sender_keys: Rc<RefCell<HashMap<String, CoreSenderKeyRecord>>>,
}

impl JsStorageAdapter {
    pub fn new(js_storage: JsValue) -> Self {
        Self {
            js_storage,
            cached_identity_key_pair: Rc::new(RefCell::new(None)),
            cached_registration_id: Rc::new(RefCell::new(None)),
            cached_sessions: Rc::new(RefCell::new(HashMap::new())),
            cached_sender_keys: Rc::new(RefCell::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsKeyPairBytes {
    #[serde(default, alias = "pubKey", alias = "publicKey", alias = "public")]
    public_key: Option<ByteBuf>,
    #[serde(default, alias = "privKey", alias = "privateKey", alias = "private")]
    private_key: Option<ByteBuf>,
}

impl JsKeyPairBytes {
    fn into_vecs(self) -> Option<(Vec<u8>, Vec<u8>)> {
        match (self.public_key, self.private_key) {
            (Some(public_key), Some(private_key)) => {
                Some((public_key.into_vec(), private_key.into_vec()))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsKeyEnvelope {
    #[serde(flatten)]
    inline: JsKeyPairBytes,
    #[serde(default, rename = "keyPair", alias = "key_pair")]
    key_pair: Option<JsKeyPairBytes>,
}

impl JsKeyEnvelope {
    fn into_vecs(self) -> Option<(Vec<u8>, Vec<u8>)> {
        self.inline
            .into_vecs()
            .or_else(|| self.key_pair.and_then(|pair| pair.into_vecs()))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsPreKeyRecordPayload {
    #[serde(default, alias = "preKeyId", alias = "keyId")]
    id: Option<u32>,
    #[serde(flatten)]
    keys: JsKeyEnvelope,
}

impl JsPreKeyRecordPayload {
    fn into_record(self, requested_id: PreKeyId) -> SignalResult<PreKeyRecord> {
        let effective_id = self.id.unwrap_or_else(|| requested_id.into());
        let (public_key, private_key) = self
            .keys
            .into_vecs()
            .ok_or_else(|| invalid_js_data("load_pre_key", "Missing public/private key bytes"))?;

        let normalized_public_key = ensure_curve_key_with_prefix(public_key);
        let key_pair = KeyPair::from_public_and_private(&normalized_public_key, &private_key)?;
        Ok(PreKeyRecord::new(PreKeyId::from(effective_id), &key_pair))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsSignedPreKeyRecordPayload {
    #[serde(default, alias = "keyId")]
    id: Option<u32>,
    #[serde(default)]
    timestamp: Option<u64>,
    #[serde(default, alias = "sig", alias = "signatureBytes")]
    signature: Option<ByteBuf>,
    #[serde(flatten)]
    keys: JsKeyEnvelope,
}

impl JsSignedPreKeyRecordPayload {
    fn into_record(self, requested_id: SignedPreKeyId) -> SignalResult<SignedPreKeyRecord> {
        let effective_id = self.id.unwrap_or_else(|| requested_id.into());
        let (public_key, private_key) = self.keys.into_vecs().ok_or_else(|| {
            invalid_js_data("load_signed_pre_key", "Missing public/private key bytes")
        })?;
        let signature = self
            .signature
            .map(ByteBuf::into_vec)
            .ok_or_else(|| invalid_js_data("load_signed_pre_key", "Missing signature bytes"))?;
        let timestamp_ms = self.timestamp.unwrap_or(0);
        let normalized_public_key = ensure_curve_key_with_prefix(public_key);
        let key_pair = KeyPair::from_public_and_private(&normalized_public_key, &private_key)?;
        let timestamp = Timestamp::from_epoch_millis(timestamp_ms);
        Ok(SignedPreKeyRecord::new(
            SignedPreKeyId::from(effective_id),
            timestamp,
            &key_pair,
            &signature,
        ))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsIdentityKeyPairPayload {
    #[serde(flatten)]
    keys: JsKeyEnvelope,
}

impl JsIdentityKeyPairPayload {
    fn into_pair(self) -> SignalResult<IdentityKeyPair> {
        let (public_key, private_key) = self.keys.into_vecs().ok_or_else(|| {
            invalid_js_data("get_identity_key_pair", "Missing public/private key bytes")
        })?;
        let normalized_public_key = ensure_curve_key_with_prefix(public_key);
        let identity_key = IdentityKey::try_from(normalized_public_key.as_slice())?;
        let private_key = PrivateKey::deserialize(&private_key)?;
        Ok(IdentityKeyPair::new(identity_key, private_key))
    }
}

fn invalid_js_data(context: &'static str, message: impl Into<String>) -> SignalProtocolError {
    SignalProtocolError::InvalidState(context, message.into())
}

fn ensure_curve_key_with_prefix(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() == 33 && bytes.first().copied() == Some(0x05) {
        return bytes;
    }

    if bytes.len() == 32 {
        let mut prefixed = Vec::with_capacity(33);
        prefixed.push(0x05);
        prefixed.extend_from_slice(&bytes);
        return prefixed;
    }

    bytes
}

fn js_to_signal_error(e: JsValue) -> libsignal::SignalProtocolError {
    libsignal::SignalProtocolError::FfiBindingError(format!("{:?}", e))
}

async fn promise_to_option_value(promise: Promise) -> SignalResult<Option<JsValue>> {
    let value = JsFuture::from(promise).await.map_err(js_to_signal_error)?;
    if value.is_null() || value.is_undefined() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn deserialize_js_value<T: DeserializeOwned>(
    value: JsValue,
    context: &'static str,
) -> SignalResult<T> {
    serde_wasm_bindgen::from_value(value).map_err(|err| invalid_js_data(context, err.to_string()))
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

        let address_str = address.to_string();

        if let Some(record) = self.cached_sessions.borrow().get(&address_str) {
            return Ok(Some(record.clone()));
        }

        let promise =
            load_session(&self.js_storage, address.to_string()).map_err(js_to_signal_error)?;
        let bytes = js_promise_to_bytes!(promise)?;
        match bytes {
            Some(data) => {
                let record = CoreSessionRecord::deserialize(&data)?;
                self.cached_sessions
                    .borrow_mut()
                    .insert(address_str, record.clone());
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    async fn store_session(
        &mut self,
        address: &libsignal::ProtocolAddress,
        record: &CoreSessionRecord,
    ) -> SignalResult<()> {
        console_error_panic_hook::set_once();
        let address_str = address.to_string();
        self.cached_sessions
            .borrow_mut()
            .insert(address_str.clone(), record.clone());
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
        if let Some(pair) = *self.cached_identity_key_pair.borrow() {
            return Ok(pair);
        }

        let promise = get_identity_key_pair(&self.js_storage).map_err(js_to_signal_error)?;
        let value = promise_to_option_value(promise).await?;
        let js_value = value.ok_or_else(|| {
            SignalProtocolError::InvalidState("get_identity_key_pair", "JS returned null".into())
        })?;
        let payload: JsIdentityKeyPairPayload =
            deserialize_js_value(js_value, "get_identity_key_pair")?;
        let key_pair = payload.into_pair()?;

        self.cached_identity_key_pair.borrow_mut().replace(key_pair);

        Ok(key_pair)
    }
    async fn get_local_registration_id(&self) -> SignalResult<u32> {
        if let Some(id) = *self.cached_registration_id.borrow() {
            return Ok(id);
        }

        let promise = get_local_registration_id(&self.js_storage).map_err(js_to_signal_error)?;
        let result = JsFuture::from(promise).await.map_err(js_to_signal_error)?;
        let registration = result.as_f64().ok_or_else(|| {
            SignalProtocolError::InvalidState(
                "get_local_registration_id",
                "JS did not return a number".into(),
            )
        })? as u32;

        self.cached_registration_id
            .borrow_mut()
            .replace(registration);

        Ok(registration)
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
        let value = promise_to_option_value(promise).await?;
        let js_value = value.ok_or(SignalProtocolError::InvalidPreKeyId)?;
        let payload: JsPreKeyRecordPayload = deserialize_js_value(js_value, "load_pre_key")?;
        payload.into_record(prekey_id)
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
        let value = promise_to_option_value(promise).await?;
        let js_value = value.ok_or(SignalProtocolError::InvalidSignedPreKeyId)?;
        let payload: JsSignedPreKeyRecordPayload =
            deserialize_js_value(js_value, "load_signed_pre_key")?;
        payload.into_record(signed_prekey_id)
    }

    async fn save_signed_pre_key(
        &mut self,
        _id: SignedPreKeyId,
        _record: &SignedPreKeyRecord,
    ) -> SignalResult<()> {
        Ok(())
    }
}

#[async_trait(?Send)]
impl SenderKeyStore for JsStorageAdapter {
    async fn load_sender_key(
        &mut self,
        sender_key_name: &CoreSenderKeyName,
    ) -> SignalResult<Option<CoreSenderKeyRecord>> {
        console_error_panic_hook::set_once();

        let key_id = format!(
            "{}::{}",
            sender_key_name.group_id(),
            sender_key_name.sender_id()
        );

        if let Some(record) = self.cached_sender_keys.borrow().get(&key_id) {
            return Ok(Some(record.clone()));
        }

        let promise =
            load_sender_key(&self.js_storage, key_id.clone()).map_err(js_to_signal_error)?;
        let bytes = js_promise_to_bytes!(promise)?;

        match bytes {
            Some(data) => {
                let record = CoreSenderKeyRecord::deserialize(&data)?;
                self.cached_sender_keys
                    .borrow_mut()
                    .insert(key_id, record.clone());
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    async fn store_sender_key(
        &mut self,
        sender_key_name: &CoreSenderKeyName,
        record: &CoreSenderKeyRecord,
    ) -> SignalResult<()> {
        console_error_panic_hook::set_once();

        let key_id = format!(
            "{}::{}",
            sender_key_name.group_id(),
            sender_key_name.sender_id()
        );

        self.cached_sender_keys
            .borrow_mut()
            .insert(key_id.clone(), record.clone());

        let bytes = record.serialize()?;
        let promise =
            store_sender_key(&self.js_storage, key_id, &bytes).map_err(js_to_signal_error)?;
        JsFuture::from(promise).await.map_err(js_to_signal_error)?;
        Ok(())
    }
}

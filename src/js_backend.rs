//! JS storage backend adapter.
//!
//! Implements the full `Backend` trait (SignalStore + AppSyncStore + ProtocolStore + DeviceStore)
//! by delegating all storage operations to three JavaScript callback functions:
//!
//! - `get(store: string, key: string) -> Promise<Uint8Array | null>`
//! - `set(store: string, key: string, value: Uint8Array) -> Promise<void>`
//! - `delete(store: string, key: string) -> Promise<void>`
//!
//! Complex types (Device, AppStateSyncKey, HashState, etc.) are serialized as JSON bytes.

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use async_trait::async_trait;
use js_sys::{Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use wacore::appstate::hash::HashState;
use wacore::store::Device;
use wacore::store::InMemoryBackend;
use wacore::store::error::Result;
use wacore::store::traits::*;
use wacore_appstate::processor::AppStateMutationMAC;
use wacore_binary::jid::Jid;

// ---------------------------------------------------------------------------
// Store name constants
// ---------------------------------------------------------------------------

const STORE_IDENTITY: &str = "identity";
const STORE_SESSION: &str = "session";
const STORE_PREKEY: &str = "prekey";
const STORE_SIGNED_PREKEY: &str = "signed_prekey";
const STORE_SENDER_KEY: &str = "sender_key";
const STORE_SYNC_KEY: &str = "sync_key";
const STORE_SYNC_VERSION: &str = "sync_version";
const STORE_MUTATION_MAC: &str = "mutation_mac";
const STORE_DEVICE: &str = "device";
const STORE_SKDM: &str = "skdm";
const STORE_LID_MAPPING: &str = "lid_mapping";
const STORE_BASE_KEY: &str = "base_key";
const STORE_DEVICE_LIST: &str = "device_list";
const STORE_FORGET_MARKS: &str = "forget_marks";
const STORE_TC_TOKEN: &str = "tc_token";
const STORE_SENT_MESSAGE: &str = "sent_message";
const STORE_META: &str = "meta";

// ---------------------------------------------------------------------------
// Public API: backend factory
// ---------------------------------------------------------------------------

/// Get a new InMemoryBackend instance (fallback when no JS store is provided).
pub(crate) fn new_in_memory_backend() -> Arc<dyn Backend> {
    Arc::new(InMemoryBackend::default())
}

/// Create a JsBackend from JS callback functions.
pub(crate) fn new_js_backend(
    get_fn: js_sys::Function,
    set_fn: js_sys::Function,
    delete_fn: js_sys::Function,
) -> Arc<dyn Backend> {
    Arc::new(JsBackend::new(get_fn, set_fn, delete_fn))
}

// ---------------------------------------------------------------------------
// JsBackend struct
// ---------------------------------------------------------------------------

/// Storage backend that delegates all persistence to JavaScript callbacks.
pub struct JsBackend {
    get_fn: js_sys::Function,
    set_fn: js_sys::Function,
    delete_fn: js_sys::Function,
    next_device_id: AtomicI32,
    /// In-memory cache of sent message keys — avoids O(n²) JSON re-serialization
    /// on every store_sent_message call. Loaded lazily on first access.
    sent_message_keys: async_lock::Mutex<Option<Vec<String>>>,
}

crate::wasm_send_sync!(JsBackend);

impl JsBackend {
    fn new(
        get_fn: js_sys::Function,
        set_fn: js_sys::Function,
        delete_fn: js_sys::Function,
    ) -> Self {
        Self {
            get_fn,
            set_fn,
            delete_fn,
            next_device_id: AtomicI32::new(1),
            sent_message_keys: async_lock::Mutex::new(None),
        }
    }

    /// Get or lazily load the sent message keys list.
    async fn get_sent_keys(&self) -> Result<async_lock::MutexGuard<'_, Option<Vec<String>>>> {
        let mut guard = self.sent_message_keys.lock().await;
        if guard.is_none() {
            let keys: Vec<String> = self
                .js_get_json(STORE_META, "sent_message_keys")
                .await?
                .unwrap_or_default();
            *guard = Some(keys);
        }
        Ok(guard)
    }

    /// Persist the in-memory key list to JS store.
    /// Only called during cleanup/expiration — never on the send hot path.
    async fn flush_sent_keys(&self, keys: &Vec<String>) -> Result<()> {
        self.js_set_json(STORE_META, "sent_message_keys", keys)
            .await
    }

    // ── JS call helpers ──────────────────────────────────────────────────

    async fn js_get(&self, store: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let result = self
            .get_fn
            .call2(&JsValue::NULL, &store.into(), &key.into())
            .map_err(|e| js_err_to_store_err("get", e))?;

        let resolved = resolve_promise(result)
            .await
            .map_err(|e| js_err_to_store_err("get", e))?;

        if resolved.is_null() || resolved.is_undefined() {
            return Ok(None);
        }

        if let Some(arr) = resolved.dyn_ref::<Uint8Array>() {
            Ok(Some(arr.to_vec()))
        } else {
            Ok(None)
        }
    }

    async fn js_set(&self, store: &str, key: &str, value: &[u8]) -> Result<()> {
        let uint8 = Uint8Array::from(value);
        let result = self
            .set_fn
            .call3(&JsValue::NULL, &store.into(), &key.into(), &uint8.into())
            .map_err(|e| js_err_to_store_err("set", e))?;

        resolve_promise(result)
            .await
            .map_err(|e| js_err_to_store_err("set", e))?;

        Ok(())
    }

    async fn js_delete(&self, store: &str, key: &str) -> Result<()> {
        let result = self
            .delete_fn
            .call2(&JsValue::NULL, &store.into(), &key.into())
            .map_err(|e| js_err_to_store_err("delete", e))?;

        resolve_promise(result)
            .await
            .map_err(|e| js_err_to_store_err("delete", e))?;

        Ok(())
    }

    // ── Serialization helpers ────────────────────────────────────────────

    async fn js_get_json<T: serde::de::DeserializeOwned>(
        &self,
        store: &str,
        key: &str,
    ) -> Result<Option<T>> {
        match self.js_get(store, key).await? {
            Some(bytes) => {
                let value: T = serde_json::from_slice(&bytes)
                    .map_err(|e| store_err(format!("deserialize {store}/{key}: {e}")))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    async fn js_set_json<T: serde::Serialize>(
        &self,
        store: &str,
        key: &str,
        value: &T,
    ) -> Result<()> {
        let bytes = serde_json::to_vec(value)
            .map_err(|e| store_err(format!("serialize {store}/{key}: {e}")))?;
        self.js_set(store, key, &bytes).await
    }
}

// ---------------------------------------------------------------------------
// SignalStore
// ---------------------------------------------------------------------------

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl SignalStore for JsBackend {
    async fn put_identity(&self, address: &str, key: [u8; 32]) -> Result<()> {
        self.js_set(STORE_IDENTITY, address, &key).await
    }

    async fn load_identity(&self, address: &str) -> Result<Option<Vec<u8>>> {
        self.js_get(STORE_IDENTITY, address).await
    }

    async fn delete_identity(&self, address: &str) -> Result<()> {
        self.js_delete(STORE_IDENTITY, address).await
    }

    async fn get_session(&self, address: &str) -> Result<Option<Vec<u8>>> {
        self.js_get(STORE_SESSION, address).await
    }

    async fn put_session(&self, address: &str, session: &[u8]) -> Result<()> {
        self.js_set(STORE_SESSION, address, session).await
    }

    async fn delete_session(&self, address: &str) -> Result<()> {
        self.js_delete(STORE_SESSION, address).await
    }

    async fn store_prekey(&self, id: u32, record: &[u8], _uploaded: bool) -> Result<()> {
        self.js_set(STORE_PREKEY, &id.to_string(), record).await
    }

    async fn load_prekey(&self, id: u32) -> Result<Option<Vec<u8>>> {
        self.js_get(STORE_PREKEY, &id.to_string()).await
    }

    async fn remove_prekey(&self, id: u32) -> Result<()> {
        self.js_delete(STORE_PREKEY, &id.to_string()).await
    }

    async fn get_max_prekey_id(&self) -> Result<u32> {
        // Read the stored max prekey ID from meta store.
        // We maintain this as a counter that gets updated on each store_prekey call.
        match self.js_get(STORE_META, "max_prekey_id").await? {
            Some(bytes) => {
                let s = String::from_utf8(bytes).unwrap_or_default();
                Ok(s.parse::<u32>().unwrap_or(0))
            }
            None => Ok(0),
        }
    }

    async fn store_prekeys_batch(&self, keys: &[(u32, Vec<u8>)], uploaded: bool) -> Result<()> {
        let mut max_id = self.get_max_prekey_id().await?;
        for (id, record) in keys {
            self.store_prekey(*id, record, uploaded).await?;
            if *id > max_id {
                max_id = *id;
            }
        }
        // Update the tracked max prekey ID
        self.js_set(STORE_META, "max_prekey_id", max_id.to_string().as_bytes())
            .await?;
        Ok(())
    }

    async fn store_signed_prekey(&self, id: u32, record: &[u8]) -> Result<()> {
        self.js_set(STORE_SIGNED_PREKEY, &id.to_string(), record)
            .await?;
        // Track the ID in a list for load_all_signed_prekeys
        let mut ids = self.get_signed_prekey_ids().await?;
        if !ids.contains(&id) {
            ids.push(id);
            self.js_set_json(STORE_META, "signed_prekey_ids", &ids)
                .await?;
        }
        Ok(())
    }

    async fn load_signed_prekey(&self, id: u32) -> Result<Option<Vec<u8>>> {
        self.js_get(STORE_SIGNED_PREKEY, &id.to_string()).await
    }

    async fn load_all_signed_prekeys(&self) -> Result<Vec<(u32, Vec<u8>)>> {
        let ids = self.get_signed_prekey_ids().await?;
        let mut result = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(record) = self.load_signed_prekey(id).await? {
                result.push((id, record));
            }
        }
        Ok(result)
    }

    async fn remove_signed_prekey(&self, id: u32) -> Result<()> {
        self.js_delete(STORE_SIGNED_PREKEY, &id.to_string()).await?;
        // Remove from tracked IDs
        let mut ids = self.get_signed_prekey_ids().await?;
        ids.retain(|&i| i != id);
        self.js_set_json(STORE_META, "signed_prekey_ids", &ids)
            .await?;
        Ok(())
    }

    async fn put_sender_key(&self, address: &str, record: &[u8]) -> Result<()> {
        self.js_set(STORE_SENDER_KEY, address, record).await
    }

    async fn get_sender_key(&self, address: &str) -> Result<Option<Vec<u8>>> {
        self.js_get(STORE_SENDER_KEY, address).await
    }

    async fn delete_sender_key(&self, address: &str) -> Result<()> {
        self.js_delete(STORE_SENDER_KEY, address).await
    }
}

// Helper for signed prekey ID tracking
impl JsBackend {
    async fn get_signed_prekey_ids(&self) -> Result<Vec<u32>> {
        match self
            .js_get_json::<Vec<u32>>(STORE_META, "signed_prekey_ids")
            .await?
        {
            Some(ids) => Ok(ids),
            None => Ok(Vec::new()),
        }
    }
}

// ---------------------------------------------------------------------------
// AppSyncStore
// ---------------------------------------------------------------------------

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl AppSyncStore for JsBackend {
    async fn get_sync_key(&self, key_id: &[u8]) -> Result<Option<AppStateSyncKey>> {
        let hex_id = to_hex(key_id);
        self.js_get_json(STORE_SYNC_KEY, &hex_id).await
    }

    async fn set_sync_key(&self, key_id: &[u8], key: AppStateSyncKey) -> Result<()> {
        let hex_id = to_hex(key_id);
        self.js_set_json(STORE_SYNC_KEY, &hex_id, &key).await?;
        // Track latest sync key ID
        self.js_set(STORE_META, "latest_sync_key_id", key_id).await
    }

    async fn get_version(&self, name: &str) -> Result<HashState> {
        Ok(self
            .js_get_json(STORE_SYNC_VERSION, name)
            .await?
            .unwrap_or_default())
    }

    async fn set_version(&self, name: &str, state: HashState) -> Result<()> {
        self.js_set_json(STORE_SYNC_VERSION, name, &state).await
    }

    async fn put_mutation_macs(
        &self,
        name: &str,
        _version: u64,
        mutations: &[AppStateMutationMAC],
    ) -> Result<()> {
        for m in mutations {
            let key = format!("{}:{}", name, to_hex(&m.index_mac));
            self.js_set(STORE_MUTATION_MAC, &key, &m.value_mac).await?;
        }
        Ok(())
    }

    async fn get_mutation_mac(&self, name: &str, index_mac: &[u8]) -> Result<Option<Vec<u8>>> {
        let key = format!("{}:{}", name, to_hex(index_mac));
        self.js_get(STORE_MUTATION_MAC, &key).await
    }

    async fn delete_mutation_macs(&self, name: &str, index_macs: &[Vec<u8>]) -> Result<()> {
        for im in index_macs {
            let key = format!("{}:{}", name, to_hex(im));
            self.js_delete(STORE_MUTATION_MAC, &key).await?;
        }
        Ok(())
    }

    async fn get_latest_sync_key_id(&self) -> Result<Option<Vec<u8>>> {
        self.js_get(STORE_META, "latest_sync_key_id").await
    }
}

// ---------------------------------------------------------------------------
// ProtocolStore
// ---------------------------------------------------------------------------

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ProtocolStore for JsBackend {
    // --- SKDM Tracking ---

    async fn get_skdm_recipients(&self, group_jid: &str) -> Result<Vec<Jid>> {
        Ok(self
            .js_get_json(STORE_SKDM, group_jid)
            .await?
            .unwrap_or_default())
    }

    async fn add_skdm_recipients(&self, group_jid: &str, device_jids: &[Jid]) -> Result<()> {
        let mut list: Vec<Jid> = self.get_skdm_recipients(group_jid).await?;
        for jid in device_jids {
            if !list.contains(jid) {
                list.push(jid.clone());
            }
        }
        self.js_set_json(STORE_SKDM, group_jid, &list).await
    }

    async fn clear_skdm_recipients(&self, group_jid: &str) -> Result<()> {
        self.js_delete(STORE_SKDM, group_jid).await
    }

    // --- LID-PN Mapping ---

    async fn get_lid_mapping(&self, lid: &str) -> Result<Option<LidPnMappingEntry>> {
        self.js_get_json(STORE_LID_MAPPING, &format!("lid:{lid}"))
            .await
    }

    async fn get_pn_mapping(&self, phone: &str) -> Result<Option<LidPnMappingEntry>> {
        // First look up LID from the reverse index
        match self
            .js_get(STORE_LID_MAPPING, &format!("pn:{phone}"))
            .await?
        {
            Some(lid_bytes) => {
                let lid = String::from_utf8(lid_bytes).unwrap_or_default();
                self.get_lid_mapping(&lid).await
            }
            None => Ok(None),
        }
    }

    async fn put_lid_mapping(&self, entry: &LidPnMappingEntry) -> Result<()> {
        // Check if existing mapping has a different phone number (stale reverse entry)
        if let Some(old_entry) = self.get_lid_mapping(&entry.lid).await?
            && old_entry.phone_number != entry.phone_number
        {
            self.js_delete(STORE_LID_MAPPING, &format!("pn:{}", old_entry.phone_number))
                .await?;
        }
        // Store the forward mapping (lid -> entry)
        self.js_set_json(STORE_LID_MAPPING, &format!("lid:{}", entry.lid), entry)
            .await?;
        // Store the reverse mapping (pn -> lid)
        self.js_set(
            STORE_LID_MAPPING,
            &format!("pn:{}", entry.phone_number),
            entry.lid.as_bytes(),
        )
        .await?;
        // Track LID in list for get_all_lid_mappings
        let mut lids: Vec<String> = self
            .js_get_json(STORE_META, "lid_list")
            .await?
            .unwrap_or_default();
        if !lids.contains(&entry.lid) {
            lids.push(entry.lid.clone());
            self.js_set_json(STORE_META, "lid_list", &lids).await?;
        }
        Ok(())
    }

    async fn get_all_lid_mappings(&self) -> Result<Vec<LidPnMappingEntry>> {
        let lids: Vec<String> = self
            .js_get_json(STORE_META, "lid_list")
            .await?
            .unwrap_or_default();
        let mut result = Vec::with_capacity(lids.len());
        for lid in lids {
            if let Some(entry) = self.get_lid_mapping(&lid).await? {
                result.push(entry);
            }
        }
        Ok(result)
    }

    // --- Base Key Collision Detection ---

    async fn save_base_key(&self, address: &str, message_id: &str, base_key: &[u8]) -> Result<()> {
        let key = format!("{address}:{message_id}");
        self.js_set(STORE_BASE_KEY, &key, base_key).await
    }

    async fn has_same_base_key(
        &self,
        address: &str,
        message_id: &str,
        current_base_key: &[u8],
    ) -> Result<bool> {
        let key = format!("{address}:{message_id}");
        match self.js_get(STORE_BASE_KEY, &key).await? {
            Some(stored) => Ok(stored == current_base_key),
            None => Ok(false),
        }
    }

    async fn delete_base_key(&self, address: &str, message_id: &str) -> Result<()> {
        let key = format!("{address}:{message_id}");
        self.js_delete(STORE_BASE_KEY, &key).await
    }

    // --- Device Registry ---

    async fn update_device_list(&self, record: DeviceListRecord) -> Result<()> {
        self.js_set_json(STORE_DEVICE_LIST, &record.user, &record)
            .await
    }

    async fn get_devices(&self, user: &str) -> Result<Option<DeviceListRecord>> {
        self.js_get_json(STORE_DEVICE_LIST, user).await
    }

    // --- Sender Key Status (Lazy Deletion) ---

    async fn mark_forget_sender_key(&self, group_jid: &str, participant: &str) -> Result<()> {
        let mut marks: Vec<String> = self
            .js_get_json(STORE_FORGET_MARKS, group_jid)
            .await?
            .unwrap_or_default();
        if !marks.contains(&participant.to_string()) {
            marks.push(participant.to_string());
            self.js_set_json(STORE_FORGET_MARKS, group_jid, &marks)
                .await?;
        }
        Ok(())
    }

    async fn consume_forget_marks(&self, group_jid: &str) -> Result<Vec<String>> {
        let marks: Vec<String> = self
            .js_get_json(STORE_FORGET_MARKS, group_jid)
            .await?
            .unwrap_or_default();
        if !marks.is_empty() {
            self.js_delete(STORE_FORGET_MARKS, group_jid).await?;
        }
        Ok(marks)
    }

    // --- TcToken Storage ---

    async fn get_tc_token(&self, jid: &str) -> Result<Option<TcTokenEntry>> {
        self.js_get_json(STORE_TC_TOKEN, jid).await
    }

    async fn put_tc_token(&self, jid: &str, entry: &TcTokenEntry) -> Result<()> {
        self.js_set_json(STORE_TC_TOKEN, jid, entry).await?;
        // Track JID in list for get_all_tc_token_jids
        let mut jids: Vec<String> = self
            .js_get_json(STORE_META, "tc_token_jids")
            .await?
            .unwrap_or_default();
        if !jids.contains(&jid.to_string()) {
            jids.push(jid.to_string());
            self.js_set_json(STORE_META, "tc_token_jids", &jids).await?;
        }
        Ok(())
    }

    async fn delete_tc_token(&self, jid: &str) -> Result<()> {
        self.js_delete(STORE_TC_TOKEN, jid).await?;
        // Remove from tracked JIDs
        let mut jids: Vec<String> = self
            .js_get_json(STORE_META, "tc_token_jids")
            .await?
            .unwrap_or_default();
        jids.retain(|j| j != jid);
        self.js_set_json(STORE_META, "tc_token_jids", &jids).await
    }

    async fn get_all_tc_token_jids(&self) -> Result<Vec<String>> {
        Ok(self
            .js_get_json(STORE_META, "tc_token_jids")
            .await?
            .unwrap_or_default())
    }

    async fn delete_expired_tc_tokens(&self, cutoff_timestamp: i64) -> Result<u32> {
        let jids = self.get_all_tc_token_jids().await?;
        let mut deleted = 0u32;
        let mut remaining_jids = Vec::new();
        for jid in jids {
            if let Some(entry) = self
                .js_get_json::<TcTokenEntry>(STORE_TC_TOKEN, &jid)
                .await?
            {
                if entry.token_timestamp < cutoff_timestamp {
                    self.js_delete(STORE_TC_TOKEN, &jid).await?;
                    deleted += 1;
                } else {
                    remaining_jids.push(jid);
                }
            }
        }
        self.js_set_json(STORE_META, "tc_token_jids", &remaining_jids)
            .await?;
        Ok(deleted)
    }

    // --- Sent Message Store ---

    async fn store_sent_message(
        &self,
        chat_jid: &str,
        message_id: &str,
        payload: &[u8],
    ) -> Result<()> {
        let key = format!("{chat_jid}:{message_id}");
        let now = wacore::time::now_secs();
        let mut data = Vec::with_capacity(8 + payload.len());
        data.extend_from_slice(&now.to_be_bytes());
        data.extend_from_slice(payload);
        self.js_set(STORE_SENT_MESSAGE, &key, &data).await?;

        // Track key in memory only — no serialization on the hot path.
        // The key list is persisted during delete_expired_sent_messages (periodic cleanup).
        let mut guard = self.sent_message_keys.lock().await;
        guard.get_or_insert_with(Vec::new).push(key);
        Ok(())
    }

    async fn take_sent_message(&self, chat_jid: &str, message_id: &str) -> Result<Option<Vec<u8>>> {
        let key = format!("{chat_jid}:{message_id}");
        match self.js_get(STORE_SENT_MESSAGE, &key).await? {
            Some(data) if data.len() > 8 => {
                self.js_delete(STORE_SENT_MESSAGE, &key).await?;

                // Remove from in-memory index (no disk flush — cleanup will reconcile)
                let mut guard = self.sent_message_keys.lock().await;
                if let Some(ref mut keys) = *guard {
                    keys.retain(|k| k != &key);
                }

                // Skip 8-byte timestamp prefix
                Ok(Some(data[8..].to_vec()))
            }
            _ => Ok(None),
        }
    }

    async fn delete_expired_sent_messages(&self, cutoff_timestamp: i64) -> Result<u32> {
        let mut guard = self.get_sent_keys().await?;
        let keys = match guard.as_mut() {
            Some(k) => k,
            None => return Ok(0),
        };

        let mut deleted = 0u32;
        let mut remaining = Vec::new();
        for key in keys.drain(..) {
            if let Some(data) = self.js_get(STORE_SENT_MESSAGE, &key).await? {
                if data.len() >= 8 {
                    let ts = i64::from_be_bytes(data[..8].try_into().unwrap_or([0; 8]));
                    if ts < cutoff_timestamp {
                        self.js_delete(STORE_SENT_MESSAGE, &key).await?;
                        deleted += 1;
                        continue;
                    }
                }
                remaining.push(key);
            }
        }
        *keys = remaining;
        self.flush_sent_keys(keys).await?;
        Ok(deleted)
    }
}

// ---------------------------------------------------------------------------
// DeviceStore
// ---------------------------------------------------------------------------

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl DeviceStore for JsBackend {
    async fn save(&self, device: &Device) -> Result<()> {
        self.js_set_json(STORE_DEVICE, "device", device).await?;

        // `account` (AdvSignedDeviceIdentity) is #[serde(skip)] in Device,
        // so we persist it separately as raw protobuf bytes — same approach
        // as SQLite storage which uses a dedicated column.
        if let Some(ref account) = device.account {
            use prost::Message;
            self.js_set(STORE_DEVICE, "account", &account.encode_to_vec())
                .await?;
        }

        Ok(())
    }

    async fn load(&self) -> Result<Option<Device>> {
        let mut device: Option<Device> = self.js_get_json(STORE_DEVICE, "device").await?;

        // Restore the #[serde(skip)] `account` field from its separate key.
        if let Some(ref mut dev) = device
            && let Some(bytes) = self.js_get(STORE_DEVICE, "account").await?
        {
            use prost::Message;
            match waproto::whatsapp::AdvSignedDeviceIdentity::decode(bytes.as_slice()) {
                Ok(account) => dev.account = Some(account),
                Err(e) => log::warn!("Failed to decode stored account identity: {e}"),
            }
        }

        Ok(device)
    }

    async fn exists(&self) -> Result<bool> {
        Ok(self.js_get(STORE_DEVICE, "device").await?.is_some())
    }

    async fn create(&self) -> Result<i32> {
        let id = self.next_device_id.fetch_add(1, Ordering::Relaxed);
        // Materialize a default Device if none exists (same behavior as InMemoryBackend)
        if !self.exists().await? {
            self.save(&Device::new()).await?;
        }
        Ok(id)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn resolve_promise(value: JsValue) -> std::result::Result<JsValue, JsValue> {
    if value.is_instance_of::<Promise>() {
        JsFuture::from(Promise::unchecked_from_js(value)).await
    } else {
        Ok(value)
    }
}

fn js_err_to_store_err(context: &str, e: JsValue) -> wacore::store::error::StoreError {
    let msg = if let Some(s) = e.as_string() {
        s
    } else {
        format!("{:?}", e)
    };
    wacore::store::error::StoreError::Database(format!("JS {context}: {msg}"))
}

fn store_err(msg: String) -> wacore::store::error::StoreError {
    wacore::store::error::StoreError::Serialization(msg)
}

/// Simple hex encoding for byte slices (avoids adding `hex` crate dependency).
fn to_hex(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX_CHARS[(b >> 4) as usize] as char);
        s.push(HEX_CHARS[(b & 0xf) as usize] as char);
    }
    s
}

use async_trait::async_trait;
use base64::prelude::*;
use hmac::{Hmac, Mac};
use js_sys::{Promise, Uint8Array};
use prost::Message;
use sha2::Sha256;

// Mirror wacore's skipped-key bounds (libsignal consts.rs) so the seed sidecar
// stays in step with the receiver-chain cache it shadows.
const MAX_FORWARD_JUMPS: u32 = 25_000;
const MAX_MESSAGE_KEYS: usize = 2000;
// Per-address chain cap for the seed sidecar (defense-in-depth; only authenticated
// messages ever reach it). Sized above wacore's worst case — MAX_RECEIVER_CHAINS
// (5) across the current + up to CLOSED_SESSIONS_MAX (40) archived sessions —
// so a legitimate multi-session record never has valid seeds evicted.
const MAX_CACHED_CHAINS: usize = 256;

/// Pre-decrypt snapshot of just enough state to compute a message's skipped-key
/// seeds — captured cheaply (no DH, no chain stepping) BEFORE wacore decrypts,
/// and only consumed AFTER the message authenticates. A forged message is
/// dropped here, so it can never drive the DH ratchet, the HMAC stepping, or a
/// cache/disk write.
pub(crate) enum SkipSnapshot {
    /// Gap within an existing receiver chain: step forward from its chain key.
    Existing {
        ratchet: Vec<u8>,
        start_key: libsignal::ChainKey,
        counter: u32,
    },
    /// Gap on a brand-new DH ratchet: derive the chain (index 0) post-auth via
    /// `RootKey::create_chain`, then step.
    NewChain {
        ratchet: Vec<u8>,
        ratchet_key: libsignal::PublicKey,
        root: libsignal::RootKey,
        our_priv: libsignal::PrivateKey,
        counter: u32,
    },
}

/// The 32-byte message-key seed wacore caches for a skipped key:
/// `HMAC-SHA256(chainKey, [0x01])` (its `MESSAGE_KEY_SEED`). Byte-identical to
/// what libsignal-node stores, so re-emitting it round-trips losslessly.
fn message_key_seed(chain_key: &[u8; 32]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(chain_key).expect("HMAC accepts any key length");
    mac.update(&[0x01]);
    mac.finalize().into_bytes().to_vec()
}

/// Build a pre-decrypt skip-seed candidate from one session state (cheap: no DH,
/// no stepping). `allow_new_chain` is true only for the current session — a new
/// DH ratchet never lands on an archived one.
fn skip_candidate(
    state: &libsignal::SessionState,
    sender_ratchet_key: &libsignal::PublicKey,
    ratchet_bytes: &[u8],
    counter: u32,
    allow_new_chain: bool,
) -> Option<SkipSnapshot> {
    match state.get_receiver_chain_key(sender_ratchet_key) {
        Ok(Some(start_key)) => {
            let start = start_key.index();
            if counter <= start || counter - start > MAX_FORWARD_JUMPS {
                return None;
            }
            Some(SkipSnapshot::Existing {
                ratchet: ratchet_bytes.to_vec(),
                start_key,
                counter,
            })
        }
        Ok(None) if allow_new_chain && counter > 0 && counter <= MAX_FORWARD_JUMPS => {
            Some(SkipSnapshot::NewChain {
                ratchet: ratchet_bytes.to_vec(),
                ratchet_key: *sender_ratchet_key,
                root: state.root_key().ok()?,
                our_priv: state.sender_ratchet_private_key().ok()?,
                counter,
            })
        }
        _ => None,
    }
}
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_bytes::ByteBuf;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use waproto::whatsapp::{
    RecordStructure, SenderKeyRecordStructure, SenderKeyStateStructure, SessionStructure,
    sender_key_state_structure::{SenderChainKey, SenderMessageKey, SenderSigningKey},
    session_structure::{
        Chain, PendingPreKey,
        chain::{ChainKey, MessageKey},
    },
};

use crate::legacy_session::{ChainSeeds, MessageKeySeed, SessionMeta, SessionSeeds};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use wacore_libsignal::protocol::{
    self as libsignal, Direction as StoreDirection, GenericSignedPreKey as _, IdentityChange,
    IdentityKey, IdentityKeyPair, IdentityKeyStore, KeyPair, PreKeyId, PreKeyRecord, PreKeyStore,
    PrivateKey, SenderKeyStore, SessionStore, SignedPreKeyId, SignedPreKeyRecord,
    SignedPreKeyStore,
};
type SignalResult<T> = wacore_libsignal::protocol::error::Result<T>;

use wacore_libsignal::protocol::SenderKeyRecord as CoreSenderKeyRecord;
use wacore_libsignal::protocol::SessionRecord as CoreSessionRecord;
use wacore_libsignal::protocol::SignalProtocolError;
use wacore_libsignal::protocol::Timestamp;
use wacore_libsignal::store::sender_key_name::SenderKeyName as CoreSenderKeyName;

#[wasm_bindgen(typescript_custom_section)]
const TS_SIGNAL_STORAGE: &str = r#"
export interface SignalStorage {
    // May return raw bytes (native proto) OR a libsignal-node session object
    // (`{_sessions, version}` / a flat session) — the bridge migrates the latter
    // transparently, so an existing Baileys store can be used as-is (drop-in).
    loadSession(address: string): Uint8Array | object | null | undefined | Promise<Uint8Array | object | null | undefined>;
    // Drop-in default: receives the libsignal-node session JSON object to persist
    // (same shape Baileys stores), keeping the on-disk format revertible. Provide
    // `storeSessionRaw` instead to receive the native proto bytes (faster, not
    // interchangeable with Baileys).
    storeSession(address: string, session: any): void | Promise<void>;
    storeSessionRaw?(address: string, record: Uint8Array): void | Promise<void>;
    getOurIdentity(): KeyPair | Promise<KeyPair>;
    getOurRegistrationId(): number | Promise<number>;
    isTrustedIdentity(name: string, identityKey: Uint8Array, direction: number): boolean | Promise<boolean>;
    loadPreKey(id: number): KeyPair | null | undefined | Promise<KeyPair | null | undefined>;
    removePreKey(id: number): void | Promise<void>;
    loadSignedPreKey(id: number): SignedPreKey | null | undefined | Promise<SignedPreKey | null | undefined>;
    loadSenderKey(keyId: string): Uint8Array | null | undefined | Promise<Uint8Array | null | undefined>;
    storeSenderKey(keyId: string, record: Uint8Array): void | Promise<void>;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "SignalStorage")]
    #[derive(Clone)]
    pub type SignalStorage;

    #[wasm_bindgen(structural, method, catch, js_name = loadSession)]
    fn js_load_session(this: &SignalStorage, address: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(structural, method, catch, js_name = storeSession)]
    fn js_store_session(
        this: &SignalStorage,
        address: &str,
        record: JsValue,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(structural, method, catch, js_name = storeSessionRaw)]
    fn js_store_session_raw(
        this: &SignalStorage,
        address: &str,
        data: &Uint8Array,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(structural, method, catch, js_name = getOurIdentity)]
    fn js_get_our_identity(this: &SignalStorage) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(structural, method, catch, js_name = getOurRegistrationId)]
    fn js_get_our_registration_id(this: &SignalStorage) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(structural, method, catch, js_name = isTrustedIdentity)]
    fn js_is_trusted_identity(
        this: &SignalStorage,
        name: &str,
        identity_key: &Uint8Array,
        direction: u32,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(structural, method, catch, js_name = loadPreKey)]
    fn js_load_pre_key(this: &SignalStorage, id: u32) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(structural, method, catch, js_name = removePreKey)]
    fn js_remove_pre_key(this: &SignalStorage, id: u32) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(structural, method, catch, js_name = loadSignedPreKey)]
    fn js_load_signed_pre_key(this: &SignalStorage, id: u32) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(structural, method, catch, js_name = loadSenderKey)]
    fn js_load_sender_key(this: &SignalStorage, key_id: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(structural, method, catch, js_name = storeSenderKey)]
    fn js_store_sender_key(
        this: &SignalStorage,
        key_id: &str,
        record: &Uint8Array,
    ) -> Result<JsValue, JsValue>;
}

#[derive(Clone)]
pub struct JsStorageAdapter {
    pub js_storage: SignalStorage,
    cached_identity_key_pair: Rc<RefCell<Option<IdentityKeyPair>>>,
    cached_registration_id: Rc<RefCell<Option<u32>>>,
    cached_sessions: Rc<RefCell<HashMap<String, CoreSessionRecord>>>,
    // Skipped-key seeds captured when importing a libsignal-node session, kept
    // per address so the drop-in write path can re-emit them in the Baileys JSON
    // (wacore's record can't carry them). Empty for bridge-originated sessions.
    cached_seeds: Rc<RefCell<HashMap<String, SessionSeeds>>>,
    cached_sender_keys: Rc<RefCell<HashMap<String, CoreSenderKeyRecord>>>,
    cached_identities: Rc<RefCell<HashMap<String, Vec<u8>>>>,
    has_store_session_raw: Rc<RefCell<Option<bool>>>,
    last_address_cache: Rc<RefCell<Option<(String, String)>>>,
    last_sender_key_cache: Rc<RefCell<Option<(String, String, String)>>>,
}

impl JsStorageAdapter {
    pub fn new(js_storage: SignalStorage) -> Self {
        Self {
            js_storage,
            cached_identity_key_pair: Rc::new(RefCell::new(None)),
            cached_registration_id: Rc::new(RefCell::new(None)),
            cached_sessions: Rc::new(RefCell::new(HashMap::new())),
            cached_seeds: Rc::new(RefCell::new(HashMap::new())),
            cached_sender_keys: Rc::new(RefCell::new(HashMap::new())),
            cached_identities: Rc::new(RefCell::new(HashMap::new())),
            has_store_session_raw: Rc::new(RefCell::new(None)),
            last_address_cache: Rc::new(RefCell::new(None)),
            last_sender_key_cache: Rc::new(RefCell::new(None)),
        }
    }

    fn has_store_session_raw(&self) -> bool {
        if let Some(has_raw) = *self.has_store_session_raw.borrow() {
            return has_raw;
        }

        let has_raw = js_sys::Reflect::has(&self.js_storage, &JsValue::from_str("storeSessionRaw"))
            .unwrap_or(false);
        self.has_store_session_raw.borrow_mut().replace(has_raw);
        has_raw
    }

    #[inline]
    fn get_address_string(&self, address: &libsignal::ProtocolAddress) -> String {
        let name = address.name();
        let cache = self.last_address_cache.borrow();
        if let Some((cached_name, cached_str)) = cache.as_ref()
            && cached_name == name
        {
            return cached_str.clone();
        }
        drop(cache);

        let addr_str = address.to_string();
        self.last_address_cache
            .borrow_mut()
            .replace((name.to_string(), addr_str.clone()));
        addr_str
    }

    #[inline]
    fn get_sender_key_id(&self, sender_key_name: &CoreSenderKeyName) -> String {
        let group_id = sender_key_name.group_id();
        let sender_id = sender_key_name.sender_id();

        let cache = self.last_sender_key_cache.borrow();
        if let Some((cached_group, cached_sender, cached_key_id)) = cache.as_ref()
            && cached_group == group_id
            && cached_sender == sender_id
        {
            return cached_key_id.clone();
        }
        drop(cache);

        let key_id = format!("{}::{}", group_id, sender_id);
        self.last_sender_key_cache.borrow_mut().replace((
            group_id.to_string(),
            sender_id.to_string(),
            key_id.clone(),
        ));
        key_id
    }

    async fn migrate_legacy_json(
        &self,
        value: JsValue,
    ) -> SignalResult<Option<(Vec<u8>, SessionSeeds)>> {
        let local_identity = self.get_identity_key_pair().await?;
        let local_identity_public: Vec<u8> = local_identity.public_key().serialize().into();
        let local_reg_id = self.get_local_registration_id().await?;

        // Seeds + per-session meta ride alongside the record so the drop-in write
        // path can re-emit them in the Baileys JSON (the wacore record can't).
        match legacy_value_to_record(&value, local_identity_public, local_reg_id)? {
            Some((record, seeds)) => Ok(Some((record.encode_to_vec(), seeds))),
            None => Ok(None),
        }
    }

    /// Drop-in losslessness, phase 1 (pre-decrypt, CHEAP): record just enough to
    /// later compute the seeds for keys this message will skip — without doing the
    /// DH ratchet or any HMAC stepping yet. wacore may decrypt a gap via the
    /// current session OR a promoted archived one, so snapshot a candidate from
    /// EVERY session (current + previous): each archived session that already owns
    /// the message's receiver chain, plus the current session for a brand-new
    /// ratchet. The post-auth commit derives them all and the export self-validates
    /// (`seed_matches_record`), so candidates from the wrong session are dropped.
    /// Returns an empty Vec in raw mode / when there's nothing to read.
    pub(crate) async fn snapshot_skip(
        &self,
        address: &libsignal::ProtocolAddress,
        sender_ratchet_key: &libsignal::PublicKey,
        counter: u32,
    ) -> Vec<SkipSnapshot> {
        if self.has_store_session_raw() {
            return Vec::new();
        }
        let address_str = self.get_address_string(address);
        // Populate the session cache once (clones only on a cold miss); read by
        // reference below — no per-message record clone.
        if !self.cached_sessions.borrow().contains_key(&address_str) {
            let _ = SessionStore::load_session(self, address).await;
        }
        let cache = self.cached_sessions.borrow();
        let Some(record) = cache.get(&address_str) else {
            return Vec::new();
        };
        let ratchet_bytes = sender_ratchet_key.serialize().to_vec();

        let mut snaps = Vec::new();
        // Current session: an existing chain OR a brand-new ratchet.
        if let Some(state) = record.session_state()
            && let Some(snap) =
                skip_candidate(state, sender_ratchet_key, &ratchet_bytes, counter, true)
        {
            snaps.push(snap);
        }
        // Archived sessions: only if they already own this receiver chain (a new
        // ratchet never targets an archived session).
        for prev in record.previous_session_states() {
            if let Ok(state) = prev
                && let Some(snap) =
                    skip_candidate(&state, sender_ratchet_key, &ratchet_bytes, counter, false)
            {
                snaps.push(snap);
            }
        }
        snaps
    }

    /// Phase 2 (post-decrypt, AUTHENTICATED): turn a snapshot into seeds and merge
    /// them. Only now do we run the DH ratchet replication (`RootKey::create_chain`
    /// — X25519 + "WhisperRatchet" KDF, reusing wacore primitives) and the HMAC
    /// chain stepping. Returns whether anything was captured (so the caller knows
    /// to re-persist the just-written JSON with the new seeds).
    pub(crate) fn commit_skip_snapshot(
        &self,
        address: &libsignal::ProtocolAddress,
        snap: SkipSnapshot,
    ) -> bool {
        let (ratchet, mut chain_key, start, counter) = match snap {
            SkipSnapshot::Existing {
                ratchet,
                start_key,
                counter,
            } => {
                let start = start_key.index();
                (ratchet, start_key, start, counter)
            }
            SkipSnapshot::NewChain {
                ratchet,
                ratchet_key,
                root,
                our_priv,
                counter,
            } => match root.create_chain(&ratchet_key, &our_priv) {
                Ok((_, ck)) => (ratchet, ck, 0, counter),
                Err(_) => return false,
            },
        };

        let mut seeds = Vec::with_capacity((counter - start) as usize);
        for index in start..counter {
            seeds.push(MessageKeySeed {
                index,
                seed: message_key_seed(chain_key.key()),
            });
            chain_key = match chain_key.next_chain_key() {
                Ok(next) => next,
                Err(_) => break,
            };
        }

        let captured = !seeds.is_empty();
        self.merge_seeds(&self.get_address_string(address), ratchet, seeds);
        captured
    }

    /// Merge freshly-captured seeds into the per-address sidecar, keyed by chain.
    /// Bounds memory like wacore bounds its caches: at most `MAX_MESSAGE_KEYS`
    /// indices per chain and `MAX_CACHED_CHAINS` chains per address.
    fn merge_seeds(&self, address: &str, ratchet_key: Vec<u8>, seeds: Vec<MessageKeySeed>) {
        if seeds.is_empty() {
            return;
        }
        let mut cache = self.cached_seeds.borrow_mut();
        let entry = cache.entry(address.to_string()).or_default();
        let chain = match entry
            .chains
            .iter_mut()
            .find(|c| c.ratchet_key == ratchet_key)
        {
            Some(c) => c,
            None => {
                entry.chains.push(ChainSeeds {
                    ratchet_key,
                    seeds: Vec::new(),
                });
                entry.chains.last_mut().expect("just pushed")
            }
        };
        for s in seeds {
            match chain.seeds.iter_mut().find(|e| e.index == s.index) {
                Some(existing) => existing.seed = s.seed,
                None => chain.seeds.push(s),
            }
        }
        if chain.seeds.len() > MAX_MESSAGE_KEYS {
            chain.seeds.sort_unstable_by_key(|s| s.index);
            let excess = chain.seeds.len() - MAX_MESSAGE_KEYS;
            chain.seeds.drain(..excess);
        }
        if entry.chains.len() > MAX_CACHED_CHAINS {
            let excess = entry.chains.len() - MAX_CACHED_CHAINS;
            entry.chains.drain(..excess);
        }
    }

    /// Record `baseKeyType = OURS` for a base key. Called while a bridge-native
    /// initiator session still has `pendingPreKey` — the only window to learn it,
    /// since wacore keeps no baseKeyType and the ack later clears pendingPreKey.
    /// Never downgrades an explicit value carried over from an import.
    fn mark_base_key_ours(&self, address: &str, base_key: &[u8]) {
        let mut cache = self.cached_seeds.borrow_mut();
        let entry = cache.entry(address.to_string()).or_default();
        match entry.sessions.iter_mut().find(|m| m.base_key == base_key) {
            Some(m) if m.base_key_type == 0 => m.base_key_type = 1,
            Some(_) => {}
            None => entry.sessions.push(SessionMeta {
                base_key: base_key.to_vec(),
                base_key_type: 1,
                last_remote_ephemeral: Vec::new(),
                has_index_info: false,
                used: 0.0,
                created: 0.0,
                closed: -1.0,
            }),
        }
    }

    /// Rewrite the persisted session JSON for `address` from the cached record +
    /// sidecar seeds. Called after a gap decrypt commits new seeds, since wacore's
    /// own store (during decrypt) ran before those seeds existed. Drop-in mode
    /// only; no-op otherwise.
    pub(crate) async fn repersist_session_json(&self, address: &libsignal::ProtocolAddress) {
        if self.has_store_session_raw() {
            return;
        }
        let address_str = self.get_address_string(address);
        let record_bytes = {
            let cache = self.cached_sessions.borrow();
            match cache.get(&address_str).and_then(|r| r.serialize().ok()) {
                Some(b) => b,
                None => return,
            }
        };
        let _ = self.write_session_json(&address_str, &record_bytes).await;
    }

    /// Build the libsignal-node JSON for a record (+ sidecar seeds) and hand it to
    /// the JS store. Shared by `store_session` and `repersist_session_json`.
    async fn write_session_json(&self, address_str: &str, record_bytes: &[u8]) -> SignalResult<()> {
        let record_struct = RecordStructure::decode(record_bytes)
            .map_err(|e| invalid_js_data("store_session", format!("decode record: {e}")))?;
        // Learn baseKeyType=OURS for a bridge-native initiator session while its
        // pendingPreKey is still present (see mark_base_key_ours).
        if let Some(cs) = record_struct.current_session.as_ref()
            && cs.pending_pre_key.is_some()
            && let Some(base_key) = cs.alice_base_key.as_deref()
        {
            self.mark_base_key_ours(address_str, base_key);
        }
        let seeds = self
            .cached_seeds
            .borrow()
            .get(address_str)
            .cloned()
            .unwrap_or_default();
        let json = crate::legacy_session::record_to_legacy_json(&record_struct, &seeds)
            .map_err(js_to_signal_error)?;
        let result = self.js_storage.js_store_session(address_str, json);
        let promise_value = result.map_err(js_to_signal_error)?;
        resolve_maybe_promise(promise_value)
            .await
            .map_err(js_to_signal_error)?;
        Ok(())
    }

    fn migrate_legacy_sender_key(&self, data: &[u8]) -> SignalResult<Option<Vec<u8>>> {
        let json_str = match std::str::from_utf8(data) {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };

        if !json_str.trim().starts_with('[') {
            return Ok(None);
        }

        let js_val = js_sys::JSON::parse(json_str).map_err(js_to_signal_error)?;

        if !js_sys::Array::is_array(&js_val) {
            return Ok(None);
        }

        let array = js_sys::Array::from(&js_val);
        let mut sender_key_states = Vec::new();

        for i in 0..array.length() {
            let state_obj = array.get(i);

            let sender_key_id = get_number(&state_obj, "senderKeyId").unwrap_or(0.0) as u32;

            let sender_chain_key_obj = get_object(&state_obj, "senderChainKey")
                .ok_or_else(|| invalid_js_data("migrate_sender_key", "Missing senderChainKey"))?;
            let iteration = get_number(&sender_chain_key_obj, "iteration").unwrap_or(0.0) as u32;
            let seed =
                get_bytes_from_buffer_json(&sender_chain_key_obj, "seed").unwrap_or_default();

            let sender_signing_key_obj = get_object(&state_obj, "senderSigningKey")
                .ok_or_else(|| invalid_js_data("migrate_sender_key", "Missing senderSigningKey"))?;
            let public_key =
                get_bytes_from_buffer_json(&sender_signing_key_obj, "public").unwrap_or_default();
            let private_key = get_bytes_from_buffer_json(&sender_signing_key_obj, "private");

            let sender_message_keys_arr = get_object(&state_obj, "senderMessageKeys")
                .map(|v| js_sys::Array::from(&v))
                .unwrap_or_default();
            let mut sender_message_keys = Vec::new();

            for j in 0..sender_message_keys_arr.length() {
                let msg_key_obj = sender_message_keys_arr.get(j);
                let msg_iteration = get_number(&msg_key_obj, "iteration").unwrap_or(0.0) as u32;
                let msg_seed = get_bytes_from_buffer_json(&msg_key_obj, "seed").unwrap_or_default();

                sender_message_keys.push(SenderMessageKey {
                    iteration: Some(msg_iteration),
                    seed: Some(msg_seed.into()),
                });
            }

            let signing_key = SenderSigningKey {
                public: Some(public_key.into()),
                private: private_key.map(Into::into),
            };

            let chain_key = SenderChainKey {
                iteration: Some(iteration),
                seed: Some(seed.into()),
            };

            sender_key_states.push(SenderKeyStateStructure {
                sender_key_id: Some(sender_key_id),
                sender_chain_key: Some(chain_key),
                sender_signing_key: Some(signing_key),
                sender_message_keys,
            });
        }

        let record = SenderKeyRecordStructure { sender_key_states };

        Ok(Some(record.encode_to_vec()))
    }
}

/// Convert one libsignal-node session entry into a wacore `SessionStructure`,
/// the skipped-key seed chains it carries, and the per-session metadata wacore
/// can't store (`baseKeyType`, `lastRemoteEphemeralKey`). `local_identity_public`
/// /`local_reg_id` aren't part of the libsignal-node interchange — pass empty/0
/// when only the seeds matter (the reverse path drops them anyway).
fn legacy_entry_to_session(
    session_data: &JsValue,
    local_identity_public: &[u8],
    local_reg_id: u32,
) -> SignalResult<Option<(SessionStructure, Vec<ChainSeeds>, SessionMeta)>> {
    let decode_b64 = |s: String| BASE64_STANDARD.decode(s).unwrap_or_default();

    let registration_id = get_number(session_data, "registrationId").unwrap_or(0.0) as u32;

    let current_ratchet = get_object(session_data, "currentRatchet")
        .ok_or_else(|| invalid_js_data("migrate", "Missing currentRatchet"))?;
    let root_key = decode_b64(get_string(&current_ratchet, "rootKey").unwrap_or_default());
    let previous_counter = get_number(&current_ratchet, "previousCounter").unwrap_or(0.0) as u32;

    let ephemeral_key_pair = get_object(&current_ratchet, "ephemeralKeyPair")
        .ok_or_else(|| invalid_js_data("migrate", "Missing ephemeralKeyPair"))?;
    let sender_ratchet_pub = ensure_pubkey_33(decode_b64(
        get_string(&ephemeral_key_pair, "pubKey").unwrap_or_default(),
    ));
    let sender_ratchet_priv =
        decode_b64(get_string(&ephemeral_key_pair, "privKey").unwrap_or_default());

    let index_info = get_object(session_data, "indexInfo")
        .ok_or_else(|| invalid_js_data("migrate", "Missing indexInfo"))?;
    let remote_identity = ensure_pubkey_33(decode_b64(
        get_string(&index_info, "remoteIdentityKey").unwrap_or_default(),
    ));
    let base_key = ensure_pubkey_33(decode_b64(
        get_string(&index_info, "baseKey").unwrap_or_default(),
    ));

    // Per-session metadata wacore's proto can't represent — carried in the sidecar
    // so the reverse path is lossless (baseKeyType after an ack clears
    // pendingPreKey, lastRemoteEphemeralKey, and the indexInfo timestamps that
    // drive libsignal-node's attempt order + age pruning).
    let meta = SessionMeta {
        base_key: base_key.clone(),
        base_key_type: get_number(&index_info, "baseKeyType").unwrap_or(0.0) as u32,
        last_remote_ephemeral: ensure_pubkey_33(decode_b64(
            get_string(&current_ratchet, "lastRemoteEphemeralKey").unwrap_or_default(),
        )),
        has_index_info: true,
        used: get_number(&index_info, "used").unwrap_or(0.0),
        created: get_number(&index_info, "created").unwrap_or(0.0),
        closed: get_number(&index_info, "closed").unwrap_or(-1.0),
    };

    let chains = get_object(session_data, "_chains")
        .ok_or_else(|| invalid_js_data("migrate", "Missing _chains"))?;
    let chains_obj = chains
        .dyn_ref::<js_sys::Object>()
        .ok_or_else(|| invalid_js_data("migrate", "_chains expected to be an object"))?;
    let chain_keys = js_sys::Object::keys(chains_obj);

    let mut sender_chain_struct = None;
    let mut receiver_chains_vec = Vec::new();
    let mut seed_chains = Vec::new();

    for i in 0..chain_keys.length() {
        let key = chain_keys.get(i);
        let chain = js_sys::Reflect::get(&chains, &key).map_err(|err| {
            invalid_js_data(
                "migrate",
                format!(
                    "Failed to read chain entry {:?}: {:?}",
                    key.as_string(),
                    err
                ),
            )
        })?;
        let chain_type = get_number(&chain, "chainType").unwrap_or(0.0) as u32;

        let chain_key_obj = get_object(&chain, "chainKey")
            .ok_or_else(|| invalid_js_data("migrate", "Missing chainKey for legacy chain entry"))?;
        // JS `chainKey.counter` is the last-consumed counter (fresh = -1); wacore's
        // `ChainKey.index` is the next-to-derive (fresh = 0), so they differ by one.
        // Add before the cast so -1 maps to 0 instead of saturating; without this,
        // forward-stepped messages BadMac.
        let chain_index =
            (get_number(&chain_key_obj, "counter").unwrap_or(-1.0) + 1.0).max(0.0) as u32;
        // A closed receiver chain has its `chainKey.key` deleted — keep the index
        // but leave the key absent so it stays closed instead of resurrecting as
        // an empty-key live chain.
        let chain_key_bytes = get_string(&chain_key_obj, "key")
            .map(decode_b64)
            .filter(|b| !b.is_empty());

        let message_keys_obj = get_object(&chain, "messageKeys").ok_or_else(|| {
            invalid_js_data("migrate", "Missing messageKeys for legacy chain entry")
        })?;
        let message_keys_object = message_keys_obj
            .dyn_ref::<js_sys::Object>()
            .ok_or_else(|| invalid_js_data("migrate", "Invalid messageKeys object"))?;
        let msg_keys_list = js_sys::Object::keys(message_keys_object);
        let mut message_keys = Vec::new();
        for j in 0..msg_keys_list.length() {
            // messageKeys is keyed by counter; `Object::keys` yields STRING keys.
            let idx_val = msg_keys_list.get(j);
            let idx = idx_val
                .as_string()
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| invalid_js_data("migrate", "Message key index is not a number"))?;
            let msg_key_b64 = js_sys::Reflect::get(&message_keys_obj, &idx_val)
                .map_err(|err| {
                    invalid_js_data("migrate", format!("Missing message key {}: {:?}", idx, err))
                })?
                .as_string()
                .unwrap_or_default();
            message_keys.push((idx, decode_b64(msg_key_b64)));
        }

        let ratchet_key = if chain_type == 1 {
            sender_ratchet_pub.clone()
        } else {
            ensure_pubkey_33(decode_b64(key.as_string().unwrap_or_default()))
        };

        // Stash the raw seeds so the reverse (export) path is lossless.
        if !message_keys.is_empty() {
            seed_chains.push(ChainSeeds {
                ratchet_key: ratchet_key.clone(),
                seeds: message_keys
                    .iter()
                    .map(|(index, seed)| MessageKeySeed {
                        index: *index,
                        seed: seed.clone(),
                    })
                    .collect(),
            });
        }

        let built = Chain {
            sender_ratchet_key: Some(ratchet_key),
            sender_ratchet_key_private: (chain_type == 1).then(|| sender_ratchet_priv.clone()),
            chain_key: Some(ChainKey {
                index: Some(chain_index),
                key: chain_key_bytes.map(Into::into),
            }),
            message_keys: legacy_message_keys(message_keys),
        };

        if chain_type == 1 {
            sender_chain_struct = Some(built);
        } else if chain_type == 2 {
            receiver_chains_vec.push(built);
        }
    }

    // pendingPreKey: the alice-side discriminator; preserving it keeps an
    // initiator's not-yet-acked session sending PreKeyWhisperMessages.
    // `get_object` returns `Some(undefined)` for an absent key, so filter to a
    // real object — otherwise we'd forge an empty pendingPreKey and wacore would
    // reject the session with "invalid pending PreKey message base key".
    let pending_pre_key = get_object(session_data, "pendingPreKey")
        .filter(|ppk| ppk.is_object())
        .map(|ppk| PendingPreKey {
            pre_key_id: get_number(&ppk, "preKeyId").map(|n| n as u32),
            signed_pre_key_id: get_number(&ppk, "signedKeyId").map(|n| n as i32),
            base_key: Some(ensure_pubkey_33(decode_b64(
                get_string(&ppk, "baseKey").unwrap_or_default(),
            ))),
        });

    let session = SessionStructure {
        session_version: Some(3),
        local_identity_public: Some(local_identity_public.to_vec()),
        remote_identity_public: Some(remote_identity),
        root_key: Some(root_key),
        previous_counter: Some(previous_counter),
        sender_chain: sender_chain_struct,
        receiver_chains: receiver_chains_vec,
        pending_key_exchange: None,
        pending_pre_key,
        remote_registration_id: Some(registration_id),
        local_registration_id: Some(local_reg_id),
        needs_refresh: None,
        alice_base_key: Some(base_key),
    };

    Ok(Some((session, seed_chains, meta)))
}

/// Convert a libsignal-node value (a flat session, or a `{_sessions}` wrapper
/// with current + archived sessions) into a wacore `RecordStructure` plus the
/// `SessionSeeds` sidecar. The open session (`indexInfo.closed === -1`) becomes
/// `current_session`; the rest become `previous_sessions` (most-recently-closed
/// first), so archived sessions survive a revert too. Returns `None` when there
/// is nothing migratable.
fn legacy_value_to_record(
    value: &JsValue,
    local_identity_public: Vec<u8>,
    local_reg_id: u32,
) -> SignalResult<Option<(RecordStructure, SessionSeeds)>> {
    let has_reg_id =
        js_sys::Reflect::has(value, &JsValue::from_str("registrationId")).unwrap_or(false);
    let has_ratchet =
        js_sys::Reflect::has(value, &JsValue::from_str("currentRatchet")).unwrap_or(false);

    // (entry, closed) pairs to convert. A flat session is treated as open.
    let mut entries: Vec<(JsValue, f64)> = Vec::new();
    if has_reg_id && has_ratchet {
        let closed = get_object(value, "indexInfo")
            .and_then(|i| get_number(&i, "closed"))
            .unwrap_or(-1.0);
        entries.push((value.clone(), closed));
    } else {
        if !js_sys::Reflect::has(value, &JsValue::from_str("_sessions")).unwrap_or(false) {
            return Ok(None);
        }
        let sessions = get_object(value, "_sessions")
            .ok_or_else(|| invalid_js_data("migrate", "Missing _sessions"))?;
        let sessions_obj = sessions
            .dyn_ref::<js_sys::Object>()
            .ok_or_else(|| invalid_js_data("migrate", "Invalid _sessions object"))?;
        let keys = js_sys::Object::keys(sessions_obj);
        for i in 0..keys.length() {
            let entry =
                js_sys::Reflect::get(&sessions, &keys.get(i)).map_err(js_to_signal_error)?;
            if !js_sys::Reflect::has(&entry, &JsValue::from_str("registrationId")).unwrap_or(false)
            {
                continue; // not a real session entry
            }
            let closed = get_object(&entry, "indexInfo")
                .and_then(|i| get_number(&i, "closed"))
                .unwrap_or(-1.0);
            entries.push((entry, closed));
        }
    }
    if entries.is_empty() {
        return Ok(None);
    }

    let mut current: Option<SessionStructure> = None;
    let mut previous: Vec<(SessionStructure, f64)> = Vec::new();
    let mut all_chains: Vec<ChainSeeds> = Vec::new();
    let mut all_meta: Vec<SessionMeta> = Vec::new();

    for (entry, closed) in entries {
        let Some((session, chains, meta)) =
            legacy_entry_to_session(&entry, &local_identity_public, local_reg_id)?
        else {
            continue;
        };
        all_chains.extend(chains);
        all_meta.push(meta);
        // First open session is current; everything else is archived.
        if closed < 0.0 && current.is_none() {
            current = Some(session);
        } else {
            previous.push((session, closed));
        }
    }

    // Most-recently-closed first (matches libsignal-node's removeOldSessions order).
    previous.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    // No open session → leave `current_session` empty (do NOT promote an archived
    // one). libsignal-node refuses to encrypt with no open session and the bridge
    // must mirror that, while still keeping the archived sessions for decrypt and
    // for a lossless revert (no phantom open session resurrected).
    let previous_sessions: Vec<SessionStructure> = previous.into_iter().map(|(s, _)| s).collect();
    if current.is_none() && previous_sessions.is_empty() {
        return Ok(None);
    }

    Ok(Some((
        RecordStructure {
            current_session: current,
            previous_sessions,
        },
        SessionSeeds {
            chains: all_chains,
            sessions: all_meta,
        },
    )))
}

/// Parse a libsignal-node session JSON into the bridge's persisted pair
/// `{ record, seeds }` (both `Uint8Array`). Inverse of `exportLegacySession`;
/// together they round-trip a Baileys session losslessly. Returns `null` when
/// the value isn't a migratable session.
#[wasm_bindgen(js_name = importLegacySession)]
pub fn import_legacy_session(value: JsValue) -> Result<JsValue, JsValue> {
    let (record, seeds) = match legacy_value_to_record(&value, Vec::new(), 0)
        .map_err(|e| JsValue::from_str(&e.to_string()))?
    {
        Some(pair) => pair,
        None => return Ok(JsValue::NULL),
    };

    let out = js_sys::Object::new();
    js_sys::Reflect::set(
        &out,
        &JsValue::from_str("record"),
        &Uint8Array::from(record.encode_to_vec().as_slice()),
    )?;
    js_sys::Reflect::set(
        &out,
        &JsValue::from_str("seeds"),
        &Uint8Array::from(seeds.encode_to_vec().as_slice()),
    )?;
    Ok(out.into())
}

/// libsignal-node caches each skipped message key as the raw 32-byte HMAC seed
/// (`HMAC-SHA256(chainKey, [0x01])`), keyed by counter. wacore's `MessageKey`
/// instead stores the post-HKDF split (cipher/mac/iv), so reuse wacore's own
/// `MessageKeyGenerator` to derive it from the seed — same KDF, no duplicated
/// crypto and no change to the upstream WhatsApp proto.
///
/// The previous code stuffed the seed straight into `cipher_key` and zeroed
/// mac/iv, corrupting every skipped key and breaking out-of-order decryption.
/// Malformed (non-32-byte) seeds are dropped — the protocol just asks for a
/// retry of that one out-of-order message.
/// libsignal-node tolerates a bare 32-byte DJB public key; wacore requires the
/// 0x05-prefixed 33-byte form. Normalize on import so an unprefixed key (rare,
/// but accepted upstream) doesn't get rejected. Leaves 33-byte keys untouched.
fn ensure_pubkey_33(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() == 32 {
        let mut prefixed = Vec::with_capacity(33);
        prefixed.push(0x05);
        prefixed.extend_from_slice(&bytes);
        prefixed
    } else {
        bytes
    }
}

fn legacy_message_keys(msg_keys: Vec<(u32, Vec<u8>)>) -> Vec<MessageKey> {
    msg_keys
        .into_iter()
        .filter_map(|(index, seed)| {
            let seed: [u8; 32] = seed.try_into().ok()?;
            Some(libsignal::MessageKeyGenerator::new_from_seed(&seed, index).into_pb())
        })
        .collect()
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

#[inline]
fn js_to_signal_error(e: JsValue) -> libsignal::SignalProtocolError {
    libsignal::SignalProtocolError::FfiBindingError(format!("{:?}", e))
}

#[inline]
async fn resolve_maybe_promise(value: JsValue) -> Result<JsValue, JsValue> {
    if value.is_instance_of::<Promise>() {
        return JsFuture::from(Promise::unchecked_from_js(value)).await;
    }
    Ok(value)
}

#[inline]
async fn resolve_maybe_promise_optional(value: JsValue) -> SignalResult<Option<JsValue>> {
    let resolved = resolve_maybe_promise(value)
        .await
        .map_err(js_to_signal_error)?;
    if resolved.is_null() || resolved.is_undefined() {
        Ok(None)
    } else {
        Ok(Some(resolved))
    }
}

#[inline]
fn deserialize_js_value<T: DeserializeOwned>(
    value: JsValue,
    context: &'static str,
) -> SignalResult<T> {
    serde_wasm_bindgen::from_value(value).map_err(|err| invalid_js_data(context, err.to_string()))
}

#[inline]
fn js_array_to_bytes(array: &js_sys::Array) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(array.length() as usize);
    for i in 0..array.length() {
        if let Some(val) = array.get(i).as_f64() {
            bytes.push(val as u8);
        }
    }
    bytes
}

#[inline]
fn js_value_to_bytes(value: &JsValue) -> Option<Vec<u8>> {
    if let Some(arr) = value.dyn_ref::<Uint8Array>() {
        return Some(arr.to_vec());
    }

    if js_sys::Array::is_array(value) {
        return Some(js_array_to_bytes(&js_sys::Array::from(value)));
    }

    if let Ok(data) = js_sys::Reflect::get(value, &JsValue::from_str("data"))
        && js_sys::Array::is_array(&data)
    {
        return Some(js_array_to_bytes(&js_sys::Array::from(&data)));
    }

    None
}

fn is_legacy_session_object(value: &JsValue) -> bool {
    let has_sessions =
        js_sys::Reflect::has(value, &JsValue::from_str("_sessions")).unwrap_or(false);
    let has_reg_id =
        js_sys::Reflect::has(value, &JsValue::from_str("registrationId")).unwrap_or(false);
    let has_ratchet =
        js_sys::Reflect::has(value, &JsValue::from_str("currentRatchet")).unwrap_or(false);

    has_sessions || (has_reg_id && has_ratchet)
}

fn get_string(obj: &JsValue, key: &str) -> Option<String> {
    js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .ok()
        .and_then(|v| v.as_string())
}

fn get_object(obj: &JsValue, key: &str) -> Option<JsValue> {
    js_sys::Reflect::get(obj, &JsValue::from_str(key)).ok()
}

fn get_number(obj: &JsValue, key: &str) -> Option<f64> {
    js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .ok()
        .and_then(|v| v.as_f64())
}

fn get_bytes_from_buffer_json(obj: &JsValue, key: &str) -> Option<Vec<u8>> {
    let val = js_sys::Reflect::get(obj, &JsValue::from_str(key)).ok()?;
    if val.is_undefined() || val.is_null() {
        return None;
    }

    // Check for Buffer-like object { type: "Buffer", data: [...] }
    let type_prop = js_sys::Reflect::get(&val, &JsValue::from_str("type")).ok();
    let data_prop = js_sys::Reflect::get(&val, &JsValue::from_str("data")).ok();
    if let (Some(t), Some(d)) = (type_prop, data_prop)
        && t.as_string().as_deref() == Some("Buffer")
        && js_sys::Array::is_array(&d)
    {
        return Some(js_array_to_bytes(&js_sys::Array::from(&d)));
    }

    // Try Uint8Array
    if let Some(arr) = val.dyn_ref::<Uint8Array>() {
        return Some(arr.to_vec());
    }

    // Try plain JS array
    if js_sys::Array::is_array(&val) {
        return Some(js_array_to_bytes(&js_sys::Array::from(&val)));
    }

    // Try base64 string
    val.as_string()
        .and_then(|s| BASE64_STANDARD.decode(&s).ok())
}

#[async_trait(?Send)]
impl SessionStore for JsStorageAdapter {
    async fn load_session(
        &self,
        address: &libsignal::ProtocolAddress,
    ) -> SignalResult<Option<CoreSessionRecord>> {
        let address_str = self.get_address_string(address);

        if let Some(record) = self.cached_sessions.borrow().get(&address_str) {
            return Ok(Some(record.clone()));
        }

        let result = self
            .js_storage
            .js_load_session(&address_str)
            .map_err(js_to_signal_error)?;
        let value = resolve_maybe_promise(result)
            .await
            .map_err(js_to_signal_error)?;

        if value.is_null() || value.is_undefined() {
            return Ok(None);
        }

        let bytes = if let Some(b) = js_value_to_bytes(&value) {
            Some(b)
        } else if is_legacy_session_object(&value) {
            match self.migrate_legacy_json(value).await? {
                Some((record_bytes, seeds)) => {
                    // Remember the skipped-key seeds so a later write-back can
                    // reproduce the exact Baileys JSON.
                    self.cached_seeds
                        .borrow_mut()
                        .insert(address_str.clone(), seeds);
                    Some(record_bytes)
                }
                None => None,
            }
        } else {
            None
        };

        match bytes {
            Some(data) => {
                let record = CoreSessionRecord::deserialize(&data)?;
                // Insert into cache and return a clone - this is required since HashMap takes ownership
                let result = record.clone();
                self.cached_sessions
                    .borrow_mut()
                    .insert(address_str, record);
                Ok(Some(result))
            }
            None => Ok(None),
        }
    }

    async fn has_session(&self, address: &libsignal::ProtocolAddress) -> SignalResult<bool> {
        Ok(SessionStore::load_session(self, address).await?.is_some())
    }

    async fn store_session(
        &mut self,
        address: &libsignal::ProtocolAddress,
        record: CoreSessionRecord,
    ) -> SignalResult<()> {
        let address_str = self.get_address_string(address);

        let bytes = record.serialize()?;

        self.cached_sessions
            .borrow_mut()
            .insert(address_str.clone(), record);

        if self.has_store_session_raw() {
            // Native fast path: hand back the raw wacore proto bytes.
            let uint8 = Uint8Array::from(bytes.as_slice());
            let result = self.js_storage.js_store_session_raw(&address_str, &uint8);
            let promise_value = result.map_err(js_to_signal_error)?;
            resolve_maybe_promise(promise_value)
                .await
                .map_err(js_to_signal_error)?;
            Ok(())
        } else {
            // Drop-in path: persist the libsignal-node JSON so the on-disk format
            // stays Baileys-compatible (re-attaching captured skipped-key seeds).
            self.write_session_json(&address_str, &bytes).await
        }
    }
}

#[async_trait(?Send)]
impl IdentityKeyStore for JsStorageAdapter {
    async fn get_identity_key_pair(&self) -> SignalResult<IdentityKeyPair> {
        if let Some(pair) = self.cached_identity_key_pair.borrow().as_ref().cloned() {
            return Ok(pair);
        }

        let result = self
            .js_storage
            .js_get_our_identity()
            .map_err(js_to_signal_error)?;
        let value = resolve_maybe_promise_optional(result).await?;

        let js_value = value.ok_or_else(|| {
            SignalProtocolError::InvalidState("get_identity_key_pair", "JS returned null".into())
        })?;

        let payload: JsIdentityKeyPairPayload =
            deserialize_js_value(js_value, "get_identity_key_pair")?;
        let key_pair = payload.into_pair()?;

        self.cached_identity_key_pair
            .borrow_mut()
            .replace(key_pair.clone());

        Ok(key_pair)
    }

    async fn get_local_registration_id(&self) -> SignalResult<u32> {
        if let Some(id) = *self.cached_registration_id.borrow() {
            return Ok(id);
        }

        let result = self
            .js_storage
            .js_get_our_registration_id()
            .map_err(js_to_signal_error)?;
        let value = resolve_maybe_promise(result)
            .await
            .map_err(js_to_signal_error)?;

        let registration = value.as_f64().ok_or_else(|| {
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
        let address_name = address.name().to_string();
        let identity_bytes = identity.serialize();

        if let Some(cached_key) = self.cached_identities.borrow().get(&address_name)
            && cached_key.as_slice() == identity_bytes.as_slice()
        {
            return Ok(true);
        }

        let direction_val = match direction {
            StoreDirection::Sending => 0,
            StoreDirection::Receiving => 1,
        };

        let uint8 = Uint8Array::from(identity_bytes.as_slice());
        let result = self
            .js_storage
            .js_is_trusted_identity(&address_name, &uint8, direction_val)
            .map_err(js_to_signal_error)?;

        let value = resolve_maybe_promise(result)
            .await
            .map_err(js_to_signal_error)?;

        let trusted = value.as_bool().unwrap_or(false);

        if trusted {
            self.cached_identities
                .borrow_mut()
                .insert(address_name, identity_bytes.to_vec());
        }

        Ok(trusted)
    }

    async fn save_identity(
        &mut self,
        address: &libsignal::ProtocolAddress,
        identity: &libsignal::IdentityKey,
    ) -> SignalResult<IdentityChange> {
        let address_name = address.name().to_string();
        let identity_bytes = identity.serialize();

        let changed = if let Some(cached_key) = self.cached_identities.borrow().get(&address_name) {
            cached_key.as_slice() != identity_bytes.as_slice()
        } else {
            false
        };

        self.cached_identities
            .borrow_mut()
            .insert(address_name, identity_bytes.to_vec());

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
        let result = self
            .js_storage
            .js_load_pre_key(prekey_id.into())
            .map_err(js_to_signal_error)?;
        let value = resolve_maybe_promise_optional(result).await?;

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
        let result = self
            .js_storage
            .js_remove_pre_key(prekey_id.into())
            .map_err(js_to_signal_error)?;
        resolve_maybe_promise(result)
            .await
            .map_err(js_to_signal_error)?;
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
        let result = self
            .js_storage
            .js_load_signed_pre_key(signed_prekey_id.into())
            .map_err(js_to_signal_error)?;
        let value = resolve_maybe_promise_optional(result).await?;

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
        &self,
        sender_key_name: &CoreSenderKeyName,
    ) -> SignalResult<Option<CoreSenderKeyRecord>> {
        let key_id = self.get_sender_key_id(sender_key_name);

        if let Some(record) = self.cached_sender_keys.borrow().get(&key_id) {
            return Ok(Some(record.clone()));
        }

        let result = self
            .js_storage
            .js_load_sender_key(&key_id)
            .map_err(js_to_signal_error)?;
        let value = resolve_maybe_promise(result)
            .await
            .map_err(js_to_signal_error)?;

        if value.is_null() || value.is_undefined() {
            return Ok(None);
        }

        let bytes = js_value_to_bytes(&value);

        let Some(data) = bytes else {
            return Ok(None);
        };

        // Try direct deserialization first (standard protobuf format)
        let record = match CoreSenderKeyRecord::deserialize(&data) {
            Ok(record) => record,
            Err(_) => {
                // Fall back to legacy JSON format migration
                let migrated_bytes = self.migrate_legacy_sender_key(&data)?.ok_or_else(|| {
                    SignalProtocolError::InvalidState(
                        "load_sender_key",
                        "Failed to deserialize sender key record".into(),
                    )
                })?;
                CoreSenderKeyRecord::deserialize(&migrated_bytes)?
            }
        };

        self.cached_sender_keys
            .borrow_mut()
            .insert(key_id, record.clone());
        Ok(Some(record))
    }

    async fn store_sender_key(
        &mut self,
        sender_key_name: &CoreSenderKeyName,
        record: CoreSenderKeyRecord,
    ) -> SignalResult<()> {
        let key_id = self.get_sender_key_id(sender_key_name);

        let bytes = record.serialize()?;

        self.cached_sender_keys
            .borrow_mut()
            .insert(key_id.clone(), record);

        // Drop-in mode (no storeSessionRaw) persists the Baileys sender-key JSON
        // so a group session can revert; raw mode keeps the native proto bytes.
        let out_bytes = if self.has_store_session_raw() {
            bytes
        } else {
            crate::legacy_session::sender_key_record_to_legacy_json(&bytes)
                .map_err(js_to_signal_error)?
        };
        let uint8 = Uint8Array::from(out_bytes.as_slice());

        let result = self
            .js_storage
            .js_store_sender_key(&key_id, &uint8)
            .map_err(js_to_signal_error)?;
        resolve_maybe_promise(result)
            .await
            .map_err(js_to_signal_error)?;

        Ok(())
    }
}

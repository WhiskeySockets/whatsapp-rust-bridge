//! Lossless interop with the libsignal-node (Baileys upstream) on-disk session
//! format. wacore stores a session as the official WhatsApp `RecordStructure`
//! protobuf; libsignal-node stores a JSON `{_sessions, version}` object whose
//! skipped message keys are raw 32-byte HKDF seeds. wacore keeps only the
//! post-HKDF split (cipher/mac/iv) and HKDF is one-way, so the seeds cannot be
//! recovered from a wacore record alone.
//!
//! To make the round-trip lossless WITHOUT changing the upstream proto, the
//! bridge keeps those seeds in a small local `SessionSeeds` message (defined
//! here with `prost` derives — same wire format as a `.proto`, but no `protoc`
//! build dependency) persisted alongside the wacore record. `export_legacy_session`
//! rebuilds the exact libsignal-node JSON from the pair.

use base64::prelude::*;
use js_sys::{Array, Object, Reflect, Uint8Array};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

use prost::Message;
use waproto::whatsapp::session_structure::chain::MessageKey;
use waproto::whatsapp::{RecordStructure, SenderKeyRecordStructure, SessionStructure};

use wacore_libsignal::protocol::MessageKeyGenerator;

/// A captured seed is trustworthy only if it re-derives to the exact split
/// wacore stored for that key — reuse wacore's own derivation to check.
fn seed_matches_record(seed: &[u8; 32], index: u32, record: &MessageKey) -> bool {
    let derived = MessageKeyGenerator::new_from_seed(seed, index).into_pb();
    derived.cipher_key == record.cipher_key
        && derived.mac_key == record.mac_key
        && derived.iv == record.iv
}

// libsignal-node enum values (base_key_type.js / chain_type.js).
const BASE_KEY_TYPE_OURS: f64 = 1.0;
const BASE_KEY_TYPE_THEIRS: f64 = 2.0;
const CHAIN_TYPE_SENDING: f64 = 1.0;
const CHAIN_TYPE_RECEIVING: f64 = 2.0;

/// Sidecar for the libsignal-node session details the wacore proto can't carry:
/// skipped-key seeds (per chain) and per-session metadata (`baseKeyType`,
/// `lastRemoteEphemeralKey`). Hand-derived `prost::Message` (no `.proto`/`protoc`).
#[derive(Clone, PartialEq, Message)]
pub struct SessionSeeds {
    #[prost(message, repeated, tag = "1")]
    pub chains: Vec<ChainSeeds>,
    #[prost(message, repeated, tag = "2")]
    pub sessions: Vec<SessionMeta>,
}

/// Per-session libsignal-node fields absent from wacore's `SessionStructure`,
/// keyed by the session's base key. `base_key_type`: 1=OURS, 2=THEIRS, 0=unknown.
/// `has_index_info` distinguishes a real imported indexInfo (whose timestamps we
/// preserve verbatim) from a bridge-native session (synthesize on export).
#[derive(Clone, PartialEq, Message)]
pub struct SessionMeta {
    #[prost(bytes = "vec", tag = "1")]
    pub base_key: Vec<u8>,
    #[prost(uint32, tag = "2")]
    pub base_key_type: u32,
    #[prost(bytes = "vec", tag = "3")]
    pub last_remote_ephemeral: Vec<u8>,
    #[prost(bool, tag = "4")]
    pub has_index_info: bool,
    #[prost(double, tag = "5")]
    pub used: f64,
    #[prost(double, tag = "6")]
    pub created: f64,
    #[prost(double, tag = "7")]
    pub closed: f64,
}

#[derive(Clone, PartialEq, Message)]
pub struct ChainSeeds {
    #[prost(bytes = "vec", tag = "1")]
    pub ratchet_key: Vec<u8>,
    #[prost(message, repeated, tag = "2")]
    pub seeds: Vec<MessageKeySeed>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MessageKeySeed {
    #[prost(uint32, tag = "1")]
    pub index: u32,
    #[prost(bytes = "vec", tag = "2")]
    pub seed: Vec<u8>,
}

#[inline]
fn b64(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(bytes)
}

#[inline]
fn set(obj: &Object, key: &str, value: &JsValue) -> Result<(), JsValue> {
    Reflect::set(obj, &JsValue::from_str(key), value)?;
    Ok(())
}

#[inline]
fn set_str(obj: &Object, key: &str, value: &str) -> Result<(), JsValue> {
    set(obj, key, &JsValue::from_str(value))
}

#[inline]
fn set_num(obj: &Object, key: &str, value: f64) -> Result<(), JsValue> {
    set(obj, key, &JsValue::from_f64(value))
}

/// wacore `ChainKey.index` is the next-to-derive counter (fresh = 0); JS
/// `chainKey.counter` is the last-used one (fresh = -1). Inverse of the +1 the
/// migration applies on the way in.
#[inline]
fn rust_index_to_js_counter(index: u32) -> f64 {
    index as f64 - 1.0
}

/// Rebuild the libsignal-node `{_sessions, version}` JSON object from a wacore
/// `RecordStructure` plus its seed sidecar. Inverse of the migration in
/// `storage_adapter`; together they round-trip a Baileys session losslessly.
#[wasm_bindgen(js_name = exportLegacySession)]
pub fn export_legacy_session(record: &[u8], seeds: &[u8]) -> Result<JsValue, JsValue> {
    let record = RecordStructure::decode(record)
        .map_err(|e| JsValue::from_str(&format!("exportLegacySession: bad record: {e}")))?;
    let seeds = SessionSeeds::decode(seeds)
        .map_err(|e| JsValue::from_str(&format!("exportLegacySession: bad seeds: {e}")))?;
    record_to_legacy_json(&record, &seeds)
}

/// Pure (non-wasm) core of `exportLegacySession`, callable from the storage
/// adapter's write path so `store_session` can persist the Baileys on-disk
/// format directly.
pub fn record_to_legacy_json(
    record: &RecordStructure,
    seeds: &SessionSeeds,
) -> Result<JsValue, JsValue> {
    // ratchet_key bytes -> (counter -> seed). Built once, shared across chains.
    let mut seed_map: HashMap<Vec<u8>, HashMap<u32, Vec<u8>>> = HashMap::new();
    for chain in &seeds.chains {
        let entry = seed_map.entry(chain.ratchet_key.clone()).or_default();
        for s in &chain.seeds {
            entry.insert(s.index, s.seed.clone());
        }
    }
    // base_key bytes -> per-session meta (baseKeyType, lastRemoteEphemeral).
    let meta_map: HashMap<&[u8], &SessionMeta> = seeds
        .sessions
        .iter()
        .map(|m| (m.base_key.as_slice(), m))
        .collect();

    let sessions = Object::new();

    if let Some(current) = record.current_session.as_ref() {
        let (base_key, entry) = session_to_entry(current, &seed_map, &meta_map, -1.0)?;
        set(&sessions, &base_key, &entry)?;
    }

    // Archived sessions: wacore drops the original `closed` timestamp, so
    // synthesize a descending one (most-recently-archived first) — only its
    // ordering matters to libsignal-node's removeOldSessions.
    for (i, prev) in record.previous_sessions.iter().enumerate() {
        let (base_key, entry) = session_to_entry(
            prev,
            &seed_map,
            &meta_map,
            (record.previous_sessions.len() - i) as f64,
        )?;
        if !Reflect::has(&sessions, &JsValue::from_str(&base_key)).unwrap_or(false) {
            set(&sessions, &base_key, &entry)?;
        }
    }

    let out = Object::new();
    set(&out, "_sessions", &sessions)?;
    set_str(&out, "version", "v1")?;
    Ok(out.into())
}

/// Build one libsignal-node `SessionEntry`; returns `(baseKeyBase64, entry)`.
fn session_to_entry(
    session: &SessionStructure,
    seed_map: &HashMap<Vec<u8>, HashMap<u32, Vec<u8>>>,
    meta_map: &HashMap<&[u8], &SessionMeta>,
    closed: f64,
) -> Result<(String, Object), JsValue> {
    let empty = Vec::new();
    let base_key = session.alice_base_key.as_deref().unwrap_or(&empty);
    let meta = meta_map.get(base_key).copied();

    let sender_chain = session.sender_chain.as_ref();
    let sender_pub = sender_chain
        .and_then(|c| c.sender_ratchet_key.as_deref())
        .unwrap_or(&empty);
    let sender_priv = sender_chain
        .and_then(|c| c.sender_ratchet_key_private.as_deref())
        .unwrap_or(&empty);

    // lastRemoteEphemeralKey = the most recent peer ratchet = the tail receiver
    // chain (wacore appends the newest), so derive it from the CURRENT record —
    // this stays correct after the bridge ratchets. Only fall back to the imported
    // meta when there's no receiver chain yet (an initiator before its first
    // reply), where the record can't supply it.
    let last_remote = session
        .receiver_chains
        .last()
        .and_then(|c| c.sender_ratchet_key.as_deref())
        .filter(|b| !b.is_empty())
        .or_else(|| {
            meta.map(|m| m.last_remote_ephemeral.as_slice())
                .filter(|b| !b.is_empty())
        })
        .unwrap_or(&empty);

    let ratchet = Object::new();
    let ekp = Object::new();
    set_str(&ekp, "pubKey", &b64(sender_pub))?;
    set_str(&ekp, "privKey", &b64(sender_priv))?;
    set(&ratchet, "ephemeralKeyPair", &ekp)?;
    set_str(&ratchet, "lastRemoteEphemeralKey", &b64(last_remote))?;
    set_num(
        &ratchet,
        "previousCounter",
        session.previous_counter.unwrap_or(0) as f64,
    )?;
    set_str(
        &ratchet,
        "rootKey",
        &b64(session.root_key.as_deref().unwrap_or(&empty)),
    )?;

    let index_info = Object::new();
    set_str(&index_info, "baseKey", &b64(base_key))?;
    // baseKeyType: prefer the captured value (the only source once `pendingPreKey`
    // has been cleared by an ack); fall back to the pendingPreKey discriminator
    // (present → we initiated → OURS).
    let base_key_type = match meta.map(|m| m.base_key_type) {
        Some(1) => BASE_KEY_TYPE_OURS,
        Some(2) => BASE_KEY_TYPE_THEIRS,
        _ if session.pending_pre_key.is_some() => BASE_KEY_TYPE_OURS,
        _ => BASE_KEY_TYPE_THEIRS,
    };
    set_num(&index_info, "baseKeyType", base_key_type)?;
    // Preserve the original timestamps for an imported session (they drive
    // libsignal-node's decrypt-attempt order + age-based pruning); synthesize for
    // a bridge-native one (`closed` from the caller, used/created unknown → 0).
    let with_meta = meta.filter(|m| m.has_index_info);
    set_num(
        &index_info,
        "closed",
        with_meta.map(|m| m.closed).unwrap_or(closed),
    )?;
    set_num(
        &index_info,
        "used",
        with_meta.map(|m| m.used).unwrap_or(0.0),
    )?;
    set_num(
        &index_info,
        "created",
        with_meta.map(|m| m.created).unwrap_or(0.0),
    )?;
    set_str(
        &index_info,
        "remoteIdentityKey",
        &b64(session.remote_identity_public.as_deref().unwrap_or(&empty)),
    )?;

    let chains = Object::new();
    if let Some(c) = sender_chain {
        let chain = chain_to_js(c, CHAIN_TYPE_SENDING, sender_pub, seed_map, true)?;
        set(&chains, &b64(sender_pub), &chain)?;
    }
    // Only the newest (tail) receiver chain is "live"; libsignal-node marks older
    // ones closed by omitting `chainKey.key`. wacore keeps their keys to decrypt
    // late messages, but a faithful revert must close them (their cached
    // messageKeys are still emitted, so old skipped messages stay decryptable).
    let last_rc = session.receiver_chains.len().saturating_sub(1);
    for (i, rc) in session.receiver_chains.iter().enumerate() {
        let ratchet_key = rc.sender_ratchet_key.as_deref().unwrap_or(&empty);
        let chain = chain_to_js(
            rc,
            CHAIN_TYPE_RECEIVING,
            ratchet_key,
            seed_map,
            i == last_rc,
        )?;
        set(&chains, &b64(ratchet_key), &chain)?;
    }

    let entry = Object::new();
    set_num(
        &entry,
        "registrationId",
        session.remote_registration_id.unwrap_or(0) as f64,
    )?;
    set(&entry, "currentRatchet", &ratchet)?;
    set(&entry, "indexInfo", &index_info)?;
    set(&entry, "_chains", &chains)?;

    if let Some(ppk) = session.pending_pre_key.as_ref() {
        let pending = Object::new();
        if let Some(id) = ppk.pre_key_id {
            set_num(&pending, "preKeyId", id as f64)?;
        }
        set_num(
            &pending,
            "signedKeyId",
            ppk.signed_pre_key_id.unwrap_or(0) as f64,
        )?;
        set_str(
            &pending,
            "baseKey",
            &b64(ppk.base_key.as_deref().unwrap_or(&empty)),
        )?;
        set(&entry, "pendingPreKey", &pending)?;
    }

    Ok((b64(base_key), entry))
}

/// Build one `_chains` value: `{ chainKey: {counter, key}, chainType, messageKeys }`.
fn chain_to_js(
    chain: &waproto::whatsapp::session_structure::Chain,
    chain_type: f64,
    ratchet_key: &[u8],
    seed_map: &HashMap<Vec<u8>, HashMap<u32, Vec<u8>>>,
    live: bool,
) -> Result<Object, JsValue> {
    let ck = Object::new();
    let index = chain.chain_key.as_ref().and_then(|c| c.index).unwrap_or(0);
    set_num(&ck, "counter", rust_index_to_js_counter(index))?;
    // Emit `chainKey.key` only for a live chain that actually has a key. A closed
    // chain (libsignal-node deletes its key) or an explicitly-closed older
    // receiver chain leaves it absent — keeping it closed rather than reviving it.
    let chain_key_bytes = chain.chain_key.as_ref().and_then(|c| c.key.as_deref());
    if let Some(key) = chain_key_bytes.filter(|k| live && !k.is_empty()) {
        set_str(&ck, "key", &b64(key))?;
    }

    // Skipped message keys: re-emit the raw seed for each key still present in
    // the record. A seed is emitted ONLY if it re-derives to the exact split
    // (cipher/mac/iv) wacore stored for that key — this self-validates the
    // sidecar, so a stale/forged/wrong-session seed is dropped (peer retries that
    // one message) rather than exported as wrong key material. Keys the bridge
    // never captured a seed for are likewise skipped.
    let message_keys = Object::new();
    let seeds = seed_map.get(ratchet_key);
    for mk in &chain.message_keys {
        let index = mk.index.unwrap_or(0);
        let Some(seed) = seeds.and_then(|m| m.get(&index)) else {
            continue;
        };
        let Ok(seed_arr) = <[u8; 32]>::try_from(seed.as_slice()) else {
            continue;
        };
        if seed_matches_record(&seed_arr, index, mk) {
            set_str(&message_keys, &index.to_string(), &b64(seed))?;
        }
    }

    let chain = Object::new();
    set(&chain, "chainKey", &ck)?;
    set_num(&chain, "chainType", chain_type)?;
    set(&chain, "messageKeys", &message_keys)?;
    Ok(chain)
}

/// Node's `Buffer.toJSON()` shape — `{type:'Buffer', data:[…bytes]}` with a
/// numeric array — which is exactly what Baileys' sender-key store produces
/// (`JSON.stringify(record.serialize())`, no BufferJSON replacer). Baileys'
/// reviver and the bridge's own reader both expect this array form.
fn buffer_json(bytes: &[u8]) -> Result<JsValue, JsValue> {
    let o = Object::new();
    set_str(&o, "type", "Buffer")?;
    let data = Array::new();
    for &b in bytes {
        data.push(&JsValue::from_f64(b as f64));
    }
    set(&o, "data", &data)?;
    Ok(o.into())
}

/// Reverse of `migrate_legacy_sender_key`: a wacore `SenderKeyRecordStructure`
/// → the UTF-8 bytes of libsignal-node's `JSON.stringify(states)` (Baileys'
/// on-disk sender-key format). Lets a group session revert to Baileys.
pub fn sender_key_record_to_legacy_json(record: &[u8]) -> Result<Vec<u8>, JsValue> {
    let rec = SenderKeyRecordStructure::decode(record)
        .map_err(|e| JsValue::from_str(&format!("exportLegacySenderKey: bad record: {e}")))?;

    let states = Array::new();
    for s in &rec.sender_key_states {
        let state = Object::new();
        set_num(&state, "senderKeyId", s.sender_key_id.unwrap_or(0) as f64)?;

        let chain = Object::new();
        if let Some(ck) = s.sender_chain_key.as_ref() {
            set_num(&chain, "iteration", ck.iteration.unwrap_or(0) as f64)?;
            set(
                &chain,
                "seed",
                &buffer_json(ck.seed.as_deref().unwrap_or(&[]))?,
            )?;
        }
        set(&state, "senderChainKey", &chain)?;

        let signing = Object::new();
        if let Some(sk) = s.sender_signing_key.as_ref() {
            set(
                &signing,
                "public",
                &buffer_json(sk.public.as_deref().unwrap_or(&[]))?,
            )?;
            if let Some(private_key) = sk.private.as_deref() {
                set(&signing, "private", &buffer_json(private_key)?)?;
            }
        }
        set(&state, "senderSigningKey", &signing)?;

        let mks = Array::new();
        for mk in &s.sender_message_keys {
            let m = Object::new();
            set_num(&m, "iteration", mk.iteration.unwrap_or(0) as f64)?;
            set(&m, "seed", &buffer_json(mk.seed.as_deref().unwrap_or(&[]))?)?;
            mks.push(&m);
        }
        set(&state, "senderMessageKeys", &mks)?;

        states.push(&state);
    }

    let json = js_sys::JSON::stringify(&states)?;
    let s = json
        .as_string()
        .ok_or_else(|| JsValue::from_str("exportLegacySenderKey: stringify produced no string"))?;
    Ok(s.into_bytes())
}

/// wasm wrapper around `sender_key_record_to_legacy_json`.
#[wasm_bindgen(js_name = exportLegacySenderKey)]
pub fn export_legacy_sender_key(record: &[u8]) -> Result<Uint8Array, JsValue> {
    let bytes = sender_key_record_to_legacy_json(record)?;
    Ok(Uint8Array::from(bytes.as_slice()))
}

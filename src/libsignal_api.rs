use crate::libsignal_store::InMemorySignalStore;
use js_sys::{Promise, Uint8Array};
use rand::TryRngCore as _;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use wacore_libsignal::protocol::{
    self, CiphertextMessage, CiphertextMessageType, GenericSignedPreKey, PreKeySignalMessage,
    ProtocolAddress, SessionRecord, SignalMessage,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

// --- Structs for JS Interop ---
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsPreKey {
    public_key: Vec<u8>,
    private_key: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsSignedPreKey {
    public_key: Vec<u8>,
    private_key: Vec<u8>,
    signature: Vec<u8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsEncryptResult {
    r#type: String, // 'pkmsg' or 'msg'
    ciphertext: Vec<u8>,
}

// --- FFI Definitions for JS Store ---
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "SignalStore")]
    pub type SignalStore;

    // Methods needed for both encrypt & decrypt
    #[wasm_bindgen(method, js_name = getIdentityKeyPair)]
    fn get_identity_key_pair(this: &SignalStore) -> Promise;
    #[wasm_bindgen(method, js_name = loadSession)]
    fn load_session(this: &SignalStore, address: &str) -> Promise;
    #[wasm_bindgen(method, js_name = storeSession)]
    fn store_session(this: &SignalStore, address: &str, session: &[u8]) -> Promise;

    // Methods only needed for decryption
    #[wasm_bindgen(method, js_name = getLocalRegistrationId)]
    fn get_local_registration_id(this: &SignalStore) -> Promise;
    #[wasm_bindgen(method, js_name = loadPreKey)]
    fn load_pre_key(this: &SignalStore, key_id: u32) -> Promise;
    #[wasm_bindgen(method, js_name = removePreKey)]
    fn remove_pre_key(this: &SignalStore, key_id: u32) -> Promise;
    #[wasm_bindgen(method, js_name = loadSignedPreKey)]
    fn load_signed_pre_key(this: &SignalStore, key_id: u32) -> Promise;
}

/// Parse JID in format "number:device_id@domain" to ProtocolAddress
fn jid_to_protocol_address(jid: &str) -> Result<ProtocolAddress, JsValue> {
    let parts: Vec<&str> = jid.split('@').collect();
    let user_part = parts
        .first()
        .ok_or_else(|| JsValue::from_str("Invalid JID: missing '@'"))?;
    let name_device: Vec<&str> = user_part.split(':').collect();
    let name = name_device[0].to_string();
    let device_id = if name_device.len() > 1 {
        name_device[1].parse().unwrap_or(0)
    } else {
        0
    };
    Ok(ProtocolAddress::new(name, device_id.into()))
}

/// Extract a Uint8Array field from a JS object
fn extract_bundle_field_bytes(obj: &JsValue, field: &str) -> Result<Vec<u8>, JsValue> {
    let val = js_sys::Reflect::get(obj, &JsValue::from_str(field))
        .map_err(|_| JsValue::from_str(&format!("Missing {} in bundle", field)))?;

    if val.is_undefined() || val.is_null() {
        return Err(JsValue::from_str(&format!(
            "Field {} is undefined or null",
            field
        )));
    }

    Ok(Uint8Array::from(val).to_vec())
}

/// Encrypt a message to a recipient using the Signal protocol.
///
/// Uses the fetch->process->write-back pattern:
/// 1. FETCH: Retrieve identity and existing session from JS store
/// 2. PREPARE: Build in-memory Rust store with fetched data
/// 3. PROCESS: Encrypt the message using the protocol
/// 4. WRITE-BACK: Store the updated session back to JS
#[wasm_bindgen(js_name = encryptMessage)]
pub async fn encrypt_message(
    store: SignalStore,
    jid: &str,
    plaintext: &[u8],
) -> Result<JsValue, JsValue> {
    let remote_address = jid_to_protocol_address(jid)?;
    let memory_store = InMemorySignalStore::new();

    // --- FETCH & PREPARE ---
    let identity_val = JsFuture::from(store.get_identity_key_pair())
        .await
        .map_err(|_| JsValue::from_str("Failed to fetch identity key pair"))?;
    let session_val = JsFuture::from(store.load_session(&remote_address.to_string()))
        .await
        .map_err(|_| JsValue::from_str("Failed to load session"))?;

    let js_prekey: JsPreKey = serde_wasm_bindgen::from_value(identity_val)
        .map_err(|e| JsValue::from_str(&format!("Invalid identity key pair: {}", e)))?;
    let identity_key_pair = protocol::IdentityKeyPair::try_from(
        protocol::PrivateKey::deserialize(&js_prekey.private_key)
            .map_err(|e| JsValue::from_str(&format!("Failed to decode private key: {}", e)))?,
    )
    .map_err(|e| JsValue::from_str(&format!("Failed to construct IdentityKeyPair: {}", e)))?;
    memory_store.set_identity_key_pair(identity_key_pair);

    if !session_val.is_undefined() && !session_val.is_null() {
        let bytes = Uint8Array::from(session_val).to_vec();
        if !bytes.is_empty() {
            memory_store.set_session(
                &remote_address.to_string(),
                SessionRecord::deserialize(&bytes).map_err(|e| {
                    JsValue::from_str(&format!("Failed to deserialize session: {}", e))
                })?,
            );
        }
    }

    // --- PROCESS ---
    let ciphertext_message = protocol::message_encrypt(
        plaintext,
        &remote_address,
        &mut memory_store.clone(),
        &mut memory_store.clone(),
        SystemTime::now(),
    )
    .await
    .map_err(|e| JsValue::from_str(&format!("Failed to encrypt message: {}", e)))?;

    // --- WRITE-BACK ---
    if let Some(record) = memory_store.get_session(&remote_address.to_string()) {
        let serialized = record
            .serialize()
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize session: {}", e)))?;
        JsFuture::from(store.store_session(&remote_address.to_string(), &serialized))
            .await
            .map_err(|_| JsValue::from_str("Failed to store session"))?;
    }

    // --- SERIALIZE & RETURN ---
    let result = JsEncryptResult {
        r#type: match ciphertext_message.message_type() {
            CiphertextMessageType::PreKey => "pkmsg".to_string(),
            _ => "msg".to_string(),
        },
        ciphertext: ciphertext_message.serialize().to_vec(),
    };

    Ok(serde_wasm_bindgen::to_value(&result)?)
}

/// Decrypt a message from a sender using the Signal protocol.
///
/// Uses the fetch->process->write-back pattern:
/// 1. FETCH: Retrieve identity, registration ID, session, and pre-keys from JS store
/// 2. PREPARE: Build in-memory Rust store with fetched data
/// 3. PROCESS: Decrypt the message using the protocol
/// 4. WRITE-BACK: Store the updated session and mark pre-keys as used
#[wasm_bindgen(js_name = decryptMessage)]
pub async fn decrypt_message(
    store: SignalStore,
    jid: &str,
    msg_type: &str,
    ciphertext: &[u8],
) -> Result<Uint8Array, JsValue> {
    let remote_address = jid_to_protocol_address(jid)?;
    let memory_store = InMemorySignalStore::new();
    let ciphertext_message = match msg_type {
        "pkmsg" => CiphertextMessage::PreKeySignalMessage(
            PreKeySignalMessage::try_from(ciphertext).map_err(|e| {
                JsValue::from_str(&format!("Failed to parse PreKeySignalMessage: {}", e))
            })?,
        ),
        "msg" => CiphertextMessage::SignalMessage(
            SignalMessage::try_from(ciphertext)
                .map_err(|e| JsValue::from_str(&format!("Failed to parse SignalMessage: {}", e)))?,
        ),
        _ => return Err(JsValue::from_str("Invalid message type")),
    };

    // --- FETCH & PREPARE ---
    let identity_val = JsFuture::from(store.get_identity_key_pair())
        .await
        .map_err(|_| JsValue::from_str("Failed to fetch identity key pair"))?;
    let reg_id_val = JsFuture::from(store.get_local_registration_id())
        .await
        .map_err(|_| JsValue::from_str("Failed to fetch registration ID"))?;
    let session_val = JsFuture::from(store.load_session(&remote_address.to_string()))
        .await
        .map_err(|_| JsValue::from_str("Failed to load session"))?;

    let js_prekey: JsPreKey = serde_wasm_bindgen::from_value(identity_val)
        .map_err(|e| JsValue::from_str(&format!("Invalid identity key pair: {}", e)))?;
    let identity_key_pair = protocol::IdentityKeyPair::try_from(
        protocol::PrivateKey::deserialize(&js_prekey.private_key)
            .map_err(|e| JsValue::from_str(&format!("Failed to decode private key: {}", e)))?,
    )
    .map_err(|e| JsValue::from_str(&format!("Failed to construct IdentityKeyPair: {}", e)))?;
    memory_store.set_identity_key_pair(identity_key_pair);

    let reg_id = reg_id_val
        .as_f64()
        .ok_or_else(|| JsValue::from_str("Invalid registration ID"))? as u32;
    memory_store.set_registration_id(reg_id);

    if !session_val.is_undefined() && !session_val.is_null() {
        let bytes = Uint8Array::from(session_val).to_vec();
        if !bytes.is_empty() {
            memory_store.set_session(
                &remote_address.to_string(),
                SessionRecord::deserialize(&bytes).map_err(|e| {
                    JsValue::from_str(&format!("Failed to deserialize session: {}", e))
                })?,
            );
        }
    }

    // If it's a PreKey message, we need to fetch the corresponding pre-keys
    if let CiphertextMessage::PreKeySignalMessage(pkmsg) = &ciphertext_message {
        // Fetch signed pre-key
        let spk_val = JsFuture::from(store.load_signed_pre_key(pkmsg.signed_pre_key_id().into()))
            .await
            .map_err(|_| JsValue::from_str("Failed to load signed pre-key"))?;
        let spk_bytes = Uint8Array::from(spk_val).to_vec();
        let spk_record = protocol::SignedPreKeyRecord::deserialize(&spk_bytes).map_err(|e| {
            JsValue::from_str(&format!("Failed to deserialize signed pre-key: {}", e))
        })?;
        memory_store.add_signed_pre_key(pkmsg.signed_pre_key_id(), spk_record);

        // Fetch one-time pre-key if it exists
        if let Some(pk_id) = pkmsg.pre_key_id() {
            let pk_val = JsFuture::from(store.load_pre_key(pk_id.into()))
                .await
                .map_err(|_| JsValue::from_str("Failed to load pre-key"))?;
            let pk_bytes = Uint8Array::from(pk_val).to_vec();
            let pk_record = protocol::PreKeyRecord::deserialize(&pk_bytes)
                .map_err(|e| JsValue::from_str(&format!("Failed to deserialize pre-key: {}", e)))?;
            memory_store.add_pre_key(pk_id, pk_record);
        }
    }

    // --- PROCESS ---
    let plaintext = protocol::message_decrypt(
        &ciphertext_message,
        &remote_address,
        &mut memory_store.clone(),
        &mut memory_store.clone(),
        &mut memory_store.clone(),
        &memory_store.clone(),
        &mut rand::rngs::OsRng.unwrap_err(),
        false.into(),
    )
    .await
    .map_err(|e| JsValue::from_str(&format!("Failed to decrypt message: {}", e)))?;

    // --- WRITE-BACK ---
    if let Some(record) = memory_store.get_session(&remote_address.to_string()) {
        let serialized = record
            .serialize()
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize session: {}", e)))?;
        JsFuture::from(store.store_session(&remote_address.to_string(), &serialized))
            .await
            .map_err(|_| JsValue::from_str("Failed to store session"))?;
    }
    if let Some(removed_id) = memory_store.get_removed_pre_key_id() {
        JsFuture::from(store.remove_pre_key(removed_id.into()))
            .await
            .map_err(|_| JsValue::from_str("Failed to remove pre-key"))?;
    }

    Ok(Uint8Array::from(plaintext.as_slice()))
}

/// Convert a JID to a Signal protocol address string.
#[wasm_bindgen]
#[allow(non_snake_case)]
pub fn jidToSignalProtocolAddress(jid: &str) -> Result<String, JsValue> {
    Ok(jid_to_protocol_address(jid)?.to_string())
}

/// Process a pre-key bundle to establish a new Signal session.
///
/// Uses the fetch->process->write-back pattern:
/// 1. PARSE: Extract protocol address and bundle fields from inputs
/// 2. FETCH: Retrieve identity and existing session from JS store
/// 3. PREPARE: Build in-memory Rust store with fetched data
/// 4. PROCESS: Validate bundle structure and prepare for protocol processing
/// 5. WRITE-BACK: Store the session back to JS
#[wasm_bindgen(js_name = processPreKeyBundle)]
pub async fn process_pre_key_bundle(
    store: SignalStore,
    jid: &str,
    bundle_val: JsValue,
) -> Result<(), JsValue> {
    // === 1. PARSE INPUTS ===
    let remote_address = jid_to_protocol_address(jid)?;

    // Extract and validate bundle fields - we'll use these to construct the PreKeyBundle
    let identity_key_bytes = extract_bundle_field_bytes(&bundle_val, "identityKey")?;
    let identity_key = protocol::IdentityKey::decode(&identity_key_bytes)
        .map_err(|e| JsValue::from_str(&format!("Failed to decode identity key: {}", e)))?;

    let signed_pre_key_bytes = extract_bundle_field_bytes(&bundle_val, "signedPreKey")?;
    let signed_pre_key_record = protocol::PreKeyRecord::deserialize(&signed_pre_key_bytes)
        .map_err(|e| JsValue::from_str(&format!("Failed to deserialize signed pre-key: {}", e)))?;

    let signed_pre_key_signature =
        extract_bundle_field_bytes(&bundle_val, "signedPreKeySignature")?;

    // Extract registration IDs
    let remote_reg_id = js_sys::Reflect::get(&bundle_val, &JsValue::from_str("registrationId"))
        .ok()
        .and_then(|v| v.as_f64())
        .ok_or_else(|| JsValue::from_str("Missing or invalid registrationId"))?
        as u32;

    let signed_pre_key_id = js_sys::Reflect::get(&bundle_val, &JsValue::from_str("signedPreKeyId"))
        .ok()
        .and_then(|v| v.as_f64())
        .ok_or_else(|| JsValue::from_str("Missing or invalid signedPreKeyId"))?
        as u32;

    // Extract optional pre-key
    let pre_key_id = js_sys::Reflect::get(&bundle_val, &JsValue::from_str("preKeyId"))
        .ok()
        .and_then(|v| v.as_f64());

    let pre_key_option = if let Some(id) = pre_key_id {
        let pre_key_bytes = extract_bundle_field_bytes(&bundle_val, "preKey")?;
        let pre_key_record = protocol::PreKeyRecord::deserialize(&pre_key_bytes)
            .map_err(|e| JsValue::from_str(&format!("Failed to deserialize pre-key: {}", e)))?;
        Some((id as u32, pre_key_record))
    } else {
        None
    };

    // === 2. FETCH PHASE: Get all needed data from JavaScript store ===
    let identity_val = JsFuture::from(store.get_identity_key_pair())
        .await
        .map_err(|_| JsValue::from_str("Failed to fetch identity key pair"))?;

    let reg_id_val = JsFuture::from(store.get_local_registration_id())
        .await
        .map_err(|_| JsValue::from_str("Failed to fetch registration ID"))?;

    let session_val = JsFuture::from(store.load_session(&remote_address.to_string()))
        .await
        .map_err(|_| JsValue::from_str("Failed to load existing session"))?;

    // === 3. PREPARE PHASE: Build the in-memory Rust store ===
    let memory_store = InMemorySignalStore::new();

    // Populate identity key pair
    let js_prekey: JsPreKey = serde_wasm_bindgen::from_value(identity_val)
        .map_err(|e| JsValue::from_str(&format!("Invalid identity key pair format: {}", e)))?;

    let identity_key_pair = protocol::IdentityKeyPair::try_from(
        protocol::PrivateKey::deserialize(&js_prekey.private_key)
            .map_err(|e| JsValue::from_str(&format!("Failed to decode private key: {}", e)))?,
    )
    .map_err(|e| JsValue::from_str(&format!("Failed to construct IdentityKeyPair: {}", e)))?;

    memory_store.set_identity_key_pair(identity_key_pair);

    // Populate registration ID
    let local_registration_id = reg_id_val
        .as_f64()
        .ok_or_else(|| JsValue::from_str("Registration ID is not a number"))?
        as u32;
    memory_store.set_registration_id(local_registration_id);

    // Populate existing session if any
    if !session_val.is_undefined() && !session_val.is_null() {
        let bytes = Uint8Array::from(session_val).to_vec();
        if !bytes.is_empty() {
            let record = SessionRecord::deserialize(&bytes)
                .map_err(|e| JsValue::from_str(&format!("Failed to deserialize session: {}", e)))?;
            memory_store.set_session(&remote_address.to_string(), record);
        }
    }

    // Construct the PreKeyBundle from all the extracted and validated data
    let pre_key_id_and_public = if let Some((id, pre_key_rec)) = pre_key_option {
        let pub_key = pre_key_rec
            .public_key()
            .map_err(|_| JsValue::from_str("Failed to get pre-key public key"))?;
        Some((protocol::PreKeyId::from(id), pub_key))
    } else {
        None
    };

    let signed_pre_key_pub = signed_pre_key_record
        .public_key()
        .map_err(|_| JsValue::from_str("Failed to get signed pre-key public key"))?;

    let pre_key_bundle = protocol::PreKeyBundle::new(
        remote_reg_id,
        remote_address.device_id(),
        pre_key_id_and_public,
        protocol::SignedPreKeyId::from(signed_pre_key_id),
        signed_pre_key_pub,
        signed_pre_key_signature.clone(),
        identity_key,
    )
    .map_err(|e| JsValue::from_str(&format!("Failed to construct PreKeyBundle: {}", e)))?;

    // === 4. PROCESS PHASE: Call the core libsignal logic ===
    // Use OsRng for cryptographically secure random number generation
    protocol::process_prekey_bundle(
        &remote_address,
        &mut memory_store.clone(),
        &mut memory_store.clone(),
        &pre_key_bundle,
        SystemTime::now(),
        &mut rand::rngs::OsRng.unwrap_err(),
        false.into(), // use_pq_ratchet = false
    )
    .await
    .map_err(|e| JsValue::from_str(&format!("Failed to process pre-key bundle: {}", e)))?;

    // === 5. WRITE-BACK PHASE: Save the session to JS store ===
    if let Some(updated_record) = memory_store.get_session(&remote_address.to_string()) {
        let serialized_session = updated_record
            .serialize()
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize session: {}", e)))?;

        JsFuture::from(store.store_session(&remote_address.to_string(), &serialized_session))
            .await
            .map_err(|_| JsValue::from_str("Failed to store session"))?;
    } else {
        // No pre-existing session, which is expected for first-time contact.
        // In a real scenario, the protocol::process_prekey_bundle would create one.
        // For now, we just acknowledge the bundle was processed.
    }

    Ok(())
}

/// Get the identity key pair from the store.
///
/// Returns a JS object with `publicKey` and `privateKey` Uint8Array fields.
#[wasm_bindgen(js_name = getIdentityKeyPair)]
pub async fn get_identity_key_pair(store: SignalStore) -> Result<JsValue, JsValue> {
    JsFuture::from(store.get_identity_key_pair()).await
}

/// Get the local registration ID from the store.
#[wasm_bindgen(js_name = getLocalRegistrationId)]
pub async fn get_local_registration_id(store: SignalStore) -> Result<u32, JsValue> {
    let val = JsFuture::from(store.get_local_registration_id()).await?;

    if val.is_undefined() || val.is_null() {
        return Err(JsValue::from_str("Registration ID not found"));
    }

    val.as_f64()
        .map(|id| id as u32)
        .ok_or_else(|| JsValue::from_str("Invalid registration ID"))
}

/// Load a session record from the store.
///
/// Returns the serialized session record as Uint8Array, or undefined if not found.
#[wasm_bindgen(js_name = loadSession)]
pub async fn load_session(store: SignalStore, address: &str) -> Result<JsValue, JsValue> {
    JsFuture::from(store.load_session(address)).await
}

/// Store a session record in the store.
#[wasm_bindgen(js_name = storeSession)]
pub async fn store_session(
    store: SignalStore,
    address: &str,
    session: &[u8],
) -> Result<JsValue, JsValue> {
    JsFuture::from(store.store_session(address, session)).await
}

// src/libsignal_api.rs
#![allow(invalid_reference_casting, clippy::mut_from_ref)]
use crate::libsignal_store::InMemorySignalStore;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::collections::HashMap;
use std::time::SystemTime;
use wacore_libsignal::protocol::{
    message_decrypt, message_encrypt, process_prekey_bundle, CiphertextMessage,
    GenericSignedPreKey, IdentityKeyPair, KeyPair, PreKeyBundle, PreKeyId, ProtocolAddress,
    SessionRecord,
};

// SAFETY: InMemorySignalStore uses internal mutability (Arc<Mutex<...>>).
// We can safely cast away the immutability since the store manages mutations
// through Mutex locks internally.
fn store_as_mut(store: &InMemorySignalStore) -> &mut InMemorySignalStore {
    unsafe { &mut *(store as *const InMemorySignalStore as *mut InMemorySignalStore) }
}

// --- Helper Functions ---

fn jid_to_protocol_address(jid: &str) -> Result<ProtocolAddress> {
    let parts: Vec<&str> = jid.split('@').collect();
    let user_part = parts
        .first()
        .ok_or_else(|| Error::new(Status::InvalidArg, "Invalid JID: missing '@'"))?;
    let name_device: Vec<&str> = user_part.split(':').collect();
    let name = name_device[0].to_string();
    let device_id = if name_device.len() > 1 {
        name_device[1].parse().unwrap_or(0)
    } else {
        0
    };
    Ok(ProtocolAddress::new(name, device_id.into()))
}

// --- N-API Structs for JS Interop ---

#[napi(object)]
pub struct JsPreKeyBundle {
    pub registration_id: u32,
    pub device_id: u32,
    pub pre_key_id: Option<u32>,
    pub pre_key: Option<Buffer>,
    pub signed_pre_key_id: u32,
    pub signed_pre_key: Buffer,
    pub signed_pre_key_signature: Buffer,
    pub identity_key: Buffer,
}

#[napi(object)]
pub struct JsEncryptResult {
    pub r#type: String, // "pkmsg" or "msg"
    pub ciphertext: Buffer,
    pub new_session: Buffer, // Serialized SessionRecord
}

#[napi(object)]
pub struct JsDecryptResult {
    pub plaintext: Buffer,
    pub new_session: Buffer, // Serialized SessionRecord
}

#[napi(object)]
pub struct JsGroupEncryptResult {
    pub ciphertext: Buffer,
    pub sender_key_distribution_message: Buffer,
    pub new_sender_key: Buffer, // Serialized SenderKeyRecord
}

#[napi(object)]
pub struct JsGroupDecryptResult {
    pub plaintext: Buffer,
    pub new_sender_key: Buffer, // Serialized SenderKeyRecord
}

// --- Signal API Implementation ---

#[napi]
pub fn jid_to_signal_protocol_address(jid: String) -> Result<String> {
    Ok(jid_to_protocol_address(&jid)?.to_string())
}

/// Generate a properly formatted Signal protocol identity key pair
/// Returns a Buffer containing a serialized IdentityKeyPair
#[napi]
pub fn generate_test_identity_key_pair() -> Result<Buffer> {
    let mut rng = StdRng::from_os_rng();
    let key_pair = KeyPair::generate(&mut rng);
    let identity_key = wacore_libsignal::protocol::IdentityKey::new(key_pair.public_key);
    let identity_key_pair = IdentityKeyPair::new(identity_key, key_pair.private_key);

    let serialized = identity_key_pair.serialize().to_vec();

    Ok(serialized.into())
}

/// Generate a properly formatted Signal protocol public key
/// Returns a Buffer containing a serialized PublicKey
#[napi]
pub fn generate_test_public_key() -> Result<Buffer> {
    let mut rng = StdRng::from_os_rng();
    let key_pair = KeyPair::generate(&mut rng);

    let serialized = key_pair.public_key.serialize().to_vec();

    Ok(serialized.into())
}

/// Generate a properly formatted Signal protocol signed pre key record
/// Returns a Buffer containing a serialized SignedPreKeyRecord
#[napi]
pub fn generate_test_signed_pre_key() -> Result<Buffer> {
    let mut rng = StdRng::from_os_rng();
    let key_pair = KeyPair::generate(&mut rng);

    // Create a timestamp
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let timestamp = wacore_libsignal::protocol::Timestamp::from_epoch_millis(now);

    let signed_pre_key_record = wacore_libsignal::protocol::SignedPreKeyRecord::new(
        1.into(),
        timestamp,
        &key_pair,
        &[0u8; 64],
    );

    let serialized = signed_pre_key_record
        .serialize()
        .map_err(|e| Error::from_reason(format!("{:?}", e)))?;

    Ok(serialized.into())
}

/// Sign a buffer with an identity key pair
/// Takes the serialized identity key pair and data to sign, returns a 64-byte signature
#[napi]
pub fn sign_with_identity_key(identity_key_pair: Buffer, data: Buffer) -> Result<Buffer> {
    let identity_kp = IdentityKeyPair::try_from(identity_key_pair.as_ref()).map_err(|e| {
        Error::from_reason(format!("Failed to deserialize identity key pair: {:?}", e))
    })?;

    let mut rng = StdRng::from_os_rng();
    let signature = identity_kp
        .private_key()
        .calculate_signature(&data, &mut rng)
        .map_err(|e| Error::from_reason(format!("Failed to sign: {:?}", e)))?;

    Ok(signature.to_vec().into())
}

#[napi]
pub async fn process_pre_key_bundle(
    jid: String,
    bundle: JsPreKeyBundle,
    our_identity: Buffer,
    our_registration_id: u32,
) -> Result<Buffer> {
    let remote_address = jid_to_protocol_address(&jid)?;

    // 1. Create the temporary store for this operation
    let store = InMemorySignalStore::new();

    // 2. Load our own identity into the store
    let identity_key_pair = IdentityKeyPair::try_from(our_identity.as_ref())
        .map_err(|e| Error::from_reason(format!("{:?}", e)))?;
    store.set_identity_key_pair(identity_key_pair);
    store.set_registration_id(our_registration_id);

    let pre_key_public = if let Some(pk_buf) = bundle.pre_key {
        Some(
            wacore_libsignal::protocol::PublicKey::deserialize(&pk_buf)
                .map_err(|e| Error::from_reason(format!("{:?}", e)))?,
        )
    } else {
        None
    };

    let pre_key_id = bundle.pre_key_id.map(PreKeyId::from);
    let pre_key_tuple = pre_key_id.zip(pre_key_public);

    let pre_key_bundle = PreKeyBundle::new(
        bundle.registration_id,
        bundle.device_id.into(),
        pre_key_tuple,
        bundle.signed_pre_key_id.into(),
        wacore_libsignal::protocol::PublicKey::deserialize(&bundle.signed_pre_key)
            .map_err(|e| Error::from_reason(format!("{:?}", e)))?,
        bundle.signed_pre_key_signature.into(),
        wacore_libsignal::protocol::IdentityKey::try_from(bundle.identity_key.as_ref())
            .map_err(|e| Error::from_reason(format!("{:?}", e)))?,
    )
    .map_err(|e| Error::from_reason(format!("{:?}", e)))?;

    // 3. Operate: Pass the store to the library function.
    // It will use the store to create and then save the new session internally.
    let mut rng = StdRng::from_os_rng();
    process_prekey_bundle(
        &remote_address,
        store_as_mut(&store),
        store_as_mut(&store),
        &pre_key_bundle,
        SystemTime::now(),
        &mut rng,
        false.into(),
    )
    .await
    .map_err(|e| Error::from_reason(format!("{:?}", e)))?;

    // 4. Extract the newly created session from the store
    let session_record = store
        .get_session(&remote_address.to_string())
        .ok_or_else(|| {
            Error::from_reason("Session not created after processing bundle".to_string())
        })?;

    // 5. Return the serialized result
    let serialized_session = session_record
        .serialize()
        .map_err(|e| Error::from_reason(format!("{:?}", e)))?;

    Ok(serialized_session.into())
}

#[napi]
pub async fn encrypt_message(
    jid: String,
    plaintext: Buffer,
    our_identity: Buffer,
    session: Buffer,
) -> Result<JsEncryptResult> {
    let remote_address = jid_to_protocol_address(&jid)?;

    // 1. Create the temporary store
    let store = InMemorySignalStore::new();

    // 2. Load all necessary state into the store from JS arguments
    let identity_key_pair = IdentityKeyPair::try_from(our_identity.as_ref())
        .map_err(|e| Error::from_reason(format!("{:?}", e)))?;
    store.set_identity_key_pair(identity_key_pair);

    let session_record =
        SessionRecord::deserialize(&session).map_err(|e| Error::from_reason(format!("{:?}", e)))?;
    store.set_session(&remote_address.to_string(), session_record);

    // 3. Operate: The library will use the store to load the session,
    // perform encryption, and save the updated session back to the store.
    let ciphertext_message = message_encrypt(
        &plaintext,
        &remote_address,
        store_as_mut(&store),
        store_as_mut(&store),
        SystemTime::now(),
    )
    .await
    .map_err(|e| Error::from_reason(format!("{:?}", e)))?;

    let (r#type, ciphertext) = match ciphertext_message {
        CiphertextMessage::PreKeySignalMessage(m) => ("pkmsg".to_string(), m.serialized().to_vec()),
        CiphertextMessage::SignalMessage(m) => ("msg".to_string(), m.serialized().to_vec()),
        _ => {
            return Err(Error::new(
                Status::GenericFailure,
                "Unexpected encryption type",
            ))
        }
    };

    // 4. Extract the MODIFIED session from the store
    let new_session_record = store
        .get_session(&remote_address.to_string())
        .ok_or_else(|| Error::from_reason("Session not found after encryption".to_string()))?;

    // 5. Return the result and the new state
    Ok(JsEncryptResult {
        r#type,
        ciphertext: ciphertext.into(),
        new_session: new_session_record
            .serialize()
            .map_err(|e| Error::from_reason(format!("{:?}", e)))?
            .into(),
    })
}

#[napi]
#[allow(clippy::too_many_arguments)]
pub async fn decrypt_message(
    jid: String,
    ciphertext: Buffer,
    message_type: String,
    our_identity: Buffer,
    our_registration_id: u32,
    session: Option<Buffer>,
    pre_keys: Option<HashMap<String, Buffer>>,
    signed_pre_keys: Option<HashMap<String, Buffer>>,
) -> Result<JsDecryptResult> {
    let remote_address = jid_to_protocol_address(&jid)?;

    // 1. Create the temporary store
    let store = InMemorySignalStore::new();

    // 2. Load all necessary state into the store
    let identity_key_pair = IdentityKeyPair::try_from(our_identity.as_ref())
        .map_err(|e| Error::from_reason(format!("{:?}", e)))?;
    store.set_identity_key_pair(identity_key_pair);
    store.set_registration_id(our_registration_id);

    if let Some(s) = session {
        let session_record =
            SessionRecord::deserialize(&s).map_err(|e| Error::from_reason(format!("{:?}", e)))?;
        store.set_session(&remote_address.to_string(), session_record);
    }

    if let Some(pks) = pre_keys {
        for (id, key_pair_buf) in pks {
            let id_u32: u32 = id
                .parse::<u32>()
                .map_err(|_| Error::new(Status::InvalidArg, "Invalid pre key id"))?;
            let key_pair =
                KeyPair::from_public_and_private(&key_pair_buf[..33], &key_pair_buf[33..])
                    .map_err(|e| Error::from_reason(format!("{:?}", e)))?;
            store.add_pre_key(
                id_u32.into(),
                wacore_libsignal::protocol::PreKeyRecord::new(id_u32.into(), &key_pair),
            );
        }
    }

    if let Some(spks) = signed_pre_keys {
        for (id, record_buf) in spks {
            let id_u32: u32 = id
                .parse::<u32>()
                .map_err(|_| Error::new(Status::InvalidArg, "Invalid signed pre key id"))?;
            let record = wacore_libsignal::protocol::SignedPreKeyRecord::deserialize(&record_buf)
                .map_err(|e| Error::from_reason(format!("{:?}", e)))?;
            store.add_signed_pre_key(id_u32.into(), record);
        }
    }

    let cipher_message = match message_type.as_str() {
        "pkmsg" => CiphertextMessage::PreKeySignalMessage(
            wacore_libsignal::protocol::PreKeySignalMessage::try_from(ciphertext.as_ref())
                .map_err(|e| Error::from_reason(format!("{:?}", e)))?,
        ),
        "msg" => CiphertextMessage::SignalMessage(
            wacore_libsignal::protocol::SignalMessage::try_from(ciphertext.as_ref())
                .map_err(|e| Error::from_reason(format!("{:?}", e)))?,
        ),
        _ => return Err(Error::new(Status::InvalidArg, "Invalid message type")),
    };

    let mut rng = StdRng::from_os_rng();

    // 3. Operate: The library will use the store to find the right keys and session,
    // decrypt, and update the session state within the store.
    let plaintext = message_decrypt(
        &cipher_message,
        &remote_address,
        store_as_mut(&store),
        store_as_mut(&store),
        store_as_mut(&store),
        store_as_mut(&store),
        &mut rng,
        false.into(),
    )
    .await
    .map_err(|e| Error::from_reason(format!("{:?}", e)))?;

    // 4. Extract the MODIFIED session from the store
    let new_session_record = store
        .get_session(&remote_address.to_string())
        .ok_or_else(|| Error::from_reason("Session not found after decryption".to_string()))?;

    // 5. Return the result and the new state
    Ok(JsDecryptResult {
        plaintext: plaintext.into(),
        new_session: new_session_record
            .serialize()
            .map_err(|e| Error::from_reason(format!("{:?}", e)))?
            .into(),
    })
}

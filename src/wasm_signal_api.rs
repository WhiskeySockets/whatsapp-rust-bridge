// whatsapp-rust-bridge/src/wasm_signal_api.rs

use rand_core::{OsRng, TryRngCore};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use wacore::libsignal::protocol::{
    self as signal, BobSignalProtocolParameters, KeyPair, SessionRecord, UsePQRatchet,
    initialize_bob_session,
};

// --- DTOs for the new, explicit API ---

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmBobParameters {
    // Our keys
    pub identity_key_pair: WasmKeyPair,
    pub signed_pre_key_pair: WasmKeyPair,
    pub one_time_pre_key_pair: Option<WasmKeyPair>,
    pub local_registration_id: u32,

    // Their keys (from the pkmsg)
    pub remote_identity_key: Vec<u8>, // Must be 33 bytes (with 0x05 prefix)
    pub remote_base_key: Vec<u8>,     // Must be 33 bytes (with 0x05 prefix)
    pub remote_registration_id: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmDecryptWithSessionOpts {
    pub session_record: Vec<u8>,
    pub ciphertext: Vec<u8>, // The inner SignalMessage bytes
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmDecryptResult {
    pub plaintext: Vec<u8>,
    pub session_record: Vec<u8>, // The updated session record
}

#[derive(Serialize, Deserialize)]
pub struct WasmKeyPair {
    pub public: Vec<u8>,
    pub private: Vec<u8>,
}

fn js_value_to_err(e: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&e.to_string()).into()
}

/// Creates a new session record from the receiver's (Bob's) perspective.
/// This is the first step when handling a 'pkmsg'.
#[wasm_bindgen(js_name = "establishSessionFromBob")]
pub fn establish_session_from_bob(opts_val: JsValue) -> Result<Vec<u8>, JsValue> {
    let opts: WasmBobParameters = serde_wasm_bindgen::from_value(opts_val)?;

    let our_identity_key_pair = KeyPair::from_public_and_private(
        &opts.identity_key_pair.public,
        &opts.identity_key_pair.private,
    )
    .map_err(js_value_to_err)?
    .into();

    let our_signed_pre_key_pair = KeyPair::from_public_and_private(
        &opts.signed_pre_key_pair.public,
        &opts.signed_pre_key_pair.private,
    )
    .map_err(js_value_to_err)?;

    let our_one_time_pre_key_pair = opts
        .one_time_pre_key_pair
        .map(|kp| KeyPair::from_public_and_private(&kp.public, &kp.private))
        .transpose()
        .map_err(js_value_to_err)?;

    let our_ratchet_key_pair = KeyPair::generate(&mut OsRng.unwrap_err());

    let their_identity_key =
        signal::IdentityKey::decode(&opts.remote_identity_key).map_err(js_value_to_err)?;
    let their_base_key =
        signal::PublicKey::deserialize(&opts.remote_base_key).map_err(js_value_to_err)?;

    let params = BobSignalProtocolParameters::new(
        our_identity_key_pair,
        our_signed_pre_key_pair,
        our_one_time_pre_key_pair,
        our_ratchet_key_pair,
        their_identity_key,
        their_base_key,
        UsePQRatchet::No,
    );

    let mut new_session_state = initialize_bob_session(&params).map_err(js_value_to_err)?;
    new_session_state.set_local_registration_id(opts.local_registration_id);
    new_session_state.set_remote_registration_id(opts.remote_registration_id);

    let new_record = SessionRecord::new(new_session_state);

    new_record.serialize().map_err(js_value_to_err)
}

/// Decrypts a standard Signal message using an existing session record.
/// This is used for 'msg' types and for the inner message of a 'pkmsg' AFTER a session has been established.
#[wasm_bindgen(js_name = "decryptWithSession")]
pub fn decrypt_with_session(opts_val: JsValue) -> Result<JsValue, JsValue> {
    let opts: WasmDecryptWithSessionOpts = serde_wasm_bindgen::from_value(opts_val)?;
    let mut record = SessionRecord::deserialize(&opts.session_record).map_err(js_value_to_err)?;
    let message =
        signal::SignalMessage::try_from(opts.ciphertext.as_slice()).map_err(js_value_to_err)?;

    let plaintext = decrypt_with_session_record(&message, &mut record)?;

    let result = WasmDecryptResult {
        plaintext,
        session_record: record.serialize().map_err(js_value_to_err)?,
    };

    serde_wasm_bindgen::to_value(&result).map_err(Into::into)
}

// Internal helper function remains largely the same
fn decrypt_with_session_record(
    message: &signal::SignalMessage,
    record: &mut SessionRecord,
) -> Result<Vec<u8>, JsValue> {
    // ... (This function's implementation from the previous answer is correct)
    let session_state = record
        .session_state_mut()
        .ok_or_else(|| js_value_to_err("SessionRecord has no current state"))?;

    let their_ephemeral = message.sender_ratchet_key();
    let counter = message.counter();

    let mut chain_key = match session_state.get_receiver_chain_key(their_ephemeral) {
        Ok(Some(key)) => key,
        _ => {
            let root_key = session_state.root_key().map_err(js_value_to_err)?;
            let our_ephemeral_priv = session_state
                .sender_ratchet_private_key()
                .map_err(js_value_to_err)?;
            let (new_root_key, new_chain_key) = root_key
                .create_chain(their_ephemeral, &our_ephemeral_priv)
                .map_err(js_value_to_err)?;
            let our_new_ephemeral = KeyPair::generate(&mut OsRng.unwrap_err());
            let (final_root_key, new_sender_chain) = new_root_key
                .create_chain(their_ephemeral, &our_new_ephemeral.private_key)
                .map_err(js_value_to_err)?;
            session_state.set_root_key(&final_root_key);
            session_state.add_receiver_chain(their_ephemeral, &new_chain_key);
            session_state.set_sender_chain(&our_new_ephemeral, &new_sender_chain);
            new_chain_key
        }
    };
    while chain_key.index() < counter {
        chain_key = chain_key.next_chain_key();
    }
    let message_keys = chain_key.message_keys().generate_keys();
    session_state
        .set_receiver_chain_key(their_ephemeral, &chain_key.next_chain_key())
        .map_err(js_value_to_err)?;

    let mac_valid = message
        .verify_mac(
            &session_state
                .remote_identity_key()
                .map_err(js_value_to_err)?
                .unwrap(),
            &session_state
                .local_identity_key()
                .map_err(js_value_to_err)?,
            message_keys.mac_key(),
        )
        .map_err(js_value_to_err)?;
    if !mac_valid {
        return Err(js_value_to_err("MAC verification failed"));
    }

    wacore::libsignal::crypto::aes_256_cbc_decrypt(
        message.body(),
        message_keys.cipher_key(),
        message_keys.iv(),
    )
    .map_err(|e| js_value_to_err(format!("AES decryption failed: {}", e)))
}

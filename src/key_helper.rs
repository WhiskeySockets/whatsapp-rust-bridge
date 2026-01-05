use js_sys::{TypeError, Uint8Array};
use rand::TryRngCore as _;
use rand::rngs::OsRng;
use serde::Serialize;
use tsify_next::Tsify;
use wacore_libsignal::core::curve::{KeyPair as CoreKeyPair, PrivateKey as CorePrivateKey};
use wasm_bindgen::prelude::*;

// Re-export KeyPair from curve module for consistency
pub use crate::curve::KeyPair;

/// A signed pre-key with its signature
#[derive(Debug, Clone, Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct SignedPreKey {
    pub key_id: u32,
    pub key_pair: KeyPair,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub signature: Vec<u8>,
}

/// A pre-key for key exchange
#[derive(Debug, Clone, Serialize, Tsify)]
#[tsify(into_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PreKey {
    pub key_id: u32,
    pub key_pair: KeyPair,
}

fn map_err(err: impl std::fmt::Display) -> JsValue {
    TypeError::new(&err.to_string()).into()
}

#[wasm_bindgen(js_name = generateIdentityKeyPair)]
pub fn generate_identity_key_pair() -> KeyPair {
    crate::curve::generate_key_pair()
}

#[wasm_bindgen(js_name = generateRegistrationId)]
pub fn generate_registration_id() -> u32 {
    let mut bytes = [0u8; 2];
    OsRng.unwrap_err().try_fill_bytes(&mut bytes).unwrap();
    (u16::from_le_bytes(bytes) & 0x3FFF) as u32
}

#[wasm_bindgen(js_name = generateSignedPreKey)]
pub fn generate_signed_pre_key(
    identity_key_pair: KeyPair,
    signed_key_id: u32,
) -> Result<SignedPreKey, JsValue> {
    // Validate private key length
    if identity_key_pair.priv_key.len() != 32 {
        return Err(TypeError::new("identityKeyPair.privKey must be 32 bytes").into());
    }

    let identity_private_key =
        CorePrivateKey::deserialize(&identity_key_pair.priv_key).map_err(map_err)?;

    let pre_key_pair = CoreKeyPair::generate(&mut OsRng.unwrap_err());
    let pre_key_public_bytes = pre_key_pair.public_key.serialize();

    let signature = identity_private_key
        .calculate_signature(&pre_key_public_bytes, &mut OsRng.unwrap_err())
        .map_err(map_err)?;

    Ok(SignedPreKey {
        key_id: signed_key_id,
        key_pair: KeyPair {
            pub_key: pre_key_public_bytes.to_vec(),
            priv_key: pre_key_pair.private_key.serialize().to_vec(),
        },
        signature: signature.to_vec(),
    })
}

#[wasm_bindgen(js_name = generatePreKey)]
pub fn generate_pre_key(key_id: u32) -> PreKey {
    let key_pair = CoreKeyPair::generate(&mut OsRng.unwrap_err());

    PreKey {
        key_id,
        key_pair: KeyPair {
            pub_key: key_pair.public_key.serialize().to_vec(),
            priv_key: key_pair.private_key.serialize().to_vec(),
        },
    }
}

#[wasm_bindgen(js_name = _serializeIdentityKeyPair)]
pub fn _serialize_identity_key_pair(key_pair: KeyPair) -> Uint8Array {
    let pub_key_tag = (1 << 3) | 2;
    let priv_key_tag = (2 << 3) | 2;

    // Pre-allocate: 1 tag + 1 len + pub_key + 1 tag + 1 len + priv_key
    let capacity = 2 + key_pair.pub_key.len() + 2 + key_pair.priv_key.len();
    let mut buffer = Vec::with_capacity(capacity);

    buffer.push(pub_key_tag);
    buffer.push(key_pair.pub_key.len() as u8);
    buffer.extend_from_slice(&key_pair.pub_key);
    buffer.push(priv_key_tag);
    buffer.push(key_pair.priv_key.len() as u8);
    buffer.extend_from_slice(&key_pair.priv_key);

    Uint8Array::from(buffer.as_slice())
}

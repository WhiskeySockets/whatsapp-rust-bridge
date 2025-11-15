use js_sys::{Object, Reflect, TypeError, Uint8Array};
use rand::TryRngCore as _;
use rand::rngs::OsRng;
use serde::Deserialize;
use wacore_libsignal::core::curve::{KeyPair as CoreKeyPair, PrivateKey as CorePrivateKey};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "{ pubKey: Uint8Array; privKey: Uint8Array }")]
    pub type KeyPairType;

    #[wasm_bindgen(
        typescript_type = "{ keyId: number; keyPair: KeyPairType; signature: Uint8Array }"
    )]
    pub type SignedPreKeyType;

    #[wasm_bindgen(typescript_type = "{ keyId: number; keyPair: KeyPairType }")]
    pub type PreKeyType;
}

fn map_err(err: impl std::fmt::Display) -> JsValue {
    TypeError::new(&err.to_string()).into()
}

#[wasm_bindgen(js_name = generateIdentityKeyPair)]
pub fn generate_identity_key_pair() -> Result<KeyPairType, JsValue> {
    let key_pair_js_value = crate::curve::generate_key_pair()?;

    Ok(key_pair_js_value.unchecked_into::<KeyPairType>())
}

#[wasm_bindgen(js_name = generateRegistrationId)]
pub fn generate_registration_id() -> u32 {
    let mut bytes = [0u8; 2];
    OsRng.unwrap_err().try_fill_bytes(&mut bytes).unwrap();
    (u16::from_le_bytes(bytes) & 0x3FFF) as u32
}

#[wasm_bindgen(js_name = generateSignedPreKey)]
pub fn generate_signed_pre_key(
    identity_key_pair_val: &JsValue,
    signed_key_id_val: &JsValue,
) -> Result<SignedPreKeyType, JsValue> {
    if !identity_key_pair_val.is_object() {
        let error = TypeError::new("identityKeyPair.privKey must be a Uint8Array");
        return Err(error.into());
    }

    let id_priv_key_val = Reflect::get(identity_key_pair_val, &"privKey".into())?;

    let id_priv_key_bytes = match id_priv_key_val.dyn_into::<Uint8Array>() {
        Ok(arr) => arr.to_vec(),
        Err(_) => {
            let error = TypeError::new("identityKeyPair.privKey must be a Uint8Array");
            return Err(error.into());
        }
    };

    if id_priv_key_bytes.len() != 32 {
        let error = TypeError::new("identityKeyPair.privKey must be a Uint8Array");
        return Err(error.into());
    }

    let identity_private_key = CorePrivateKey::deserialize(&id_priv_key_bytes).map_err(map_err)?;

    let signed_key_id_f = match signed_key_id_val.as_f64() {
        Some(n) if n.is_finite() => n,
        _ => {
            let err = TypeError::new("signedKeyId must be a non-negative integer");
            return Err(err.into());
        }
    };

    if signed_key_id_f < 0.0 || signed_key_id_f.fract() != 0.0 || signed_key_id_f > u32::MAX as f64
    {
        let err = TypeError::new("signedKeyId must be a non-negative integer");
        return Err(err.into());
    }

    let signed_key_id = signed_key_id_f as u32;

    let pre_key_pair = CoreKeyPair::generate(&mut OsRng.unwrap_err());
    let pre_key_public_bytes_with_prefix = pre_key_pair.public_key.serialize();

    let signature = identity_private_key
        .calculate_signature(&pre_key_public_bytes_with_prefix, &mut OsRng.unwrap_err())
        .map_err(map_err)?;

    let key_pair_obj = Object::new();
    Reflect::set(
        &key_pair_obj,
        &"pubKey".into(),
        &Uint8Array::from(pre_key_public_bytes_with_prefix.as_ref()).into(),
    )?;
    Reflect::set(
        &key_pair_obj,
        &"privKey".into(),
        &Uint8Array::from(pre_key_pair.private_key.serialize().as_ref()).into(),
    )?;

    let result_obj = Object::new();
    Reflect::set(&result_obj, &"keyId".into(), &(signed_key_id as f64).into())?;
    Reflect::set(&result_obj, &"keyPair".into(), &key_pair_obj.into())?;
    Reflect::set(
        &result_obj,
        &"signature".into(),
        &Uint8Array::from(signature.as_ref()).into(),
    )?;

    Ok(result_obj.unchecked_into())
}

#[wasm_bindgen(js_name = generatePreKey)]
pub fn generate_pre_key(key_id_val: &JsValue) -> Result<PreKeyType, JsValue> {
    let key_id_f = match key_id_val.as_f64() {
        Some(n) if n.is_finite() => n,
        _ => {
            let err = TypeError::new("keyId must be a non-negative integer");
            return Err(err.into());
        }
    };

    if key_id_f < 0.0 || key_id_f.fract() != 0.0 || key_id_f > u32::MAX as f64 {
        let err = TypeError::new("keyId must be a non-negative integer");
        return Err(err.into());
    }

    let key_id = key_id_f as u32;
    let key_pair = CoreKeyPair::generate(&mut OsRng.unwrap_err());

    let key_pair_obj = Object::new();
    Reflect::set(
        &key_pair_obj,
        &"pubKey".into(),
        &Uint8Array::from(key_pair.public_key.serialize().as_ref()).into(),
    )?;
    Reflect::set(
        &key_pair_obj,
        &"privKey".into(),
        &Uint8Array::from(key_pair.private_key.serialize().as_ref()).into(),
    )?;

    let result_obj = Object::new();
    Reflect::set(&result_obj, &"keyId".into(), &(key_id as f64).into())?;
    Reflect::set(&result_obj, &"keyPair".into(), &key_pair_obj.into())?;

    Ok(result_obj.unchecked_into())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsKeyPair {
    pub_key: Vec<u8>,
    priv_key: Vec<u8>,
}

#[wasm_bindgen(js_name = _serializeIdentityKeyPair)]
pub fn _serialize_identity_key_pair(key_pair_val: JsValue) -> Result<Uint8Array, JsValue> {
    let js_key_pair: JsKeyPair = serde_wasm_bindgen::from_value(key_pair_val)?;

    let pub_key_tag = (1 << 3) | 2;
    let priv_key_tag = (2 << 3) | 2;

    let mut buffer = Vec::new();
    buffer.push(pub_key_tag);
    buffer.push(js_key_pair.pub_key.len() as u8);
    buffer.extend_from_slice(&js_key_pair.pub_key);
    buffer.push(priv_key_tag);
    buffer.push(js_key_pair.priv_key.len() as u8);
    buffer.extend_from_slice(&js_key_pair.priv_key);

    Ok(Uint8Array::from(buffer.as_slice()))
}

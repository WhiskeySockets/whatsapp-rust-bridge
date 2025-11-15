use curve25519_dalek::{Scalar, constants::ED25519_BASEPOINT_TABLE};
use js_sys::{Object, Reflect, Uint8Array};
use rand::{TryRngCore, rngs::OsRng};
use wacore_libsignal::{
    core::curve::{
        KeyPair as CoreKeyPair, PrivateKey as CorePrivateKey, PublicKey as CorePublicKey,
    },
    protocol::CurveError,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Object, typescript_type = "{ pubKey: Uint8Array; privKey: Uint8Array }")]
    pub type KeyPair;
}

#[wasm_bindgen(js_name = generateKeyPair)]
pub fn generate_key_pair() -> Result<KeyPair, JsValue> {
    let pair = CoreKeyPair::generate(&mut OsRng.unwrap_err());

    let pub_key_array = Uint8Array::from(pair.public_key.serialize().as_ref());
    let priv_key_array = Uint8Array::from(&pair.private_key.serialize()[..]);

    let key_pair = Object::new();
    Reflect::set(&key_pair, &JsValue::from_str("pubKey"), &pub_key_array)?;
    Reflect::set(&key_pair, &JsValue::from_str("privKey"), &priv_key_array)?;

    Ok(key_pair.unchecked_into())
}

fn map_err(err: impl std::fmt::Display + 'static) -> JsValue {
    if let Some(curve_err) = (&err as &dyn std::any::Any).downcast_ref::<CurveError>() {
        match curve_err {
            CurveError::BadKeyLength(_, 0) => return JsValue::from_str("Invalid private key type"),
            CurveError::BadKeyLength(_, len) if *len != 32 => {
                return JsValue::from_str("Incorrect private key length");
            }
            _ => (),
        }
    }
    JsValue::from_str(&err.to_string())
}

fn parse_private_key(bytes: &[u8]) -> Result<CorePrivateKey, JsValue> {
    CorePrivateKey::deserialize(bytes).map_err(map_err)
}

fn parse_public_key(bytes: &[u8]) -> Result<CorePublicKey, JsValue> {
    if bytes.len() == 33 && bytes[0] == 0x05 {
        CorePublicKey::deserialize(bytes).map_err(map_err)
    } else if bytes.len() == 32 {
        let mut key_with_prefix = Vec::with_capacity(33);
        key_with_prefix.push(0x05);
        key_with_prefix.extend_from_slice(bytes);
        CorePublicKey::deserialize(&key_with_prefix).map_err(map_err)
    } else {
        Err(JsValue::from_str(&format!(
            "Invalid public key length: {}",
            bytes.len()
        )))
    }
}

#[wasm_bindgen(js_name = calculateAgreement)]
pub fn calculate_agreement(
    public_key_bytes: &[u8],
    private_key_bytes: &[u8],
) -> Result<Uint8Array, JsValue> {
    if public_key_bytes.len() != 32 && public_key_bytes.len() != 33 {
        return Err(JsValue::from_str("Invalid public key"));
    }
    if private_key_bytes.len() != 32 {
        return Err(JsValue::from_str("Incorrect private key length"));
    }

    let priv_key = parse_private_key(private_key_bytes)?;
    let pub_key = parse_public_key(public_key_bytes)?;
    let secret = priv_key.calculate_agreement(&pub_key).map_err(map_err)?;
    Ok(Uint8Array::from(secret.as_ref()))
}

#[wasm_bindgen(js_name = calculateSignature)]
pub fn calculate_signature(
    private_key_bytes: &[u8],
    message: &[u8],
) -> Result<Uint8Array, JsValue> {
    if private_key_bytes.is_empty() {
        return Err(JsValue::from_str("Invalid private key type"));
    }
    if private_key_bytes.len() != 32 {
        return Err(JsValue::from_str("Incorrect private key length"));
    }

    let priv_key = parse_private_key(private_key_bytes)?;
    let signature = priv_key
        .calculate_signature(message, &mut OsRng.unwrap_err())
        .map_err(map_err)?;
    Ok(Uint8Array::from(signature.as_ref()))
}

#[wasm_bindgen(js_name = verifySignature)]
pub fn verify_signature(
    public_key_bytes: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<bool, JsValue> {
    if signature.len() != 64 {
        return Err(JsValue::from_str("Invalid signature"));
    }
    if public_key_bytes.len() != 32 && public_key_bytes.len() != 33 {
        return Err(JsValue::from_str("Invalid public key"));
    }

    let pub_key = parse_public_key(public_key_bytes)?;
    let signature_array: &[u8; 64] = signature
        .try_into()
        .map_err(|_| JsValue::from_str("Signature must be 64 bytes long"))?;
    Ok(pub_key.verify_signature_for_multipart_message(&[message], signature_array))
}

fn derive_signing_public_key(private_key_bytes: &[u8; 32]) -> [u8; 32] {
    let mut clamped_bytes = *private_key_bytes;
    clamped_bytes[0] &= 248;
    clamped_bytes[31] &= 127;
    clamped_bytes[31] |= 64;

    let scalar = Scalar::from_bytes_mod_order(clamped_bytes);

    let public_point = &scalar * ED25519_BASEPOINT_TABLE;
    public_point.compress().to_bytes()
}

#[wasm_bindgen(js_name = getPublicFromPrivateKey)]
pub fn get_public_from_private_key(private_key_bytes: &[u8]) -> Result<Uint8Array, JsValue> {
    let private_key_array: [u8; 32] = private_key_bytes
        .try_into()
        .map_err(|_| JsValue::from_str("Private key must be 32 bytes long"))?;

    let pub_key_bytes = derive_signing_public_key(&private_key_array);

    let mut pub_key_with_prefix = Vec::with_capacity(33);
    pub_key_with_prefix.push(0x05);
    pub_key_with_prefix.extend_from_slice(&pub_key_bytes);

    Ok(Uint8Array::from(pub_key_with_prefix.as_slice()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signing_key_derivation_from_all_zero_private() {
        let all_zero_private_key = [0u8; 32];

        let expected_public_key_bytes: [u8; 32] = [
            105, 62, 71, 151, 44, 175, 82, 124, 120, 131, 173, 27, 57, 130, 47, 2, 111, 71, 219,
            42, 176, 225, 145, 153, 85, 184, 153, 58, 160, 68, 17, 209,
        ];

        let derived_public_key = derive_signing_public_key(&all_zero_private_key);

        assert_eq!(derived_public_key, expected_public_key_bytes);
    }
}

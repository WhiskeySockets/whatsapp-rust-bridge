use napi::bindgen_prelude::*;
use napi_derive::napi;
use wacore_libsignal::protocol::{KeyPair, PrivateKey, PublicKey};

// Matches Baileys' Curve.generateKeyPair
#[napi]
pub fn generate_key_pair() -> Result<JsKeyPair> {
    let key_pair = KeyPair::generate(&mut rand::rng());
    Ok(JsKeyPair {
        public_key: key_pair.public_key.serialize().to_vec().into(),
        private_key: key_pair.private_key.serialize().into(),
    })
}

// Matches Baileys' Curve.sharedKey
#[napi]
pub fn calculate_agreement(public_key: Uint8Array, private_key: Uint8Array) -> Result<Uint8Array> {
    let public_key = PublicKey::deserialize(&public_key)
        .map_err(|e| napi::Error::from_reason(format!("{:?}", e)))?;
    let private_key = PrivateKey::deserialize(&private_key)
        .map_err(|e| napi::Error::from_reason(format!("{:?}", e)))?;
    let agreement = private_key
        .calculate_agreement(&public_key)
        .map_err(|e| napi::Error::from_reason(format!("{:?}", e)))?;
    Ok(agreement.to_vec().into())
}

// Matches Baileys' Curve.sign
#[napi]
pub fn calculate_signature(private_key: Uint8Array, message: Uint8Array) -> Result<Uint8Array> {
    let private_key = PrivateKey::deserialize(&private_key)
        .map_err(|e| napi::Error::from_reason(format!("{:?}", e)))?;
    let signature = private_key
        .calculate_signature(&message, &mut rand::rng())
        .map_err(|e| napi::Error::from_reason(format!("{:?}", e)))?;
    Ok(signature.to_vec().into())
}

// Matches Baileys' Curve.verify
#[napi]
pub fn verify_signature(
    public_key: Uint8Array,
    message: Uint8Array,
    signature: Uint8Array,
) -> Result<bool> {
    let public_key = PublicKey::deserialize(&public_key)
        .map_err(|e| napi::Error::from_reason(format!("{:?}", e)))?;
    Ok(public_key.verify_signature(&message, &signature))
}

#[napi(object)]
pub struct JsKeyPair {
    pub public_key: Uint8Array,
    pub private_key: Uint8Array,
}

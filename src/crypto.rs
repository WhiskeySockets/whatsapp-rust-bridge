use aes::Aes256;
use aes::cipher::{
    BlockDecryptMut, BlockEncryptMut, KeyIvInit, StreamCipher, block_padding::Pkcs7,
};
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit};
use cbc::{Decryptor, Encryptor};
use ctr::Ctr128BE;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use js_sys::Uint8Array;
use md5::Md5;
use pbkdf2::pbkdf2;
use rand::{TryRngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::num::NonZeroU32;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

type Aes256Ctr = Ctr128BE<Aes256>;
type Aes256CbcEnc = Encryptor<Aes256>;
type Aes256CbcDec = Decryptor<Aes256>;

const KEY_BUNDLE_TYPE: u8 = 0x05;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum HmacVariant {
    #[serde(rename = "sha256")]
    Sha256,
    #[serde(rename = "sha512")]
    Sha512,
}


#[wasm_bindgen(js_name = generateSignalPubKey)]
pub fn generate_signal_pub_key(pub_key: &[u8]) -> Uint8Array {
    if pub_key.len() == 33 {
        let arr = Uint8Array::new_with_length(33);
        arr.copy_from(pub_key);
        arr
    } else {
        // Need to allocate for the prefix case
        let mut result = [0u8; 33];
        result[0] = KEY_BUNDLE_TYPE;
        let len = pub_key.len().min(32);
        result[1..1 + len].copy_from_slice(&pub_key[..len]);
        let arr = Uint8Array::new_with_length((1 + len) as u32);
        arr.copy_from(&result[..1 + len]);
        arr
    }
}


#[wasm_bindgen(js_name = aesEncryptGCM)]
pub fn aes_encrypt_gcm(
    plaintext: &[u8],
    key: &[u8],
    iv: &[u8],
    additional_data: &[u8],
) -> Result<Uint8Array, JsValue> {
    if iv.len() != 12 {
        return Err(JsValue::from_str("AES-GCM requires 12 byte IV"));
    }

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| JsValue::from_str(&format!("Invalid key length: {}", e)))?;

    let nonce = aes_gcm::Nonce::from_slice(iv);

    let payload = aes_gcm::aead::Payload {
        msg: plaintext,
        aad: additional_data,
    };

    let ciphertext = cipher
        .encrypt(nonce, payload)
        .map_err(|e| JsValue::from_str(&format!("AES-GCM encrypt error: {}", e)))?;

    let result = Uint8Array::new_with_length(ciphertext.len() as u32);
    result.copy_from(&ciphertext);
    Ok(result)
}


#[wasm_bindgen(js_name = aesDecryptGCM)]
pub fn aes_decrypt_gcm(
    ciphertext: &[u8],
    key: &[u8],
    iv: &[u8],
    additional_data: &[u8],
) -> Result<Uint8Array, JsValue> {
    if iv.len() != 12 {
        return Err(JsValue::from_str("AES-GCM requires 12 byte IV"));
    }

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| JsValue::from_str(&format!("Invalid key length: {}", e)))?;

    let nonce = aes_gcm::Nonce::from_slice(iv);

    let payload = aes_gcm::aead::Payload {
        msg: ciphertext,
        aad: additional_data,
    };

    let plaintext = cipher
        .decrypt(nonce, payload)
        .map_err(|e| JsValue::from_str(&format!("AES-GCM decrypt error: {}", e)))?;

    let result = Uint8Array::new_with_length(plaintext.len() as u32);
    result.copy_from(&plaintext);
    Ok(result)
}


#[wasm_bindgen(js_name = aesEncryptCTR)]
pub fn aes_encrypt_ctr(plaintext: &[u8], key: &[u8], iv: &[u8]) -> Result<Uint8Array, JsValue> {
    if iv.len() != 16 {
        return Err(JsValue::from_str("AES-CTR requires 16 byte IV"));
    }
    let mut cipher = Aes256Ctr::new(key.into(), iv.into());
    let mut buffer = plaintext.to_vec();
    cipher.apply_keystream(&mut buffer);

    let result = Uint8Array::new_with_length(buffer.len() as u32);
    result.copy_from(&buffer);
    Ok(result)
}


#[wasm_bindgen(js_name = aesDecryptCTR)]
pub fn aes_decrypt_ctr(ciphertext: &[u8], key: &[u8], iv: &[u8]) -> Result<Uint8Array, JsValue> {
    // CTR encryption and decryption are the same operation
    aes_encrypt_ctr(ciphertext, key, iv)
}


#[wasm_bindgen(js_name = aesDecrypt)]
pub fn aes_decrypt(buffer: &[u8], key: &[u8]) -> Result<Uint8Array, JsValue> {
    if buffer.len() < 16 {
        return Err(JsValue::from_str(
            "Buffer too short for AES-CBC (needs 16 byte IV)",
        ));
    }
    let iv = &buffer[0..16];
    let ciphertext = &buffer[16..];
    aes_decrypt_with_iv(ciphertext, key, iv)
}


#[wasm_bindgen(js_name = aesDecryptWithIV)]
pub fn aes_decrypt_with_iv(buffer: &[u8], key: &[u8], iv: &[u8]) -> Result<Uint8Array, JsValue> {
    if iv.len() != 16 {
        return Err(JsValue::from_str("AES-CBC requires 16 byte IV"));
    }
    let cipher = Aes256CbcDec::new(key.into(), iv.into());
    let mut buf = buffer.to_vec();

    let plaintext = cipher
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| JsValue::from_str(&format!("AES-CBC decrypt error: {}", e)))?;

    let result = Uint8Array::new_with_length(plaintext.len() as u32);
    result.copy_from(plaintext);
    Ok(result)
}


#[wasm_bindgen(js_name = aesEncrypt)]
pub fn aes_encrypt(buffer: &[u8], key: &[u8]) -> Result<Uint8Array, JsValue> {
    let mut iv = [0u8; 16];
    OsRng
        .try_fill_bytes(&mut iv)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    let encrypted = aes_encrypt_with_iv(buffer, key, &iv)?;

    let result = Uint8Array::new_with_length((16 + encrypted.length()) as u32);

    // Copy IV
    result.set(&Uint8Array::from(&iv[..]), 0);

    // Copy encrypted content
    result.set(&encrypted, 16);

    Ok(result)
}


#[wasm_bindgen(js_name = aesEncryptWithIV)]
pub fn aes_encrypt_with_iv(buffer: &[u8], key: &[u8], iv: &[u8]) -> Result<Uint8Array, JsValue> {
    if iv.len() != 16 {
        return Err(JsValue::from_str("AES-CBC requires 16 byte IV"));
    }
    let cipher = Aes256CbcEnc::new(key.into(), iv.into());
    let plaintext_len = buffer.len();
    let mut buf = vec![0u8; plaintext_len + 16]; // Sufficient capacity for padding
    buf[..plaintext_len].copy_from_slice(buffer);

    let ciphertext = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_len)
        .map_err(|e| JsValue::from_str(&format!("AES-CBC encrypt error: {}", e)))?;

    let result = Uint8Array::new_with_length(ciphertext.len() as u32);
    result.copy_from(ciphertext);
    Ok(result)
}


#[wasm_bindgen(js_name = hmacSign)]
pub fn hmac_sign(
    buffer: &[u8],
    key: &[u8],
    variant: Option<HmacVariant>,
) -> Result<Uint8Array, JsValue> {
    let v = variant.unwrap_or(HmacVariant::Sha256);

    match v {
        HmacVariant::Sha256 => {
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)
                .map_err(|e| JsValue::from_str(&format!("HMAC error: {}", e)))?;
            mac.update(buffer);
            let result = mac.finalize().into_bytes();
            let arr = Uint8Array::new_with_length(result.len() as u32);
            arr.copy_from(&result);
            Ok(arr)
        }
        HmacVariant::Sha512 => {
            let mut mac = <Hmac<Sha512> as Mac>::new_from_slice(key)
                .map_err(|e| JsValue::from_str(&format!("HMAC error: {}", e)))?;
            mac.update(buffer);
            let result = mac.finalize().into_bytes();
            let arr = Uint8Array::new_with_length(result.len() as u32);
            arr.copy_from(&result);
            Ok(arr)
        }
    }
}


#[wasm_bindgen(js_name = sha256)]
pub fn sha256(buffer: &[u8]) -> Uint8Array {
    let mut hasher = Sha256::new();
    hasher.update(buffer);
    let result = hasher.finalize();
    let arr = Uint8Array::new_with_length(32);
    arr.copy_from(&result);
    arr
}


#[wasm_bindgen(js_name = md5)]
pub fn md5_hash(buffer: &[u8]) -> Uint8Array {
    let mut hasher = Md5::new();
    hasher.update(buffer);
    let result = hasher.finalize();
    let arr = Uint8Array::new_with_length(16);
    arr.copy_from(&result);
    arr
}

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct HkdfInfo {
    #[tsify(type = "Uint8Array | null")]
    #[serde(with = "serde_bytes")]
    pub salt: Option<Vec<u8>>,
    pub info: Option<String>,
}

// Helper for HKDF since object handling in WASM can be tricky

#[wasm_bindgen(js_name = hkdf)]
pub fn hkdf(buffer: &[u8], expanded_length: usize, info: HkdfInfo) -> Result<Uint8Array, JsValue> {
    let salt_bytes = info.salt.as_deref();
    let info_bytes = info.info.as_deref().map(|s| s.as_bytes()).unwrap_or(&[]);

    let hk = Hkdf::<Sha256>::new(salt_bytes, buffer);
    let mut okm = vec![0u8; expanded_length];

    hk.expand(info_bytes, &mut okm)
        .map_err(|_| JsValue::from_str("HKDF expansion failed"))?;

    let arr = Uint8Array::new_with_length(okm.len() as u32);
    arr.copy_from(&okm);
    Ok(arr)
}


#[wasm_bindgen(js_name = derivePairingCodeKey)]
pub fn derive_pairing_code_key(pairing_code: &str, salt: &[u8]) -> Result<Uint8Array, JsValue> {
    let iterations = 2 << 16; // 131072
    let mut result = [0u8; 32];

    let pairing_code_bytes = pairing_code.as_bytes();

    let iter = NonZeroU32::new(iterations).unwrap();

    pbkdf2::<Hmac<Sha256>>(pairing_code_bytes, salt, iter.get(), &mut result)
        .map_err(|e| JsValue::from_str(&format!("PBKDF2 error: {}", e)))?;

    let arr = Uint8Array::new_with_length(32);
    arr.copy_from(&result);
    Ok(arr)
}

/// A signed key pair containing the key pair, signature, and key ID
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct SignedKeyPair {
    pub key_pair: CryptoKeyPair,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub signature: Vec<u8>,
    pub key_id: u32,
}

/// A cryptographic key pair for crypto module
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct CryptoKeyPair {
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub private: Vec<u8>,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub public: Vec<u8>,
}

#[wasm_bindgen(js_name = signedKeyPair)]
pub fn signed_key_pair(identity_private_key: &[u8], key_id: u32) -> Result<SignedKeyPair, JsValue> {
    use crate::curve::{calculate_signature, generate_key_pair};

    // Generate a new key pair
    let pre_key = generate_key_pair();

    // Generate the public key with prefix for signing
    let pub_key = generate_signal_pub_key(&pre_key.pub_key[1..]); // Remove 0x05 prefix if present
    let pub_key_bytes: Vec<u8> = pub_key.to_vec();

    // Sign the prefixed public key with the identity private key
    let signature = calculate_signature(identity_private_key, &pub_key_bytes)?;
    let signature_bytes: Vec<u8> = signature.to_vec();

    Ok(SignedKeyPair {
        key_pair: CryptoKeyPair {
            private: pre_key.priv_key,
            public: pre_key.pub_key[1..].to_vec(), // Remove 0x05 prefix to match Baileys API
        },
        signature: signature_bytes,
        key_id,
    })
}

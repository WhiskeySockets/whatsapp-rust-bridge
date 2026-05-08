//! `SignalCryptoProvider` backed by host JS callbacks (typically `node:crypto`).
//!
//! When installed via `setCryptoProvider`, all AES-CBC / AES-GCM / HMAC-SHA256
//! calls inside `wacore_libsignal` are delegated to the host runtime. On
//! Node/Bun this routes through OpenSSL, which uses AES-NI hardware
//! acceleration — a 5-15x win over the pure-Rust soft AES fallback.
//!
//! The Rust soft impl stays compiled in as the default provider, so this is
//! purely additive: callers that don't install a JS provider keep the
//! previous behaviour.

use js_sys::Uint8Array;
use wacore_libsignal::crypto::{
    CryptoProviderError, RustCryptoProvider, SignalCryptoProvider, set_crypto_provider,
};
use wasm_bindgen::prelude::*;

// Below this many bytes of AES input, the per-call FFI cost (Rust → JS →
// node:crypto → Rust round-trip) eats the AES-NI win and we'd regress vs the
// pure-Rust soft AES path. Measured at ~256-512 B on Linux x86_64.
// Conservatively picked at 512 to keep zero regression on small Signal
// session messages while still catching anything noise/media-sized.
const AES_NATIVE_THRESHOLD: usize = 512;

#[wasm_bindgen(typescript_custom_section)]
const TS_CRYPTO: &str = r#"
/** Native crypto callbacks installed via `setCryptoProvider`. */
export interface JsCryptoCallbacks {
    aesCbc256Encrypt(key: Uint8Array, iv: Uint8Array, plaintext: Uint8Array): Uint8Array;
    aesCbc256Decrypt(key: Uint8Array, iv: Uint8Array, ciphertext: Uint8Array): Uint8Array;
    aesGcm256Encrypt(key: Uint8Array, nonce: Uint8Array, aad: Uint8Array, plaintext: Uint8Array): Uint8Array;
    aesGcm256Decrypt(key: Uint8Array, nonce: Uint8Array, aad: Uint8Array, ciphertextWithTag: Uint8Array): Uint8Array;
    hmacSha256(key: Uint8Array, data: Uint8Array): Uint8Array;
}
"#;

pub struct JsCryptoAdapter {
    aes_cbc_encrypt: js_sys::Function,
    aes_cbc_decrypt: js_sys::Function,
    aes_gcm_encrypt: js_sys::Function,
    aes_gcm_decrypt: js_sys::Function,
    hmac_sha256: js_sys::Function,
}

// `SignalCryptoProvider: Send + Sync + 'static`. js_sys::Function is !Send/!Sync
// by default but wasm32 is single-threaded, so the provider can never actually
// race. Mark the adapter as Send/Sync to satisfy the trait bound.
unsafe impl Send for JsCryptoAdapter {}
unsafe impl Sync for JsCryptoAdapter {}

impl JsCryptoAdapter {
    fn from_js(obj: &JsValue) -> Result<Self, JsValue> {
        fn extract(obj: &JsValue, name: &str) -> Result<js_sys::Function, JsValue> {
            js_sys::Reflect::get(obj, &JsValue::from_str(name))?
                .dyn_into::<js_sys::Function>()
                .map_err(|_| JsValue::from_str(&format!("crypto.{name} must be a function")))
        }
        Ok(Self {
            aes_cbc_encrypt: extract(obj, "aesCbc256Encrypt")?,
            aes_cbc_decrypt: extract(obj, "aesCbc256Decrypt")?,
            aes_gcm_encrypt: extract(obj, "aesGcm256Encrypt")?,
            aes_gcm_decrypt: extract(obj, "aesGcm256Decrypt")?,
            hmac_sha256: extract(obj, "hmacSha256")?,
        })
    }
}

#[inline]
fn append_array_to_vec(out: &mut Vec<u8>, arr: Uint8Array) {
    let len = arr.length() as usize;
    let start = out.len();
    out.resize(start + len, 0);
    arr.copy_to(&mut out[start..]);
}

#[inline]
fn js_to_array(value: JsValue) -> Result<Uint8Array, CryptoProviderError> {
    value
        .dyn_into::<Uint8Array>()
        .map_err(|_| CryptoProviderError::BackendFailed)
}

impl SignalCryptoProvider for JsCryptoAdapter {
    fn aes_256_cbc_encrypt(
        &self,
        key: &[u8; 32],
        iv: &[u8; 16],
        plaintext: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), CryptoProviderError> {
        // SAFETY: The views alias wasm linear memory. They're consumed inside
        // call3 before any other Rust code can run (single-threaded wasm) and
        // immediately copied into the result Buffer on the JS side, so wasm
        // memory cannot grow / detach the views during the call.
        let ret = unsafe {
            let key_v = Uint8Array::view(&key[..]);
            let iv_v = Uint8Array::view(&iv[..]);
            let pt_v = Uint8Array::view(plaintext);
            self.aes_cbc_encrypt
                .call3(&JsValue::NULL, &key_v, &iv_v, &pt_v)
        }
        .map_err(|_| CryptoProviderError::BackendFailed)?;
        append_array_to_vec(out, js_to_array(ret)?);
        Ok(())
    }

    fn aes_256_cbc_decrypt(
        &self,
        key: &[u8; 32],
        iv: &[u8; 16],
        ciphertext: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), CryptoProviderError> {
        out.clear();
        let ret = unsafe {
            let key_v = Uint8Array::view(&key[..]);
            let iv_v = Uint8Array::view(&iv[..]);
            let ct_v = Uint8Array::view(ciphertext);
            self.aes_cbc_decrypt
                .call3(&JsValue::NULL, &key_v, &iv_v, &ct_v)
        }
        .map_err(|_| CryptoProviderError::BackendFailed)?;
        append_array_to_vec(out, js_to_array(ret)?);
        Ok(())
    }

    fn aes_256_gcm_encrypt(
        &self,
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
        plaintext: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), CryptoProviderError> {
        let ret = unsafe {
            let key_v = Uint8Array::view(&key[..]);
            let nonce_v = Uint8Array::view(&nonce[..]);
            let aad_v = Uint8Array::view(aad);
            let pt_v = Uint8Array::view(plaintext);
            let args = js_sys::Array::of4(&key_v, &nonce_v, &aad_v, &pt_v);
            self.aes_gcm_encrypt.apply(&JsValue::NULL, &args)
        }
        .map_err(|_| CryptoProviderError::BackendFailed)?;
        append_array_to_vec(out, js_to_array(ret)?);
        Ok(())
    }

    fn aes_256_gcm_decrypt(
        &self,
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
        ciphertext_with_tag: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), CryptoProviderError> {
        // node:crypto throws on auth failure; map any thrown JsValue to AuthFailed.
        let ret = unsafe {
            let key_v = Uint8Array::view(&key[..]);
            let nonce_v = Uint8Array::view(&nonce[..]);
            let aad_v = Uint8Array::view(aad);
            let ct_v = Uint8Array::view(ciphertext_with_tag);
            let args = js_sys::Array::of4(&key_v, &nonce_v, &aad_v, &ct_v);
            self.aes_gcm_decrypt.apply(&JsValue::NULL, &args)
        }
        .map_err(|_| CryptoProviderError::AuthFailed)?;
        append_array_to_vec(out, js_to_array(ret)?);
        Ok(())
    }

    fn hmac_sha256(&self, key: &[u8], input: &[u8]) -> [u8; 32] {
        let ret = unsafe {
            let key_v = Uint8Array::view(key);
            let data_v = Uint8Array::view(input);
            self.hmac_sha256.call2(&JsValue::NULL, &key_v, &data_v)
        }
        .expect("hmacSha256 callback threw");
        let arr: Uint8Array = ret.dyn_into().expect("hmacSha256 must return Uint8Array");
        debug_assert_eq!(arr.length() as usize, 32);
        let mut out = [0u8; 32];
        arr.copy_to(&mut out);
        out
    }
}

/// Hybrid: routes large AES ops through `JsCryptoAdapter` (native AES-NI via
/// the host runtime) and small AES + all HMAC through the pure-Rust default.
/// The size split avoids the FFI dispatch cost dominating on small inputs.
struct HybridCryptoAdapter {
    js: JsCryptoAdapter,
    rust: RustCryptoProvider,
}

unsafe impl Send for HybridCryptoAdapter {}
unsafe impl Sync for HybridCryptoAdapter {}

impl SignalCryptoProvider for HybridCryptoAdapter {
    fn aes_256_cbc_encrypt(
        &self,
        key: &[u8; 32],
        iv: &[u8; 16],
        plaintext: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), CryptoProviderError> {
        if plaintext.len() < AES_NATIVE_THRESHOLD {
            self.rust.aes_256_cbc_encrypt(key, iv, plaintext, out)
        } else {
            self.js.aes_256_cbc_encrypt(key, iv, plaintext, out)
        }
    }

    fn aes_256_cbc_decrypt(
        &self,
        key: &[u8; 32],
        iv: &[u8; 16],
        ciphertext: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), CryptoProviderError> {
        if ciphertext.len() < AES_NATIVE_THRESHOLD {
            self.rust.aes_256_cbc_decrypt(key, iv, ciphertext, out)
        } else {
            self.js.aes_256_cbc_decrypt(key, iv, ciphertext, out)
        }
    }

    fn aes_256_gcm_encrypt(
        &self,
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
        plaintext: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), CryptoProviderError> {
        if plaintext.len() < AES_NATIVE_THRESHOLD {
            self.rust
                .aes_256_gcm_encrypt(key, nonce, aad, plaintext, out)
        } else {
            self.js.aes_256_gcm_encrypt(key, nonce, aad, plaintext, out)
        }
    }

    fn aes_256_gcm_decrypt(
        &self,
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
        ciphertext_with_tag: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), CryptoProviderError> {
        // Tag is 16 trailing bytes; threshold is on plaintext size.
        let pt_len = ciphertext_with_tag.len().saturating_sub(16);
        if pt_len < AES_NATIVE_THRESHOLD {
            self.rust
                .aes_256_gcm_decrypt(key, nonce, aad, ciphertext_with_tag, out)
        } else {
            self.js
                .aes_256_gcm_decrypt(key, nonce, aad, ciphertext_with_tag, out)
        }
    }

    fn hmac_sha256(&self, key: &[u8], input: &[u8]) -> [u8; 32] {
        // libsignal's HMAC inputs are tiny (32 B chain keys, short MAC
        // payloads) where FFI dispatch always loses — keep the Rust impl.
        self.rust.hmac_sha256(key, input)
    }
}

/// Install a JS crypto provider. Routes large AES operations through the
/// supplied callbacks (typically `node:crypto`, getting AES-NI hardware
/// acceleration); small AES and all HMAC stay in the default pure-Rust path
/// so we never pay FFI overhead for sub-threshold workloads.
///
/// One-shot: subsequent calls return an error because libsignal's provider
/// is a `OnceLock`.
#[wasm_bindgen(js_name = setCryptoProvider)]
pub fn set_crypto_provider_js(callbacks: JsValue) -> Result<(), JsValue> {
    let js = JsCryptoAdapter::from_js(&callbacks)?;
    let hybrid = HybridCryptoAdapter {
        js,
        rust: RustCryptoProvider,
    };
    set_crypto_provider(hybrid).map_err(JsValue::from_str)
}

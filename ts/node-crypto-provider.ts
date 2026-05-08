// node:crypto-backed implementation of `JsCryptoCallbacks`.
//
// `setCryptoProvider` (in src/js_crypto.rs) wires these into libsignal so
// AES-CBC / AES-GCM / HMAC-SHA256 dispatch to OpenSSL. On x86_64/aarch64
// Node/Bun, that means AES-NI hardware acceleration — typically 5-15x
// faster than the pure-Rust soft impl that ships as the default.

import { createCipheriv, createDecipheriv, createHmac } from "node:crypto";

const GCM_TAG_LEN = 16;

export function makeNodeCryptoProvider() {
  return {
    aesCbc256Encrypt(key: Uint8Array, iv: Uint8Array, plaintext: Uint8Array): Uint8Array {
      const c = createCipheriv("aes-256-cbc", key, iv);
      return Buffer.concat([c.update(plaintext), c.final()]);
    },

    aesCbc256Decrypt(key: Uint8Array, iv: Uint8Array, ciphertext: Uint8Array): Uint8Array {
      const c = createDecipheriv("aes-256-cbc", key, iv);
      return Buffer.concat([c.update(ciphertext), c.final()]);
    },

    aesGcm256Encrypt(
      key: Uint8Array,
      nonce: Uint8Array,
      aad: Uint8Array,
      plaintext: Uint8Array,
    ): Uint8Array {
      const c = createCipheriv("aes-256-gcm", key, nonce);
      if (aad.length > 0) c.setAAD(aad);
      const enc = c.update(plaintext);
      const fin = c.final();
      const tag = c.getAuthTag();
      const out = Buffer.allocUnsafe(enc.length + fin.length + tag.length);
      enc.copy(out, 0);
      fin.copy(out, enc.length);
      tag.copy(out, enc.length + fin.length);
      return out;
    },

    aesGcm256Decrypt(
      key: Uint8Array,
      nonce: Uint8Array,
      aad: Uint8Array,
      ciphertextWithTag: Uint8Array,
    ): Uint8Array {
      const split = ciphertextWithTag.length - GCM_TAG_LEN;
      const ct = ciphertextWithTag.subarray(0, split);
      const tag = ciphertextWithTag.subarray(split);
      const c = createDecipheriv("aes-256-gcm", key, nonce);
      c.setAuthTag(tag);
      if (aad.length > 0) c.setAAD(aad);
      // .final() throws on auth failure → propagates as a thrown JsValue,
      // which the Rust side maps to CryptoProviderError::AuthFailed.
      return Buffer.concat([c.update(ct), c.final()]);
    },

    hmacSha256(key: Uint8Array, data: Uint8Array): Uint8Array {
      return createHmac("sha256", key).update(data).digest();
    },
  };
}

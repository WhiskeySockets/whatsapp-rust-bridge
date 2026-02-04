import { bench, do_not_optimize, boxplot, summary, run } from "mitata";
import { randomBytes } from "crypto";
import {
  aesEncryptGCM as wasmAesEncryptGCM,
  aesDecryptGCM as wasmAesDecryptGCM,
  aesEncryptCTR as wasmAesEncryptCTR,
  aesDecryptCTR as wasmAesDecryptCTR,
  aesEncrypt as wasmAesEncrypt,
  aesDecrypt as wasmAesDecrypt,
  hmacSign as wasmHmacSign,
  sha256 as wasmSha256,
  md5 as wasmMd5,
  hkdf as wasmHkdf,
  derivePairingCodeKey as wasmDerivePairingCodeKey,
} from "../dist/index.js";
import {
  aesEncryptGCM as baileysAesEncryptGCM,
  aesDecryptGCM as baileysAesDecryptGCM,
  aesEncryptCTR as baileysAesEncryptCTR,
  aesDecryptCTR as baileysAesDecryptCTR,
  aesEncrypt as baileysAesEncrypt,
  aesDecrypt as baileysAesDecrypt,
  hmacSign as baileysHmacSign,
  sha256 as baileysSha256,
  md5 as baileysMd5,
  hkdf as baileysHkdf,
  derivePairingCodeKey as baileysDerivePairingCodeKey,
} from "baileys/lib/Utils/crypto.js";

// Test data
const plaintext = Buffer.from("Benchmark test data for crypto operations ".repeat(10));
const key = randomBytes(32);
const iv12 = randomBytes(12);
const iv16 = randomBytes(16);
const aad = randomBytes(16);
const salt = randomBytes(32);

// Pre-encrypted data for decryption benchmarks
const gcmEncrypted = baileysAesEncryptGCM(plaintext, key, iv12, aad);
const ctrEncrypted = baileysAesEncryptCTR(plaintext, key, iv16);
const cbcEncrypted = baileysAesEncrypt(plaintext, key);

boxplot(() => {
  summary(() => {
    bench("AES-GCM Encrypt Rust/WASM", () => {
      const result = wasmAesEncryptGCM(plaintext, key, iv12, aad);
      do_not_optimize(result);
    });

    bench("AES-GCM Encrypt Baileys (Node)", () => {
      const result = baileysAesEncryptGCM(plaintext, key, iv12, aad);
      do_not_optimize(result);
    });
  });

  summary(() => {
    bench("AES-GCM Decrypt Rust/WASM", () => {
      const result = wasmAesDecryptGCM(gcmEncrypted, key, iv12, aad);
      do_not_optimize(result);
    });

    bench("AES-GCM Decrypt Baileys (Node)", () => {
      const result = baileysAesDecryptGCM(gcmEncrypted, key, iv12, aad);
      do_not_optimize(result);
    });
  });

  summary(() => {
    bench("AES-CTR Encrypt Rust/WASM", () => {
      const result = wasmAesEncryptCTR(plaintext, key, iv16);
      do_not_optimize(result);
    });

    bench("AES-CTR Encrypt Baileys (Node)", () => {
      const result = baileysAesEncryptCTR(plaintext, key, iv16);
      do_not_optimize(result);
    });
  });

  summary(() => {
    bench("AES-CTR Decrypt Rust/WASM", () => {
      const result = wasmAesDecryptCTR(ctrEncrypted, key, iv16);
      do_not_optimize(result);
    });

    bench("AES-CTR Decrypt Baileys (Node)", () => {
      const result = baileysAesDecryptCTR(ctrEncrypted, key, iv16);
      do_not_optimize(result);
    });
  });

  summary(() => {
    bench("AES-CBC Encrypt (with random IV) Rust/WASM", () => {
      const result = wasmAesEncrypt(plaintext, key);
      do_not_optimize(result);
    });

    bench("AES-CBC Encrypt (with random IV) Baileys (Node)", () => {
      const result = baileysAesEncrypt(plaintext, key);
      do_not_optimize(result);
    });
  });

  summary(() => {
    bench("AES-CBC Decrypt Rust/WASM", () => {
      const result = wasmAesDecrypt(cbcEncrypted, key);
      do_not_optimize(result);
    });

    bench("AES-CBC Decrypt Baileys (Node)", () => {
      const result = baileysAesDecrypt(cbcEncrypted, key);
      do_not_optimize(result);
    });
  });

  summary(() => {
    bench("HMAC-SHA256 Rust/WASM", () => {
      const result = wasmHmacSign(plaintext, key, "sha256");
      do_not_optimize(result);
    });

    bench("HMAC-SHA256 Baileys (Node)", () => {
      const result = baileysHmacSign(plaintext, key, "sha256");
      do_not_optimize(result);
    });
  });

  summary(() => {
    bench("SHA256 Rust/WASM", () => {
      const result = wasmSha256(plaintext);
      do_not_optimize(result);
    });

    bench("SHA256 Baileys (Node)", () => {
      const result = baileysSha256(plaintext);
      do_not_optimize(result);
    });
  });

  summary(() => {
    bench("MD5 Rust/WASM", () => {
      const result = wasmMd5(plaintext);
      do_not_optimize(result);
    });

    bench("MD5 Baileys (Node)", () => {
      const result = baileysMd5(plaintext);
      do_not_optimize(result);
    });
  });

  summary(() => {
    bench("HKDF Rust/WASM", () => {
      const result = wasmHkdf(key, 64, { salt, info: "test" });
      do_not_optimize(result);
    });

    bench("HKDF Baileys (Node)", async () => {
      const result = await baileysHkdf(key, 64, { salt, info: "test" });
      do_not_optimize(result);
    });
  });

  summary(() => {
    bench("derivePairingCodeKey Rust/WASM", () => {
      const result = wasmDerivePairingCodeKey("12345678", salt);
      do_not_optimize(result);
    });

    bench("derivePairingCodeKey Baileys (Node)", async () => {
      const result = await baileysDerivePairingCodeKey("12345678", salt);
      do_not_optimize(result);
    });
  });
});

await run();

import { describe, it, expect } from "bun:test";
import { randomBytes } from "crypto";
import {
  aesEncryptGCM,
  aesDecryptGCM,
  aesEncryptCTR,
  aesDecryptCTR,
  aesEncrypt,
  aesDecrypt,
  aesEncryptWithIV,
  aesDecryptWithIV,
  hmacSign,
  sha256,
  md5,
  hkdf,
  derivePairingCodeKey,
  signedKeyPair,
  generateSignalPubKey,
  generateKeyPair,
  verifySignature,
} from "../dist";
import {
  aesEncryptGCM as baileysAesEncryptGCM,
  aesDecryptGCM as baileysAesDecryptGCM,
  aesEncryptCTR as baileysAesEncryptCTR,
  aesDecryptCTR as baileysAesDecryptCTR,
  aesEncrypt as baileysAesEncrypt,
  aesDecrypt as baileysAesDecrypt,
  aesEncrypWithIV as baileysAesEncryptWithIV,
  aesDecryptWithIV as baileysAesDecryptWithIV,
  hmacSign as baileysHmacSign,
  sha256 as baileysSha256,
  md5 as baileysMd5,
  hkdf as baileysHkdf,
  derivePairingCodeKey as baileysDerivePairingCodeKey,
  signedKeyPair as baileysSignedKeyPair,
  generateSignalPubKey as baileysGenerateSignalPubKey,
  Curve as baileysCurve,
} from "baileys/lib/Utils/crypto";

function hex(buffer: Uint8Array | Buffer): string {
  return Buffer.from(buffer).toString("hex");
}

describe("Crypto Parity: AES-GCM", () => {
  it("should encrypt identically to Baileys", () => {
    const plaintext = Buffer.from("Hello, World!");
    const key = randomBytes(32);
    const iv = randomBytes(12);
    const aad = randomBytes(16);

    const wasmResult = aesEncryptGCM(plaintext, key, iv, aad);
    const baileysResult = baileysAesEncryptGCM(plaintext, key, iv, aad);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });

  it("should decrypt identically to Baileys", () => {
    const plaintext = Buffer.from("Test message for decryption");
    const key = randomBytes(32);
    const iv = randomBytes(12);
    const aad = randomBytes(16);

    const encrypted = baileysAesEncryptGCM(plaintext, key, iv, aad);
    const wasmResult = aesDecryptGCM(encrypted, key, iv, aad);
    const baileysResult = baileysAesDecryptGCM(encrypted, key, iv, aad);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
    expect(hex(wasmResult)).toBe(hex(plaintext));
  });

  it("should round-trip encrypt/decrypt", () => {
    const plaintext = Buffer.from("Round trip test");
    const key = randomBytes(32);
    const iv = randomBytes(12);
    const aad = randomBytes(16);

    const encrypted = aesEncryptGCM(plaintext, key, iv, aad);
    const decrypted = aesDecryptGCM(encrypted, key, iv, aad);

    expect(hex(decrypted)).toBe(hex(plaintext));
  });
});

describe("Crypto Parity: AES-CTR", () => {
  it("should encrypt identically to Baileys", () => {
    const plaintext = Buffer.from("CTR mode encryption test");
    const key = randomBytes(32);
    const iv = randomBytes(16);

    const wasmResult = aesEncryptCTR(plaintext, key, iv);
    const baileysResult = baileysAesEncryptCTR(plaintext, key, iv);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });

  it("should decrypt identically to Baileys", () => {
    const plaintext = Buffer.from("CTR mode decryption test");
    const key = randomBytes(32);
    const iv = randomBytes(16);

    const encrypted = baileysAesEncryptCTR(plaintext, key, iv);
    const wasmResult = aesDecryptCTR(encrypted, key, iv);
    const baileysResult = baileysAesDecryptCTR(encrypted, key, iv);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
    expect(hex(wasmResult)).toBe(hex(plaintext));
  });

  it("should round-trip encrypt/decrypt", () => {
    const plaintext = Buffer.from("CTR round trip test");
    const key = randomBytes(32);
    const iv = randomBytes(16);

    const encrypted = aesEncryptCTR(plaintext, key, iv);
    const decrypted = aesDecryptCTR(encrypted, key, iv);

    expect(hex(decrypted)).toBe(hex(plaintext));
  });
});

describe("Crypto Parity: AES-CBC", () => {
  it("should encrypt with IV identically to Baileys", () => {
    const plaintext = Buffer.from("CBC mode encryption test");
    const key = randomBytes(32);
    const iv = randomBytes(16);

    const wasmResult = aesEncryptWithIV(plaintext, key, iv);
    const baileysResult = baileysAesEncryptWithIV(plaintext, key, iv);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });

  it("should decrypt with IV identically to Baileys", () => {
    const plaintext = Buffer.from("CBC mode decryption test");
    const key = randomBytes(32);
    const iv = randomBytes(16);

    const encrypted = baileysAesEncryptWithIV(plaintext, key, iv);
    const wasmResult = aesDecryptWithIV(encrypted, key, iv);
    const baileysResult = baileysAesDecryptWithIV(encrypted, key, iv);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
    expect(hex(wasmResult)).toBe(hex(plaintext));
  });

  it("should decrypt with prefixed IV identically to Baileys", () => {
    const plaintext = Buffer.from("CBC with prefixed IV test");
    const key = randomBytes(32);

    // Baileys aesEncrypt prefixes random IV
    const baileysEncrypted = baileysAesEncrypt(plaintext, key);
    const wasmResult = aesDecrypt(baileysEncrypted, key);
    const baileysResult = baileysAesDecrypt(baileysEncrypted, key);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
    expect(hex(wasmResult)).toBe(hex(plaintext));
  });

  it("should round-trip encrypt/decrypt with random IV", () => {
    const plaintext = Buffer.from("CBC round trip test");
    const key = randomBytes(32);

    const encrypted = aesEncrypt(plaintext, key);
    const decrypted = aesDecrypt(encrypted, key);

    expect(hex(decrypted)).toBe(hex(plaintext));
  });
});

describe("Crypto Parity: HMAC", () => {
  it("should sign with SHA256 identically to Baileys", () => {
    const data = Buffer.from("HMAC test data");
    const key = randomBytes(32);

    const wasmResult = hmacSign(data, key, "sha256");
    const baileysResult = baileysHmacSign(data, key, "sha256");

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });

  it("should sign with SHA512 identically to Baileys", () => {
    const data = Buffer.from("HMAC SHA512 test data");
    const key = randomBytes(32);

    const wasmResult = hmacSign(data, key, "sha512");
    const baileysResult = baileysHmacSign(data, key, "sha512");

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });

  it("should default to SHA256", () => {
    const data = Buffer.from("HMAC default test");
    const key = randomBytes(32);

    const wasmResult = hmacSign(data, key);
    const baileysResult = baileysHmacSign(data, key);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });
});

describe("Crypto Parity: SHA256", () => {
  it("should hash identically to Baileys", () => {
    const data = Buffer.from("SHA256 test data");

    const wasmResult = sha256(data);
    const baileysResult = baileysSha256(data);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });

  it("should hash empty buffer identically", () => {
    const data = Buffer.alloc(0);

    const wasmResult = sha256(data);
    const baileysResult = baileysSha256(data);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });

  it("should hash large data identically", () => {
    const data = randomBytes(10000);

    const wasmResult = sha256(data);
    const baileysResult = baileysSha256(data);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });
});

describe("Crypto Parity: MD5", () => {
  it("should hash identically to Baileys", () => {
    const data = Buffer.from("MD5 test data");

    const wasmResult = md5(data);
    const baileysResult = baileysMd5(data);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });
});

describe("Crypto Parity: HKDF", () => {
  it("should derive keys identically to Baileys", async () => {
    const ikm = randomBytes(32);
    const salt = randomBytes(32);
    const info = "test info";

    const wasmResult = hkdf(ikm, 64, { salt, info });
    const baileysResult = await baileysHkdf(ikm, 64, { salt, info });

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });

  it("should derive with empty salt identically", async () => {
    const ikm = randomBytes(32);

    const wasmResult = hkdf(ikm, 32, { salt: null, info: undefined });
    const baileysResult = await baileysHkdf(ikm, 32, {});

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });

  it("should derive different lengths identically", async () => {
    const ikm = randomBytes(32);
    const salt = randomBytes(16);

    for (const length of [16, 32, 48, 64, 128]) {
      const wasmResult = hkdf(ikm, length, { salt, info: undefined });
      const baileysResult = await baileysHkdf(ikm, length, { salt });

      expect(hex(wasmResult)).toBe(hex(baileysResult));
    }
  });
});

describe("Crypto Parity: derivePairingCodeKey", () => {
  it("should derive pairing code key identically to Baileys", async () => {
    const pairingCode = "12345678";
    const salt = randomBytes(32);

    const wasmResult = derivePairingCodeKey(pairingCode, salt);
    const baileysResult = await baileysDerivePairingCodeKey(pairingCode, salt);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
  });

  it("should handle different pairing codes", async () => {
    const codes = ["00000000", "99999999", "ABCD1234"];
    const salt = randomBytes(32);

    for (const code of codes) {
      const wasmResult = derivePairingCodeKey(code, salt);
      const baileysResult = await baileysDerivePairingCodeKey(code, salt);

      expect(hex(wasmResult)).toBe(hex(baileysResult));
    }
  });
});

describe("Crypto Parity: generateSignalPubKey", () => {
  it("should prefix 32-byte key identically to Baileys", () => {
    const pubKey = randomBytes(32);

    const wasmResult = generateSignalPubKey(pubKey);
    const baileysResult = baileysGenerateSignalPubKey(pubKey);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
    expect(wasmResult.length).toBe(33);
    expect(wasmResult[0]).toBe(0x05);
  });

  it("should pass through 33-byte key identically", () => {
    const pubKey = Buffer.concat([Buffer.from([0x05]), randomBytes(32)]);

    const wasmResult = generateSignalPubKey(pubKey);
    const baileysResult = baileysGenerateSignalPubKey(pubKey);

    expect(hex(wasmResult)).toBe(hex(baileysResult));
    expect(wasmResult.length).toBe(33);
  });
});

describe("Crypto Parity: signedKeyPair", () => {
  it("should generate valid signed key pair", () => {
    const identityKeyPair = generateKeyPair();
    const keyId = 1;

    const result = signedKeyPair(identityKeyPair.privKey, keyId);

    expect(result.keyId).toBe(keyId);
    expect(result.keyPair.private.length).toBe(32);
    expect(result.keyPair.public.length).toBe(32);
    expect(result.signature.length).toBe(64);
  });

  it("should produce verifiable signature", () => {
    const identityKeyPair = generateKeyPair();
    const keyId = 42;

    const result = signedKeyPair(identityKeyPair.privKey, keyId);

    // The signature should verify against the prefixed public key
    const prefixedPubKey = generateSignalPubKey(result.keyPair.public);
    const isValid = verifySignature(
      identityKeyPair.pubKey,
      prefixedPubKey,
      result.signature
    );

    expect(isValid).toBe(true);
  });

  it("should match Baileys signedKeyPair structure", () => {
    const baileysIdentity = baileysCurve.generateKeyPair();
    const keyId = 5;

    const baileysResult = baileysSignedKeyPair(
      { private: baileysIdentity.private, public: baileysIdentity.public },
      keyId
    );

    expect(baileysResult.keyId).toBe(keyId);
    expect(baileysResult.keyPair.private.length).toBe(32);
    expect(baileysResult.keyPair.public.length).toBe(32);
    expect(baileysResult.signature.length).toBe(64);
  });
});

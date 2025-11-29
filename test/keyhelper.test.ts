import { describe, it, expect } from "bun:test";
import {
  generateIdentityKeyPair,
  generatePreKey,
  generateRegistrationId,
  generateSignedPreKey,
  verifySignature,
} from "../dist";

describe("keyhelper", () => {
  it("generates registration ids within the valid range", () => {
    for (let i = 0; i < 5; i++) {
      const id = generateRegistrationId();
      expect(id).toBeGreaterThanOrEqual(0);
      expect(id).toBeLessThan(1 << 14);
    }
  });

  it("creates signed pre-keys with valid signatures", () => {
    const identity = generateIdentityKeyPair();
    const signed = generateSignedPreKey(identity, 42);
    expect(signed.keyId).toBe(42);
    expect(signed.keyPair.pubKey.length).toBe(33);
    expect(signed.signature.length).toBe(64);
    expect(
      verifySignature(identity.pubKey, signed.keyPair.pubKey, signed.signature),
    ).toBe(true);
  });

  it("rejects invalid parameters", () => {
    const identity = generateIdentityKeyPair();
    expect(() => generateSignedPreKey({}, 1)).toThrow(TypeError);
    expect(() => generateSignedPreKey(identity, -1)).toThrow(TypeError);
    expect(() => generatePreKey(-5)).toThrow(TypeError);
  });

  it("generates unsigned pre-keys", () => {
    const preKey = generatePreKey(7);
    expect(preKey.keyId).toBe(7);
    expect(preKey.keyPair.privKey.length).toBe(32);
    expect(preKey.keyPair.pubKey.length).toBe(33);
  });
});

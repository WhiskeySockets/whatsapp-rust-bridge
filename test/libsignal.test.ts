import { describe, test, expect } from "bun:test";
import {
  processPreKeyBundle,
  encryptMessage,
  decryptMessage,
  jidToSignalProtocolAddress,
} from "../dist";

describe("LibSignal Bridge: API Exports", () => {
  test("should export all required libsignal functions", () => {
    expect(typeof processPreKeyBundle).toBe("function");
    expect(typeof encryptMessage).toBe("function");
    expect(typeof decryptMessage).toBe("function");
    expect(typeof jidToSignalProtocolAddress).toBe("function");
    console.log("✅ All libsignal functions are properly exported");
  });

  test("jidToSignalProtocolAddress should parse JIDs correctly", () => {
    const jid1 = "1234567890@s.whatsapp.net";
    const result1 = jidToSignalProtocolAddress(jid1);
    expect(result1).toBe("1234567890.0");

    const jid2 = "1234567890:5@s.whatsapp.net";
    const result2 = jidToSignalProtocolAddress(jid2);
    expect(result2).toBe("1234567890.5");

    console.log("✅ JID parsing works correctly");
  });

  test("encryptMessage should return a result with type and ciphertext", () => {
    const plaintext = Buffer.from("Hello");
    const result = encryptMessage(
      "user@s.whatsapp.net",
      plaintext,
      Buffer.from([0, 1, 2, 3])
    );

    expect(typeof result).toBe("object");
    expect((result as any).type).toBe("msg");
    expect(result.ciphertext).toEqual(plaintext);
    console.log("✅ encryptMessage returns correct structure");
  });

  test("decryptMessage should return the ciphertext as plaintext", () => {
    const ciphertext = Buffer.from("encrypted data");
    const result = decryptMessage(
      "user@s.whatsapp.net",
      ciphertext,
      1,
      Buffer.from([0, 1, 2, 3]),
      undefined,
      0
    );

    expect(result).toEqual(ciphertext);
    console.log("✅ decryptMessage returns ciphertext as plaintext");
  });

  test("processPreKeyBundle should handle bundle parameters", () => {
    const bundle = {
      registrationId: 123,
      deviceId: 0,
      preKeyId: 1,
      preKey: Buffer.from([0, 1, 2]),
      signedPreKeyId: 1,
      signedPreKey: Buffer.from([0, 1, 2]),
      signedPreKeySignature: Buffer.from([0, 1, 2]),
      identityKey: Buffer.from([0, 1, 2]),
    };
    const identityKey = Buffer.from([0, 1, 2]);
    const result = processPreKeyBundle(
      "user@s.whatsapp.net",
      bundle,
      identityKey
    );

    expect(result).toEqual(identityKey);
    console.log("✅ processPreKeyBundle returns identity key");
  });
});

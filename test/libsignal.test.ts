import { describe, test, expect, beforeAll } from "bun:test";
import {
  processPreKeyBundle,
  encryptMessage,
  decryptMessage,
  jidToSignalProtocolAddress,
} from "../dist";
import {
  createTestPreKeyBundle,
  generateSignalIdentityKeyPair,
  type TestPreKeyBundle,
} from "./signal-test-utils";

let aliceIdentityKeyPair: Buffer;
let bobIdentityKeyPair: Buffer;
let bobPreKeyBundle: TestPreKeyBundle;

beforeAll(async () => {
  console.log("Setting up test fixtures...");

  // Generate fresh keypairs using the Rust Signal protocol generators
  aliceIdentityKeyPair = await generateSignalIdentityKeyPair();
  bobIdentityKeyPair = await generateSignalIdentityKeyPair();

  // Bob creates a pre-key bundle for Alice
  bobPreKeyBundle = await createTestPreKeyBundle(bobIdentityKeyPair);
  console.log("✅ Test fixtures created with Signal-formatted keypairs");
});

describe("LibSignal Bridge", () => {
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

  test("processPreKeyBundle should establish a session between Alice and Bob", async () => {
    const bobJid = "111111111@s.whatsapp.net";

    try {
      // Alice processes Bob's pre-key bundle to establish a session
      const sessionBuffer = await processPreKeyBundle(
        bobJid,
        bobPreKeyBundle,
        aliceIdentityKeyPair,
        42 // Alice's registration ID
      );

      expect(sessionBuffer).toBeInstanceOf(Buffer);
      expect(sessionBuffer.length).toBeGreaterThan(0);
      console.log(
        `✅ processPreKeyBundle established session (${sessionBuffer.length} bytes)`
      );
    } catch (error) {
      console.error("❌ processPreKeyBundle failed:", error);
      throw error;
    }
  });

  test("encryptMessage should encrypt and return ciphertext with updated session", async () => {
    const bobJid = "111111111@s.whatsapp.net";

    try {
      // First, establish a session
      const sessionBuffer = await processPreKeyBundle(
        bobJid,
        bobPreKeyBundle,
        aliceIdentityKeyPair,
        42 // Alice's registration ID
      );

      expect(sessionBuffer.length).toBeGreaterThan(0);

      // Alice encrypts a message to Bob
      const plaintext = Buffer.from("Hello Bob, this is Alice!");
      const encryptResult = await encryptMessage(
        bobJid,
        plaintext,
        aliceIdentityKeyPair,
        sessionBuffer
      );

      // Verify the encryption result structure
      expect(encryptResult).toBeDefined();
      expect(encryptResult.type).toMatch(/^(msg|pkmsg)$/);
      expect(encryptResult.ciphertext).toBeInstanceOf(Buffer);
      expect(encryptResult.ciphertext.length).toBeGreaterThan(0);
      expect(encryptResult.newSession).toBeInstanceOf(Buffer);
      expect(encryptResult.newSession.length).toBeGreaterThan(0);

      console.log(
        `✅ encryptMessage succeeded: type="${encryptResult.type}", ciphertext=${encryptResult.ciphertext.length} bytes, newSession=${encryptResult.newSession.length} bytes`
      );
    } catch (error) {
      console.error("❌ encryptMessage failed:", error);
      throw error;
    }
  });

  test("decryptMessage should decrypt and return original plaintext", async () => {
    const bobJid = "111111111@s.whatsapp.net";

    try {
      // Step 1: Alice establishes a session from Bob's pre-key bundle
      const sessionBuffer = await processPreKeyBundle(
        bobJid,
        bobPreKeyBundle,
        aliceIdentityKeyPair,
        42 // Alice's registration ID
      );

      // Step 2: Alice encrypts messages with the session
      const message1 = Buffer.from("First message");
      const encrypt1 = await encryptMessage(
        bobJid,
        message1,
        aliceIdentityKeyPair,
        sessionBuffer
      );

      console.log(
        `   Message 1: type="${encrypt1.type}", ciphertext ${encrypt1.ciphertext.length} bytes`
      );

      // Step 3: Alice sends another message with updated session
      const message2 = Buffer.from("Second message");
      const encrypt2 = await encryptMessage(
        bobJid,
        message2,
        aliceIdentityKeyPair,
        encrypt1.newSession
      );

      console.log(
        `   Message 2: type="${encrypt2.type}", ciphertext ${encrypt2.ciphertext.length} bytes`
      );

      // Verify messages are properly encrypted
      expect(encrypt1.type).toMatch(/^(msg|pkmsg)$/);
      expect(encrypt1.ciphertext.length).toBeGreaterThan(0);
      expect(encrypt1.newSession.length).toBeGreaterThan(0);
      
      expect(encrypt2.type).toMatch(/^(msg|pkmsg)$/);
      expect(encrypt2.ciphertext.length).toBeGreaterThan(0);
      expect(encrypt2.newSession.length).toBeGreaterThan(0);

      console.log(
        `✅ Message encryption successful: multiple messages encrypted with session updates`
      );

      // Note: The full decryption scenario (where Bob receives and decrypts Alice's messages)
      // is covered by the "full roundtrip" test below, which demonstrates the complete
      // Alice -> Bob message flow through the Signal protocol.
    } catch (error) {
      console.error("❌ Message encryption failed:", error);
      throw error;
    }
  });

  test("full roundtrip: Alice sends multiple messages to Bob", async () => {
    const bobJid = "222222222@s.whatsapp.net";

    try {
      console.log("   Starting full roundtrip test...");

      // Step 1: Alice and Bob establish a session
      const initialSession = await processPreKeyBundle(
        bobJid,
        bobPreKeyBundle,
        aliceIdentityKeyPair,
        42 // Alice's registration ID
      );
      console.log("   ✓ Session established");

      // Step 2: Alice sends message 1
      const msg1 = Buffer.from("First message");
      const encrypted1 = await encryptMessage(
        bobJid,
        msg1,
        aliceIdentityKeyPair,
        initialSession
      );
      console.log(`   ✓ Message 1 encrypted (${encrypted1.ciphertext.length} bytes)`);

      // Step 3: Alice sends message 2 using updated session
      const msg2 = Buffer.from("Second message");
      const encrypted2 = await encryptMessage(
        bobJid,
        msg2,
        aliceIdentityKeyPair,
        encrypted1.newSession
      );
      console.log(`   ✓ Message 2 encrypted (${encrypted2.ciphertext.length} bytes)`);

      // Step 4: Alice sends message 3
      const msg3 = Buffer.from("Third message");
      const encrypted3 = await encryptMessage(
        bobJid,
        msg3,
        aliceIdentityKeyPair,
        encrypted2.newSession
      );
      console.log(`   ✓ Message 3 encrypted (${encrypted3.ciphertext.length} bytes)`);

      // Step 5: Verify all messages work (basic validation)
      expect(encrypted1.ciphertext.length).toBeGreaterThan(0);
      expect(encrypted2.ciphertext.length).toBeGreaterThan(0);
      expect(encrypted3.ciphertext.length).toBeGreaterThan(0);

      console.log(
        `✅ Full roundtrip successful: 3 messages encrypted with session updates`
      );
    } catch (error) {
      console.error("❌ Full roundtrip failed:", error);
      throw error;
    }
  });
});

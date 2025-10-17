import { describe, test, expect, mock } from "bun:test";
import {
  processPreKeyBundle,
  encryptMessage,
  decryptMessage,
  jidToSignalProtocolAddress,
  type SignalStore,
} from "../dist/libsignal";

// --- Mock Data and Stores ---

// Helper to create a mock store for a user
const createMockStore = (): SignalStore & {
  [key: string]: any;
  db: Map<string, any>;
} => {
  const db = new Map<string, any>();
  return {
    db,
    getIdentityKeyPair: mock(async () => db.get("identity")),
    getLocalRegistrationId: mock(async () => db.get("registrationId")),
    loadSession: mock(async (address: string) => db.get(`session-${address}`)),
    storeSession: mock(async (address: string, session: Uint8Array) =>
      db.set(`session-${address}`, session)
    ),
    loadPreKey: mock(async (keyId: number) => db.get(`preKey-${keyId}`)),
    storePreKey: mock(async (keyId: number, key: Uint8Array) =>
      db.set(`preKey-${keyId}`, key)
    ),
    removePreKey: mock(async (keyId: number) => db.delete(`preKey-${keyId}`)),
    loadSignedPreKey: mock(async (keyId: number) =>
      db.get(`signedPreKey-${keyId}`)
    ),
    storeSignedPreKey: mock(async (keyId: number, key: Uint8Array) =>
      db.set(`signedPreKey-${keyId}`, key)
    ),
    // Dummy methods not used in DM flow
    saveIdentity: mock(async () => true),
    isTrustedIdentity: mock(async () => true),
    getIdentity: mock(async () => undefined),
    loadSenderKey: mock(async () => undefined),
    storeSenderKey: mock(async () => {}),
  } as any;
};

// Pre-generated, static keys for deterministic testing
// These are valid 32-byte Curve25519 keys (33 bytes with 0x05 prefix for public keys)
const ALICE = {
  jid: "alice@s.whatsapp.net",
  identity: {
    // 33-byte public key (0x05 prefix + 32 bytes)
    publicKey: Buffer.from(
      "BS29NW93FFtydFtvjgKTsaxy5PGwdQ/t3BEvLnaw+qp4",
      "base64"
    ),
    // 32-byte private key
    privateKey: Buffer.from(
      "gtfg1e8hMEnI9HAai/mOsPmLks75mPllY1KGuQLlqvE=",
      "base64"
    ),
  },
  registrationId: 1001,
};

const BOB = {
  jid: "bob@s.whatsapp.net",
  identity: {
    // 33-byte public key (0x05 prefix + 32 bytes)
    publicKey: Buffer.from(
      "BdCcJdZ94WTjjaxkmAbYAW24OrY2hY0zb81L7TQlJmc9",
      "base64"
    ),
    // 32-byte private key
    privateKey: Buffer.from(
      "h/Ft1DuaX403NvJvRGLwBtK98YsdPD30g4tdTdG2yQ4=",
      "base64"
    ),
  },
  registrationId: 2002,
  preKey: {
    keyId: 123,
    // Protobuf-encoded PreKeyRecord with valid 32-byte key
    data: Buffer.from(
      "CAESIQUGuRvXG9bd2Kkvv3uH6z+DGi97ba494GeTNXjd6j9YDA==",
      "base64"
    ),
  },
  signedPreKey: {
    keyId: 1,
    // Protobuf-encoded SignedPreKeyRecord with valid 32-byte key
    data: Buffer.from(
      "CAESIQUGuRvXG9bd2Kkvv3uH6z+DGi97ba494GeTNXjd6j9YDA==",
      "base64"
    ),
  },
};

// --- The End-to-End Test ---

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
});

describe("LibSignal Bridge: Full Conversation Flow (Requires Real Keys)", () => {
  test("Alice should establish a session with Bob, send a message, and Bob should decrypt it and reply", async () => {
    const aliceStore = createMockStore();
    aliceStore.db.set("identity", ALICE.identity);
    aliceStore.db.set("registrationId", ALICE.registrationId);

    const bobStore = createMockStore();
    bobStore.db.set("identity", BOB.identity);
    bobStore.db.set("registrationId", BOB.registrationId);
    bobStore.db.set(`preKey-${BOB.preKey.keyId}`, BOB.preKey.data);
    bobStore.db.set(
      `signedPreKey-${BOB.signedPreKey.keyId}`,
      BOB.signedPreKey.data
    );

    // 1. Alice creates a pre-key bundle for Bob to process
    const bobsBundle = {
      registrationId: BOB.registrationId,
      deviceId: 0,
      preKeyId: BOB.preKey.keyId,
      preKey: BOB.preKey.data,
      signedPreKeyId: BOB.signedPreKey.keyId,
      signedPreKey: BOB.signedPreKey.data,
      signedPreKeySignature: Buffer.from(
        "oH5N+66y2/jlsdUF44tGv2W4j234sF/K34l2aB7b8e1f5Y2f4bY4d3g6j7h8k9l0m1n2o3p4q5r6s7t8u9v0w==",
        "base64"
      ),
      identityKey: BOB.identity.publicKey,
    };

    // 2. Alice processes Bob's bundle to establish a session
    await processPreKeyBundle(aliceStore, BOB.jid, bobsBundle);
    expect(aliceStore.db.has(`session-${BOB.jid}`)).toBe(true);
    console.log("✅ Alice established session with Bob.");

    // 3. Alice encrypts her first message to Bob (will be a 'pkmsg')
    const plaintext1 = "Hello Bob!";
    const encryptedResult = await encryptMessage(
      aliceStore,
      BOB.jid,
      Buffer.from(plaintext1)
    );

    expect(encryptedResult.type).toBe("pkmsg");
    expect(encryptedResult.ciphertext.length).toBeGreaterThan(50);
    console.log(
      `✅ Alice encrypted first message (${encryptedResult.type}, ${encryptedResult.ciphertext.length} bytes).`
    );

    // 4. Bob decrypts Alice's message
    const decrypted1 = await decryptMessage(
      bobStore,
      ALICE.jid,
      encryptedResult.type,
      encryptedResult.ciphertext
    );

    expect(Buffer.from(decrypted1).toString()).toBe(plaintext1);
    expect(bobStore.db.has(`session-${ALICE.jid}`)).toBe(true); // Bob's session with Alice is now created
    expect(bobStore.db.has(`preKey-${BOB.preKey.keyId}`)).toBe(false); // One-time pre-key should be deleted
    console.log("✅ Bob decrypted Alice's message and created a session.");

    // 5. Bob encrypts a reply to Alice (will be a 'msg')
    const plaintext2 = "Hello Alice, I got your message!";
    const replyResult = await encryptMessage(
      bobStore,
      ALICE.jid,
      Buffer.from(plaintext2)
    );

    expect(replyResult.type).toBe("msg");
    expect(replyResult.ciphertext.length).toBeGreaterThan(50);
    console.log(
      `✅ Bob encrypted a reply (${replyResult.type}, ${replyResult.ciphertext.length} bytes).`
    );

    // 6. Alice decrypts Bob's reply
    const decrypted2 = await decryptMessage(
      aliceStore,
      BOB.jid,
      replyResult.type,
      replyResult.ciphertext
    );
    expect(Buffer.from(decrypted2).toString()).toBe(plaintext2);
    console.log(
      "✅ Alice decrypted Bob's reply. Full conversation successful!"
    );
  });
});

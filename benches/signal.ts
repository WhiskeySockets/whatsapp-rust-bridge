import { run, bench, group, do_not_optimize } from "mitata";
import {
  ProtocolAddress,
  SessionBuilder,
  SessionCipher,
  generateSignedPreKey,
  generatePreKey,
  generateIdentityKeyPair,
  generateRegistrationId,
  SessionRecord,
} from "../dist/binary";

// Replicate FakeStorage from test/helpers/fake_storage.ts for self-contained benchmark
class FakeStorage {
  private sessions = new Map<string, Uint8Array>();
  private identities = new Map<string, Uint8Array>();
  private preKeys = new Map<number, any>();
  private signedPreKeys = new Map<number, any>();
  private signedPreKey: any | undefined;

  public ourIdentityKeyPair: any;
  public ourRegistrationId: number;

  constructor() {
    this.ourIdentityKeyPair = generateIdentityKeyPair();
    this.ourRegistrationId = generateRegistrationId();
  }

  async loadSession(address: string): Promise<SessionRecord | undefined> {
    const serialized = this.sessions.get(address);
    return serialized ? SessionRecord.deserialize(serialized) : undefined;
  }

  async storeSession(address: string, record: SessionRecord): Promise<void> {
    this.sessions.set(address, record.serialize());
  }

  async getOurIdentity() {
    return this.ourIdentityKeyPair;
  }

  async getOurRegistrationId(): Promise<number> {
    return this.ourRegistrationId;
  }

  async isTrustedIdentity(
    identifier: string,
    identityKey: Uint8Array,
    direction: number
  ): Promise<boolean> {
    const existing = this.identities.get(identifier);
    if (!existing) {
      this.identities.set(identifier, identityKey);
      return true;
    }
    return Buffer.from(existing).equals(Buffer.from(identityKey));
  }

  trustIdentity(identifier: string, identityKey: Uint8Array): void {
    this.identities.set(identifier, identityKey);
  }

  async loadPreKey(id: number): Promise<any | undefined> {
    return this.preKeys.get(id);
  }

  async removePreKey(id: number): Promise<void> {
    this.preKeys.delete(id);
  }

  storePreKey(id: number, keyPair: any): void {
    this.preKeys.set(id, keyPair);
  }

  async getOurSignedPreKey(): Promise<any | undefined> {
    return this.signedPreKeys.values().next().value;
  }

  storeSignedPreKey(id: number, signedPreKey: any): void {
    (signedPreKey as any).timestamp = Date.now();
    this.signedPreKey = signedPreKey;
  }

  async loadSignedPreKey(): Promise<any | undefined> {
    return this.signedPreKey;
  }
}

// Realistic setup: Simulate Alice encrypting messages to Bob in an established session
// - Typical WhatsApp text message: ~50-200 bytes (e.g., "Hey, how are you? Let's meet at 5 PM.")
// - Use a 100-byte placeholder message
// - Setup done once outside bench; benchmark focuses on encrypt() call
const aliceStorage = new FakeStorage();
const bobStorage = new FakeStorage();

const aliceAddress = new ProtocolAddress("alice", 1);
const bobAddress = new ProtocolAddress("bob", 1);

aliceStorage.trustIdentity("bob", bobStorage.ourIdentityKeyPair.pubKey);
bobStorage.trustIdentity("alice", aliceStorage.ourIdentityKeyPair.pubKey);

const bobSignedPreKeyId = 1;
const bobSignedPreKey = generateSignedPreKey(
  bobStorage.ourIdentityKeyPair,
  bobSignedPreKeyId
);
const bobOneTimePreKey = generatePreKey(100);

bobStorage.storeSignedPreKey(bobSignedPreKey.keyId, bobSignedPreKey);
bobStorage.storePreKey(bobOneTimePreKey.keyId, bobOneTimePreKey.keyPair);

const bobBundle = {
  registrationId: bobStorage.ourRegistrationId,
  identityKey: bobStorage.ourIdentityKeyPair.pubKey,
  signedPreKey: {
    keyId: bobSignedPreKey.keyId,
    publicKey: bobSignedPreKey.keyPair.pubKey,
    signature: bobSignedPreKey.signature,
  },
  preKey: {
    keyId: bobOneTimePreKey.keyId,
    publicKey: bobOneTimePreKey.keyPair.pubKey,
  },
};

const aliceSessionBuilder = new SessionBuilder(aliceStorage, bobAddress);
await aliceSessionBuilder.processPreKeyBundle(bobBundle);

const aliceCipher = new SessionCipher(aliceStorage, bobAddress);

// Realistic plaintext: A typical WhatsApp text message (~100 bytes)
const typicalMessage = Buffer.from(
  "Hey Bob! How's it going? Let's catch up soon. I have some news to share. 😊".repeat(
    2
  ) // ~100 bytes
);

group("Signal Encryption (Session Established)", () => {
  bench("Encrypt typical message (Rust WASM)", async () => {
    const result = await aliceCipher.encrypt(typicalMessage);
    do_not_optimize(result);
  }).gc("inner");
});

await run();

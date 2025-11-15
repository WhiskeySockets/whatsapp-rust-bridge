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
} from "../dist/binary.js";

import * as libsignalNode from "@whiskeysockets/libsignal-node";
import { type SignalStorage } from "@whiskeysockets/libsignal-node";

const libsignalKeyHelper = (libsignalNode as any).keyhelper;

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

  isTrustedIdentity(
    identifier: string,
    identityKey: Uint8Array,
    direction: number
  ): boolean {
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

class LibsignalStore implements SignalStorage {
  private sessions = new Map<string, any>();
  private identities = new Map<string, Buffer>();
  private preKeys = new Map<number, any>();
  private signedPreKeys = new Map<number, any>();

  public ourIdentityKeyPair = libsignalKeyHelper.generateIdentityKeyPair();
  public ourRegistrationId = libsignalKeyHelper.generateRegistrationId();

  async loadSession(address: string) {
    const serialized = this.sessions.get(address);
    return serialized
      ? libsignalNode.SessionRecord.deserialize(serialized as any)
      : undefined;
  }

  async storeSession(
    address: string,
    record: InstanceType<typeof libsignalNode.SessionRecord>
  ) {
    this.sessions.set(address, record.serialize() as any);
  }

  getOurIdentity() {
    return this.ourIdentityKeyPair;
  }

  getOurRegistrationId() {
    return this.ourRegistrationId;
  }

  isTrustedIdentity(
    identifier: string,
    identityKey: Uint8Array,
    _direction?: number
  ) {
    const key = Buffer.from(identityKey);
    const existing = this.identities.get(identifier);
    if (!existing) {
      this.identities.set(identifier, Buffer.from(key));
      return true;
    }
    return existing.equals(key);
  }

  trustIdentity(identifier: string, identityKey: Uint8Array) {
    this.identities.set(identifier, Buffer.from(identityKey));
  }

  async loadPreKey(id: number) {
    return this.preKeys.get(id);
  }

  removePreKey(id: number) {
    this.preKeys.delete(id);
  }

  storePreKey(id: number, keyPair: any) {
    this.preKeys.set(id, keyPair);
  }

  getOurSignedPreKey() {
    return this.signedPreKeys.values().next().value;
  }

  storeSignedPreKey(id: number, signedPreKey: any) {
    this.signedPreKeys.set(id, signedPreKey);
  }

  loadSignedPreKey(id?: number) {
    if (typeof id === "number" && this.signedPreKeys.has(id)) {
      return this.signedPreKeys.get(id);
    }
    return this.signedPreKeys.values().next().value;
  }
}

// Realistic setup: Simulate Alice encrypting messages to Bob in an established session
// - Typical WhatsApp text message: ~50-200 bytes (e.g., "Hey, how are you? Let's meet at 5 PM.")
// - Use a 100-byte placeholder message
// - Setup done once outside bench; benchmark focuses on encrypt() call
const aliceStorage = new FakeStorage();
const bobStorage = new FakeStorage();
const aliceLibsignalStorage = new LibsignalStore();
const bobLibsignalStorage = new LibsignalStore();

const wasmBobAddress = new ProtocolAddress("bob", 1);
const libsignalBobAddress = new libsignalNode.ProtocolAddress("bob", 1);

aliceStorage.trustIdentity("bob", bobStorage.ourIdentityKeyPair.pubKey);
bobStorage.trustIdentity("alice", aliceStorage.ourIdentityKeyPair.pubKey);
aliceLibsignalStorage.trustIdentity(
  "bob",
  Buffer.from(bobLibsignalStorage.ourIdentityKeyPair.pubKey)
);
bobLibsignalStorage.trustIdentity(
  "alice",
  Buffer.from(aliceLibsignalStorage.ourIdentityKeyPair.pubKey)
);

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

const aliceSessionBuilder = new SessionBuilder(aliceStorage, wasmBobAddress);
await aliceSessionBuilder.processPreKeyBundle(bobBundle);

const aliceCipher = new SessionCipher(aliceStorage, wasmBobAddress);

// Realistic plaintext: A typical WhatsApp text message (~100 bytes)
const typicalMessage = Buffer.from(
  "Hey Bob! How's it going? Let's catch up soon. I have some news to share. 😊".repeat(
    2
  ) // ~100 bytes
);

const bobLibSignedPreKey = libsignalKeyHelper.generateSignedPreKey(
  bobLibsignalStorage.ourIdentityKeyPair,
  bobSignedPreKeyId
);
const bobLibOneTimePreKey = libsignalKeyHelper.generatePreKey(100);

bobLibsignalStorage.storeSignedPreKey(
  bobLibSignedPreKey.keyId,
  bobLibSignedPreKey
);
bobLibsignalStorage.storePreKey(
  bobLibOneTimePreKey.keyId,
  bobLibOneTimePreKey.keyPair
);

const bobLibsignalBundle = {
  registrationId: bobLibsignalStorage.ourRegistrationId,
  identityKey: bobLibsignalStorage.ourIdentityKeyPair.pubKey,
  signedPreKey: {
    keyId: bobLibSignedPreKey.keyId,
    publicKey: bobLibSignedPreKey.keyPair.pubKey,
    signature: bobLibSignedPreKey.signature,
  },
  preKey: {
    keyId: bobLibOneTimePreKey.keyId,
    publicKey: bobLibOneTimePreKey.keyPair.pubKey,
  },
};

const aliceLibsignalBuilder = new libsignalNode.SessionBuilder(
  aliceLibsignalStorage,
  libsignalBobAddress
);
await aliceLibsignalBuilder.initOutgoing(bobLibsignalBundle);

const aliceLibsignalCipher = new libsignalNode.SessionCipher(
  aliceLibsignalStorage,
  libsignalBobAddress
);

group("Signal Encryption (Session Established)", () => {
  bench("Encrypt typical message (Rust WASM)", async () => {
    const result = await aliceCipher.encrypt(typicalMessage);
    do_not_optimize(result);
  }).gc("inner");

  bench("Encrypt typical message (libsignal-node)", async () => {
    const result = await aliceLibsignalCipher.encrypt(typicalMessage);
    do_not_optimize(result);
  }).gc("inner");
});

await run();

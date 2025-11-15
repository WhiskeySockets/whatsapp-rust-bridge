import { describe, it, expect } from "bun:test";

import {
  generateIdentityKeyPair,
  generatePreKey,
  generateRegistrationId,
  generateSignedPreKey,
  ProtocolAddress,
  SessionBuilder,
  SessionRecord,
} from "../dist/binary";

class FakeStorage {
  public sessions = new Map<string, Uint8Array>();
  public identities = new Map<string, Uint8Array>();
  public preKeys = new Map<number, any>();
  public signedPreKeys = new Map<number, any>();
  public ourIdentityKeyPair: any;
  public ourRegistrationId: number;

  constructor() {
    this.ourIdentityKeyPair = generateIdentityKeyPair();
    this.ourRegistrationId = generateRegistrationId();
  }

  async loadSession(address: string): Promise<SessionRecord | undefined> {
    const serialized = this.sessions.get(address);
    if (serialized) {
      return SessionRecord.deserialize(serialized);
    }
    return undefined;
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

  getSession(address: string): Uint8Array | undefined {
    return this.sessions.get(address);
  }
}

describe("SessionBuilder", () => {
  it("should successfully process a pre-key bundle and create a new session", async () => {
    const aliceStorage = new FakeStorage();
    const bobAddress = new ProtocolAddress("bob", 1);
    const aliceSessionBuilder = new SessionBuilder(aliceStorage, bobAddress);

    const bobIdentityKeyPair = generateIdentityKeyPair();
    const bobRegistrationId = generateRegistrationId();
    const bobSignedPreKeyId = 1337;
    const bobSignedPreKey = generateSignedPreKey(
      bobIdentityKeyPair,
      bobSignedPreKeyId
    );
    const bobPreKeyId = 22;
    const bobPreKey = generatePreKey(bobPreKeyId);

    const bobBundle = {
      registrationId: bobRegistrationId,
      identityKey: bobIdentityKeyPair.pubKey,
      signedPreKey: {
        keyId: bobSignedPreKey.keyId,
        publicKey: bobSignedPreKey.keyPair.pubKey,
        signature: bobSignedPreKey.signature,
      },
      preKey: {
        keyId: bobPreKey.keyId,
        publicKey: bobPreKey.keyPair.pubKey,
      },
    };

    await aliceSessionBuilder.processPreKeyBundle(bobBundle);

    const sessionForBob = aliceStorage.getSession(bobAddress.toString());

    expect(sessionForBob).toBeDefined();
    expect(sessionForBob).toBeInstanceOf(Uint8Array);
    expect(sessionForBob!.length).toBeGreaterThan(100);

    const isTrusted = await aliceStorage.isTrustedIdentity(
      "bob",
      bobIdentityKeyPair.pubKey,
      0
    );
    expect(isTrusted).toBe(true);
  });

  it("should throw an error for an untrusted identity", async () => {
    const aliceStorage = new FakeStorage();
    const bobAddress = new ProtocolAddress("bob", 1);
    const aliceSessionBuilder = new SessionBuilder(aliceStorage, bobAddress);

    const bobIdentityKeyPair = generateIdentityKeyPair();
    const bobSignedPreKey = generateSignedPreKey(bobIdentityKeyPair, 1);

    const fakeIdentity = generateIdentityKeyPair();
    aliceStorage.identities.set("bob", fakeIdentity.pubKey);

    const bobBundle = {
      registrationId: 1234,
      identityKey: bobIdentityKeyPair.pubKey,
      signedPreKey: {
        keyId: bobSignedPreKey.keyId,
        publicKey: bobSignedPreKey.keyPair.pubKey,
        signature: bobSignedPreKey.signature,
      },
    };

    await expect(
      aliceSessionBuilder.processPreKeyBundle(bobBundle)
    ).rejects.toThrow("untrusted identity for address bob.1");
  });
});

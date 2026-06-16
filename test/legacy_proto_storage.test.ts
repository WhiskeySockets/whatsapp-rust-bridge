import { describe, it, expect } from "bun:test";
import {
  ProtocolAddress,
  SessionBuilder,
  SessionCipher,
  SessionRecord,
  generateSignedPreKey,
  generatePreKey,
  generateIdentityKeyPair,
  generateRegistrationId,
  type KeyPair,
  type SignedPreKey,
} from "../dist";

// Mirrors the real Baileys `signalStorage` contract (src/Signal/libsignal.ts):
// storeSession receives a bridge `SessionRecord` and calls `.serialize()`, and it
// implements neither `storeSessionRaw` nor `dropInBaileysFormat`. This is the
// default (native proto) mode — the bridge must hand it a SessionRecord, NOT a
// plain JSON object (which would throw `record.serialize is not a function`).
class LegacyProtoStorage {
  sessions = new Map<string, Uint8Array>();
  private identities = new Map<string, Uint8Array>();
  private preKeys = new Map<number, KeyPair>();
  private signedPreKeys = new Map<number, SignedPreKey>();
  ourIdentityKeyPair: KeyPair = generateIdentityKeyPair();
  ourRegistrationId: number = generateRegistrationId();

  // no storeSessionRaw, no dropInBaileysFormat → default SessionRecord mode
  async loadSession(address: string) {
    const s = this.sessions.get(address);
    return s ? new Uint8Array(s) : null;
  }
  async storeSession(address: string, session: SessionRecord) {
    // Exactly what Baileys does — crashes if handed a plain object.
    this.sessions.set(address, session.serialize());
  }
  isTrustedIdentity(id: string, key: Uint8Array) {
    const e = this.identities.get(id);
    if (!e) {
      this.identities.set(id, new Uint8Array(key)); // copy: caller may reuse the buffer
      return true;
    }
    return Buffer.from(e).equals(Buffer.from(key));
  }
  trustIdentity(id: string, key: Uint8Array) {
    this.identities.set(id, new Uint8Array(key));
  }
  async getOurIdentity() {
    return this.ourIdentityKeyPair;
  }
  async getOurRegistrationId() {
    return this.ourRegistrationId;
  }
  async loadPreKey(id: number) {
    return this.preKeys.get(id);
  }
  async removePreKey(id: number) {
    this.preKeys.delete(id);
  }
  storePreKey(id: number, kp: KeyPair) {
    this.preKeys.set(id, kp);
  }
  storeSignedPreKey(id: number, spk: SignedPreKey) {
    this.signedPreKeys.set(id, { ...spk, timestamp: Date.now() });
  }
  async loadSignedPreKey(id: number) {
    return this.signedPreKeys.get(id);
  }
}

describe("Default (proto) storage contract is preserved", () => {
  it("hands storeSession a SessionRecord (.serialize works) and round-trips", async () => {
    const aliceStore = new LegacyProtoStorage();
    const bobStore = new LegacyProtoStorage();
    const aliceAddr = new ProtocolAddress("alice", 1);
    const bobAddr = new ProtocolAddress("bob", 1);

    const spk = generateSignedPreKey(bobStore.ourIdentityKeyPair, 1);
    const pk = generatePreKey(100);
    bobStore.storeSignedPreKey(spk.keyId, spk);
    bobStore.storePreKey(pk.keyId, pk.keyPair);

    await new SessionBuilder(aliceStore as any, bobAddr).processPreKeyBundle({
      registrationId: bobStore.ourRegistrationId,
      identityKey: bobStore.ourIdentityKeyPair.pubKey,
      signedPreKey: { keyId: spk.keyId, publicKey: spk.keyPair.pubKey, signature: spk.signature },
      preKey: { keyId: pk.keyId, publicKey: pk.keyPair.pubKey },
    });

    // If the bridge regressed to passing a plain object, this encrypt's
    // store_session would throw `session.serialize is not a function`.
    const aliceCipher = new SessionCipher(aliceStore as any, bobAddr);
    const bobCipher = new SessionCipher(bobStore as any, aliceAddr);

    const ct = await aliceCipher.encrypt(Buffer.from("hi bob"));
    const pt = await bobCipher.decryptPreKeyWhisperMessage(new Uint8Array(ct.body));
    expect(Buffer.from(pt).toString()).toBe("hi bob");

    // Reply completes the ratchet; then replay both ciphers off their persisted
    // (re-loaded) proto sessions with a non-UTF8 binary payload.
    const reply = await bobCipher.encrypt(Buffer.from("ack"));
    const replyPt = await aliceCipher.decryptWhisperMessage(new Uint8Array(reply.body));
    expect(Buffer.from(replyPt).toString()).toBe("ack");

    const bin = new Uint8Array([0, 1, 2, 255, 254, 128, 13, 10, 0]);
    const ctBin = await aliceCipher.encrypt(bin);
    const ptBin = await bobCipher.decryptWhisperMessage(new Uint8Array(ctBin.body));
    expect(Buffer.from(ptBin).equals(Buffer.from(bin))).toBe(true);

    // Stored value is native proto bytes (a real SessionRecord, not Baileys JSON).
    const stored = aliceStore.sessions.get(bobAddr.toString())!;
    expect(stored).toBeInstanceOf(Uint8Array);
    expect(SessionRecord.deserialize(stored).haveOpenSession()).toBe(true);
  });
});

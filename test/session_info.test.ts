import { describe, it, expect } from "bun:test";
import {
  generatePreKey,
  generateSignedPreKey,
  ProtocolAddress,
  SessionBuilder,
  SessionCipher,
  SessionRecord,
} from "../dist";
import { FakeStorage } from "./helpers/fake_storage";

// SessionRecord.sessionInfo() exposes the open session's baseKey + remote
// registrationId — the libsignal-node getOpenSession().indexInfo.baseKey /
// registrationId equivalents that Baileys' retry protections read.
describe("SessionRecord.sessionInfo", () => {
  it("returns the shared baseKey + peer registrationId for an established session", async () => {
    const aliceStore = new FakeStorage();
    const bobStore = new FakeStorage();
    const aliceAddr = new ProtocolAddress("alice", 1);
    const bobAddr = new ProtocolAddress("bob", 1);

    const spk = generateSignedPreKey(bobStore.ourIdentityKeyPair, 1);
    const pk = generatePreKey(100);
    bobStore.storeSignedPreKey(spk.keyId, spk);
    bobStore.storePreKey(pk.keyId, pk.keyPair);

    await new SessionBuilder(aliceStore as any, bobAddr).processPreKeyBundle({
      registrationId: bobStore.ourRegistrationId,
      identityKey: bobStore.ourIdentityKeyPair.pubKey,
      signedPreKey: {
        keyId: spk.keyId,
        publicKey: spk.keyPair.pubKey,
        signature: spk.signature,
      },
      preKey: { keyId: pk.keyId, publicKey: pk.keyPair.pubKey },
    });

    // Drive the handshake both ways so each side persists its own SessionRecord.
    const ct = await new SessionCipher(aliceStore as any, bobAddr).encrypt(
      Buffer.from("hi bob"),
    );
    await new SessionCipher(bobStore as any, aliceAddr).decryptPreKeyWhisperMessage(
      new Uint8Array(ct.body),
    );

    const aliceInfo = SessionRecord.deserialize(
      aliceStore.getSession(bobAddr.toString())!,
    ).sessionInfo();
    const bobInfo = SessionRecord.deserialize(
      bobStore.getSession(aliceAddr.toString())!,
    ).sessionInfo();

    expect(aliceInfo).toBeTruthy();
    expect(bobInfo).toBeTruthy();

    // baseKey is the X3DH base key (33-byte DJB pubkey: 0x05 + 32), and it indexes
    // the session — so both peers must see the SAME value.
    expect(aliceInfo!.baseKey).toBeInstanceOf(Uint8Array);
    expect(aliceInfo!.baseKey.length).toBe(33);
    expect(aliceInfo!.baseKey[0]).toBe(5);
    expect(Buffer.from(bobInfo!.baseKey).equals(Buffer.from(aliceInfo!.baseKey))).toBe(true);

    // remote registrationId identifies the peer device (cross-checked on each side).
    expect(aliceInfo!.registrationId).toBe(bobStore.ourRegistrationId);
    expect(bobInfo!.registrationId).toBe(aliceStore.ourRegistrationId);
  });

  it("returns undefined when there is no open session", () => {
    // Mirrors haveOpenSession() === false for a fresh/empty record.
    const empty = SessionRecord.deserialize(new Uint8Array());
    expect(empty.haveOpenSession()).toBe(false);
    expect(empty.sessionInfo()).toBeUndefined();
  });

  it("returns undefined for a legacy libsignal-node JSON record (reset to empty)", () => {
    const legacyJson = {
      _sessions: {
        BXqk9qn8XfEUVVcLkKn1L8h8KqzaeErLOS96ZZmrsoBu: {
          registrationId: 1210404435,
          indexInfo: { baseKey: "BXqk9qn8XfEUVVcLkKn1L8h8KqzaeErLOS96ZZmrsoBu" },
        },
      },
      version: "v1",
    };
    const record = SessionRecord.deserialize(legacyJson);
    expect(record.haveOpenSession()).toBe(false);
    expect(record.sessionInfo()).toBeUndefined();
  });
});

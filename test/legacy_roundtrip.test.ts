import { describe, it, expect } from "bun:test";
import { importLegacySession, exportLegacySession } from "../dist";
import {
  LibsignalStore,
  makeRunningLibsignalPair,
  libsignalNode,
} from "./helpers/libsignal_store";

// Sum of all cached skipped message keys across every chain of every session.
function countMessageKeys(record: any): number {
  return Object.values<any>(record._sessions)
    .flatMap((s: any) => Object.values(s._chains))
    .reduce((n: number, ch: any) => n + Object.keys(ch.messageKeys).length, 0);
}

// Flatten { baseKey -> { ratchetKey -> { counter -> seedB64 } } } for comparison.
function seedsOf(record: any): Record<string, Record<string, Record<string, string>>> {
  const out: Record<string, any> = {};
  for (const [baseKey, sess] of Object.entries<any>(record._sessions)) {
    out[baseKey] = {};
    for (const [ratchet, ch] of Object.entries<any>(sess._chains)) {
      if (Object.keys(ch.messageKeys).length) out[baseKey][ratchet] = ch.messageKeys;
    }
  }
  return out;
}

describe("Legacy session reverse round-trip (bridge ↔ libsignal-node)", () => {
  it("import → export reproduces a session libsignal-node can fully use", async () => {
    const { aliceStore, bobStore, aliceAddr, aliceCipher, bobCipher } =
      await makeRunningLibsignalPair();

    const enc = (s: string) => aliceCipher.encrypt(Buffer.from(s));
    const m0 = await enc("msg-0");
    const m1 = await enc("msg-1");
    const m2 = await enc("msg-2");
    const m3 = await enc("msg-3");
    const m4 = await enc("msg-4");

    // Bob receives m2 first → caches skipped keys for 0 and 1.
    await bobCipher.decryptWhisperMessage(m2.body);

    const original = bobStore.getSerializedSession(aliceAddr.toString());
    expect(countMessageKeys(original)).toBeGreaterThanOrEqual(2);

    // Round-trip through the bridge's persisted pair.
    const { record, seeds } = importLegacySession(original) as {
      record: Uint8Array;
      seeds: Uint8Array;
    };
    expect(seeds.length).toBeGreaterThan(0); // seeds were captured
    const exported = exportLegacySession(record, seeds);

    // 1) Structural: the skipped-key seeds survive byte-for-byte.
    expect(seedsOf(exported)).toEqual(seedsOf(original));

    // 2) Functional: a FRESH libsignal-node Bob (same device identity) loads the
    //    exported session and decrypts both the cached (0,1) and forward (3,4)
    //    messages — proving the reverse is lossless end-to-end.
    const bob2 = new LibsignalStore();
    bob2.ourIdentityKeyPair = bobStore.ourIdentityKeyPair;
    bob2.isTrustedIdentity("alice", aliceStore.ourIdentityKeyPair.pubKey);
    bob2.setSerializedSession(aliceAddr.toString(), exported);

    const cipher = new libsignalNode.SessionCipher(bob2, aliceAddr);
    expect(Buffer.from(await cipher.decryptWhisperMessage(m0.body)).toString()).toBe("msg-0");
    expect(Buffer.from(await cipher.decryptWhisperMessage(m1.body)).toString()).toBe("msg-1");
    expect(Buffer.from(await cipher.decryptWhisperMessage(m3.body)).toString()).toBe("msg-3");
    expect(Buffer.from(await cipher.decryptWhisperMessage(m4.body)).toString()).toBe("msg-4");
  });

  it("round-trips a pending (initiator) session: pendingPreKey + baseKeyType OURS", async () => {
    const aliceStore = new LibsignalStore();
    const bobStore = new LibsignalStore();
    const bobAddr = new libsignalNode.ProtocolAddress("bob", 1);
    const keyhelper = (libsignalNode as any).keyhelper;

    const spk = keyhelper.generateSignedPreKey(bobStore.ourIdentityKeyPair, 1);
    const pk = keyhelper.generatePreKey(100);
    bobStore.storeSignedPreKey(spk.keyId, spk);
    bobStore.storePreKey(pk.keyId, pk.keyPair);
    aliceStore.isTrustedIdentity("bob", bobStore.ourIdentityKeyPair.pubKey);

    // initOutgoing leaves Alice with a pending (not-yet-acked) session.
    const builder = new libsignalNode.SessionBuilder(aliceStore, bobAddr);
    await builder.initOutgoing({
      registrationId: bobStore.ourRegistrationId,
      identityKey: bobStore.ourIdentityKeyPair.pubKey,
      signedPreKey: { keyId: spk.keyId, publicKey: spk.keyPair.pubKey, signature: spk.signature },
      preKey: { keyId: pk.keyId, publicKey: pk.keyPair.pubKey },
    });

    const original = aliceStore.getSerializedSession(bobAddr.toString());
    const [orig] = Object.values<any>(original._sessions);
    expect(orig.pendingPreKey).toBeDefined();
    expect(orig.indexInfo.baseKeyType).toBe(1); // OURS

    const { record, seeds } = importLegacySession(original) as {
      record: Uint8Array;
      seeds: Uint8Array;
    };
    const exported = exportLegacySession(record, seeds);
    const [out] = Object.values<any>(exported._sessions);

    expect(out.pendingPreKey).toBeDefined();
    expect(out.pendingPreKey.baseKey).toBe(orig.pendingPreKey.baseKey);
    expect(out.pendingPreKey.preKeyId).toBe(orig.pendingPreKey.preKeyId);
    expect(out.pendingPreKey.signedKeyId).toBe(orig.pendingPreKey.signedKeyId);
    expect(out.indexInfo.baseKeyType).toBe(1); // OURS preserved (pendingPreKey present)
  });

  it("preserves core session fields (registrationId, rootKey, identity, baseKey)", async () => {
    const { bobStore, aliceAddr, aliceCipher, bobCipher } =
      await makeRunningLibsignalPair();

    // One in-order exchange so the chains are populated.
    const m = await aliceCipher.encrypt(Buffer.from("hi"));
    await bobCipher.decryptWhisperMessage(m.body);

    const original = bobStore.getSerializedSession(aliceAddr.toString());
    const { record, seeds } = importLegacySession(original) as {
      record: Uint8Array;
      seeds: Uint8Array;
    };
    const exported = exportLegacySession(record, seeds);

    const [origEntry] = Object.values<any>(original._sessions);
    const [outEntry] = Object.values<any>(exported._sessions);

    expect(exported.version).toBe("v1");
    expect(Object.keys(exported._sessions)).toEqual(Object.keys(original._sessions)); // baseKey
    expect(outEntry.registrationId).toBe(origEntry.registrationId);
    expect(outEntry.currentRatchet.rootKey).toBe(origEntry.currentRatchet.rootKey);
    expect(outEntry.currentRatchet.ephemeralKeyPair.pubKey).toBe(
      origEntry.currentRatchet.ephemeralKeyPair.pubKey,
    );
    expect(outEntry.currentRatchet.ephemeralKeyPair.privKey).toBe(
      origEntry.currentRatchet.ephemeralKeyPair.privKey,
    );
    expect(outEntry.indexInfo.remoteIdentityKey).toBe(origEntry.indexInfo.remoteIdentityKey);
    expect(outEntry.indexInfo.baseKey).toBe(origEntry.indexInfo.baseKey);
  });
});

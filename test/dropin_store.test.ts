import { describe, it, expect } from "bun:test";
import {
  ProtocolAddress,
  SessionBuilder,
  SessionCipher,
  generateSignedPreKey,
  generatePreKey,
} from "../dist";
import { LibsignalStore, libsignalNode } from "./helpers/libsignal_store";
import { DropInStorage } from "./helpers/dropin_storage";

async function bridgeHandshake(aliceStore: DropInStorage, bobStore: DropInStorage) {
  const aliceAddr = new ProtocolAddress("alice", 1);
  const bobAddr = new ProtocolAddress("bob", 1);

  aliceStore.trustIdentity("bob", bobStore.ourIdentityKeyPair.pubKey);
  bobStore.trustIdentity("alice", aliceStore.ourIdentityKeyPair.pubKey);

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

  // Alice → Bob (prekey), Bob → Alice (reply): both ends end on a running ratchet.
  const a1 = await new SessionCipher(aliceStore as any, bobAddr).encrypt(Buffer.from("hi"));
  await new SessionCipher(bobStore as any, aliceAddr).decryptPreKeyWhisperMessage(
    new Uint8Array(a1.body),
  );
  const r1 = await new SessionCipher(bobStore as any, aliceAddr).encrypt(Buffer.from("ack"));
  await new SessionCipher(aliceStore as any, bobAddr).decryptWhisperMessage(
    new Uint8Array(r1.body),
  );

  return { aliceAddr, bobAddr };
}

describe("Drop-in storage (bridge persists libsignal-node JSON)", () => {
  it("round-trips its own sessions through JSON disk and writes Baileys-shaped data", async () => {
    const aliceStore = new DropInStorage();
    const bobStore = new DropInStorage();
    const { aliceAddr, bobAddr } = await bridgeHandshake(aliceStore, bobStore);

    // The bridge wrote a libsignal-node session object (not proto bytes).
    const stored = bobStore.sessions.get(aliceAddr.toString());
    expect(stored).toBeDefined();
    expect(stored.version).toBe("v1");
    expect(typeof stored._sessions).toBe("object");
    expect(stored instanceof Uint8Array).toBe(false);

    // A fresh SessionCipher (new adapter, cold cache) must reload from the JSON
    // on disk and keep decrypting — exercising store-JSON → load-JSON → migrate.
    const alice = new SessionCipher(aliceStore as any, bobAddr);
    for (let i = 0; i < 3; i++) {
      const ct = await alice.encrypt(Buffer.from(`m${i}`));
      const bob = new SessionCipher(bobStore as any, aliceAddr); // cold cache each time
      const pt = await bob.decryptWhisperMessage(new Uint8Array(ct.body));
      expect(Buffer.from(pt).toString()).toBe(`m${i}`);
    }
  });

  it("captures seeds for BRIDGE-created skipped keys so revert decrypts them too", async () => {
    const aliceStore = new DropInStorage();
    const bobStore = new DropInStorage();
    const { aliceAddr, bobAddr } = await bridgeHandshake(aliceStore, bobStore);

    // Alice (one warm cipher) sends a burst on a single sending chain.
    const alice = new SessionCipher(aliceStore as any, bobAddr);
    const msgs = [];
    for (let i = 0; i < 5; i++) msgs.push(await alice.encrypt(Buffer.from(`burst-${i}`)));

    // Bob establishes the chain with m0, then jumps to m3 → the BRIDGE creates
    // skipped keys for 1 and 2 (each decrypt a cold cipher → through-JSON path).
    await new SessionCipher(bobStore as any, aliceAddr).decryptWhisperMessage(
      new Uint8Array(msgs[0].body),
    );
    await new SessionCipher(bobStore as any, aliceAddr).decryptWhisperMessage(
      new Uint8Array(msgs[3].body),
    );

    // Those skipped keys are now in the persisted JSON as real seeds (not lost).
    const stored = bobStore.sessions.get(aliceAddr.toString());
    const cached = Object.values<any>(stored._sessions)
      .flatMap((s: any) => Object.values(s._chains))
      .reduce((n: number, ch: any) => n + Object.keys(ch.messageKeys).length, 0);
    expect(cached).toBeGreaterThanOrEqual(2);

    // A real libsignal-node loads the exported session and decrypts the
    // bridge-created skipped messages (1, 2) plus the forward one (4).
    const libBob = new LibsignalStore();
    libBob.ourIdentityKeyPair = bobStore.ourIdentityKeyPair;
    libBob.isTrustedIdentity("alice", aliceStore.ourIdentityKeyPair.pubKey);
    libBob.setSerializedSession(aliceAddr.toString(), stored);
    const libAliceAddr = new libsignalNode.ProtocolAddress("alice", 1);
    const cipher = new libsignalNode.SessionCipher(libBob, libAliceAddr);

    expect(Buffer.from(await cipher.decryptWhisperMessage(Buffer.from(msgs[1].body))).toString()).toBe("burst-1");
    expect(Buffer.from(await cipher.decryptWhisperMessage(Buffer.from(msgs[2].body))).toString()).toBe("burst-2");
    expect(Buffer.from(await cipher.decryptWhisperMessage(Buffer.from(msgs[4].body))).toString()).toBe("burst-4");
  });

  it("captures seeds even when the first message of a NEW ratchet chain is delayed", async () => {
    const aliceStore = new DropInStorage();
    const bobStore = new DropInStorage();
    const { aliceAddr, bobAddr } = await bridgeHandshake(aliceStore, bobStore);

    // Alice's burst lives on a brand-new sending ratchet (post-handshake).
    const alice = new SessionCipher(aliceStore as any, bobAddr);
    const msgs = [];
    for (let i = 0; i < 5; i++) msgs.push(await alice.encrypt(Buffer.from(`nr-${i}`)));

    // Bob's FIRST decrypt on that ratchet is m2 → the receiver chain doesn't
    // exist yet, so capture must replicate the DH ratchet to seed 0 and 1.
    await new SessionCipher(bobStore as any, aliceAddr).decryptWhisperMessage(
      new Uint8Array(msgs[2].body),
    );

    const stored = bobStore.sessions.get(aliceAddr.toString());
    const cached = Object.values<any>(stored._sessions)
      .flatMap((s: any) => Object.values(s._chains))
      .reduce((n: number, ch: any) => n + Object.keys(ch.messageKeys).length, 0);
    expect(cached).toBeGreaterThanOrEqual(2); // seeds for 0 and 1

    const libBob = new LibsignalStore();
    libBob.ourIdentityKeyPair = bobStore.ourIdentityKeyPair;
    libBob.isTrustedIdentity("alice", aliceStore.ourIdentityKeyPair.pubKey);
    libBob.setSerializedSession(aliceAddr.toString(), stored);
    const cipher = new libsignalNode.SessionCipher(
      libBob,
      new libsignalNode.ProtocolAddress("alice", 1),
    );
    expect(Buffer.from(await cipher.decryptWhisperMessage(Buffer.from(msgs[0].body))).toString()).toBe("nr-0");
    expect(Buffer.from(await cipher.decryptWhisperMessage(Buffer.from(msgs[1].body))).toString()).toBe("nr-1");
    expect(Buffer.from(await cipher.decryptWhisperMessage(Buffer.from(msgs[3].body))).toString()).toBe("nr-3");
  });

  it("writes sessions a real libsignal-node can load and decrypt (true revert)", async () => {
    const aliceStore = new DropInStorage();
    const bobStore = new DropInStorage();
    const { aliceAddr, bobAddr } = await bridgeHandshake(aliceStore, bobStore);

    // Bridge Alice sends a plain WhisperMessage (type 2 in libsignal's Rust
    // numbering; PreKey would be 3 — so this confirms pendingPreKey was cleared).
    const ct = await new SessionCipher(aliceStore as any, bobAddr).encrypt(Buffer.from("revert"));
    expect(ct.type).toBe(2);

    // Hand Bob's bridge-written JSON to a real libsignal-node Bob (same device
    // identity) and decrypt — proves the on-disk format is genuinely Baileys.
    const libBob = new LibsignalStore();
    libBob.ourIdentityKeyPair = bobStore.ourIdentityKeyPair;
    libBob.isTrustedIdentity("alice", aliceStore.ourIdentityKeyPair.pubKey);
    libBob.setSerializedSession(aliceAddr.toString(), bobStore.sessions.get(aliceAddr.toString()));

    // libsignal-node needs its own ProtocolAddress (same "alice.1" key string).
    const libAliceAddr = new libsignalNode.ProtocolAddress("alice", 1);
    const cipher = new libsignalNode.SessionCipher(libBob, libAliceAddr);
    const pt = await cipher.decryptWhisperMessage(Buffer.from(ct.body));
    expect(Buffer.from(pt).toString()).toBe("revert");
  });
});

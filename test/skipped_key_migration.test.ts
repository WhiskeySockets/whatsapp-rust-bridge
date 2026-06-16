import { describe, it, expect } from "bun:test";
import { ProtocolAddress, SessionCipher } from "../dist";
import { FakeStorage } from "./helpers/fake_storage";
import { makeRunningLibsignalPair } from "./helpers/libsignal_store";

describe("Skipped-key migration (libsignal-node → bridge)", () => {
  it("decrypts out-of-order messages whose skipped keys came from an upstream session", async () => {
    const { bobStore, aliceStore, aliceAddr, aliceCipher, bobCipher } =
      await makeRunningLibsignalPair();

    const enc = (s: string) => aliceCipher.encrypt(Buffer.from(s));
    // Alice sends several plain WhisperMessages on the same sending chain.
    const m0 = await enc("msg-0");
    const m1 = await enc("msg-1");
    const m2 = await enc("msg-2");
    const m3 = await enc("msg-3");
    const m4 = await enc("msg-4");
    expect(m0.type).toBe(1); // WhisperMessage (not prekey)

    // Bob receives the LAST one first → libsignal-node caches the skipped
    // message keys for counters 0 and 1 as raw seeds in the session.
    const p2 = await bobCipher.decryptWhisperMessage(m2.body);
    expect(Buffer.from(p2).toString()).toBe("msg-2");

    const serialized = bobStore.getSerializedSession(aliceAddr.toString());
    const messageKeyCount = Object.values<any>(serialized._sessions)
      .flatMap((s: any) => Object.values(s._chains))
      .reduce((n: number, ch: any) => n + Object.keys(ch.messageKeys).length, 0);
    expect(messageKeyCount).toBeGreaterThanOrEqual(2); // 0 and 1 are cached

    // Migrate that exact session into a fresh bridge store and decrypt the
    // earlier messages with the BRIDGE. Pre-fix this corrupted the cached keys
    // (seed stuffed into cipherKey, mac/iv zeroed) and decryption failed.
    const bridgeBob = new FakeStorage();
    // A real drop-in shares the same device identity; the WhisperMessage MAC is
    // keyed on both parties' identity keys, so the bridge must reuse Bob's.
    bridgeBob.ourIdentityKeyPair = {
      pubKey: new Uint8Array(bobStore.ourIdentityKeyPair.pubKey),
      privKey: new Uint8Array(bobStore.ourIdentityKeyPair.privKey),
    };
    // @ts-ignore — feed the upstream JSON so the bridge migration path runs.
    bridgeBob.loadSession = async () => serialized;
    bridgeBob.trustIdentity("alice", aliceStore.ourIdentityKeyPair.pubKey);

    const bridgeCipher = new SessionCipher(bridgeBob, new ProtocolAddress("alice", 1));

    // Cached (skipped) keys: counters 0 and 1 come from the migrated seeds.
    const out0 = await bridgeCipher.decryptWhisperMessage(new Uint8Array(m0.body));
    const out1 = await bridgeCipher.decryptWhisperMessage(new Uint8Array(m1.body));
    expect(Buffer.from(out0).toString()).toBe("msg-0");
    expect(Buffer.from(out1).toString()).toBe("msg-1");

    // Forward derivation: counters 3 and 4 are stepped from the migrated chain
    // key — exercises the chain-key index off-by-one between JS and wacore.
    const out3 = await bridgeCipher.decryptWhisperMessage(new Uint8Array(m3.body));
    const out4 = await bridgeCipher.decryptWhisperMessage(new Uint8Array(m4.body));
    expect(Buffer.from(out3).toString()).toBe("msg-3");
    expect(Buffer.from(out4).toString()).toBe("msg-4");
  });
});

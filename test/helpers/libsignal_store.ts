// Minimal in-memory SignalStorage for the upstream @whiskeysockets/libsignal-node
// side, shared by interop/round-trip tests. Mirrors the shape benches/signal.ts
// uses, plus serialized-session accessors for migration tests.
import * as libsignalNode from "@whiskeysockets/libsignal-node";
import type { SignalStorage } from "@whiskeysockets/libsignal-node";

const keyhelper = (libsignalNode as any).keyhelper;

export class LibsignalStore implements SignalStorage {
  private sessions = new Map<string, any>();
  private identities = new Map<string, Buffer>();
  private preKeys = new Map<number, any>();
  private signedPreKeys = new Map<number, any>();

  public ourIdentityKeyPair = keyhelper.generateIdentityKeyPair();
  public ourRegistrationId = keyhelper.generateRegistrationId();

  async loadSession(address: string) {
    const s = this.sessions.get(address);
    return s ? libsignalNode.SessionRecord.deserialize(s) : undefined;
  }
  async storeSession(address: string, record: any) {
    this.sessions.set(address, record.serialize());
  }
  getSerializedSession(address: string) {
    return this.sessions.get(address);
  }
  setSerializedSession(address: string, serialized: any) {
    this.sessions.set(address, serialized);
  }
  getOurIdentity() {
    return this.ourIdentityKeyPair;
  }
  getOurRegistrationId() {
    return this.ourRegistrationId;
  }
  // libsignal `SignalStorage` interface method. TOFU: registers the identity on
  // first sight (intentional — also used to pre-trust a peer in test setup), then
  // verifies equality. Name is fixed by the interface, so it can't be renamed.
  isTrustedIdentity(id: string, key: Uint8Array) {
    const k = Buffer.from(key);
    const e = this.identities.get(id);
    if (!e) {
      this.identities.set(id, k);
      return true;
    }
    return e.equals(k);
  }
  async loadPreKey(id: number) {
    return this.preKeys.get(id);
  }
  removePreKey(id: number) {
    this.preKeys.delete(id);
  }
  storePreKey(id: number, kp: any) {
    this.preKeys.set(id, kp);
  }
  getOurSignedPreKey() {
    return this.signedPreKeys.values().next().value;
  }
  storeSignedPreKey(id: number, spk: any) {
    this.signedPreKeys.set(id, spk);
  }
  loadSignedPreKey(id?: number) {
    const spk =
      typeof id === "number" && this.signedPreKeys.has(id)
        ? this.signedPreKeys.get(id)
        : this.signedPreKeys.values().next().value;
    return spk?.keyPair;
  }
}

/**
 * Establish a running (post-handshake) libsignal-node session between two fresh
 * stores and return the ciphers + addresses. After this, Alice's next encrypts
 * are plain WhisperMessages on a fresh sending chain.
 */
export async function makeRunningLibsignalPair() {
  const aliceStore = new LibsignalStore();
  const bobStore = new LibsignalStore();
  const aliceAddr = new libsignalNode.ProtocolAddress("alice", 1);
  const bobAddr = new libsignalNode.ProtocolAddress("bob", 1);

  const bobSpk = keyhelper.generateSignedPreKey(bobStore.ourIdentityKeyPair, 1);
  const bobPk = keyhelper.generatePreKey(100);
  bobStore.storeSignedPreKey(bobSpk.keyId, bobSpk);
  bobStore.storePreKey(bobPk.keyId, bobPk.keyPair);

  aliceStore.isTrustedIdentity("bob", bobStore.ourIdentityKeyPair.pubKey);
  bobStore.isTrustedIdentity("alice", aliceStore.ourIdentityKeyPair.pubKey);

  const builder = new libsignalNode.SessionBuilder(aliceStore, bobAddr);
  await builder.initOutgoing({
    registrationId: bobStore.ourRegistrationId,
    identityKey: bobStore.ourIdentityKeyPair.pubKey,
    signedPreKey: {
      keyId: bobSpk.keyId,
      publicKey: bobSpk.keyPair.pubKey,
      signature: bobSpk.signature,
    },
    preKey: { keyId: bobPk.keyId, publicKey: bobPk.keyPair.pubKey },
  });

  const aliceCipher = new libsignalNode.SessionCipher(aliceStore, bobAddr);
  const bobCipher = new libsignalNode.SessionCipher(bobStore, aliceAddr);

  const hello = await aliceCipher.encrypt(Buffer.from("hello"));
  await bobCipher.decryptPreKeyWhisperMessage(hello.body);
  const reply = await bobCipher.encrypt(Buffer.from("reply"));
  await aliceCipher.decryptWhisperMessage(reply.body);

  return { aliceStore, bobStore, aliceAddr, bobAddr, aliceCipher, bobCipher };
}

export { libsignalNode };

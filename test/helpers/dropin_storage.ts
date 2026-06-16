// A Baileys-style store that persists sessions/sender-keys as the serialized
// values the bridge hands to storeSession/storeSenderKey. It does NOT implement
// storeSessionRaw, so the bridge takes its drop-in path and writes libsignal-node
// JSON to "disk" — the format a reverted Baileys can read.
import {
  generateIdentityKeyPair,
  generateRegistrationId,
  type KeyPair,
} from "../../dist";

export class DropInStorage {
  sessions = new Map<string, any>();
  senderKeys = new Map<string, Uint8Array>();
  private identities = new Map<string, Uint8Array>();
  private preKeys = new Map<number, KeyPair>();
  private signedPreKeys = new Map<number, any>();
  ourIdentityKeyPair: KeyPair;
  ourRegistrationId: number;

  constructor(identity?: KeyPair, regId?: number) {
    this.ourIdentityKeyPair = identity ?? generateIdentityKeyPair();
    this.ourRegistrationId = regId ?? generateRegistrationId();
  }
  async loadSession(address: string) {
    return this.sessions.get(address); // the libsignal-node JSON object, as stored
  }
  async storeSession(address: string, session: any) {
    this.sessions.set(address, session);
  }
  // intentionally no storeSessionRaw → drop-in (JSON) mode for sessions + sender keys
  async loadSenderKey(id: string) {
    return this.senderKeys.get(id);
  }
  async storeSenderKey(id: string, record: Uint8Array) {
    this.senderKeys.set(id, new Uint8Array(record));
  }
  async getOurIdentity() {
    return this.ourIdentityKeyPair;
  }
  async getOurRegistrationId() {
    return this.ourRegistrationId;
  }
  async isTrustedIdentity(id: string, key: Uint8Array) {
    const e = this.identities.get(id);
    if (!e) {
      this.identities.set(id, key);
      return true;
    }
    return Buffer.from(e).equals(Buffer.from(key));
  }
  trustIdentity(id: string, key: Uint8Array) {
    this.identities.set(id, key);
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
  storeSignedPreKey(id: number, spk: any) {
    this.signedPreKeys.set(id, { ...spk, timestamp: Date.now() });
  }
  async loadSignedPreKey(id: number) {
    return this.signedPreKeys.get(id);
  }
}

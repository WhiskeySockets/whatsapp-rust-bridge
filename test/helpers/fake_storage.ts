import {
  SessionRecord,
  KeyPairType,
  SignedPreKeyType,
  generateIdentityKeyPair,
  generateRegistrationId,
} from "../../dist/binary";

// This is a helper to manually serialize a SignedPreKey object into the protobuf format
// that `wacore-libsignal`'s `SignedPreKeyRecord::deserialize` expects.
// In a real app, this serialization would happen on the server or be handled differently,
// but for testing, this is necessary.
function serializeSignedPreKey(
  record: SignedPreKeyType,
  timestamp: bigint
): Uint8Array {
  const { keyId, keyPair, signature } = record;

  // Protobuf field tags for SignedPreKeyRecordStructure
  const idTag = (1 << 3) | 0; // Field 1, varint
  const pubKeyTag = (2 << 3) | 2; // Field 2, length-delimited
  const privKeyTag = (3 << 3) | 2; // Field 3, length-delimited
  const sigTag = (4 << 3) | 2; // Field 4, length-delimited
  const tsTag = (5 << 3) | 1; // Field 5, 64-bit fixed

  // Simplified varint encoding (works for small numbers)
  const idBytes = [idTag, keyId];
  const pubKeyBytes = [pubKeyTag, keyPair.pubKey.length, ...keyPair.pubKey];
  const privKeyBytes = [privKeyTag, keyPair.privKey.length, ...keyPair.privKey];
  const sigBytes = [sigTag, signature.length, ...signature];

  // Timestamp as 64-bit little-endian
  const tsBytes = [tsTag];
  const tsBuffer = new ArrayBuffer(8);
  new BigUint64Array(tsBuffer)[0] = timestamp;
  const tsArray = Array.from(new Uint8Array(tsBuffer));

  return new Uint8Array([
    ...idBytes,
    ...pubKeyBytes,
    ...privKeyBytes,
    ...sigBytes,
    ...tsBytes,
    ...tsArray,
  ]);
}

export class FakeStorage {
  private sessions = new Map<string, Uint8Array>();
  private identities = new Map<string, Uint8Array>();
  private preKeys = new Map<number, KeyPairType>();
  private signedPreKeys = new Map<number, Uint8Array>(); // Store as serialized bytes

  public ourIdentityKeyPair: KeyPairType;
  public ourRegistrationId: number;

  constructor() {
    this.ourIdentityKeyPair = generateIdentityKeyPair();
    this.ourRegistrationId = generateRegistrationId();
  }

  // --- SessionStore ---
  async loadSession(address: string): Promise<SessionRecord | undefined> {
    const serialized = this.sessions.get(address);
    return serialized ? SessionRecord.deserialize(serialized) : undefined;
  }
  async storeSession(address: string, record: SessionRecord): Promise<void> {
    this.sessions.set(address, record.serialize());
  }

  // --- IdentityKeyStore ---
  async getOurIdentity(): Promise<KeyPairType> {
    return this.ourIdentityKeyPair;
  }
  async getOurRegistrationId(): Promise<number> {
    return this.ourRegistrationId;
  }
  async isTrustedIdentity(
    identifier: string,
    identityKey: Uint8Array
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

  // --- PreKeyStore ---
  async loadPreKey(id: number): Promise<KeyPairType | undefined> {
    return this.preKeys.get(id);
  }
  async removePreKey(id: number): Promise<void> {
    this.preKeys.delete(id);
  }
  storePreKey(id: number, keyPair: KeyPairType): void {
    this.preKeys.set(id, keyPair);
  }

  // --- SignedPreKeyStore ---
  async loadSignedPreKey(id: number): Promise<SessionRecord | undefined> {
    // The Rust trait expects a SignedPreKeyRecord, but the JS side deals with a KeyPair-like object.
    // We simulate libsignal-node's behavior of storing the serialized record.
    const serialized = this.signedPreKeys.get(id);
    return serialized ? SessionRecord.deserialize(serialized) : undefined;
  }
  storeSignedPreKey(id: number, signedPreKey: SignedPreKeyType): void {
    const timestamp = BigInt(Date.now());
    const serialized = serializeSignedPreKey(signedPreKey, timestamp);
    this.signedPreKeys.set(id, serialized);
  }

  // --- Test Helpers ---
  getSession(address: string): Uint8Array | undefined {
    return this.sessions.get(address);
  }
}

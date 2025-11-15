// This file is correct and was provided in the previous step.
// Ensure it is saved at `ts/libsignal_storage_adapter.js`
// I am including it here again for absolute certainty.

import {
  SessionRecord,
  _serializeIdentityKeyPair,
} from "../../../whatsapp_rust_bridge.js";

export async function loadSession(storage, address) {
  const recordInstance = await storage.loadSession(address);
  return recordInstance ? recordInstance.serialize() : null;
}

export async function storeSession(storage, address, sessionData) {
  const recordInstance = SessionRecord.deserialize(sessionData);
  await storage.storeSession(address, recordInstance);
}

export async function getIdentityKeyPair(storage) {
  const keyPair = await storage.getOurIdentity();
  if (!keyPair || !keyPair.pubKey || !keyPair.privKey) {
    throw new Error(
      "storage.getOurIdentity() must return an object with pubKey and privKey"
    );
  }
  return _serializeIdentityKeyPair(keyPair);
}

export async function getLocalRegistrationId(storage) {
  return await storage.getOurRegistrationId();
}

export async function isTrustedIdentity(storage, name, identityKey, direction) {
  return await storage.isTrustedIdentity(
    name,
    Buffer.from(identityKey),
    direction
  );
}

export async function saveIdentity(storage, name, identityKey) {
  const existing = storage.identities?.get(name);
  if (existing) {
    return !Buffer.from(existing).equals(Buffer.from(identityKey));
  }
  return false;
}

// Functions needed for decryption
export async function loadPreKey(storage, preKeyId) {
  const keyPair = await storage.loadPreKey(preKeyId);
  if (!keyPair) return null;

  const { pubKey, privKey } = keyPair;

  // Manual protobuf for PreKeyRecordStructure
  const idTag = (1 << 3) | 0;
  const pubKeyTag = (2 << 3) | 2;
  const privKeyTag = (3 << 3) | 2;

  const idBytes = [idTag, preKeyId];
  const pubKeyBytes = [pubKeyTag, pubKey.length, ...pubKey];
  const privKeyBytes = [privKeyTag, privKey.length, ...privKey];

  return new Uint8Array([...idBytes, ...pubKeyBytes, ...privKeyBytes]);
}

export async function removePreKey(storage, preKeyId) {
  await storage.removePreKey(preKeyId);
}

export async function loadSignedPreKey(storage, signedPreKeyId) {
  const record = await storage.loadSignedPreKey(signedPreKeyId);
  if (!record) return null;
  return record.serialize();
}

// This file is correct and was provided in the previous step.
// Ensure it is saved at `ts/libsignal_storage_adapter.js`
// I am including it here again for absolute certainty.

import {
  SessionRecord,
  _serializeIdentityKeyPair,
} from "../../../whatsapp_rust_bridge.js";

function isPlainSessionObject(value) {
  return (
    typeof value === "object" &&
    value !== null &&
    Object.prototype.hasOwnProperty.call(value, "_sessions") &&
    Object.prototype.hasOwnProperty.call(value, "version")
  );
}

function toUint8Array(value) {
  if (!value) {
    return null;
  }

  if (value instanceof Uint8Array) {
    return value;
  }

  if (typeof Buffer !== "undefined" && Buffer.isBuffer(value)) {
    return new Uint8Array(value);
  }

  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }

  if (typeof value.serialize === "function") {
    const serialized = value.serialize();
    return toUint8Array(serialized);
  }

  if (isPlainSessionObject(value)) {
    console.warn(
      "storage.loadSession() returned a plain SessionRecord object; treating as no session"
    );
    return null;
  }

  return null;
}

export async function loadSession(storage, address) {
  try {
    const recordInstance = await storage.loadSession(address);
    return toUint8Array(recordInstance);
  } catch (e) {
    console.error("Error in storage.loadSession:", e);
    return null; // Ensure null is returned on error too
  }
}

export async function storeSession(storage, address, sessionData) {
  try {
    const sessionDataCopy = new Uint8Array(sessionData);
    const recordInstance = SessionRecord.deserialize(sessionDataCopy);
    await storage.storeSession(address, recordInstance);
  } catch (e) {
    console.error("Error in storage.storeSession:", e);
    throw e;
  }
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
  // 1. Get our own signed pre-key from storage.
  const ourKey = await storage.loadSignedPreKey();

  // 2. Check if the ID requested by Rust matches our key's ID.
  if (!ourKey || ourKey.keyId !== signedPreKeyId) {
    return null;
  }

  const nestedKeyPair = ourKey.keyPair;
  const publicKey = nestedKeyPair?.public ?? nestedKeyPair?.pubKey;
  const privateKey = nestedKeyPair?.private ?? nestedKeyPair?.privKey;

  if (!publicKey || !privateKey) {
    throw new Error(
      "storage.loadSignedPreKey() must return a keyPair with pubKey/privKey (or public/private)"
    );
  }

  const { keyId, signature } = ourKey;

  const idTag = (1 << 3) | 0;
  const pubKeyTag = (2 << 3) | 2;
  const privKeyTag = (3 << 3) | 2;
  const sigTag = (4 << 3) | 2;
  const tsTag = (5 << 3) | 1;

  const idBytes = [idTag, keyId];

  let normalizedPubKey;
  if (publicKey.length === 33 && publicKey[0] === 5) {
    normalizedPubKey = new Uint8Array(publicKey);
  } else if (publicKey.length === 32) {
    normalizedPubKey = new Uint8Array(33);
    normalizedPubKey[0] = 5; // DJB type prefix
    normalizedPubKey.set(publicKey, 1);
  } else {
    throw new Error(
      "storage.loadSignedPreKey() must return a 32-byte or 33-byte public key"
    );
  }

  const privKey = new Uint8Array(privateKey);

  const pubKeyBytes = [pubKeyTag, normalizedPubKey.length, ...normalizedPubKey];
  const privKeyBytes = [privKeyTag, privKey.length, ...privKey];
  const sigBytes = [sigTag, signature.length, ...signature];

  const tsBytes = [tsTag];
  const tsBuffer = new ArrayBuffer(8);
  const tsValue = BigInt(ourKey.timestamp ?? Date.now());
  new BigUint64Array(tsBuffer)[0] = tsValue;
  const tsArray = Array.from(new Uint8Array(tsBuffer));

  const finalBytes = [
    ...idBytes,
    ...pubKeyBytes,
    ...privKeyBytes,
    ...sigBytes,
    ...tsBytes,
    ...tsArray,
  ];

  return new Uint8Array(finalBytes);
}

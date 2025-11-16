// This file is correct and was provided in the previous step.
// Ensure it is saved at `ts/libsignal_storage_adapter.js`
// I am including it here again for absolute certainty.

import {
  SessionRecord,
  _serializeIdentityKeyPair,
} from "../../../whatsapp_rust_bridge.js";

function encodeVarint(value) {
  if (!Number.isInteger(value) || value < 0) {
    throw new TypeError("encodeVarint expects a non-negative integer");
  }

  const bytes = [];
  let remaining = value >>> 0;
  while (remaining >= 0x80) {
    bytes.push((remaining & 0x7f) | 0x80);
    remaining >>>= 7;
  }
  bytes.push(remaining);
  return bytes;
}

function ensureUint8Array(value) {
  if (value instanceof Uint8Array) {
    return value;
  }

  if (typeof Buffer !== "undefined" && Buffer.isBuffer(value)) {
    return new Uint8Array(value);
  }

  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }

  if (value instanceof ArrayBuffer) {
    return new Uint8Array(value);
  }

  throw new TypeError("Expected a value convertible to Uint8Array");
}

function ensureCurveKeyWithPrefix(keyBytes) {
  const bytes = ensureUint8Array(keyBytes);
  if (bytes.length === 33 && bytes[0] === 0x05) {
    return bytes;
  }

  if (bytes.length === 32) {
    const withPrefix = new Uint8Array(33);
    withPrefix[0] = 0x05;
    withPrefix.set(bytes, 1);
    return withPrefix;
  }

  return bytes;
}

function isPlainSessionObject(value) {
  if (typeof value !== "object" || value === null) {
    return false;
  }

  const proto = Object.getPrototypeOf(value);
  const looksPlain = proto === Object.prototype || proto === null;

  return (
    looksPlain &&
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
    throw new Error(
      "storage.loadSession() returned a plain SessionRecord object (_sessions/version). LibSignal expects serialized bytes."
    );
  }

  return null;
}

export async function loadSession(storage, address) {
  try {
    const recordInstance = await storage.loadSession(address);
    return toUint8Array(recordInstance);
  } catch (e) {
    console.error("Error in storage.loadSession:", e);
    throw e;
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

  const pubKey = ensureCurveKeyWithPrefix(keyPair.pubKey);
  const privKey = ensureUint8Array(keyPair.privKey);

  const idTag = (1 << 3) | 0;
  const pubKeyTag = (2 << 3) | 2;
  const privKeyTag = (3 << 3) | 2;

  const idBytes = [idTag, ...encodeVarint(preKeyId)];
  const pubKeyBytes = [pubKeyTag, ...encodeVarint(pubKey.length), ...pubKey];
  const privKeyBytes = [
    privKeyTag,
    ...encodeVarint(privKey.length),
    ...privKey,
  ];

  return new Uint8Array([...idBytes, ...pubKeyBytes, ...privKeyBytes]);
}

export async function removePreKey(storage, preKeyId) {
  await storage.removePreKey(preKeyId);
}

export async function loadSignedPreKey(storage, signedPreKeyId) {
  // 1. Get our own signed pre-key from storage.
  const ourKey = await storage.loadSignedPreKey(signedPreKeyId);

  // 2. Check if the ID requested by Rust matches our key's ID.
  if (!ourKey || ourKey.keyId !== signedPreKeyId) {
    return null;
  }

  const keyPair = ourKey.keyPair; // Nested structure
  const pubKey = keyPair?.pubKey ?? keyPair?.public ?? ourKey.publicKey;
  const privKey = keyPair?.privKey ?? keyPair?.private ?? ourKey.privateKey;

  if (!pubKey || !privKey) {
    console.error(
      "loadSignedPreKey: Could not find public or private key in the stored signed pre-key object.",
      ourKey
    );
    throw new Error("Invalid signed pre-key format in storage");
  }

  const { keyId, signature, timestamp } = ourKey;

  // Manual protobuf for SignedPreKeyRecordStructure
  // message SignedPreKeyRecordStructure {
  //   optional uint32 id = 1;
  //   optional bytes publicKey = 2;
  //   optional bytes privateKey = 3;
  //   optional bytes signature = 4;
  //   optional fixed64 timestamp = 5;
  // }
  const idTag = (1 << 3) | 0; // Field 1, varint
  const pubKeyTag = (2 << 3) | 2; // Field 2, length-delimited
  const privKeyTag = (3 << 3) | 2; // Field 3, length-delimited
  const sigTag = (4 << 3) | 2; // Field 4, length-delimited
  const tsTag = (5 << 3) | 1; // Field 5, 64-bit fixed

  const normalizedPubKey = ensureCurveKeyWithPrefix(pubKey);
  const privKeyBytes = ensureUint8Array(privKey);
  const signatureBytes = ensureUint8Array(signature);

  const idBytes = [idTag, ...encodeVarint(keyId)];
  const pubKeyBytes = [
    pubKeyTag,
    ...encodeVarint(normalizedPubKey.length),
    ...normalizedPubKey,
  ];
  const privBytes = [
    privKeyTag,
    ...encodeVarint(privKeyBytes.length),
    ...privKeyBytes,
  ];
  const sigBytes = [
    sigTag,
    ...encodeVarint(signatureBytes.length),
    ...signatureBytes,
  ];

  const tsBytes = [tsTag];
  const tsBuffer = new ArrayBuffer(8);
  const tsValue = BigInt(timestamp ?? Date.now());
  new DataView(tsBuffer).setBigUint64(0, tsValue, true);
  const tsArray = Array.from(new Uint8Array(tsBuffer));

  const finalBytes = [
    ...idBytes,
    ...pubKeyBytes,
    ...privBytes,
    ...sigBytes,
    ...tsBytes,
    ...tsArray,
  ];

  return new Uint8Array(finalBytes);
}

export async function loadSenderKey(storage, keyId) {
  // The keyId will be the string from SenderKeyName.toString()
  const record = await storage.loadSenderKey(keyId);
  // We expect the storage to return a Uint8Array or something convertible
  return toUint8Array(record);
}

export async function storeSenderKey(storage, keyId, senderKeyData) {
  try {
    // senderKeyData is a Uint8Array from Rust's SenderKeyRecord.serialize()
    const senderKeyCopy = new Uint8Array(senderKeyData);
    await storage.storeSenderKey(keyId, senderKeyCopy);
  } catch (e) {
    console.error("Error in storage.storeSenderKey:", e);
    throw e;
  }
}

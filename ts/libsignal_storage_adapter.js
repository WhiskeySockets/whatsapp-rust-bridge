// This file is correct and was provided in the previous step.
// Ensure it is saved at `ts/libsignal_storage_adapter.js`
// I am including it here again for absolute certainty.

import { SessionRecord } from "../../../whatsapp_rust_bridge.js";

// Helper to compare Uint8Arrays (web-compatible replacement for Buffer.equals)
function uint8ArrayEquals(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

export async function loadSession(storage, address) {
  try {
    const recordInstance = await storage.loadSession(address);
    if (recordInstance == null) {
      return null;
    }

    return recordInstance;
  } catch (e) {
    console.error("Error in storage.loadSession:", e);
    throw e;
  }
}

export async function storeSession(storage, address, sessionData) {
  try {
    // Pass raw bytes directly - avoid deserialize/serialize round-trip
    // The storage implementation can wrap it in SessionRecord if needed
    const sessionDataCopy = new Uint8Array(sessionData);

    // Check if storage expects raw bytes or SessionRecord
    // Most implementations just store the bytes anyway
    if (storage.storeSessionRaw) {
      await storage.storeSessionRaw(address, sessionDataCopy);
    } else {
      const recordInstance = SessionRecord.deserialize(sessionDataCopy);
      await storage.storeSession(address, recordInstance);
    }
  } catch (e) {
    console.error("Error in storage.storeSession:", e);
    throw e;
  }
}

export async function getIdentityKeyPair(storage) {
  const keyPair = await storage.getOurIdentity();

  return {
    pubKey: keyPair.pubKey,
    privKey: keyPair.privKey,
  };
}

export async function getLocalRegistrationId(storage) {
  return await storage.getOurRegistrationId();
}

export async function isTrustedIdentity(storage, name, identityKey, direction) {
  return await storage.isTrustedIdentity(
    name,
    new Uint8Array(identityKey),
    direction,
  );
}

export async function saveIdentity(storage, name, identityKey) {
  const existing = storage.identities?.get(name);
  if (existing) {
    return !uint8ArrayEquals(
      new Uint8Array(existing),
      new Uint8Array(identityKey),
    );
  }
  return false;
}

// Functions needed for decryption
export async function loadPreKey(storage, preKeyId) {
  const keyPair = await storage.loadPreKey(preKeyId);
  if (!keyPair) return null;

  return {
    id: preKeyId,
    pubKey: keyPair.pubKey,
    privKey: keyPair.privKey,
  };
}

export async function removePreKey(storage, preKeyId) {
  await storage.removePreKey(preKeyId);
}

export async function loadSignedPreKey(storage, signedPreKeyId) {
  // 1. Get our own signed pre-key from storage.
  const ourKey = await storage.loadSignedPreKey(signedPreKeyId);

  // 2. Check if the ID requested by Rust matches our key's ID.
  if (!ourKey) {
    return null;
  }

  const id = ourKey.keyId;

  if (id !== signedPreKeyId) {
    return null;
  }

  const keyPair = ourKey.keyPair;
  const { pubKey, privKey } = keyPair;
  const signatureSource = ourKey.signature;

  return {
    id,
    timestamp: ourKey.timestamp ?? Date.now(),
    pubKey,
    privKey,
    signature: signatureSource,
  };
}

export async function loadSenderKey(storage, keyId) {
  // The keyId will be the string from SenderKeyName.toString()
  const record = await storage.loadSenderKey(keyId);
  if (record == null) {
    return null;
  }

  return record;
}

export async function storeSenderKey(storage, keyId, senderKeyData) {
  try {
    await storage.storeSenderKey(keyId, senderKeyData);
  } catch (e) {
    console.error("Error in storage.storeSenderKey:", e);
    throw e;
  }
}

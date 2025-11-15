// This file acts as the bridge between the Rust WASM module and the user-provided
// JavaScript storage object. It translates calls and data formats between the two.

// We import the WASM-generated SessionRecord class.
import { SessionRecord, _serializeIdentityKeyPair } from "../../../whatsapp_rust_bridge.js";

// --- SessionStore Functions ---

export async function loadSession(storage, address) {
  // 1. Call the user's `loadSession` method.
  const recordInstance = await storage.loadSession(address);
  // 2. If a record is found, serialize it for Rust. Otherwise, return null.
  return recordInstance ? recordInstance.serialize() : null;
}

export async function storeSession(storage, address, sessionData) {
  // 1. Rust provides raw session bytes.
  // 2. Wrap them in the `SessionRecord` class instance the user expects.
  const recordInstance = SessionRecord.deserialize(sessionData);
  // 3. Call the user's `storeSession`.
  await storage.storeSession(address, recordInstance);
}

// --- IdentityKeyStore Functions ---

export async function getIdentityKeyPair(storage) {
  // 1. Get the key pair object { pubKey, privKey } from user storage.
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
  // `libsignal-node`'s storage interface expects the identityKey as a Buffer.
  // The direction (0 for sending, 1 for receiving) is also passed.
  return await storage.isTrustedIdentity(
    name,
    Buffer.from(identityKey),
    direction
  );
}

export async function saveIdentity(storage, name, identityKey) {
  // `libsignal-node` has no `saveIdentity`. The logic is handled by `isTrustedIdentity`.
  // To correctly report if an identity was *changed*, we inspect the storage.
  // This assumes the `FakeStorage` class has an `identities` map.
  const existing = storage.identities?.get(name);
  if (existing) {
    // Return true if the existing key is different from the new one.
    return !Buffer.from(existing).equals(Buffer.from(identityKey));
  }
  // It's a new identity, so it was not "changed".
  return false;
}

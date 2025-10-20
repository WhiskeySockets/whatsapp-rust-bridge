/**
 * Signal Protocol test utilities
 * Uses the Rust-generated test key generators for proper Signal protocol formatting
 */

import {
  generateTestIdentityKeyPair,
  generateTestPublicKey,
  generateTestSignedPreKey,
  signWithIdentityKey,
} from "../dist";

/**
 * Generates a valid Signal protocol identity key pair
 * This is properly formatted for the Signal protocol library
 */
export async function generateSignalIdentityKeyPair(): Promise<Buffer> {
  return generateTestIdentityKeyPair();
}

/**
 * Generates a valid Signal protocol public key
 */
export async function generateSignalPublicKey(): Promise<Buffer> {
  return generateTestPublicKey();
}

/**
 * Generates a valid Signal protocol signed pre-key
 */
export async function generateSignalSignedPreKey(): Promise<Buffer> {
  return generateTestSignedPreKey();
}

/**
 * Extracts the public key from a serialized Signal protocol IdentityKeyPair
 * The IdentityKeyPair is protobuf-encoded: field 1 (public key, 33 bytes), field 2 (private key, 32 bytes)
 * We extract just the public key bytes
 */
function extractPublicKeyFromIdentityKeyPair(identityKeyPair: Buffer): Buffer {
  // Protobuf format: 0x0a (field 1, wire type 2), 0x21 (length = 33), then 33 bytes of public key
  // The public key includes the DJB type byte (0x05) + 32 bytes of key material
  if (identityKeyPair.length >= 35 && identityKeyPair[0] === 0x0a && identityKeyPair[1] === 0x21) {
    return identityKeyPair.slice(2, 35); // Skip protobuf header, take 33 bytes
  }
  // Fallback to slicing if format is different
  return identityKeyPair.slice(0, 33);
}

/**
 * Creates a valid PreKeyBundle for testing
 */
export interface TestPreKeyBundle {
  registrationId: number;
  deviceId: number;
  preKeyId: number;
  preKey: Buffer;
  signedPreKeyId: number;
  signedPreKey: Buffer;
  signedPreKeySignature: Buffer;
  identityKey: Buffer;
  // Store the pre-key and signed-pre-key records for decryption
  preKeyRecord?: Buffer;
  signedPreKeyRecord?: Buffer;
}

export async function createTestPreKeyBundle(
  identityKeyPair: Buffer
): Promise<TestPreKeyBundle> {
  // Generate fresh pre-keys using Rust generators
  const preKey = await generateSignalPublicKey();
  const signedPreKey = await generateSignalPublicKey();
  
  // Also generate the full pre-key record and signed pre-key record
  // (which include the private key material needed for decryption)
  const preKeyRecord = await generateSignalPublicKey(); // This will be the full record
  const signedPreKeyRecord = await generateSignalSignedPreKey();
  
  // Sign the signed pre-key with the identity key pair's private key
  const signedPreKeySignature = await signWithIdentityKey(identityKeyPair, signedPreKey);

  // Extract the identity public key from the identity key pair protobuf
  const identityKey = extractPublicKeyFromIdentityKeyPair(identityKeyPair);

  return {
    registrationId: 42,
    deviceId: 0,
    preKeyId: 1,
    preKey,
    signedPreKeyId: 1,
    signedPreKey,
    signedPreKeySignature,
    identityKey,
    // Store full records for decryption
    preKeyRecord,
    signedPreKeyRecord,
  };
}

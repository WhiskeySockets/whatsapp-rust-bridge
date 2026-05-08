import { initSync, setCryptoProvider } from "../pkg/whatsapp_rust_bridge.js";

import {
  nosimdWasmBase64,
  simdWasmBase64,
} from "./macro.js" with { type: "macro" };

import { makeNodeCryptoProvider } from "./node-crypto-provider.js";

// Minimal WASM module that uses i8x16.splat + i8x16.popcnt. WebAssembly.validate
// returns false on engines without SIMD (e.g. V8 on x86 without SSE4.1).
const SIMD_PROBE = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1,
  8, 0, 65, 0, 253, 15, 253, 98, 11,
]);

function base64ToUint8Array(base64: string): Uint8Array {
  const binaryString = atob(base64);
  const bytes = new Uint8Array(binaryString.length);
  for (let i = 0; i < binaryString.length; i++) {
    bytes[i] = binaryString.charCodeAt(i);
  }
  return bytes;
}

function tryInit(b64: string): boolean {
  try {
    initSync({ module: base64ToUint8Array(b64) });
    return true;
  } catch {
    return false;
  }
}

const forceNoSimd =
  typeof process !== "undefined" &&
  process.env?.WHATSAPP_RUST_BRIDGE_FORCE_NOSIMD === "1";

const simdSupported = !forceNoSimd && WebAssembly.validate(SIMD_PROBE);

let simdUsed = false;
if (simdSupported && tryInit(simdWasmBase64())) {
  simdUsed = true;
} else {
  initSync({ module: base64ToUint8Array(nosimdWasmBase64()) });
}

// Route libsignal's AES-CBC / AES-GCM / HMAC through node:crypto (OpenSSL
// with AES-NI). Soft-fallback if node:crypto isn't available — the wasm
// already ships its own pure-Rust implementations as the default provider.
let nativeCryptoActive = false;
try {
  setCryptoProvider(makeNodeCryptoProvider());
  nativeCryptoActive = true;
} catch {
  // fall through; libsignal keeps using the default RustCryptoProvider
}

export const __wasmSimdActive: boolean = simdUsed;
export const __nativeCryptoActive: boolean = nativeCryptoActive;
export * from "../pkg/whatsapp_rust_bridge.js";

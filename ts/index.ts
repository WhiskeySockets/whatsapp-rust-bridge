import { initSync } from "../pkg/whatsapp_rust_bridge.js";

import { base64Wasm, base64WasmNoSimd } from "./macro.js" with { type: "macro" };

function base64ToUint8Array(base64: string): Uint8Array {
  const binaryString = atob(base64);
  const bytes = new Uint8Array(binaryString.length);
  for (let i = 0; i < binaryString.length; i++) {
    bytes[i] = binaryString.charCodeAt(i);
  }
  return bytes;
}

// Detect WASM SIMD support
function wasmSupportsSimd(): boolean {
  try {
    // Test if SIMD is supported by compiling a minimal SIMD module
    const simdTest = new Uint8Array([
      0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10,
      1, 8, 0, 65, 0, 253, 15, 253, 98, 11,
    ]);
    return WebAssembly.validate(simdTest);
  } catch {
    return false;
  }
}

// Load appropriate WASM module based on SIMD support
const hasSimd = wasmSupportsSimd();
const wasmBase64 = hasSimd ? base64Wasm() : base64WasmNoSimd();
const bytes = base64ToUint8Array(wasmBase64);

initSync({ module: bytes });

export * from "../pkg/whatsapp_rust_bridge.js";

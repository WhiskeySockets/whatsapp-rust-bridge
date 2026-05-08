import { readFileSync } from "node:fs";
import { initSync } from "../pkg/whatsapp_rust_bridge.js";

// Minimal WASM module that uses i8x16.splat + i8x16.popcnt. WebAssembly.validate
// returns false on engines without SIMD (e.g. V8 on x86 without SSE4.1).
const SIMD_PROBE = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1,
  8, 0, 65, 0, 253, 15, 253, 98, 11,
]);

function tryInit(filename: string): boolean {
  try {
    const url = new URL(filename, import.meta.url);
    initSync({ module: readFileSync(url) });
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
if (simdSupported && tryInit("whatsapp_rust_bridge_bg.simd.wasm")) {
  simdUsed = true;
} else if (!tryInit("whatsapp_rust_bridge_bg.nosimd.wasm")) {
  throw new Error(
    "whatsapp-rust-bridge: failed to load WASM module (neither SIMD nor non-SIMD variant could be initialized)",
  );
}

export const __wasmSimdActive: boolean = simdUsed;
export * from "../pkg/whatsapp_rust_bridge.js";

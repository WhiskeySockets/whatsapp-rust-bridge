import {
  initSync,
  inputBufGrow,
  encodeFromInputBuf,
  encodeResultPtr,
  type BinaryNode,
} from "../pkg/whatsapp_rust_bridge.js";

import { base64Wasm } from "./macro.js" with { type: "macro" };

function base64ToUint8Array(base64: string): Uint8Array {
  const binaryString = atob(base64);
  const bytes = new Uint8Array(binaryString.length);
  for (let i = 0; i < binaryString.length; i++) {
    bytes[i] = binaryString.charCodeAt(i);
  }
  return bytes;
}

const bytes = base64ToUint8Array(await base64Wasm());

const wasmExports = initSync({ module: bytes });
const wasmMemory: WebAssembly.Memory = (wasmExports as any).memory;

export * from "../pkg/whatsapp_rust_bridge.js";

// ── Result descriptor (one-time init) ───────────────────────────────────
const encResultAddr = encodeResultPtr();

// ── Shared input buffer: JS writes directly into WASM memory ────────────
let inputPtr = inputBufGrow(16384);
let inputCap = 16384;
let inputU8 = new Uint8Array(wasmMemory.buffer, inputPtr, inputCap);

function growInput(minCap: number): void {
  const newCap = Math.max(inputCap * 2, minCap);
  inputPtr = inputBufGrow(newCap);
  inputCap = newCap;
  inputU8 = new Uint8Array(wasmMemory.buffer, inputPtr, inputCap);
}

// ── Fast encodeNode via packed binary protocol ──────────────────────────
//
// JS packs the BinaryNode directly into WASM memory (zero intermediate buffer),
// then one WASM call parses + marshals. Result ptr+len read via raw byte access.
// No DataView, no .subarray(), no intermediate copies.

const textEncoder = new TextEncoder();

function isAllWhitespace(s: string): boolean {
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c !== 0x20 && c !== 0x09 && c !== 0x0a && c !== 0x0d) return false;
  }
  return true;
}

/**
 * Write a string as UTF-8 bytes starting at `pos`.
 * Fast path: direct charCode write for ASCII strings (avoids subarray + encodeInto overhead).
 * Returns the number of bytes written.
 */
function writeUtf8(s: string, pos: number): number {
  const len = s.length;
  // Fast path: short strings — try direct ASCII byte write
  if (len < 128) {
    for (let i = 0; i < len; i++) {
      const c = s.charCodeAt(i);
      if (c > 0x7f) {
        // Non-ASCII detected: fall back to TextEncoder for whole string
        const r = textEncoder.encodeInto(s, inputU8.subarray(pos));
        return r.written!;
      }
      inputU8[pos + i] = c;
    }
    return len;
  }
  // Long strings: use TextEncoder
  const r = textEncoder.encodeInto(s, inputU8.subarray(pos));
  return r.written!;
}

/** Write a UTF-8 string prefixed by u16 LE length. Returns new position. */
function writeStr16(s: string, pos: number): number {
  const w = writeUtf8(s, pos + 2);
  inputU8[pos] = w & 0xff;
  inputU8[pos + 1] = (w >> 8) & 0xff;
  return pos + 2 + w;
}

function packNode(node: BinaryNode, pos: number): number {
  // Ensure at least 512 bytes headroom for this node's fixed-size fields
  if (pos + 512 > inputCap) growInput(pos + 512);

  // Tag
  pos = writeStr16(node.tag, pos);

  // Attrs: count placeholder, then key-value pairs
  const countPos = pos;
  pos += 2;
  let count = 0;

  const attrs = node.attrs;
  if (attrs) {
    for (const key in attrs) {
      const val = (attrs as Record<string, unknown>)[key];
      if (val == null) continue;
      const strVal = typeof val === "string" ? val : String(val);
      if (strVal.length === 0 || isAllWhitespace(strVal)) continue;

      // Ensure capacity for this attr (key + val, worst case 3 bytes per char)
      const need = pos + 4 + key.length * 3 + strVal.length * 3;
      if (need > inputCap) growInput(need);

      pos = writeStr16(key, pos);
      pos = writeStr16(strVal, pos);
      count++;
    }
  }
  inputU8[countPos] = count & 0xff;
  inputU8[countPos + 1] = (count >> 8) & 0xff;

  // Content
  const content = node.content;
  if (content == null) {
    inputU8[pos] = 0; // None
    pos += 1;
  } else if (typeof content === "string") {
    inputU8[pos] = 1; // String
    pos += 1;
    const need = pos + 4 + content.length * 3;
    if (need > inputCap) growInput(need);
    const cW = writeUtf8(content, pos + 4);
    inputU8[pos] = cW & 0xff;
    inputU8[pos + 1] = (cW >> 8) & 0xff;
    inputU8[pos + 2] = (cW >> 16) & 0xff;
    inputU8[pos + 3] = (cW >> 24) & 0xff;
    pos += 4 + cW;
  } else if (content instanceof Uint8Array) {
    inputU8[pos] = 2; // Bytes
    pos += 1;
    const bLen = content.length;
    const need = pos + 4 + bLen;
    if (need > inputCap) growInput(need);
    inputU8[pos] = bLen & 0xff;
    inputU8[pos + 1] = (bLen >> 8) & 0xff;
    inputU8[pos + 2] = (bLen >> 16) & 0xff;
    inputU8[pos + 3] = (bLen >> 24) & 0xff;
    pos += 4;
    inputU8.set(content, pos);
    pos += bLen;
  } else if (Array.isArray(content)) {
    inputU8[pos] = 3; // Nodes
    pos += 1;
    const cLen = content.length;
    inputU8[pos] = cLen & 0xff;
    inputU8[pos + 1] = (cLen >> 8) & 0xff;
    pos += 2;
    for (let i = 0; i < cLen; i++) {
      pos = packNode(content[i] as BinaryNode, pos);
    }
  }

  return pos;
}

/**
 * Encode a BinaryNode to WhatsApp binary format.
 * JS packs directly into WASM memory → 1 FFI call (u32 arg) → Rust marshals.
 * Result read via raw byte access from WASM memory — zero DataView, zero copies on input.
 */
export function encodeNode(node: BinaryNode): Uint8Array {
  // Refresh input view if WASM memory grew since last call
  if (inputU8.buffer !== wasmMemory.buffer) {
    inputU8 = new Uint8Array(wasmMemory.buffer, inputPtr, inputCap);
  }

  const written = packNode(node, 0);
  encodeFromInputBuf(written);

  // Read result ptr+len from WASM memory (raw byte access, no DataView)
  const mem = new Uint8Array(wasmMemory.buffer);
  const a = encResultAddr;
  const ptr =
    (mem[a] | (mem[a + 1] << 8) | (mem[a + 2] << 16) | (mem[a + 3] << 24)) >>>
    0;
  const len =
    (mem[a + 4] |
      (mem[a + 5] << 8) |
      (mem[a + 6] << 16) |
      (mem[a + 7] << 24)) >>>
    0;

  // Single JS-native slice: one alloc + one memcpy, no FFI crossings
  return new Uint8Array(wasmMemory.buffer, ptr, len).slice();
}

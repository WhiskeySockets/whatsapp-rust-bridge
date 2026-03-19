import {
  initSync,
  inputBufGrow,
  encodeFromInputBuf,
  encodeResultPtr,
  decodeNodeToPacked,
  NoiseSession,
  type BinaryNode,
} from "../pkg/whatsapp_rust_bridge.js";

// These will be available after wasm-pack generates bindings for new methods:
// NoiseSession.prototype.encodeFrameFromInputBuf(packed_len)
// NoiseSession.prototype.decodeFramePackedFromInputBuf(len)

import { base64Wasm } from "./macro.js" with { type: "macro" };

function base64ToUint8Array(base64: string): Uint8Array {
  const binaryString = atob(base64);
  const bytes = new Uint8Array(binaryString.length);
  for (let i = 0; i < binaryString.length; i++) {
    bytes[i] = binaryString.charCodeAt(i);
  }
  return bytes;
}

const bytes = base64ToUint8Array(base64Wasm());

const wasmExports = initSync({ module: bytes });
const wasmMemory: WebAssembly.Memory = (wasmExports as any).memory;

export * from "../pkg/whatsapp_rust_bridge.js";

// ── Result descriptor (one-time init) ───────────────────────────────────
const encResultAddr = encodeResultPtr();

// ── Cached WASM memory view ─────────────────────────────────────────────
// Avoids creating `new Uint8Array(wasmMemory.buffer)` on every hot call.
// Refreshed only when WASM memory grows (buffer identity changes).
let wasmMem = new Uint8Array(wasmMemory.buffer);

function refreshMem(): Uint8Array {
  if (wasmMem.buffer !== wasmMemory.buffer) {
    wasmMem = new Uint8Array(wasmMemory.buffer);
  }
  return wasmMem;
}

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
// No DataView, no intermediate copies (TextEncoder writes into a subarray view).

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
  if (w > 0xffff) {
    throw new RangeError(`String too long for u16 length prefix: ${w} bytes`);
  }
  inputU8[pos] = w & 0xff;
  inputU8[pos + 1] = (w >> 8) & 0xff;
  return pos + 2 + w;
}

function packNode(node: BinaryNode, pos: number): number {
  // Ensure enough capacity for the tag (worst case 3 bytes per char) + fixed fields
  const tagNeed = pos + 2 + node.tag.length * 3 + 512;
  if (tagNeed > inputCap) growInput(tagNeed);

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
    if (cLen > 0xffff) {
      throw new RangeError(`Too many child nodes: ${cLen} (max 65535)`);
    }
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

  // Read result ptr+len from cached WASM view (no new Uint8Array creation)
  const m = refreshMem();
  const a = encResultAddr;
  const ptr =
    (m[a] | (m[a + 1] << 8) | (m[a + 2] << 16) | (m[a + 3] << 24)) >>> 0;
  const len =
    (m[a + 4] | (m[a + 5] << 8) | (m[a + 6] << 16) | (m[a + 7] << 24)) >>> 0;

  return m.slice(ptr, ptr + len);
}

// ── Fast decodeNode via packed binary protocol ──────────────────────────
//
// Rust decodes WA binary → NodeRef → packed LNP buffer in WASM memory.
// JS reads the buffer and constructs plain { tag, attrs, content } objects.
// One FFI call, zero wrappers, zero FinalizationRegistry.

const textDecoder = new TextDecoder();

/** Read a u16-LE-prefixed UTF-8 string. ASCII fast path for short strings. */
function readStr16(mem: Uint8Array, p: number[]): string {
  const len = mem[p[0]] | (mem[p[0] + 1] << 8);
  p[0] += 2;
  if (len < 64) {
    const start = p[0];
    for (let i = 0; i < len; i++) {
      if (mem[start + i] > 0x7f) {
        // Non-ASCII: fall back to TextDecoder
        const str = textDecoder.decode(
          new Uint8Array(mem.buffer, mem.byteOffset + start, len),
        );
        p[0] += len;
        return str;
      }
    }
    // All ASCII: build string from char codes
    let s = "";
    for (let i = 0; i < len; i++) {
      s += String.fromCharCode(mem[start + i]);
    }
    p[0] += len;
    return s;
  }
  const str = textDecoder.decode(
    new Uint8Array(mem.buffer, mem.byteOffset + p[0], len),
  );
  p[0] += len;
  return str;
}

/** Unpack a single BinaryNode from packed LNP buffer. */
function unpackNode(mem: Uint8Array, p: number[]): BinaryNode {
  const tag = readStr16(mem, p);

  const attrCount = mem[p[0]] | (mem[p[0] + 1] << 8);
  p[0] += 2;
  const attrs: Record<string, string> = {};
  for (let i = 0; i < attrCount; i++) {
    const k = readStr16(mem, p);
    attrs[k] = readStr16(mem, p);
  }

  const contentType = mem[p[0]++];
  let content: BinaryNode["content"] = undefined;

  if (contentType === 1) {
    // String
    const len =
      mem[p[0]] |
      (mem[p[0] + 1] << 8) |
      (mem[p[0] + 2] << 16) |
      (mem[p[0] + 3] << 24);
    p[0] += 4;
    content = textDecoder.decode(
      new Uint8Array(mem.buffer, mem.byteOffset + p[0], len),
    );
    p[0] += len;
  } else if (contentType === 2) {
    // Bytes — .slice() to avoid retaining WASM memory
    const len =
      (mem[p[0]] |
        (mem[p[0] + 1] << 8) |
        (mem[p[0] + 2] << 16) |
        (mem[p[0] + 3] << 24)) >>>
      0;
    p[0] += 4;
    content = mem.slice(p[0], p[0] + len);
    p[0] += len;
  } else if (contentType === 3) {
    // Child nodes
    const count = mem[p[0]] | (mem[p[0] + 1] << 8);
    p[0] += 2;
    const children: BinaryNode[] = new Array(count);
    for (let i = 0; i < count; i++) {
      children[i] = unpackNode(mem, p);
    }
    content = children;
  }

  return { tag, attrs, content };
}

/**
 * Decode WhatsApp binary format to a plain BinaryNode object.
 * Rust decodes + serializes to packed LNP → JS reads from WASM memory.
 * One FFI call, zero wrappers, zero FinalizationRegistry.
 */
export function decodeNode(data: Uint8Array): BinaryNode {
  decodeNodeToPacked(data);

  const m = refreshMem();
  const a = encResultAddr;
  const ptr =
    (m[a] | (m[a + 1] << 8) | (m[a + 2] << 16) | (m[a + 3] << 24)) >>> 0;

  return unpackNode(m, [ptr]);
}

/**
 * Decode noise frames using the packed LNP path.
 * During handshake: returns raw Uint8Array frames.
 * Post-handshake: returns plain BinaryNode objects (zero WASM wrappers).
 */
export function decodeFrames(
  session: NoiseSession,
  data: Uint8Array,
): (Uint8Array | BinaryNode)[] {
  const rv = session.decodeFramePacked(data);

  const m = refreshMem();
  const a = encResultAddr;
  const bufLen =
    (m[a + 4] | (m[a + 5] << 8) | (m[a + 6] << 16) | (m[a + 7] << 24)) >>> 0;

  if (bufLen === 0) {
    // Handshake mode: rv is the JS Array of raw Uint8Array frames
    return rv as unknown as Uint8Array[];
  }

  // Post-handshake: unpack nodes from WASM memory
  const ptr =
    (m[a] | (m[a + 1] << 8) | (m[a + 2] << 16) | (m[a + 3] << 24)) >>> 0;
  const p = [ptr];
  const count = m[p[0]] | (m[p[0] + 1] << 8);
  p[0] += 2;

  const result: BinaryNode[] = new Array(count);
  for (let i = 0; i < count; i++) {
    result[i] = unpackNode(m, p);
  }
  return result;
}

// ── Override NoiseSession encode methods to use result descriptor ────────
//
// encodeFrameRaw: avoids Uint8Array FFI alloc on Rust side.
// encodeFrame: uses fast packed encodeNode + encodeFrameRaw instead of slow FFI path.

function readResultSlice(): Uint8Array {
  const m = refreshMem();
  const a = encResultAddr;
  const ptr =
    (m[a] | (m[a + 1] << 8) | (m[a + 2] << 16) | (m[a + 3] << 24)) >>> 0;
  const len =
    (m[a + 4] | (m[a + 5] << 8) | (m[a + 6] << 16) | (m[a + 7] << 24)) >>> 0;
  return m.slice(ptr, ptr + len);
}

NoiseSession.prototype.encodeFrameRaw = function (
  data: Uint8Array,
): Uint8Array {
  // Write data into shared input buffer — avoids passArray8ToWasm0 malloc+copy
  const len = data.length;
  if (len > inputCap) growInput(len);
  if (inputU8.buffer !== wasmMemory.buffer) {
    inputU8 = new Uint8Array(wasmMemory.buffer, inputPtr, inputCap);
  }
  inputU8.set(data);
  (this as any).encodeFrameRawFromInputBuf(len);
  return readResultSlice();
};

NoiseSession.prototype.encodeFrame = function (node: BinaryNode): Uint8Array {
  // Fused path: pack node directly into INPUT_BUF → one WASM call does
  // parse → marshal → encrypt → frame. Eliminates the encodeNode slice-out
  // + encodeFrameRaw copy-back round-trip.
  if (inputU8.buffer !== wasmMemory.buffer) {
    inputU8 = new Uint8Array(wasmMemory.buffer, inputPtr, inputCap);
  }
  const written = packNode(node, 0);
  (this as any).encodeFrameFromInputBuf(written);
  return readResultSlice();
};

NoiseSession.prototype.encrypt = function (plaintext: Uint8Array): Uint8Array {
  (this as any).encryptPacked(plaintext);
  return readResultSlice();
};

NoiseSession.prototype.decrypt = function (ciphertext: Uint8Array): Uint8Array {
  (this as any).decryptPacked(ciphertext);
  return readResultSlice();
};

NoiseSession.prototype.decodeFrame = function (
  data: Uint8Array,
): (Uint8Array | BinaryNode)[] {
  if (!(this as any).isFinished) {
    // Handshake: write to shared input buffer to avoid passArray8ToWasm0 malloc
    const dlen = data.length;
    if (dlen > inputCap) growInput(dlen);
    if (inputU8.buffer !== wasmMemory.buffer) {
      inputU8 = new Uint8Array(wasmMemory.buffer, inputPtr, inputCap);
    }
    inputU8.set(data);
    (this as any).decodeFrameHandshakeFromInputBuf(dlen);
    const m = refreshMem();
    const a = encResultAddr;
    const ptr =
      (m[a] | (m[a + 1] << 8) | (m[a + 2] << 16) | (m[a + 3] << 24)) >>> 0;
    const p = [ptr];
    const count = m[p[0]] | (m[p[0] + 1] << 8);
    p[0] += 2;
    const result: Uint8Array[] = new Array(count);
    for (let i = 0; i < count; i++) {
      const len =
        (m[p[0]] |
          (m[p[0] + 1] << 8) |
          (m[p[0] + 2] << 16) |
          (m[p[0] + 3] << 24)) >>>
        0;
      p[0] += 4;
      result[i] = m.slice(p[0], p[0] + len);
      p[0] += len;
    }
    return result;
  }

  // Post-handshake: write to shared input buffer → one WASM call does
  // feed + frame parse + decrypt + unmarshal + pack LNP
  const dlen2 = data.length;
  if (dlen2 > inputCap) growInput(dlen2);
  if (inputU8.buffer !== wasmMemory.buffer) {
    inputU8 = new Uint8Array(wasmMemory.buffer, inputPtr, inputCap);
  }
  inputU8.set(data);
  (this as any).decodeFramePackedFromInputBuf(dlen2);
  const m = refreshMem();
  const a = encResultAddr;
  const ptr =
    (m[a] | (m[a + 1] << 8) | (m[a + 2] << 16) | (m[a + 3] << 24)) >>> 0;
  const bufLen =
    (m[a + 4] | (m[a + 5] << 8) | (m[a + 6] << 16) | (m[a + 7] << 24)) >>> 0;

  if (bufLen === 0) return [];

  const p = [ptr];
  const count = m[p[0]] | (m[p[0] + 1] << 8);
  p[0] += 2;
  const nodes: BinaryNode[] = new Array(count);
  for (let i = 0; i < count; i++) {
    nodes[i] = unpackNode(m, p);
  }
  return nodes;
};

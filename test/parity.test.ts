// test/parity.test.ts
import { describe, it, expect } from "bun:test";
import { encodeBinaryNode } from "baileys";
import { encodeNode, type BinaryNode } from "../dist";

// Helper to visualize buffer differences
function hex(buffer: Uint8Array): string {
  return Buffer.from(buffer).toString("hex");
}

describe("Parity: Legacy TS vs Rust WASM", () => {
  // This is the crucial regression test for @g.us
  it("should encode '@g.us' JID identically (using JID_PAIR)", () => {
    const node: BinaryNode = {
      tag: "iq",
      attrs: {
        to: "@g.us",
        type: "get",
        xmlns: "w:g2",
        id: "test-group",
      },
      content: [],
    };

    const legacyEncoded = encodeBinaryNode(node);
    const wasmEncoded = encodeNode(node);

    // The legacy encoder detects "@g.us", decodes it to { user: "", server: "g.us" },
    // and writes [JID_PAIR, LIST_EMPTY, "g.us" (token)].
    // The buggy WASM implementation likely writes it as a raw string.

    if (hex(legacyEncoded) !== hex(wasmEncoded)) {
      console.log("Legacy:", hex(legacyEncoded));
      console.log("WASM:  ", hex(wasmEncoded));
    }

    expect(hex(wasmEncoded)).toBe(hex(legacyEncoded));
  });

  it("should encode 's.whatsapp.net' server JID identically", () => {
    const node: BinaryNode = {
      tag: "iq",
      attrs: {
        to: "s.whatsapp.net",
        type: "set",
        id: "ping",
      },
      content: [],
    };

    const legacyEncoded = encodeBinaryNode(node);
    const wasmEncoded = encodeNode(node);

    expect(hex(wasmEncoded)).toBe(hex(legacyEncoded));
  });

  it("should encode standard user JID identically", () => {
    const node: BinaryNode = {
      tag: "message",
      attrs: {
        to: "1234567890@s.whatsapp.net",
        id: "msg-1",
      },
      content: [],
    };

    const legacyEncoded = encodeBinaryNode(node);
    const wasmEncoded = encodeNode(node);

    expect(hex(wasmEncoded)).toBe(hex(legacyEncoded));
  });

  it("should encode device JID identically (AD_JID)", () => {
    const node: BinaryNode = {
      tag: "message",
      attrs: {
        to: "1234567890:2@s.whatsapp.net",
        id: "msg-device",
      },
      content: [],
    };

    const legacyEncoded = encodeBinaryNode(node);
    const wasmEncoded = encodeNode(node);

    expect(hex(wasmEncoded)).toBe(hex(legacyEncoded));
  });

  it("should encode content strings identically", () => {
    const node: BinaryNode = {
      tag: "message",
      attrs: {},
      content: "Hello World",
    };

    const legacyEncoded = encodeBinaryNode(node);
    const wasmEncoded = encodeNode(node);

    expect(hex(wasmEncoded)).toBe(hex(legacyEncoded));
  });
});

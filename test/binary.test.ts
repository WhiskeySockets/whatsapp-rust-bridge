import { describe, test, expect, beforeAll } from "bun:test";
import {
  init,
  encodeNode,
  decodeNode,
  type INode,
  type WasmNode,
} from "../ts/binary";

function arraysEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

function compareINodeToDecoded(original: INode, decoded: any): boolean {
  const decTag = decoded.tag;
  if (original.tag !== decTag) return false;

  // attrs
  const origAttrs = original.attrs;
  const decAttrs = decoded.getAttributes
    ? decoded.getAttributes()
    : decoded.attrs;
  const origKeys = Object.keys(origAttrs);
  const decKeys = Object.keys(decAttrs);
  if (origKeys.length !== decKeys.length) return false;
  for (const key of origKeys) {
    if (!(key in decAttrs) || decAttrs[key] !== origAttrs[key]) return false;
  }

  // content
  const textDecoder = new TextDecoder();
  const decContent = decoded.content;
  if (original.content === undefined) {
    if (decContent !== undefined) return false;
  } else if (typeof original.content === "string") {
    if (!(decContent instanceof Uint8Array)) return false;
    const decodedText = textDecoder.decode(decContent);
    if (decodedText !== original.content) return false;
  } else if (original.content instanceof Uint8Array) {
    if (!(decContent instanceof Uint8Array)) return false;
    if (!arraysEqual(decContent, original.content)) return false;
  } else if (Array.isArray(original.content)) {
    if (decContent !== undefined) return false;
  }

  // children
  const origChildren = Array.isArray(original.content) ? original.content : [];
  const decChildren = decoded.children || [];
  if (origChildren.length !== decChildren.length) return false;
  for (let i = 0; i < origChildren.length; i++) {
    if (!compareINodeToDecoded(origChildren[i], decChildren[i])) return false;
  }

  return true;
}

describe("Binary Marshalling", () => {
  beforeAll(async () => {
    await init();
  });

  const attributesNode: INode = {
    tag: "iq",
    attrs: {
      to: "s.whatsapp.net",
      type: "get",
      xmlns: "test-xmlns",
      id: "test-123",
    },
    content: [
      {
        tag: "query",
        attrs: {},
      },
    ],
  };

  test("should correctly encode and decode a node with attributes and children", () => {
    const binaryData = encodeNode(attributesNode);
    expect(binaryData).toBeInstanceOf(Uint8Array);
    expect(binaryData.length).toBeGreaterThan(0);

    const resultHandle: WasmNode = decodeNode(binaryData);

    expect(resultHandle).toBeInstanceOf(Object);
    expect(resultHandle.tag).toBe("iq");

    expect(resultHandle.getAttribute("xmlns")).toBe("test-xmlns");
    expect(resultHandle.getAttribute("to")).toBe("s.whatsapp.net");
    expect(resultHandle.getAttribute("nonexistent")).toBeUndefined();

    const children = resultHandle.children;
    expect(Array.isArray(children)).toBe(true);
    expect(children).toHaveLength(1);
    expect(children[0]?.tag).toBe("query");

    const attrs = resultHandle.getAttributes();
    expect(attrs).toBeInstanceOf(Object);
    expect(Object.keys(attrs)).toHaveLength(4);
    expect(attrs["xmlns"]).toBe("test-xmlns");
  });

  describe("Content Encoding Parity", () => {
    const textDecoder = new TextDecoder();

    test("should encode a JS string as string content and decode it back", () => {
      const node: INode = {
        tag: "message",
        attrs: {},
        content: "this is a simple string",
      };

      const binaryData = encodeNode(node);
      const resultHandle = decodeNode(binaryData);

      expect(resultHandle.content).toBeInstanceOf(Uint8Array);
      const decodedText = textDecoder.decode(resultHandle.content);
      expect(decodedText).toBe("this is a simple string");
    });

    test("should encode a Uint8Array as binary content and decode it back", () => {
      const binaryPayload = new Uint8Array([10, 20, 30, 250]);
      const node: INode = {
        tag: "message",
        attrs: {},
        content: binaryPayload,
      };

      const binaryData = encodeNode(node);
      const resultHandle = decodeNode(binaryData);

      expect(resultHandle.content).toBeInstanceOf(Uint8Array);
      expect(resultHandle.content).toEqual(binaryPayload);
    });

    test("should correctly handle a string that is a known token", () => {
      const node: INode = {
        tag: "message",
        attrs: {},
        content: "receipt",
      };

      const binaryData = encodeNode(node);
      const resultHandle = decodeNode(binaryData);

      expect(resultHandle.content).toBeInstanceOf(Uint8Array);
      const decodedText = textDecoder.decode(resultHandle.content);
      expect(decodedText).toBe("receipt");
      expect(binaryData.length).toBeLessThan(10);
    });

    test("should NOT confuse a byte array with a token string", () => {
      const textEncoder = new TextEncoder();
      const binaryPayload = textEncoder.encode("receipt");

      const node: INode = {
        tag: "message",
        attrs: {},
        content: binaryPayload,
      };

      const binaryData = encodeNode(node);
      const resultHandle = decodeNode(binaryData);

      expect(resultHandle.content).toBeInstanceOf(Uint8Array);
      expect(resultHandle.content).toEqual(binaryPayload);

      expect(binaryData.length).toBeGreaterThan(5);
    });
  });
});

test("should round-trip encode and decode correctly", () => {
  const node: INode = {
    tag: "message",
    attrs: { id: "123", type: "text" },
    content: "hello world",
  };

  const binaryData = encodeNode(node);
  const decoded = decodeNode(binaryData);

  expect(compareINodeToDecoded(node, decoded)).toBe(true);
});

test("should round-trip encode and decode node with children correctly", () => {
  const node: INode = {
    tag: "iq",
    attrs: {
      to: "s.whatsapp.net",
      type: "get",
      xmlns: "test-xmlns",
      id: "test-123",
    },
    content: [
      {
        tag: "query",
        attrs: {},
      },
    ],
  };

  const binaryData = encodeNode(node);
  const decoded = decodeNode(binaryData);

  expect(compareINodeToDecoded(node, decoded)).toBe(true);
});

test("should throw error when decoding truncated binary data", () => {
  const node: INode = {
    tag: "message",
    attrs: {},
    content: "receipt",
  };

  const binaryData = encodeNode(node);
  expect(binaryData.length).toBe(5); // [0, 248, 2, 19, 7]

  // Truncate to 3 bytes
  const truncatedData = binaryData.slice(0, 3);

  expect(() => decodeNode(truncatedData)).toThrow(
    "Unexpected end of binary data"
  );
});

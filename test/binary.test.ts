import { describe, test, expect, beforeAll } from "bun:test";
import { init, encodeNode, decodeNode, type INode } from "../dist/binary.js";

describe("Binary Marshalling", () => {
  beforeAll(async () => {
    await init();
  });

  // Test case 1: Attributes bug
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

  test("unmarshal should return `attrs` as a key-value object, not an array of arrays", () => {
    const binaryData = encodeNode(attributesNode);
    expect(binaryData).toBeInstanceOf(Uint8Array);
    expect(binaryData.length).toBeGreaterThan(0);

    const resultNode = decodeNode(binaryData);

    expect(resultNode.attrs).toBeInstanceOf(Object);
    expect(Array.isArray(resultNode.attrs)).toBe(false);
    expect(resultNode.attrs).toEqual(attributesNode.attrs);
    expect(resultNode.attrs.xmlns).toBe("test-xmlns");
  });

  // Test case 2: Binary content
  const binaryContentNode: INode = {
    tag: "message",
    attrs: {
      id: "binary-test-1",
      to: "s.whatsapp.net",
    },
    content: new Uint8Array([0, 1, 2, 3, 255, 128]),
  };

  test("should correctly encode and decode a node with Uint8Array content", () => {
    const binaryData = encodeNode(binaryContentNode);
    expect(binaryData).toBeInstanceOf(Uint8Array);
    expect(binaryData.length).toBeGreaterThan(0);

    const resultNode = decodeNode(binaryData);

    expect(resultNode.content).toBeInstanceOf(Uint8Array);
    expect(resultNode.content).toEqual(binaryContentNode.content!);
  });
});

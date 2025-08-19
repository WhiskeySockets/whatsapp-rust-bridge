import { describe, test, expect, beforeAll } from "bun:test";
import { init, encodeNode, decodeNode, type INode } from "../dist/binary.js";

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

  const stringAsBinaryNode: INode = {
    tag: "ref",
    attrs: {},
    content:
      "2@JsJJlgvJoPpSiB7Ju+0OsOrfTqHvPCa6sOYH4RhPaTlC23HxodJ8qUM3nmMV7DB3P7Ib0WxZ3dOuY3QMbodDQsUVyXabrWu0Di8=",
  };

  test("should treat strings as binary and decode content to Uint8Array", () => {
    const binaryData = encodeNode(stringAsBinaryNode);
    expect(binaryData).toBeInstanceOf(Uint8Array);

    const resultNode = decodeNode(binaryData);

    expect(resultNode.content).toBeInstanceOf(Uint8Array);

    const originalContentBytes = new TextEncoder().encode(
      stringAsBinaryNode.content as string
    );
    expect(resultNode.content).toEqual(originalContentBytes);
  });
});

import { describe, test, expect, beforeAll } from "bun:test";
import {
  init,
  encodeNode,
  decodeNode,
  type INode,
  type WasmNode,
} from "../ts/binary";

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
    expect(attrs["to"]).toBe("s.whatsapp.net");
    expect(attrs["nonexistent"]).toBeUndefined();
  });

  const binaryPayload = new Uint8Array([0, 1, 2, 3, 255, 128]);

  const binaryContentNode: INode = {
    tag: "message",
    attrs: {
      id: "binary-test-1",
      to: "s.whatsapp.net",
    },
    content: binaryPayload,
  };

  test("should correctly encode and decode a node with Uint8Array content", () => {
    const binaryData = encodeNode(binaryContentNode);
    expect(binaryData).toBeInstanceOf(Uint8Array);
    expect(binaryData.length).toBeGreaterThan(0);

    const resultHandle: WasmNode = decodeNode(binaryData);

    expect(resultHandle.content).toBeInstanceOf(Uint8Array);
    expect(resultHandle.content).toEqual(binaryPayload);
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

    const resultHandle: WasmNode = decodeNode(binaryData);

    expect(resultHandle.content).toBeInstanceOf(Uint8Array);

    const originalContentBytes = new TextEncoder().encode(
      stringAsBinaryNode.content as string
    );
    expect(resultHandle.content).toEqual(originalContentBytes);
  });
});

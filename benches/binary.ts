import { decodeNode, encodeNode, type BinaryNode } from "../dist/index.js";
import { run, bench, do_not_optimize, boxplot, summary } from "mitata";
import {
  encodeBinaryNode as encodeBinaryNodeOld,
  decodeBinaryNode as decodeBinaryNodeOld,
} from "baileys";

const testNode: BinaryNode = {
  tag: "message",
  attrs: {
    to: "1234567890@s.whatsapp.net",
    id: "3EB0622825A79604144A",
    type: "text",
    t: String(Math.floor(Date.now() / 1000)),
  },
  content: [
    {
      tag: "conversation",
      attrs: {},
      content:
        "Hello from a benchmark test! This is a slightly longer message to ensure the test is not trivial.",
    },
    {
      tag: "ephemeral_setting",
      attrs: {
        timestamp: String(Date.now()),
        expiration: "604800",
      },
      content: undefined,
    },
  ],
};

const wasmEncoded = Buffer.from(encodeNode(testNode));

boxplot(() => {
  summary(() => {
    bench("encodeNode Rust WASM", () => {
      const result = encodeNode(testNode);
      do_not_optimize(result);
    }).gc("inner");

    bench("encodeNode Old Baileys", () => {
      const result = encodeBinaryNodeOld(testNode);
      do_not_optimize(result);
    }).gc("inner");
  });

  summary(() => {
    bench("decodeNode Rust WASM", () => {
      const handle = decodeNode(wasmEncoded);
      do_not_optimize(handle);
    }).gc("inner");

    bench("decodeNode Old Baileys", async () => {
      const handle = await decodeBinaryNodeOld(wasmEncoded);
      do_not_optimize(handle);
    }).gc("inner");
  });

  summary(() => {
    bench("decode and attrs Rust WASM", () => {
      const handle = decodeNode(wasmEncoded);
      handle.attrs;
      handle.attrs;
    }).gc("inner");

    bench("decode and attrs Old Baileys", async () => {
      const handle = await decodeBinaryNodeOld(wasmEncoded);
      handle.attrs;
      handle.attrs;
    }).gc("inner");
  });
});

await run();

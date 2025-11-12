import { decodeNode, encodeNode, type BinaryNode } from "../dist/binary.js";
import { run, bench, group, do_not_optimize } from "mitata";

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

const wasmEncoded = encodeNode(testNode);

group("Encoding (JS Object -> Binary)", () => {
  bench("Rust WASM (marshal - allocates)", () => {
    const result = encodeNode(testNode);
    do_not_optimize(result);
  }).gc("inner");
});

group("Decoding (Binary -> JS Handle)", () => {
  bench("Rust WASM (decode to handle)", () => {
    const handle = decodeNode(wasmEncoded);
    do_not_optimize(handle);
  }).gc("inner");
});

group("Decoding and getting attrs (Binary -> JS Handle)", () => {
  bench("Rust WASM attrs (decode to handle)", () => {
    const handle = decodeNode(wasmEncoded);
    handle.attrs;
    handle.attrs;
  }).gc("inner");
});

await run();

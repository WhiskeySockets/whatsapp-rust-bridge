import { decodeNode, encodeNode } from "../dist/index.js";
import { run, bench, group } from "mitata";

const testNode = {
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
    encodeNode(testNode);
  }).gc("inner");
});

group("Decoding (Binary -> JS Handle)", () => {
  bench("Rust WASM (decode to handle)", () => {
    const handle = decodeNode(wasmEncoded);
    handle.tag;
  }).gc("inner");
});

await run();

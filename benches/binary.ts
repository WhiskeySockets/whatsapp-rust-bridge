import {
  init,
  INode,
  marshal as marshalWasm,
  unmarshal as unmarshalWasm,
  NodeBuilder,
} from "../../whatsapp-rust-bridge/dist/binary.js";
import { run, bench, group } from "mitata";

const testNode: INode = {
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

await init();

const wasmEncoded = marshalWasm(testNode);

group("Encoding (JS Object -> Binary)", () => {
  bench("Rust WASM (marshal)", () => {
    marshalWasm(testNode);
  }).gc("inner");
});

group("Decoding (Binary -> JS Object)", () => {
  bench("Rust WASM (unmarshal)", () => {
    unmarshalWasm(wasmEncoded);
  }).gc("inner");
});

group("Build a node", () => {
  bench("Rust WASM  (NodeBuilder)", () => {
    new NodeBuilder(testNode.tag)
      .attr("to", testNode.attrs!.to)
      .attr("id", testNode.attrs!.id)
      .attr("type", testNode.attrs!.type)
      .attr("t", testNode.attrs!.t)
      .children(
        (Array.isArray(testNode.content) ? testNode.content : []) as INode[]
      )
      .build();
  }).gc("inner");
});

await run();

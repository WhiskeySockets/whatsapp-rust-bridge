import { initSync } from "../pkg/whatsapp_rust_bridge.js";

import { base64Wasm } from "./macro.js" with { type: "macro" };

const bytes = Buffer.from(await base64Wasm(), "base64");

initSync({ module: bytes });

export * from "../pkg/whatsapp_rust_bridge.js";

import { initSync } from "../pkg/whatsapp_rust_bridge.js";

import wasmDataUri from "../pkg/whatsapp_rust_bridge_bg.wasm";

// @ts-expect-error ignore missing types for data URI import
const base64 = wasmDataUri.substring(wasmDataUri.indexOf(",") + 1);

const bytes = Buffer.from(base64, "base64");

initSync({ module: bytes });

export * from "../pkg/whatsapp_rust_bridge.js";

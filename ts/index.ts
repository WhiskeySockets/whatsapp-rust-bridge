import { readFileSync } from "node:fs";
import { initSync } from "../pkg/whatsapp_rust_bridge.js";

const wasmUrl = new URL("whatsapp_rust_bridge_bg.wasm", import.meta.url);
const wasmBytes = readFileSync(wasmUrl);
initSync({ module: wasmBytes });

// Runtime exports from WASM
export {
  getWasmMemoryBytes,
  getEnabledFeatures,
  decryptPollVote,
  getAggregateVotesInPollMessage,
} from "../pkg/whatsapp_rust_bridge.js";

// Pure-JS proto codec (bundled at build time, zero runtime deps for consumers)
export { encodeProto, decodeProto } from "./proto";

// Auto-assembled protobufjs-style namespace covering every ts-proto type.
// Lets `WAProto.X.encode(obj).finish()` and friends work for the full schema
// without a hand-maintained shim — see `proto-namespace.ts` for details.
export { proto } from "./proto-namespace";

// initWasmEngine and createWhatsAppClient need explicit typing
// because they use skip_typescript in Rust for complex params.
import {
  initWasmEngine as _initWasmEngine,
  createWhatsAppClient as _createWhatsAppClient,
} from "../pkg/whatsapp_rust_bridge.js";
import type { WhatsAppEvent, JsTransportCallbacks, JsHttpClientConfig, JsStoreCallbacks, CacheConfig } from "../pkg/whatsapp_rust_bridge.js";
import type { WasmWhatsAppClient } from "../pkg/whatsapp_rust_bridge.js";

export const initWasmEngine: (logger?: any, crypto?: any) => void = _initWasmEngine;
export const createWhatsAppClient: (
  transport: JsTransportCallbacks,
  httpClient: JsHttpClientConfig,
  onEvent?: ((event: WhatsAppEvent) => void) | null,
  store?: JsStoreCallbacks | null,
  cache?: CacheConfig | null,
) => Promise<WasmWhatsAppClient> = _createWhatsAppClient as any;

// All types come from pkg (Tsify types + generated wacore types via typescript_custom_section)
export type * from "../pkg/whatsapp_rust_bridge.js";

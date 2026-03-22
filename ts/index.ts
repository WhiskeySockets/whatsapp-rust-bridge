import { initSync } from "../pkg/whatsapp_rust_bridge.js";

import { base64Wasm } from "./macro.js" with { type: "macro" };

function base64ToUint8Array(base64: string): Uint8Array {
  const binaryString = atob(base64);
  const bytes = new Uint8Array(binaryString.length);
  for (let i = 0; i < binaryString.length; i++) {
    bytes[i] = binaryString.charCodeAt(i);
  }
  return bytes;
}

const bytes = base64ToUint8Array(base64Wasm());

initSync({ module: bytes });

// Runtime exports from WASM — only the functions actually used by Baileys
export {
  encodeProto,
  decodeProto,
  getWasmMemoryBytes,
  getEnabledFeatures,
  decryptPollVote,
  getAggregateVotesInPollMessage,
} from "../pkg/whatsapp_rust_bridge.js";

// initWasmEngine and createWhatsAppClient need explicit typing
// because the pkg exports have auto-generated types that conflict
// with our hand-maintained WasmWhatsAppClient interface.
import {
  initWasmEngine as _initWasmEngine,
  createWhatsAppClient as _createWhatsAppClient,
} from "../pkg/whatsapp_rust_bridge.js";
import type { WasmWhatsAppClient, WhatsAppEvent } from "../types/index.js";

export const initWasmEngine: (logger?: any) => void = _initWasmEngine;
export const createWhatsAppClient: (
  transport: import("../types/index.js").JsTransportCallbacks,
  httpClient: import("../types/index.js").JsHttpClientConfig,
  onEvent?: ((event: WhatsAppEvent) => void) | null,
  store?: import("../types/index.js").JsStoreCallbacks | null,
  cache?: import("../types/index.js").CacheConfig | null,
) => Promise<WasmWhatsAppClient> = _createWhatsAppClient as any;

// Type exports from hand-maintained types (accurate, snake_case matching serde output)
export type * from "../types/index.js";

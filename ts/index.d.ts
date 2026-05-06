export { getWasmMemoryBytes, getEnabledFeatures, decryptPollVote, getAggregateVotesInPollMessage, } from "../pkg/whatsapp_rust_bridge.js";
import type { WhatsAppEvent, JsTransportCallbacks, JsHttpClientConfig, JsStoreCallbacks, CacheConfig } from "../pkg/whatsapp_rust_bridge.js";
import type { WasmWhatsAppClient } from "../pkg/whatsapp_rust_bridge.js";
export declare const initWasmEngine: (logger?: any) => void;
export declare const createWhatsAppClient: (transport: JsTransportCallbacks, httpClient: JsHttpClientConfig, onEvent?: ((event: WhatsAppEvent) => void) | null, store?: JsStoreCallbacks | null, cache?: CacheConfig | null, version?: readonly [number, number, number] | null) => Promise<WasmWhatsAppClient>;
export type * from "../pkg/whatsapp_rust_bridge.js";

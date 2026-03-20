export { encodeProto, decodeProto, generateKeyPair, calculateAgreement, calculateSignature, verifySignature, md5, hkdf, } from "../pkg/whatsapp_rust_bridge.js";
import type { WasmWhatsAppClient, WhatsAppEvent } from "../types/index.js";
export declare const initWasmEngine: (logger?: any) => void;
export declare const createWhatsAppClient: (transport: import("../types/index.js").JsTransportCallbacks, httpClient: import("../types/index.js").JsHttpClientConfig, onEvent?: ((event: WhatsAppEvent) => void) | null, store?: import("../types/index.js").JsStoreCallbacks | null, cache?: import("../types/index.js").CacheConfig | null) => Promise<WasmWhatsAppClient>;
export type * from "../types/index.js";

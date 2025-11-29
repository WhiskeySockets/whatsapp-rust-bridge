import { initSync } from "../pkg/whatsapp_rust_bridge.js";

import { base64Wasm } from "./macro.js" with { type: "macro" };

// Web-compatible base64 decoding (works in both Node.js and browsers)
function base64ToUint8Array(base64: string): Uint8Array {
    const binaryString = atob(base64);
    const bytes = new Uint8Array(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
    }
    return bytes;
}

const bytes = base64ToUint8Array(await base64Wasm());

initSync({ module: bytes });

export * from "../pkg/whatsapp_rust_bridge.js";

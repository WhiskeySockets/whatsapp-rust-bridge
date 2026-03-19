import {readFileSync} from "fs";

export const base64Wasm = () => {
  const bytes = readFileSync("./pkg/whatsapp_rust_bridge_bg.wasm")
  return bytes.toBase64();
};

export const base64WasmNoSimd = () => {
  const bytes = readFileSync("./pkg-nosimd/whatsapp_rust_bridge_bg.wasm")
  return bytes.toBase64();
};

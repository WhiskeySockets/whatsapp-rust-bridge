import {readFileSync} from "fs";

export const base64Wasm = () => {
  const bytes = readFileSync("./pkg/whatsapp_rust_bridge_bg.wasm")

  const base64 = bytes.toBase64();

  return base64;
};

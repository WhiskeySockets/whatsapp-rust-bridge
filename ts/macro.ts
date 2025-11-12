export const base64Wasm = async () => {
  const bytes = await Bun.file("./pkg/whatsapp_rust_bridge_bg.wasm").bytes();

  const base64 = bytes.toBase64();

  return base64;
};

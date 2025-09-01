import initWasm, {
  decodeNode,
  encodeNode,
  encodeNodeTo,
  type INode,
} from "../pkg/whatsapp_rust_bridge.js";
import wasmUrl from "../pkg/whatsapp_rust_bridge_bg.wasm";

async function readRelativeFile(
  relativePath: string,
  importMetaUrl: string
): Promise<Uint8Array> {
  if (
    typeof process !== "undefined" &&
    process.versions &&
    process.versions.node
  ) {
    const { readFile } = await import("node:fs/promises");
    const { fileURLToPath } = await import("node:url");
    const { dirname, resolve } = await import("node:path");

    const modulePath = fileURLToPath(importMetaUrl);
    const filePath = resolve(dirname(modulePath), relativePath);

    return readFile(filePath);
  }

  const url = new URL(relativePath, importMetaUrl);
  const response = await fetch(url);
  const arrayBuffer = await response.arrayBuffer();
  return new Uint8Array(arrayBuffer);
}

export async function init(): Promise<void> {
  const wasmBytes = await readRelativeFile(wasmUrl as any, import.meta.url);
  await initWasm({ module_or_path: wasmBytes });
}

export { encodeNode, decodeNode, encodeNodeTo };

export type { INode };

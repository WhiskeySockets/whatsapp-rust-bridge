import initWasm, {
  unmarshal as unmarshal_node,
  marshal as marshal_node,
  type INode,
  NodeBuilder as WasmNodeBuilder,
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
  const wasmModule = new WebAssembly.Module(wasmBytes);
  await initWasm(wasmModule);
}

export type { INode };

/**
 * Unmarshals a binary buffer into a JavaScript node object.
 *
 * @param data The Uint8Array containing the binary node data.
 * @returns The deserialized node object.
 * @throws If unmarshalling fails.
 */
export function unmarshal(data: Uint8Array): INode {
  return unmarshal_node(data.slice(1));
}

/**
 * Marshals a JavaScript node object into its binary representation.
 *
 * @param node The node object to serialize.
 * @returns A Uint8Array with the binary data.
 * @throws If marshalling fails.
 */
export function marshal(node: INode): Uint8Array {
  return marshal_node(node);
}

/**
 * A fluent builder for creating and marshalling nodes.
 * This provides a type-safe wrapper around the WasmNodeBuilder.
 */
export class NodeBuilder {
  private builder: WasmNodeBuilder;

  /**
   * Creates a new NodeBuilder.
   * @param tag The tag of the XML-like node.
   */
  constructor(tag: string) {
    this.builder = new WasmNodeBuilder(tag);
  }

  /**
   * Adds an attribute to the node.
   * @param key The attribute key.
   * @param value The attribute value.
   * @returns The builder instance for chaining.
   */
  public attr(key: string, value: string): this {
    this.builder = this.builder.attr(key, value);
    return this;
  }

  /**
   * Sets the children of the node.
   * @param children An array of child nodes.
   * @returns The builder instance for chaining.
   */
  public children(children: INode[]): this {
    this.builder = this.builder.children(children);
    return this;
  }

  /**
   * Sets the content of the node to a raw byte array.
   * @param bytes The binary content.
   * @returns The builder instance for chaining.
   */
  public bytes(bytes: Uint8Array): this {
    this.builder = this.builder.bytes(bytes);
    return this;
  }

  /**
   * Finalizes the node and returns its binary representation.
   * @returns A Uint8Array with the marshalled node data.
   */
  public build(): Uint8Array {
    return this.builder.build();
  }
}

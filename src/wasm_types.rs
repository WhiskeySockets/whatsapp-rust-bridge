use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const T_NODE: &'static str = r#"
/**
 * Represents the Wasm handle to a decoded binary node.
 * This object wraps a pointer into Wasm memory and exposes
 * lightweight accessor methods to read data on demand.
 */
export class WasmNode {
    readonly tag: string;
    readonly children: INode[];
    readonly content?: Uint8Array;

    getAttribute(key: string): string | undefined;
    getAttributeAsJid(key: string): string | undefined;

    getAttributes(): { [key: string]: string };
}

/**
 * Represents a node structure for ENCODING.
 * This is the plain JavaScript object representation passed to `encodeNode`.
 */
export interface INode {
    tag: string;
    attrs: { [key: string]: string };
    content?: INode[] | string | Uint8Array;
}
"#;

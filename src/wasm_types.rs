use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const T_NODE: &'static str = r#"
/**
 * Represents a node structure for marshalling and unmarshalling.
 * This is the plain JavaScript object representation.
 * content can be:
 *  - an array of child nodes (structured content)
 *  - a string (text / binary interpreted as UTF-8)
 */
export interface INode {
    tag: string;
    attrs?: Record<string, string>;
    content?: INode[] | string | Uint8Array;
}
"#;

// The previous Rust-side WasmNode structs were removed to avoid double
// serialization overhead. Conversion is now handled manually in wasm_api.rs
// directly between JsValue and the internal Node representation for performance.

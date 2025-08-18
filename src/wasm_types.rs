use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    content?: INode[] | string;
}
"#;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum WasmNodeContent {
    Nodes(Vec<WasmNode>),
    Text(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WasmNode {
    pub tag: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attrs: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<WasmNodeContent>,
}

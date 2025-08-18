use js_sys::Array;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wacore_binary::{
    marshal::{marshal, unmarshal_ref},
    node::{Node, NodeContent, NodeContentRef, NodeRef},
};
use wasm_bindgen::prelude::*;

// --------------------------------------------------
// Serde-friendly JS <-> Rust bridge structs
// --------------------------------------------------
#[derive(Serialize, Deserialize)]
struct JsNode {
    tag: String,
    #[serde(default = "HashMap::new", skip_serializing_if = "HashMap::is_empty")]
    attrs: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<JsNodeContent>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum JsNodeContent {
    // Array of child nodes
    Nodes(Vec<JsNode>),
    // Raw bytes (Uint8Array in JS)
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
    // Convenience: allow passing a JS string which we treat as UTF-8 bytes
    String(String),
}

impl From<&NodeRef<'_>> for JsNode {
    fn from(node_ref: &NodeRef) -> Self {
        let attrs = node_ref
            .attrs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<HashMap<_, _>>();

        let content = node_ref.content.as_deref().map(|c| match c {
            NodeContentRef::Nodes(children) => {
                JsNodeContent::Nodes(children.iter().map(JsNode::from).collect())
            }
            NodeContentRef::Bytes(bytes) => JsNodeContent::Bytes(bytes.to_vec()),
        });

        Self {
            tag: node_ref.tag.to_string(),
            attrs,
            content,
        }
    }
}

impl From<JsNode> for Node {
    fn from(js_node: JsNode) -> Self {
        let content = js_node.content.map(|c| match c {
            JsNodeContent::Nodes(nodes) => {
                NodeContent::Nodes(nodes.into_iter().map(Node::from).collect())
            }
            JsNodeContent::Bytes(bytes) => NodeContent::Bytes(bytes),
            JsNodeContent::String(s) => NodeContent::Bytes(s.into_bytes()),
        });
        Node {
            tag: js_node.tag,
            attrs: js_node.attrs,
            content,
        }
    }
}

// --------------------------------------------------
// Public WASM API (bulk serde conversion)
// --------------------------------------------------
#[wasm_bindgen(js_name = marshal)]
pub fn marshal_node(node_val: JsValue) -> Result<Vec<u8>, JsValue> {
    let js_node: JsNode = serde_wasm_bindgen::from_value(node_val)?;
    let internal: Node = js_node.into();
    marshal(&internal).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen(js_name = unmarshal)]
pub fn unmarshal_node(data: &[u8]) -> Result<JsValue, JsValue> {
    let node_ref = unmarshal_ref(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let js_node = JsNode::from(&node_ref);
    serde_wasm_bindgen::to_value(&js_node).map_err(|e| JsValue::from_str(&e.to_string()))
}

// --------------------------------------------------
// Fluent NodeBuilder exposed to JS (kept for ergonomic incremental build)
// --------------------------------------------------
#[wasm_bindgen(js_name = NodeBuilder)]
pub struct WasmNodeBuilder {
    tag: String,
    attrs: HashMap<String, String>,
    content: Option<NodeContent>,
}

#[wasm_bindgen(js_class = NodeBuilder)]
impl WasmNodeBuilder {
    #[wasm_bindgen(constructor)]
    pub fn new(tag: String) -> Self {
        Self {
            tag,
            attrs: HashMap::new(),
            content: None,
        }
    }

    #[wasm_bindgen(js_name = attr)]
    pub fn attr(&mut self, key: String, value: String) {
        self.attrs.insert(key, value);
    }

    #[wasm_bindgen(js_name = children)]
    pub fn set_children(&mut self, children_val: JsValue) -> Result<(), JsValue> {
        // Accept an array of plain JS node objects and deserialize in bulk.
        if !Array::is_array(&children_val) {
            return Err(JsValue::from_str("children must be an array"));
        }
        let nodes: Vec<JsNode> = serde_wasm_bindgen::from_value(children_val)?;
        let internal_children = nodes.into_iter().map(Node::from).collect();
        self.content = Some(NodeContent::Nodes(internal_children));
        Ok(())
    }

    #[wasm_bindgen(js_name = bytes)]
    pub fn set_bytes(&mut self, bytes: Vec<u8>) {
        self.content = Some(NodeContent::Bytes(bytes));
    }

    pub fn build(self) -> Result<Vec<u8>, JsValue> {
        let node = Node {
            tag: self.tag,
            attrs: self.attrs,
            content: self.content,
        };
        marshal(&node).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

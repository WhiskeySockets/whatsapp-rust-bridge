use js_sys::{Array, Object, Uint8Array};
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

// NOTE: We intentionally removed the From<&NodeRef> for JsNode implementation that
// performed a full allocation of an owned tree. For unmarshalling we now build
// the JS object structure directly (see node_ref_to_js_value) to reduce copies
// and allocations (perf focus per docs §9 and guidance in user request).

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
    if data.is_empty() {
        return Err(JsValue::from_str("Input data cannot be empty"));
    }
    // The binary produced by wacore-binary::marshal currently prefixes a \0 byte.
    // Historically the TS layer sliced this off with data.subarray(1). We move that
    // adjustment here so the JS API can pass the buffer verbatim. For forward
    // compatibility we attempt to detect the prefix instead of assuming index 0.
    let inner = if data[0] == 0 { &data[1..] } else { data };
    let node_ref = unmarshal_ref(inner).map_err(|e| JsValue::from_str(&e.to_string()))?;
    // Directly convert the zero-copy NodeRef into a JS object without creating an
    // intermediate owned Rust tree (avoids duplicate allocations & serde encode pass).
    node_ref_to_js_value(&node_ref)
}

/// Fast path: read a single attribute value from binary data without converting
/// the entire structure into a JS object. Returns `undefined` in JS if the
/// attribute does not exist or decoding fails.
#[wasm_bindgen(js_name = getAttribute)]
pub fn get_attribute_from_binary(data: &[u8], key: &str) -> Result<Option<String>, JsValue> {
    if data.is_empty() {
        return Ok(None);
    }
    let inner = if data[0] == 0 { &data[1..] } else { data };
    let node_ref = match unmarshal_ref(inner) {
        Ok(n) => n,
        Err(_) => return Ok(None),
    };
    Ok(node_ref.get_attr(key).map(|cow| cow.to_string()))
}

/// Convert a zero-copy NodeRef view into a plain JS object (recursively) with
/// minimal intermediate allocation. Each string/byte slice is copied exactly
/// once into JS (necessary boundary crossing) while avoiding constructing an
/// owned Rust `JsNode` tree first.
fn node_ref_to_js_value(node_ref: &NodeRef) -> Result<JsValue, JsValue> {
    let obj = Object::new();

    // tag
    js_sys::Reflect::set(&obj, &"tag".into(), &node_ref.tag.as_ref().into())?;

    // attrs
    if !node_ref.attrs.is_empty() {
        let attrs_obj = Object::new();
        for (k, v) in &node_ref.attrs {
            js_sys::Reflect::set(&attrs_obj, &k.as_ref().into(), &v.as_ref().into())?;
        }
        js_sys::Reflect::set(&obj, &"attrs".into(), &attrs_obj.into())?;
    }

    // content
    if let Some(content) = &node_ref.content {
        match content.as_ref() {
            NodeContentRef::Bytes(bytes) => {
                // Copy bytes into a fresh Uint8Array. Previous attempt used a zero-copy
                // view, but benchmarks indicated higher overhead for this workload.
                let arr = Uint8Array::new_with_length(bytes.len() as u32);
                arr.copy_from(bytes);
                js_sys::Reflect::set(&obj, &"content".into(), &arr.into())?;
            }
            NodeContentRef::Nodes(children) => {
                let arr = Array::new();
                for child in children.iter() {
                    arr.push(&node_ref_to_js_value(child)?);
                }
                js_sys::Reflect::set(&obj, &"content".into(), &arr.into())?;
            }
        }
    }

    Ok(obj.into())
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

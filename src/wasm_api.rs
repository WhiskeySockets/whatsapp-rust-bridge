use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wacore_binary::{
    marshal::{marshal, unmarshal_ref},
    node::{Node, NodeContent},
    util::unpack,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "INode")]
    pub type INode;
}

#[derive(Serialize, Deserialize)]
struct JsNode {
    tag: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    attrs: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<JsNodeContent>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum JsNodeContent {
    Nodes(Vec<JsNode>),
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
    String(String),
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

impl From<Node> for JsNode {
    fn from(node: Node) -> Self {
        let content = node.content.map(|c| match c {
            NodeContent::Nodes(nodes) => {
                JsNodeContent::Nodes(nodes.into_iter().map(JsNode::from).collect())
            }
            NodeContent::Bytes(b) => JsNodeContent::Bytes(b),
        });
        JsNode {
            tag: node.tag,
            attrs: node.attrs,
            content,
        }
    }
}

#[wasm_bindgen(js_name = encodeNode)]
pub fn encode_node(node_val: JsValue) -> Result<Vec<u8>, JsValue> {
    let js_node: JsNode = serde_wasm_bindgen::from_value(node_val)?;
    let internal: Node = js_node.into();
    marshal(&internal).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen(js_name = decodeNode)]
pub fn decode_node(data: &[u8]) -> Result<INode, JsValue> {
    if data.is_empty() {
        return Err(JsValue::from_str("Input data cannot be empty"));
    }

    let unpacked_data = unpack(data).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let node_ref = unmarshal_ref(&unpacked_data).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let owned_node = node_ref.to_owned();
    let js_node = JsNode::from(owned_node);
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    Ok(js_node.serialize(&serializer)?.into())
}

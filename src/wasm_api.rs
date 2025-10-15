use js_sys::{Array, Object, Uint8Array};
use serde::Deserialize;
use serde_wasm_bindgen;
use std::collections::HashMap;
use std::mem;
use wacore_binary::{
    marshal::{marshal, unmarshal_ref},
    node::{Node, NodeContent, NodeContentRef, NodeRef},
    util::unpack,
};
use wasm_bindgen::prelude::*;

#[derive(Deserialize)]
struct JsNode {
    tag: String,
    attrs: HashMap<String, String>,
}

/// A single, recursive function to convert a JsValue to a wacore_binary::Node.
/// This function is now fully optimized with no manual FFI calls per property.
fn convert_js_value_to_node(val: JsValue) -> Result<Node, JsValue> {
    // Deserialize the entire JS object in one highly-optimized operation.
    let js_node: JsNode = serde_wasm_bindgen::from_value(val.clone())
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    let content_val = js_sys::Reflect::get(&val, &"content".into())?;
    let mut content: Option<NodeContent> = None;
    if !content_val.is_undefined() {
        if let Some(s) = content_val.as_string() {
            content = Some(NodeContent::String(s));
        } else if content_val.is_instance_of::<Uint8Array>() {
            content = Some(NodeContent::Bytes(Uint8Array::from(content_val).to_vec()));
        } else if Array::is_array(&content_val) {
            let js_array = Array::from(&content_val);
            // Recursively call this same function for all children.
            let nodes: Result<Vec<Node>, _> =
                js_array.iter().map(convert_js_value_to_node).collect();
            content = Some(NodeContent::Nodes(nodes?));
        }
    }

    Ok(Node {
        tag: js_node.tag,
        attrs: js_node.attrs,
        content,
    })
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "INode")]
    pub type INode;
}

#[wasm_bindgen]
pub struct WasmNode {
    _owned_data: Box<[u8]>,
    node_ref: Box<NodeRef<'static>>,
}

impl WasmNode {
    fn node_ref(&self) -> &NodeRef<'static> {
        &self.node_ref
    }
}

#[wasm_bindgen]
impl WasmNode {
    #[wasm_bindgen(getter)]
    pub fn tag(&self) -> String {
        self.node_ref().tag.to_string()
    }

    #[wasm_bindgen(js_name = getAttribute)]
    pub fn get_attribute(&self, key: &str) -> Option<String> {
        let mut parser = self.node_ref().attr_parser();
        parser.optional_string(key).map(|s| s.to_string())
    }

    #[wasm_bindgen(js_name = getAttributeAsJid)]
    pub fn get_attribute_as_jid(&self, key: &str) -> Option<String> {
        let mut parser = self.node_ref().attr_parser();
        parser.optional_jid(key).map(|jid| jid.to_string())
    }

    #[wasm_bindgen(getter)]
    pub fn children(&self) -> Array {
        let children_array = Array::new();
        if let Some(children) = self.node_ref().children() {
            for child_ref in children {
                if let Ok(js_child) = node_ref_to_js(child_ref.clone()) {
                    children_array.push(&js_child);
                }
            }
        }
        children_array
    }

    #[wasm_bindgen(getter)]
    pub fn content(&self) -> JsValue {
        match self.node_ref().content.as_deref() {
            Some(NodeContentRef::Bytes(bytes)) => Uint8Array::from(bytes.as_ref()).into(),
            Some(NodeContentRef::String(s)) => Uint8Array::from(s.as_bytes()).into(),
            _ => JsValue::UNDEFINED,
        }
    }

    #[wasm_bindgen(js_name = getAttributes)]
    pub fn get_attributes(&self) -> Object {
        let attrs_obj = Object::new();
        let parser = self.node_ref().attr_parser();

        // We iterate through the raw attributes from the zero-copy NodeRef
        // and build a single JavaScript object. This is one efficient FFI call.
        for (key, val) in parser.attrs.iter() {
            let key_js: JsValue = key.as_ref().into();
            let val_js: JsValue = val.as_ref().into();
            js_sys::Reflect::set(&attrs_obj, &key_js, &val_js).unwrap();
        }

        attrs_obj
    }
}

fn node_ref_to_js(node: NodeRef) -> Result<JsValue, JsValue> {
    let obj = Object::new();

    js_sys::Reflect::set(&obj, &"tag".into(), &JsValue::from_str(&node.tag))?;

    let attrs_obj = Object::new();
    for (k, v) in node.attrs.iter() {
        js_sys::Reflect::set(&attrs_obj, &(&**k).into(), &(&**v).into())?;
    }
    js_sys::Reflect::set(&obj, &"attrs".into(), &attrs_obj.into())?;

    if let Some(content_box) = node.content {
        let content_val = match *content_box {
            NodeContentRef::Nodes(ref nodes) => {
                let js_array = Array::new_with_length(nodes.len() as u32);
                for (i, n) in nodes.iter().enumerate() {
                    js_array.set(i as u32, node_ref_to_js(n.clone())?);
                }
                js_array.into()
            }
            NodeContentRef::Bytes(ref b) => Uint8Array::from(b.as_ref()).into(),
            NodeContentRef::String(ref s) => JsValue::from_str(s),
        };
        js_sys::Reflect::set(&obj, &"content".into(), &content_val)?;
    }

    Ok(obj.into())
}

#[wasm_bindgen(js_name = encodeNodeTo)]
pub fn encode_node_to(node_val: JsValue, output_buffer: &mut [u8]) -> Result<usize, JsValue> {
    let internal: Node = convert_js_value_to_node(node_val)?;

    let mut cursor = std::io::Cursor::new(output_buffer);

    wacore_binary::marshal::marshal_to(&internal, &mut cursor)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    let bytes_written = cursor.position() as usize;

    Ok(bytes_written)
}

#[wasm_bindgen(js_name = encodeNode)]
pub fn encode_node(node_val: JsValue) -> Result<Vec<u8>, JsValue> {
    let internal: Node = convert_js_value_to_node(node_val)?;
    marshal(&internal).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen(js_name = decodeNode)]
pub fn decode_node(data: &[u8]) -> Result<WasmNode, JsValue> {
    if data.is_empty() {
        return Err(JsValue::from_str("Input data cannot be empty"));
    }

    let unpacked_cow = unpack(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let owned_data: Box<[u8]> = unpacked_cow.into_owned().into_boxed_slice();

    let static_data: &'static [u8] = unsafe { mem::transmute(owned_data.as_ref()) };
    let node_ref = unmarshal_ref(static_data).map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(WasmNode {
        _owned_data: owned_data,
        node_ref: Box::new(node_ref),
    })
}

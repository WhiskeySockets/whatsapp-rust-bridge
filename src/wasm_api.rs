use js_sys::{Array, Object, Reflect};
use std::collections::HashMap;
use wacore_binary::{
    builder::NodeBuilder,
    marshal::{marshal, unmarshal_ref},
    node::{Node, NodeContent, NodeContentRef, NodeRef},
};
use wasm_bindgen::{JsCast, prelude::*};

// -----------------------------
// JS <-> internal Node conversion (zero intermediate struct)
// -----------------------------

fn js_attrs_to_hashmap(attrs_val: &JsValue) -> Result<HashMap<String, String>, JsValue> {
    if attrs_val.is_undefined() || attrs_val.is_null() {
        return Ok(HashMap::new());
    }
    if !attrs_val.is_object() {
        return Err(JsValue::from_str("attrs must be an object"));
    }
    let obj: Object = attrs_val.clone().unchecked_into();
    let keys = Object::keys(&obj);
    let mut map = HashMap::with_capacity(keys.length() as usize);
    for key in keys.iter() {
        let k = key.as_string().unwrap_or_default();
        let v_val = Reflect::get(&obj, &key)?;
        let v = v_val
            .as_string()
            .ok_or_else(|| JsValue::from_str("attribute values must be strings"))?;
        map.insert(k, v);
    }
    Ok(map)
}

fn js_value_to_node(node_val: &JsValue) -> Result<Node, JsValue> {
    if !node_val.is_object() {
        return Err(JsValue::from_str("node must be an object"));
    }
    // tag
    let tag_val = Reflect::get(node_val, &JsValue::from_str("tag"))?;
    let tag = tag_val
        .as_string()
        .ok_or_else(|| JsValue::from_str("tag must be a string"))?;
    // attrs
    let attrs_val = Reflect::get(node_val, &JsValue::from_str("attrs"))?;
    let attrs = js_attrs_to_hashmap(&attrs_val)?;
    let mut builder = NodeBuilder::new(tag).attrs(attrs);
    // content
    let content_val = Reflect::get(node_val, &JsValue::from_str("content"))?;
    if !content_val.is_undefined() && !content_val.is_null() {
        // String content
        if let Some(s) = content_val.as_string() {
            builder = builder.bytes(s.as_bytes());
        } else if content_val.is_instance_of::<js_sys::Uint8Array>() {
            let u8arr: js_sys::Uint8Array = content_val.clone().unchecked_into();
            builder = builder.bytes(u8arr.to_vec());
        } else if Array::is_array(&content_val) {
            let arr: Array = content_val.unchecked_into();
            let mut children = Vec::with_capacity(arr.length() as usize);
            for child in arr.iter() {
                children.push(js_value_to_node(&child)?);
            }
            builder = builder.children(children);
        } else {
            return Err(JsValue::from_str("unsupported content type"));
        }
    }
    Ok(builder.build())
}

// Zero-copy conversion from NodeRef (borrowed view over input buffer) directly to JsValue
fn node_ref_to_js_value(node: &NodeRef) -> Result<JsValue, JsValue> {
    let obj = Object::new();
    Reflect::set(
        &obj,
        &JsValue::from_str("tag"),
        &JsValue::from_str(node.tag.as_ref()),
    )?;
    if !node.attrs.is_empty() {
        let attrs_obj = Object::new();
        for (k, v) in &node.attrs {
            Reflect::set(
                &attrs_obj,
                &JsValue::from_str(k.as_ref()),
                &JsValue::from_str(v.as_ref()),
            )?;
        }
        Reflect::set(&obj, &JsValue::from_str("attrs"), &attrs_obj.into())?;
    }
    if let Some(content_ref) = node.content.as_deref() {
        let js_content = match content_ref {
            NodeContentRef::Nodes(children) => {
                let arr = Array::new();
                for ch in children.iter() {
                    arr.push(&node_ref_to_js_value(ch)?);
                }
                arr.into()
            }
            NodeContentRef::Bytes(bytes_cow) => {
                // Return raw bytes as Uint8Array to avoid UTF-8 validation & allocations
                js_sys::Uint8Array::from(bytes_cow.as_ref()).into()
            }
        };
        Reflect::set(&obj, &JsValue::from_str("content"), &js_content)?;
    }
    Ok(obj.into())
}

// -----------------------------
// Public WASM API
// -----------------------------

#[wasm_bindgen(js_name = marshal)]
pub fn marshal_node(node_val: JsValue) -> Result<Vec<u8>, JsValue> {
    let internal_node = js_value_to_node(&node_val)?;
    marshal(&internal_node).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen(js_name = unmarshal)]
pub fn unmarshal_node(data: &[u8]) -> Result<JsValue, JsValue> {
    let node_ref = unmarshal_ref(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    node_ref_to_js_value(&node_ref)
}

// -----------------------------
// Fluent NodeBuilder exposed to JS (avoids serde for children array)
// -----------------------------
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

    pub fn attr(mut self, key: String, value: String) -> Self {
        self.attrs.insert(key, value);
        self
    }

    #[wasm_bindgen(js_name = children)]
    pub fn set_children(mut self, children_val: JsValue) -> Result<WasmNodeBuilder, JsValue> {
        if !Array::is_array(&children_val) {
            return Err(JsValue::from_str("children must be an array"));
        }
        let arr: Array = children_val.unchecked_into();
        let mut internal_children = Vec::with_capacity(arr.length() as usize);
        for child in arr.iter() {
            internal_children.push(js_value_to_node(&child)?);
        }
        self.content = Some(NodeContent::Nodes(internal_children));
        Ok(self)
    }

    #[wasm_bindgen(js_name = bytes)]
    pub fn set_bytes(mut self, bytes: Vec<u8>) -> Self {
        self.content = Some(NodeContent::Bytes(bytes));
        self
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

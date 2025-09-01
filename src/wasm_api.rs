use js_sys::{Array, Object, Uint8Array};
use std::collections::HashMap;
use wacore_binary::{
    marshal::{marshal, unmarshal_ref},
    node::{Node, NodeContent, NodeContentRef, NodeRef},
    util::unpack,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "INode")]
    pub type INode;
}

fn js_to_node(val: JsValue) -> Result<Node, JsValue> {
    if !val.is_object() {
        return Err(JsValue::from_str("Input must be an object"));
    }
    let obj = Object::from(val);
    let mut attrs = HashMap::new();
    let mut content: Option<NodeContent> = None;
    let tag = js_sys::Reflect::get(&obj, &"tag".into())?
        .as_string()
        .ok_or_else(|| JsValue::from_str("Node must have a 'tag' string property"))?;
    let attrs_val = js_sys::Reflect::get(&obj, &"attrs".into())?;
    if !attrs_val.is_undefined() && attrs_val.is_object() {
        let attrs_obj = Object::from(attrs_val);
        for key in Object::keys(&attrs_obj).iter() {
            let key_str = key.as_string().unwrap();
            if let Some(val_str) = js_sys::Reflect::get(&attrs_obj, &key)?.as_string() {
                attrs.insert(key_str, val_str);
            }
        }
    }
    let content_val = js_sys::Reflect::get(&obj, &"content".into())?;
    if !content_val.is_undefined() {
        if let Some(s) = content_val.as_string() {
            content = Some(NodeContent::Bytes(s.into_bytes()));
        } else if content_val.is_instance_of::<Uint8Array>() {
            content = Some(NodeContent::Bytes(Uint8Array::from(content_val).to_vec()));
        } else if Array::is_array(&content_val) {
            let js_array = Array::from(&content_val);
            let nodes: Result<Vec<Node>, _> = js_array.iter().map(js_to_node).collect();
            content = Some(NodeContent::Nodes(nodes?));
        }
    }
    Ok(Node {
        tag,
        attrs,
        content,
    })
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
        };
        js_sys::Reflect::set(&obj, &"content".into(), &content_val)?;
    }

    Ok(obj.into())
}

#[wasm_bindgen(js_name = encodeNodeTo)]
pub fn encode_node_to(node_val: JsValue, output_buffer: &mut [u8]) -> Result<usize, JsValue> {
    let internal: Node = js_to_node(node_val)?;

    let mut cursor = std::io::Cursor::new(output_buffer);

    wacore_binary::marshal::marshal_to(&internal, &mut cursor)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    let bytes_written = cursor.position() as usize;

    Ok(bytes_written)
}

#[deprecated(note = "Use encodeNodeTo for better performance")]
#[wasm_bindgen(js_name = encodeNode)]
pub fn encode_node(node_val: JsValue) -> Result<Vec<u8>, JsValue> {
    let internal: Node = js_to_node(node_val)?;
    marshal(&internal).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen(js_name = decodeNode)]
pub fn decode_node(data: &[u8]) -> Result<INode, JsValue> {
    if data.is_empty() {
        return Err(JsValue::from_str("Input data cannot be empty"));
    }

    let unpacked_data = unpack(data).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let node_ref = unmarshal_ref(&unpacked_data).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let js_val = node_ref_to_js(node_ref)?;
    Ok(js_val.into())
}

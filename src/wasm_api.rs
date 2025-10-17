use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::collections::HashMap;
use std::mem;
use wacore_binary::{
    marshal::{marshal, unmarshal_ref},
    node::{Node, NodeContent, NodeRef},
    util::unpack,
};

fn convert_js_object_to_node(obj: &Object) -> napi::Result<Node> {
    let tag: String = obj.get_named_property("tag")?;

    let attrs_obj: Object = obj.get_named_property("attrs")?;
    let keys: Vec<String> = Object::keys(&attrs_obj)?;
    let mut attrs = HashMap::with_capacity(keys.len());
    for key_str in keys {
        let val_str: String = attrs_obj.get_named_property(&key_str)?;
        attrs.insert(key_str, val_str);
    }

    let mut content: Option<NodeContent> = None;
    if obj.has_named_property("content")? {
        let content_val: Unknown = obj.get_named_property("content")?;
        match content_val.get_type()? {
            ValueType::String => {
                let s: String = content_val
                    .coerce_to_string()?
                    .into_utf8()?
                    .as_str()?
                    .to_owned();
                content = Some(NodeContent::String(s));
            }
            ValueType::Object => {
                if content_val.is_buffer()? {
                    let arr_obj = content_val.coerce_to_object()?;
                    let len: u32 = arr_obj.get_named_property("length")?;
                    let mut bytes = Vec::with_capacity(len as usize);
                    for i in 0..len {
                        let val: Unknown = arr_obj.get_element::<Unknown>(i)?;
                        let num = val.coerce_to_number()?.get_double()? as u8;
                        bytes.push(num);
                    }
                    content = Some(NodeContent::Bytes(bytes));
                } else if content_val.is_array()? {
                    let arr_obj = content_val.coerce_to_object()?;
                    let len: u32 = arr_obj.get_named_property("length")?;
                    let mut nodes = Vec::with_capacity(len as usize);
                    for i in 0..len {
                        let item: Object = arr_obj.get_element::<Object>(i)?;
                        nodes.push(convert_js_object_to_node(&item)?);
                    }
                    content = Some(NodeContent::Nodes(nodes));
                }
            }
            _ => {}
        }
    }

    Ok(Node {
        tag,
        attrs,
        content,
    })
}

#[napi]
pub struct WasmNode {
    _owned_data: Box<[u8]>,
    node_ref: Box<NodeRef<'static>>,
}

#[napi]
impl WasmNode {
    #[napi(getter)]
    pub fn tag(&self) -> String {
        self.node_ref.tag.to_string()
    }

    #[napi(getter)]
    pub fn content(&self) -> Either<Uint8Array, ()> {
        match self
            .node_ref
            .content
            .as_ref()
            .and_then(|content_ref| match content_ref.as_ref() {
                wacore_binary::node::NodeContentRef::String(s) => {
                    Some(Uint8Array::new(s.as_bytes().to_vec()))
                }
                wacore_binary::node::NodeContentRef::Bytes(b) => Some(Uint8Array::new(b.to_vec())),
                _ => None,
            }) {
            Some(arr) => Either::A(arr),
            None => Either::B(()),
        }
    }

    #[napi(getter)]
    pub fn children(&self) -> Vec<WasmNode> {
        self.node_ref
            .content
            .as_ref()
            .and_then(|content_ref| match content_ref.as_ref() {
                wacore_binary::node::NodeContentRef::Nodes(nodes) => {
                    let mut result = Vec::with_capacity(nodes.len());
                    for node_ref in nodes.iter() {
                        result.push(WasmNode {
                            _owned_data: self._owned_data.clone(),
                            node_ref: Box::new(node_ref.clone()),
                        });
                    }
                    Some(result)
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    #[napi(js_name = "getAttribute")]
    pub fn get_attribute(&self, key: String) -> Either<String, ()> {
        let mut parser = self.node_ref.attr_parser();
        match parser.optional_string(&key) {
            Some(s) if !s.is_empty() => Either::A(s.to_string()),
            _ => Either::B(()),
        }
    }

    #[napi]
    pub fn get_attributes(&self) -> std::collections::HashMap<String, String> {
        let parser = self.node_ref.attr_parser();
        parser
            .attrs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
}

#[napi]
pub fn encode_node(node_val: Object) -> napi::Result<Buffer> {
    let internal: Node = convert_js_object_to_node(&node_val)?;
    let bytes =
        marshal(&internal).map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;
    Ok(bytes.into())
}

#[napi]
pub fn decode_node(data: Buffer) -> napi::Result<WasmNode> {
    let data_slice: &[u8] = &data;
    if data_slice.is_empty() {
        return Err(Error::new(Status::InvalidArg, "Input data cannot be empty"));
    }

    let unpacked_cow =
        unpack(data_slice).map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;
    let owned_data: Box<[u8]> = unpacked_cow.into_owned().into_boxed_slice();

    let static_data: &'static [u8] = unsafe { mem::transmute(owned_data.as_ref()) };
    let node_ref = unmarshal_ref(static_data)
        .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;

    Ok(WasmNode {
        _owned_data: owned_data,
        node_ref: Box::new(node_ref),
    })
}

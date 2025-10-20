use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::collections::HashMap;
use std::mem;
use std::sync::Arc;
use wacore_binary::{
    marshal::{marshal_to, unmarshal_ref},
    node::{Node, NodeContent, NodeContentRef, NodeRef},
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

    if !obj.has_named_property("content")? {
        return Ok(Node {
            tag,
            attrs,
            content: None,
        });
    }

    let content_val: Unknown = obj.get_named_property("content")?;
    let content_type = content_val.get_type()?;

    let content = match content_type {
        ValueType::String => {
            let s: String = content_val
                .coerce_to_string()?
                .into_utf8()?
                .as_str()?
                .to_owned();
            Some(NodeContent::String(s))
        }
        ValueType::Object => {
            if content_val.is_typedarray()? {
                let buffer = Uint8Array::from_unknown(content_val)?;
                Some(NodeContent::Bytes(buffer.to_vec()))
            } else if content_val.is_array()? {
                let arr_obj = content_val.coerce_to_object()?;
                let len: u32 = arr_obj.get_named_property("length")?;

                let mut nodes = Vec::with_capacity(len as usize);
                for i in 0..len {
                    let item: Object = arr_obj.get_element(i)?;
                    nodes.push(convert_js_object_to_node(&item)?);
                }
                Some(NodeContent::Nodes(nodes))
            } else {
                None
            }
        }
        _ => None,
    };

    Ok(Node {
        tag,
        attrs,
        content,
    })
}

#[napi]
pub struct BinaryNode {
    _owned_data: Arc<[u8]>,
    node_ref: Box<NodeRef<'static>>,
}

#[napi]
impl BinaryNode {
    #[napi(getter)]
    pub fn tag(&self) -> &str {
        &self.node_ref.tag
    }

    #[napi(getter)]
    pub fn attrs(&self) -> HashMap<String, String> {
        let parser = self.node_ref.attr_parser();
        let mut map = HashMap::with_capacity(parser.attrs.len());

        map.extend(
            parser
                .attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string())),
        );
        map
    }
    #[napi(
        getter,
        ts_return_type = "string | Uint8Array | BinaryNode[] | undefined"
    )]
    pub fn content<'a>(&self, env: &'a Env) -> Result<Option<Unknown<'a>>> {
        if let Some(cref) = self.node_ref.content.as_ref() {
            match cref.as_ref() {
                NodeContentRef::String(s) => {
                    let js_str = env.create_string(s)?;
                    Ok(Some(js_str.to_unknown()))
                }
                NodeContentRef::Bytes(b) => {
                    let buf = BufferSlice::copy_from(env, b)?;
                    Ok(Some(buf.to_unknown()))
                }
                NodeContentRef::Nodes(nodes) => {
                    let mut arr = env.create_array(nodes.len() as u32)?;
                    for (i, child_ref) in nodes.iter().enumerate() {
                        let child = BinaryNode {
                            _owned_data: self._owned_data.clone(),
                            node_ref: Box::new(child_ref.clone()),
                        };
                        arr.set(i as u32, child)?;
                    }
                    Ok(Some(arr.to_unknown()))
                }
            }
        } else {
            Ok(None)
        }
    }
}

#[napi]
pub fn encode_node(node_val: Object) -> napi::Result<Uint8Array> {
    let internal_node = convert_js_object_to_node(&node_val)?;
    let mut writer = Vec::with_capacity(256);
    marshal_to(&internal_node, &mut writer)
        .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;
    Ok(writer.into())
}

#[napi]
pub fn decode_node(data: Uint8Array) -> napi::Result<BinaryNode> {
    let data_slice: &[u8] = &data;
    if data_slice.is_empty() {
        return Err(Error::new(Status::InvalidArg, "Input data is empty"));
    }

    let unpacked_cow =
        unpack(data_slice).map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;
    let owned_data: Arc<[u8]> = Arc::from(unpacked_cow.into_owned());
    let static_data: &'static [u8] = unsafe { mem::transmute(owned_data.as_ref()) };
    let node_ref = unmarshal_ref(static_data)
        .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?;

    Ok(BinaryNode {
        _owned_data: owned_data,
        node_ref: Box::new(node_ref),
    })
}

#[napi(object)]
pub struct INode<'a> {
    pub tag: String,
    pub attrs: HashMap<String, String>,

    #[napi(ts_type = "INode[] | string | Uint8Array")]
    pub content: Option<Unknown<'a>>,
}

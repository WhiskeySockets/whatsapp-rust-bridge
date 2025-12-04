use js_sys::{Array, Object, Uint8Array};
use std::borrow::Cow;
use std::cell::RefCell;
use std::mem;
use std::rc::Rc;
use wacore_binary::{
    marshal::{marshal_ref, unmarshal_ref},
    node::{NodeContentRef, NodeRef},
    util::unpack,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "EncodingNode")]
    pub type EncodingNode;

    #[wasm_bindgen(extends = Object, typescript_type = "{ [key: string]: string }")]
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub type Attrs;

    #[wasm_bindgen(extends = Object, typescript_type = "BinaryNode[] | string | Uint8Array")]
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub type Content;

    #[wasm_bindgen(structural, method, getter)]
    pub fn tag(this: &EncodingNode) -> String;

    #[wasm_bindgen(structural, method, getter)]
    pub fn attrs(this: &EncodingNode) -> Attrs;

    #[wasm_bindgen(structural, method, getter)]
    pub fn content(this: &EncodingNode) -> JsValue;
}

fn js_to_node_ref(val: &EncodingNode) -> Result<NodeRef<'static>, JsValue> {
    let attrs_obj = val.attrs().unchecked_into::<Object>();
    let keys = Object::keys(&attrs_obj);
    let len = keys.length();
    let mut attrs = Vec::with_capacity(len as usize);

    for i in 0..len {
        let key_js = keys.get(i);
        let key = key_js
            .as_string()
            .ok_or_else(|| JsValue::from_str("Attribute key must be a string"))?;

        let value_js = js_sys::Reflect::get(&attrs_obj, &key_js)
            .map_err(|_| JsValue::from_str("Failed to get attribute value"))?;

        let value_str = if let Some(s) = value_js.as_string() {
            s
        } else if let Some(n) = value_js.as_f64() {
            n.to_string()
        } else if let Some(b) = value_js.as_bool() {
            b.to_string()
        } else {
            continue;
        };

        if value_str.trim().is_empty() {
            continue;
        }

        attrs.push((Cow::Owned(key), Cow::Owned(value_str)));
    }

    let content_js = val.content();

    let content = match () {
        _ if content_js.is_undefined() => Ok(None),

        _ if content_js.is_string() => {
            let string_value = content_js.as_string().ok_or_else(|| {
                JsValue::from_str("Content marked as string could not be extracted")
            })?;
            Ok(Some(NodeContentRef::String(Cow::Owned(string_value))))
        }

        _ if content_js.is_instance_of::<Uint8Array>() => {
            let byte_array = Uint8Array::from(content_js);
            let mut bytes = vec![0; byte_array.length() as usize];
            byte_array.copy_to(&mut bytes);
            Ok(Some(NodeContentRef::Bytes(Cow::Owned(bytes))))
        }

        _ if Array::is_array(&content_js) => {
            let arr = Array::from(&content_js);
            let nodes = (0..arr.length())
                .map(|i| {
                    let child_val = arr.get(i);
                    let child_node = child_val.unchecked_into::<EncodingNode>();
                    js_to_node_ref(&child_node)
                })
                .collect::<Result<Vec<NodeRef<'static>>, _>>()?;

            Ok(Some(NodeContentRef::Nodes(Box::new(nodes))))
        }

        _ => Err(JsValue::from_str("Invalid content type")),
    };

    Ok(NodeRef::new(Cow::Owned(val.tag()), attrs, content?))
}

#[wasm_bindgen(typescript_custom_section)]
const T_NODE: &'static str = r#"
export interface BinaryNode {
    tag: string;
    attrs: { [key: string]: string };
    content?: BinaryNode[] | string | Uint8Array;
}
"#;

#[wasm_bindgen]
pub struct InternalBinaryNode {
    _owned_data: Rc<[u8]>,
    node_ref: NodeRef<'static>,
    #[wasm_bindgen(skip)]
    cached_attrs: RefCell<Option<Attrs>>,
    #[wasm_bindgen(skip)]
    cached_content: RefCell<Option<Content>>,
}

impl InternalBinaryNode {
    #[inline(always)]
    fn node_ref(&self) -> &NodeRef<'static> {
        &self.node_ref
    }
}

#[wasm_bindgen]
impl InternalBinaryNode {
    #[wasm_bindgen(getter)]
    pub fn tag(&self) -> String {
        self.node_ref().tag.to_string()
    }

    #[wasm_bindgen(js_name = toJSON)]
    pub fn to_json(&self) -> JsValue {
        let obj = Object::new();

        let tag_key = JsValue::from_str("tag");
        let tag_value = JsValue::from_str(&self.node_ref().tag);
        js_sys::Reflect::set(&obj, &tag_key, &tag_value).expect("Failed to set tag");

        let attrs_key = JsValue::from_str("attrs");
        let attrs_value: JsValue = self.attrs().into();
        js_sys::Reflect::set(&obj, &attrs_key, &attrs_value).expect("Failed to set attrs");

        let content_key = JsValue::from_str("content");
        if let Some(content) = self.content() {
            let content_js: JsValue = content.into();
            if Array::is_array(&content_js) {
                let arr = Array::from(&content_js);
                let json_arr = Array::new_with_length(arr.length());
                for i in 0..arr.length() {
                    let item = arr.get(i);
                    if let Ok(to_json_fn) =
                        js_sys::Reflect::get(&item, &JsValue::from_str("toJSON"))
                        && to_json_fn.is_function()
                    {
                        let func = to_json_fn.unchecked_into::<js_sys::Function>();
                        if let Ok(json_item) = func.call0(&item) {
                            json_arr.set(i, json_item);
                            continue;
                        }
                    }
                    json_arr.set(i, item);
                }
                js_sys::Reflect::set(&obj, &content_key, &json_arr).expect("Failed to set content");
            } else {
                js_sys::Reflect::set(&obj, &content_key, &content_js)
                    .expect("Failed to set content");
            }
        }

        obj.into()
    }

    #[wasm_bindgen(getter)]
    pub fn attrs(&self) -> Attrs {
        let mut cached = self.cached_attrs.borrow_mut();
        if cached.is_none() {
            let attrs = &self.node_ref().attrs;

            let obj = Object::new();
            for (k, v) in attrs.iter() {
                let key = JsValue::from_str(k);
                let value = JsValue::from_str(v);
                js_sys::Reflect::set(&obj, &key, &value).expect("Failed to set attribute");
            }

            *cached = Some(obj.unchecked_into());
        }

        cached
            .as_ref()
            .expect("Cached attributes should be populated before access")
            .clone()
            .unchecked_into()
    }

    #[wasm_bindgen(setter)]
    pub fn set_attrs(&self, new_attrs: Attrs) {
        *self.cached_attrs.borrow_mut() = Some(new_attrs);
    }

    #[wasm_bindgen(getter)]
    pub fn content(&self) -> Option<Content> {
        let mut cached = self.cached_content.borrow_mut();
        if cached.is_none() {
            match self.node_ref().content.as_deref() {
                Some(NodeContentRef::Bytes(bytes)) => {
                    let u8arr = Uint8Array::from(bytes.as_ref());
                    *cached = Some(u8arr.unchecked_into());
                }
                Some(NodeContentRef::String(s)) => {
                    *cached = Some(JsValue::from_str(s).unchecked_into());
                }
                Some(NodeContentRef::Nodes(nodes)) => {
                    let arr = Array::new_with_length(nodes.len() as u32);
                    for (i, node_ref) in nodes.iter().enumerate() {
                        let child = InternalBinaryNode {
                            _owned_data: Rc::clone(&self._owned_data),
                            node_ref: node_ref.clone(),
                            cached_attrs: RefCell::new(None),
                            cached_content: RefCell::new(None),
                        };
                        arr.set(i as u32, child.into());
                    }
                    *cached = Some(arr.unchecked_into());
                }
                None => *cached = Some(JsValue::undefined().unchecked_into()),
            }
        }
        cached
            .as_ref()
            .map(|v| v.clone().unchecked_into::<Content>())
    }

    #[wasm_bindgen(setter)]
    pub fn set_content(&self, new_content: Content) {
        *self.cached_content.borrow_mut() = Some(new_content);
    }
}

#[wasm_bindgen(js_name = encodeNode)]
pub fn encode_node(node_val: EncodingNode) -> Result<Uint8Array, JsValue> {
    let node_ref = js_to_node_ref(&node_val)?;
    let bytes = marshal_ref(&node_ref).map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(Uint8Array::from(&bytes[..]))
}

#[wasm_bindgen(js_name = decodeNode)]
pub fn decode_node(data: Vec<u8>) -> Result<InternalBinaryNode, JsValue> {
    if data.is_empty() {
        return Err(JsValue::from_str("Input data cannot be empty"));
    }

    let unpacked_cow = unpack(&data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let owned_data: Rc<[u8]> = Rc::from(unpacked_cow.into_owned().into_boxed_slice());

    let static_data: &'static [u8] = unsafe { mem::transmute(owned_data.as_ref()) };
    let node_ref = unmarshal_ref(static_data).map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(InternalBinaryNode {
        _owned_data: owned_data,
        node_ref,
        cached_attrs: RefCell::new(None),
        cached_content: RefCell::new(None),
    })
}

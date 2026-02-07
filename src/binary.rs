use js_sys::{Array, Object, Uint8Array};
use std::borrow::Cow;
use std::cell::UnsafeCell;
use std::mem;
use std::rc::Rc;
use wacore_binary::{
    marshal::{marshal_ref_to, unmarshal_ref},
    node::{NodeContentRef, NodeRef, ValueRef},
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

/// Manual JS→NodeRef conversion used by noise_session's encode_frame.
#[inline]
pub(crate) fn js_to_node_ref(val: &EncodingNode) -> Result<NodeRef<'static>, JsValue> {
    let attrs_obj = val.attrs().unchecked_into::<Object>();
    let entries = Object::entries(&attrs_obj);
    let len = entries.length();
    let mut attrs = Vec::with_capacity(len as usize);

    for i in 0..len {
        let entry = entries.get(i);
        let entry_arr = entry.unchecked_into::<Array>();
        let key_js = entry_arr.get(0);
        let value_js = entry_arr.get(1);

        let key = match key_js.as_string() {
            Some(k) => k,
            None => continue,
        };

        let value_str = if let Some(s) = value_js.as_string() {
            if s.is_empty() || s.chars().all(|c| c.is_whitespace()) {
                continue;
            }
            s
        } else if let Some(n) = value_js.as_f64() {
            n.to_string()
        } else if let Some(b) = value_js.as_bool() {
            b.to_string()
        } else {
            continue;
        };

        attrs.push((Cow::Owned(key), ValueRef::String(Cow::Owned(value_str))));
    }

    let content_js = val.content();

    let content = if content_js.is_undefined() || content_js.is_null() {
        Ok(None)
    } else if let Some(string_value) = content_js.as_string() {
        Ok(Some(NodeContentRef::String(Cow::Owned(string_value))))
    } else if content_js.is_instance_of::<Uint8Array>() {
        let byte_array: Uint8Array = content_js.unchecked_into();
        let len = byte_array.length() as usize;
        let mut bytes = vec![0; len];
        byte_array.copy_to(&mut bytes);
        Ok(Some(NodeContentRef::Bytes(Cow::Owned(bytes))))
    } else if Array::is_array(&content_js) {
        let arr = Array::from(&content_js);
        let nodes = (0..arr.length())
            .map(|i| {
                let child_val = arr.get(i);
                let child_node = child_val.unchecked_into::<EncodingNode>();
                js_to_node_ref(&child_node)
            })
            .collect::<Result<Vec<NodeRef<'static>>, _>>()?;
        Ok(Some(NodeContentRef::Nodes(Box::new(nodes))))
    } else {
        Err(JsValue::from_str("Invalid content type"))
    };

    Ok(NodeRef::new(Cow::Owned(val.tag()), attrs, content?))
}

// ── Packed binary protocol ──────────────────────────────────────────────
//
// Format (little-endian):
//   Node := tag_len:u16 + tag:utf8 + attr_count:u16 + Attr* + content_type:u8 + Content?
//   Attr := key_len:u16 + key:utf8 + val_len:u16 + val:utf8
//   Content:
//     type=0 → None
//     type=1 → string_len:u32 + string:utf8
//     type=2 → bytes_len:u32 + bytes:raw
//     type=3 → node_count:u16 + Node*
//
// JS packs a BinaryNode into this format in pure JS (zero FFI), then a single
// wasm call passes the buffer. Rust parses with Cow::Borrowed — zero-copy strings.

#[inline]
fn read_u16(data: &[u8], pos: usize) -> u16 {
    u16::from_le_bytes([data[pos], data[pos + 1]])
}

#[inline]
fn read_u32(data: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
}

/// SAFETY contract: JS TextEncoder.encodeInto always produces valid UTF-8.
/// We skip validation to avoid scanning every byte twice.
#[inline(always)]
unsafe fn read_str<'a>(data: &'a [u8], pos: &mut usize) -> &'a str {
    let len = read_u16(data, *pos) as usize;
    *pos += 2;
    let s = unsafe { std::str::from_utf8_unchecked(&data[*pos..*pos + len]) };
    *pos += len;
    s
}

fn parse_packed_node<'a>(data: &'a [u8], pos: &mut usize) -> NodeRef<'a> {
    // SAFETY: all strings written by JS TextEncoder — guaranteed valid UTF-8.
    unsafe {
        // Tag
        let tag = read_str(data, pos);

        // Attrs
        let attr_count = read_u16(data, *pos) as usize;
        *pos += 2;
        let mut attrs = Vec::with_capacity(attr_count);

        for _ in 0..attr_count {
            let key = read_str(data, pos);
            let val = read_str(data, pos);
            attrs.push((Cow::Borrowed(key), ValueRef::String(Cow::Borrowed(val))));
        }

        // Content
        let content_type = data[*pos];
        *pos += 1;

        let content = match content_type {
            1 => {
                // String content (u32 length)
                let len = read_u32(data, *pos) as usize;
                *pos += 4;
                let s = std::str::from_utf8_unchecked(&data[*pos..*pos + len]);
                *pos += len;
                Some(NodeContentRef::String(Cow::Borrowed(s)))
            }
            2 => {
                // Bytes content (u32 length)
                let len = read_u32(data, *pos) as usize;
                *pos += 4;
                let bytes = &data[*pos..*pos + len];
                *pos += len;
                Some(NodeContentRef::Bytes(Cow::Borrowed(bytes)))
            }
            3 => {
                // Child nodes (u16 count)
                let count = read_u16(data, *pos) as usize;
                *pos += 2;
                let mut nodes = Vec::with_capacity(count);
                for _ in 0..count {
                    nodes.push(parse_packed_node(data, pos));
                }
                Some(NodeContentRef::Nodes(Box::new(nodes)))
            }
            _ => None, // 0 = None, anything else = None
        };

        NodeRef::new(Cow::Borrowed(tag), attrs, content)
    }
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
    cached_attrs: UnsafeCell<Option<Attrs>>,
    #[wasm_bindgen(skip)]
    cached_content: UnsafeCell<Option<Content>>,
}

impl InternalBinaryNode {
    #[inline(always)]
    fn node_ref(&self) -> &NodeRef<'static> {
        &self.node_ref
    }

    #[inline]
    fn convert_attrs(attrs: &[(Cow<'_, str>, ValueRef<'_>)]) -> Attrs {
        let obj = Object::new();
        for (k, v) in attrs.iter() {
            let js_value = match v.as_str() {
                Some(s) => JsValue::from_str(s),
                None => JsValue::from_str(&v.to_string()),
            };
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), &js_value);
        }
        obj.unchecked_into()
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

        let _ = js_sys::Reflect::set(
            &obj,
            &JsValue::from_str("tag"),
            &JsValue::from_str(&self.node_ref().tag),
        );
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("attrs"), &self.attrs().into());

        if let Some(content) = self.content() {
            let content_js: JsValue = content.into();
            let content_value = if Array::is_array(&content_js) {
                self.serialize_child_nodes(&content_js)
            } else {
                content_js
            };
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("content"), &content_value);
        }

        obj.into()
    }

    fn serialize_child_nodes(&self, content_js: &JsValue) -> JsValue {
        let arr = Array::from(content_js);
        let json_arr = Array::new_with_length(arr.length());
        let to_json_key = JsValue::from_str("toJSON");

        for i in 0..arr.length() {
            let item = arr.get(i);
            let serialized = js_sys::Reflect::get(&item, &to_json_key)
                .ok()
                .filter(|f| f.is_function())
                .and_then(|f| f.unchecked_into::<js_sys::Function>().call0(&item).ok())
                .unwrap_or(item);
            json_arr.set(i, serialized);
        }

        json_arr.into()
    }

    #[wasm_bindgen(getter)]
    pub fn attrs(&self) -> Attrs {
        // SAFETY: WASM is single-threaded
        let cached = unsafe { &mut *self.cached_attrs.get() };
        if let Some(attrs) = cached.as_ref() {
            return attrs.clone();
        }

        let attrs = Self::convert_attrs(&self.node_ref().attrs);
        *cached = Some(attrs.clone());
        attrs
    }

    #[wasm_bindgen(setter)]
    pub fn set_attrs(&self, new_attrs: Attrs) {
        // SAFETY: WASM is single-threaded
        unsafe { *self.cached_attrs.get() = Some(new_attrs) };
    }

    #[wasm_bindgen(getter)]
    pub fn content(&self) -> Option<Content> {
        // SAFETY: WASM is single-threaded
        let cached = unsafe { &mut *self.cached_content.get() };
        if let Some(content) = cached.as_ref() {
            return Some(content.clone());
        }

        let result: Option<Content> = match self.node_ref().content.as_deref() {
            Some(NodeContentRef::Bytes(bytes)) => {
                let bytes_ref = bytes.as_ref();
                let u8arr = Uint8Array::new_with_length(bytes_ref.len() as u32);
                u8arr.copy_from(bytes_ref);
                Some(u8arr.unchecked_into())
            }
            Some(NodeContentRef::String(s)) => Some(JsValue::from_str(s).unchecked_into()),
            Some(NodeContentRef::Nodes(nodes)) => {
                let arr = Array::new_with_length(nodes.len() as u32);
                for (i, node_ref) in nodes.iter().enumerate() {
                    let child = InternalBinaryNode {
                        _owned_data: Rc::clone(&self._owned_data),
                        node_ref: node_ref.clone(),
                        cached_attrs: UnsafeCell::new(None),
                        cached_content: UnsafeCell::new(None),
                    };
                    arr.set(i as u32, child.into());
                }
                Some(arr.unchecked_into())
            }
            None => None,
        };

        *cached = result.clone();
        result
    }

    #[wasm_bindgen(setter)]
    pub fn set_content(&self, new_content: Content) {
        // SAFETY: WASM is single-threaded
        unsafe { *self.cached_content.get() = Some(new_content) };
    }
}

// SAFETY: WASM is single-threaded — no contention on thread_local
thread_local! {
    static ENCODE_BUF: UnsafeCell<Vec<u8>> = const { UnsafeCell::new(Vec::new()) };
    static INPUT_BUF: UnsafeCell<Vec<u8>> = const { UnsafeCell::new(Vec::new()) };
    static DECODE_BUF: UnsafeCell<Vec<u8>> = const { UnsafeCell::new(Vec::new()) };
}

// ── Zero-alloc result passing ───────────────────────────────────────────
//
// Instead of `Uint8Array::new_with_length` + `copy_from` (two FFI round-trips),
// we write the marshal result into ENCODE_BUF and store (ptr, len) in a static
// descriptor. JS reads those 8 bytes directly from WASM memory and does a
// single `.slice()` — no FFI overhead for the output path.
//
// SAFETY: WASM is single-threaded — no data races on static.
struct SyncCell<T>(UnsafeCell<T>);
unsafe impl<T> Sync for SyncCell<T> {}

static ENCODE_RESULT: SyncCell<[u32; 2]> = SyncCell(UnsafeCell::new([0, 0]));

/// Returns the WASM memory address of the encode result descriptor.
/// Called once at init time, cached by JS.
#[wasm_bindgen(js_name = encodeResultPtr)]
pub fn encode_result_ptr() -> u32 {
    ENCODE_RESULT.0.get() as u32
}

// ── Shared input buffer ─────────────────────────────────────────────────
//
// JS packs the BinaryNode directly into WASM memory (zero intermediate copies).
// `inputBufGrow` ensures capacity and returns the buffer pointer so JS can
// create a Uint8Array view. `encodeFromInputBuf` reads from this buffer.

/// Ensure input buffer has at least `min_cap` bytes. Returns buffer pointer.
#[wasm_bindgen(js_name = inputBufGrow)]
pub fn input_buf_grow(min_cap: u32) -> u32 {
    INPUT_BUF.with(|cell| {
        // SAFETY: WASM is single-threaded
        let buf = unsafe { &mut *cell.get() };
        let cap = min_cap as usize;
        if buf.len() < cap {
            buf.resize(cap, 0);
        }
        buf.as_ptr() as u32
    })
}

/// Encode from shared input buffer. JS packs directly into WASM memory,
/// then calls this with the packed length. Zero input copies.
#[wasm_bindgen(js_name = encodeFromInputBuf)]
pub fn encode_from_input_buf(len: u32) {
    INPUT_BUF.with(|input_cell| {
        let input = unsafe { &*input_cell.get() };
        let packed = &input[..len as usize];
        let mut pos = 0;
        let node_ref = parse_packed_node(packed, &mut pos);

        ENCODE_BUF.with(|encode_cell| {
            let buf = unsafe { &mut *encode_cell.get() };
            buf.clear();
            marshal_ref_to(&node_ref, buf).expect("marshal failed");
            unsafe {
                let result = &mut *ENCODE_RESULT.0.get();
                result[0] = buf.as_ptr() as u32;
                result[1] = buf.len() as u32;
            }
        });
    });
}

/// Fallback: encode from JS object (used when packed path is unavailable).
#[wasm_bindgen(js_name = encodeNodeJs)]
pub fn encode_node_js(node_val: EncodingNode) -> Result<Uint8Array, JsValue> {
    let node_ref = js_to_node_ref(&node_val)?;

    ENCODE_BUF.with(|cell| {
        // SAFETY: WASM is single-threaded, no reentrant calls
        let buf = unsafe { &mut *cell.get() };
        buf.clear();
        marshal_ref_to(&node_ref, buf).map_err(|e| JsValue::from_str(&e.to_string()))?;

        let result = Uint8Array::new_with_length(buf.len() as u32);
        result.copy_from(buf);
        Ok(result)
    })
}

/// Fast path: encode from pre-packed binary buffer (JS packs, Rust decodes + marshals).
/// Zero-copy strings on the Rust side — all Cow::Borrowed from the input slice.
#[wasm_bindgen(js_name = encodeNodePacked)]
pub fn encode_node_packed(packed: &[u8]) -> Result<Uint8Array, JsValue> {
    let mut pos = 0;
    let node_ref = parse_packed_node(packed, &mut pos);

    ENCODE_BUF.with(|cell| {
        // SAFETY: WASM is single-threaded, no reentrant calls
        let buf = unsafe { &mut *cell.get() };
        buf.clear();
        marshal_ref_to(&node_ref, buf).map_err(|e| JsValue::from_str(&e.to_string()))?;

        let result = Uint8Array::new_with_length(buf.len() as u32);
        result.copy_from(buf);
        Ok(result)
    })
}

/// Fastest path: encode packed node, write result descriptor to static.
/// JS reads ptr+len directly from WASM memory — zero Uint8Array alloc on Rust side.
#[wasm_bindgen(js_name = encodeNodePackedInto)]
pub fn encode_node_packed_into(packed: &[u8]) {
    let mut pos = 0;
    let node_ref = parse_packed_node(packed, &mut pos);

    ENCODE_BUF.with(|cell| {
        // SAFETY: WASM is single-threaded, no reentrant calls
        let buf = unsafe { &mut *cell.get() };
        buf.clear();
        marshal_ref_to(&node_ref, buf).expect("marshal failed");
        // SAFETY: WASM is single-threaded
        unsafe {
            let result = &mut *ENCODE_RESULT.0.get();
            result[0] = buf.as_ptr() as u32;
            result[1] = buf.len() as u32;
        }
    });
}

#[wasm_bindgen(js_name = decodeNode)]
pub fn decode_node(data: Vec<u8>) -> Result<InternalBinaryNode, JsValue> {
    if data.is_empty() {
        return Err(JsValue::from_str("Input data cannot be empty"));
    }

    let unpacked_cow = unpack(&data).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let owned_data: Rc<[u8]> = match unpacked_cow {
        Cow::Owned(vec) => Rc::from(vec.into_boxed_slice()),
        Cow::Borrowed(slice) => Rc::from(slice),
    };

    let static_data: &'static [u8] = unsafe { mem::transmute(owned_data.as_ref()) };
    let node_ref = unmarshal_ref(static_data).map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(InternalBinaryNode {
        _owned_data: owned_data,
        node_ref,
        cached_attrs: UnsafeCell::new(None),
        cached_content: UnsafeCell::new(None),
    })
}

// ── Packed decode: NodeRef → LNP buffer ─────────────────────────────────
//
// Reverse of parse_packed_node: serializes a NodeRef into the same packed
// binary format that JS uses for encode input. JS reads this from WASM memory
// and constructs plain { tag, attrs, content } objects — zero FFI wrappers.

#[inline]
fn write_packed_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u16).to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}

fn write_packed_node(node: &NodeRef, buf: &mut Vec<u8>) {
    // Tag
    write_packed_str(buf, &node.tag);

    // Attrs
    buf.extend_from_slice(&(node.attrs.len() as u16).to_le_bytes());
    for (k, v) in &node.attrs {
        write_packed_str(buf, k);
        match v.as_str() {
            Some(s) => write_packed_str(buf, s),
            None => {
                let s = v.to_string();
                write_packed_str(buf, &s);
            }
        }
    }

    // Content
    match node.content.as_deref() {
        None => buf.push(0),
        Some(NodeContentRef::String(s)) => {
            buf.push(1);
            buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
            buf.extend_from_slice(s.as_bytes());
        }
        Some(NodeContentRef::Bytes(b)) => {
            buf.push(2);
            buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
            buf.extend_from_slice(b);
        }
        Some(NodeContentRef::Nodes(nodes)) => {
            buf.push(3);
            buf.extend_from_slice(&(nodes.len() as u16).to_le_bytes());
            for child in nodes.iter() {
                write_packed_node(child, buf);
            }
        }
    }
}

/// Release memory held by internal buffers (after processing large messages).
#[wasm_bindgen(js_name = shrinkBuffers)]
pub fn shrink_buffers() {
    ENCODE_BUF.with(|c| unsafe { (&mut *c.get()).shrink_to_fit() });
    INPUT_BUF.with(|c| unsafe { (&mut *c.get()).shrink_to_fit() });
    DECODE_BUF.with(|c| unsafe { (&mut *c.get()).shrink_to_fit() });
}

/// Decode WhatsApp binary format → packed LNP buffer in WASM memory.
/// JS reads ptr+len from ENCODE_RESULT and constructs plain JS objects.
#[wasm_bindgen(js_name = decodeNodeToPacked)]
pub fn decode_node_to_packed(data: &[u8]) -> Result<(), JsValue> {
    if data.is_empty() {
        return Err(JsValue::from_str("Input data cannot be empty"));
    }

    let unpacked_cow = unpack(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let node_ref = unmarshal_ref(&unpacked_cow).map_err(|e| JsValue::from_str(&e.to_string()))?;

    DECODE_BUF.with(|cell| {
        // SAFETY: WASM is single-threaded
        let buf = unsafe { &mut *cell.get() };
        buf.clear();
        write_packed_node(&node_ref, buf);
        unsafe {
            let result = &mut *ENCODE_RESULT.0.get();
            result[0] = buf.as_ptr() as u32;
            result[1] = buf.len() as u32;
        }
    });

    Ok(())
}

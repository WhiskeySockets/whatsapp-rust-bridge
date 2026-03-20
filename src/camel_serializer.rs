//! Custom serde Serializer that outputs JsValue with:
//! - camelCase field names (converts from Rust snake_case)
//! - Uint8Array for byte sequences (detects Vec<u8> serialized as seq of u8)
//! - Skips None, empty Vec, empty String, zero numbers, false booleans
//! - BigInt for u64/i64
//!
//! This lives entirely in the bridge — waproto stays agnostic.

use js_sys::{Object, Uint8Array};
use serde::ser::{self, Serialize};
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Error(msg.to_string())
    }
}

impl From<Error> for JsValue {
    fn from(e: Error) -> Self {
        JsValue::from_str(&e.0)
    }
}

// ---------------------------------------------------------------------------
// snake_case → camelCase
// ---------------------------------------------------------------------------

fn to_camel_case(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut upper_next = false;
    let mut started = false;

    for &b in s.as_bytes() {
        if b == b'_' {
            if started {
                upper_next = true;
            }
            continue;
        }
        started = true;
        let c = if upper_next {
            upper_next = false;
            b.to_ascii_uppercase()
        } else {
            b
        };
        bytes.push(c);
    }

    // Safe: input is valid UTF-8, we only uppercased ASCII letters
    unsafe { String::from_utf8_unchecked(bytes) }
}

// ---------------------------------------------------------------------------
// Serializer
// ---------------------------------------------------------------------------

/// Serializes Rust values to JsValue with camelCase keys and proto-friendly output.
pub struct CamelSerializer;

impl ser::Serializer for CamelSerializer {
    type Ok = JsValue;
    type Error = Error;

    type SerializeSeq = SeqSerializer;
    type SerializeTuple = SeqSerializer;
    type SerializeTupleStruct = SeqSerializer;
    type SerializeTupleVariant = SeqSerializer;
    type SerializeMap = MapSerializer;
    type SerializeStruct = StructSerializer;
    type SerializeStructVariant = StructVariantSerializer;

    fn serialize_bool(self, v: bool) -> Result<JsValue, Error> {
        Ok(JsValue::from_bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<JsValue, Error> {
        Ok(JsValue::from_f64(v as f64))
    }
    fn serialize_i16(self, v: i16) -> Result<JsValue, Error> {
        Ok(JsValue::from_f64(v as f64))
    }
    fn serialize_i32(self, v: i32) -> Result<JsValue, Error> {
        Ok(JsValue::from_f64(v as f64))
    }
    fn serialize_i64(self, v: i64) -> Result<JsValue, Error> {
        Ok(js_sys::BigInt::from(v).into())
    }
    fn serialize_u8(self, v: u8) -> Result<JsValue, Error> {
        Ok(JsValue::from_f64(v as f64))
    }
    fn serialize_u16(self, v: u16) -> Result<JsValue, Error> {
        Ok(JsValue::from_f64(v as f64))
    }
    fn serialize_u32(self, v: u32) -> Result<JsValue, Error> {
        Ok(JsValue::from_f64(v as f64))
    }
    fn serialize_u64(self, v: u64) -> Result<JsValue, Error> {
        Ok(js_sys::BigInt::from(v).into())
    }
    fn serialize_f32(self, v: f32) -> Result<JsValue, Error> {
        Ok(JsValue::from_f64(v as f64))
    }
    fn serialize_f64(self, v: f64) -> Result<JsValue, Error> {
        Ok(JsValue::from_f64(v))
    }
    fn serialize_char(self, v: char) -> Result<JsValue, Error> {
        Ok(JsValue::from_str(&v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<JsValue, Error> {
        Ok(JsValue::from_str(v))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<JsValue, Error> {
        Ok(Uint8Array::from(v).into())
    }
    fn serialize_none(self) -> Result<JsValue, Error> {
        Ok(JsValue::NULL)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<JsValue, Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<JsValue, Error> {
        Ok(JsValue::NULL)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<JsValue, Error> {
        Ok(JsValue::NULL)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
    ) -> Result<JsValue, Error> {
        Ok(JsValue::from_str(variant))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<JsValue, Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<JsValue, Error> {
        let obj = Object::new();
        let val = value.serialize(CamelSerializer)?;
        js_sys::Reflect::set(&obj, &JsValue::from_str(variant), &val)
            .map_err(|e| Error(format!("{e:?}")))?;
        Ok(obj.into())
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<SeqSerializer, Error> {
        Ok(SeqSerializer {
            items: Vec::with_capacity(len.unwrap_or(0)),
            all_u8: true,
            u8_buf: Vec::new(),
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqSerializer, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SeqSerializer, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _idx: u32,
        _variant: &'static str,
        len: usize,
    ) -> Result<SeqSerializer, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<MapSerializer, Error> {
        Ok(MapSerializer {
            obj: Object::new(),
            next_key: None,
        })
    }
    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<StructSerializer, Error> {
        Ok(StructSerializer { obj: Object::new() })
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _idx: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<StructVariantSerializer, Error> {
        Ok(StructVariantSerializer {
            variant,
            inner: StructSerializer { obj: Object::new() },
        })
    }
}

// ---------------------------------------------------------------------------
// SerializeSeq — detects all-u8 sequences → outputs Uint8Array
// ---------------------------------------------------------------------------

pub struct SeqSerializer {
    items: Vec<JsValue>,
    all_u8: bool,
    u8_buf: Vec<u8>,
}

impl ser::SerializeSeq for SeqSerializer {
    type Ok = JsValue;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let js = value.serialize(CamelSerializer)?;
        // Track if all elements are small integers (u8 range) → byte array
        if self.all_u8 {
            if let Some(n) = js.as_f64() {
                let rounded = n as u8;
                if (rounded as f64 - n).abs() < f64::EPSILON && (0.0..=255.0).contains(&n) {
                    self.u8_buf.push(rounded);
                } else {
                    self.all_u8 = false;
                }
            } else {
                self.all_u8 = false;
            }
        }
        self.items.push(js);
        Ok(())
    }

    fn end(self) -> Result<JsValue, Error> {
        // If all elements were u8, output as Uint8Array
        if self.all_u8 && !self.u8_buf.is_empty() {
            return Ok(Uint8Array::from(self.u8_buf.as_slice()).into());
        }
        let arr = js_sys::Array::new_with_length(self.items.len() as u32);
        for (i, item) in self.items.into_iter().enumerate() {
            arr.set(i as u32, item);
        }
        Ok(arr.into())
    }
}

// Reuse SeqSerializer for tuples
impl ser::SerializeTuple for SeqSerializer {
    type Ok = JsValue;
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<JsValue, Error> {
        ser::SerializeSeq::end(self)
    }
}
impl ser::SerializeTupleStruct for SeqSerializer {
    type Ok = JsValue;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<JsValue, Error> {
        ser::SerializeSeq::end(self)
    }
}
impl ser::SerializeTupleVariant for SeqSerializer {
    type Ok = JsValue;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<JsValue, Error> {
        ser::SerializeSeq::end(self)
    }
}

// ---------------------------------------------------------------------------
// SerializeStruct — camelCase keys, skip defaults
// ---------------------------------------------------------------------------

pub struct StructSerializer {
    obj: Object,
}

impl ser::SerializeStruct for StructSerializer {
    type Ok = JsValue;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let js_val = value.serialize(CamelSerializer)?;
        // Skip default values: null, empty arrays, empty strings, 0, false
        if should_skip(&js_val) {
            return Ok(());
        }
        let camel_key = to_camel_case(key);
        js_sys::Reflect::set(&self.obj, &JsValue::from_str(&camel_key), &js_val)
            .map_err(|e| Error(format!("{e:?}")))?;
        Ok(())
    }

    fn end(self) -> Result<JsValue, Error> {
        Ok(self.obj.into())
    }
}

// ---------------------------------------------------------------------------
// SerializeStructVariant
// ---------------------------------------------------------------------------

pub struct StructVariantSerializer {
    variant: &'static str,
    inner: StructSerializer,
}

impl ser::SerializeStructVariant for StructVariantSerializer {
    type Ok = JsValue;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        ser::SerializeStruct::serialize_field(&mut self.inner, key, value)
    }

    fn end(self) -> Result<JsValue, Error> {
        let obj = Object::new();
        let inner = ser::SerializeStruct::end(self.inner)?;
        js_sys::Reflect::set(&obj, &JsValue::from_str(self.variant), &inner)
            .map_err(|e| Error(format!("{e:?}")))?;
        Ok(obj.into())
    }
}

// ---------------------------------------------------------------------------
// SerializeMap
// ---------------------------------------------------------------------------

pub struct MapSerializer {
    obj: Object,
    next_key: Option<String>,
}

impl ser::SerializeMap for MapSerializer {
    type Ok = JsValue;
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        let js_key = key.serialize(CamelSerializer)?;
        self.next_key = js_key.as_string();
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self.next_key.take().unwrap_or_default();
        let js_val = value.serialize(CamelSerializer)?;
        js_sys::Reflect::set(&self.obj, &JsValue::from_str(&key), &js_val)
            .map_err(|e| Error(format!("{e:?}")))?;
        Ok(())
    }

    fn end(self) -> Result<JsValue, Error> {
        Ok(self.obj.into())
    }
}

// ---------------------------------------------------------------------------
// Skip logic — matches protobufjs behavior (only output set fields)
// ---------------------------------------------------------------------------

fn should_skip(val: &JsValue) -> bool {
    if val.is_null() || val.is_undefined() {
        return true;
    }
    if let Some(s) = val.as_string() {
        return s.is_empty();
    }
    if let Some(n) = val.as_f64() {
        return n == 0.0;
    }
    if let Some(b) = val.as_bool() {
        return !b;
    }
    // Expensive checks only for objects — avoid clone when possible
    if val.is_object() {
        if val.is_instance_of::<js_sys::Array>() {
            let arr: js_sys::Array = js_sys::Array::unchecked_from_js(val.clone());
            return arr.length() == 0;
        }
        if val.is_instance_of::<Uint8Array>() {
            let arr: Uint8Array = Uint8Array::unchecked_from_js(val.clone());
            return arr.length() == 0;
        }
        let obj: Object = Object::unchecked_from_js(val.clone());
        return js_sys::Object::keys(&obj).length() == 0;
    }
    false
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Serialize a value to JsValue with camelCase keys, Uint8Array for bytes,
/// and proto default values skipped. For proto types only.
pub fn to_js_value_camel<T: Serialize>(val: &T) -> Result<JsValue, JsValue> {
    val.serialize(CamelSerializer).map_err(|e| e.into())
}

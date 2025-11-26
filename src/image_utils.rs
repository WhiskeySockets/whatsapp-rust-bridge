use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};
use js_sys::{Object, Reflect, Uint8Array};
use std::io::Cursor;
use wasm_bindgen::prelude::*;

const JPEG_QUALITY: u8 = 50;

#[wasm_bindgen(js_name = extractImageThumb)]
pub fn extract_image_thumb(image_data: &[u8], width: u32) -> Result<Object, JsValue> {
    if width == 0 {
        return Err(JsValue::from_str("width must be greater than zero"));
    }

    let img = load_image(image_data)?;
    let (orig_width, orig_height) = img.dimensions();
    let resized = img.resize(width, width, FilterType::Triangle);

    let jpeg = encode_jpeg(&resized)?;

    let result = Object::new();
    let original = Object::new();

    Reflect::set(
        &original,
        &JsValue::from_str("width"),
        &JsValue::from_f64(orig_width as f64),
    )?;
    Reflect::set(
        &original,
        &JsValue::from_str("height"),
        &JsValue::from_f64(orig_height as f64),
    )?;

    Reflect::set(
        &result,
        &JsValue::from_str("buffer"),
        &Uint8Array::from(jpeg.as_slice()).into(),
    )?;
    Reflect::set(&result, &JsValue::from_str("original"), &original.into())?;

    Ok(result)
}

#[wasm_bindgen(js_name = generateProfilePicture)]
pub fn generate_profile_picture(image_data: &[u8], target_width: u32) -> Result<Object, JsValue> {
    if target_width == 0 {
        return Err(JsValue::from_str("target width must be greater than zero"));
    }

    let img = load_image(image_data)?;
    let resized = img.resize_to_fill(target_width, target_width, FilterType::Triangle);
    let jpeg = encode_jpeg(&resized)?;

    let result = Object::new();
    Reflect::set(
        &result,
        &JsValue::from_str("img"),
        &Uint8Array::from(jpeg.as_slice()).into(),
    )?;

    Ok(result)
}

fn load_image(image_data: &[u8]) -> Result<DynamicImage, JsValue> {
    image::load_from_memory(image_data)
        .map_err(|e| JsValue::from_str(&format!("Failed to load image: {e}")))
}

fn encode_jpeg(image: &DynamicImage) -> Result<Vec<u8>, JsValue> {
    let mut buffer = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut buffer, JPEG_QUALITY);
    encoder
        .encode_image(image)
        .map_err(|e| JsValue::from_str(&format!("Failed to encode image: {e}")))?;
    Ok(buffer.into_inner())
}

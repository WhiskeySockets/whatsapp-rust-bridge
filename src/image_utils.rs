use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};
use serde::Serialize;
use std::io::Cursor;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

const JPEG_QUALITY: u8 = 50;

/// Original image dimensions
#[derive(Debug, Clone, Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
}

/// Result of extracting an image thumbnail
#[derive(Debug, Clone, Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct ImageThumbResult {
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub buffer: Vec<u8>,
    pub original: ImageDimensions,
}

/// Result of generating a profile picture
#[derive(Debug, Clone, Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct ProfilePictureResult {
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub img: Vec<u8>,
}

#[wasm_bindgen(js_name = extractImageThumb)]
pub fn extract_image_thumb(image_data: &[u8], width: u32) -> Result<ImageThumbResult, JsValue> {
    if width == 0 {
        return Err(JsValue::from_str("width must be greater than zero"));
    }

    let img = load_image(image_data)?;
    let (orig_width, orig_height) = img.dimensions();
    let resized = img.resize(width, width, FilterType::Triangle);
    let jpeg = encode_jpeg(&resized)?;

    Ok(ImageThumbResult {
        buffer: jpeg,
        original: ImageDimensions {
            width: orig_width,
            height: orig_height,
        },
    })
}

#[wasm_bindgen(js_name = generateProfilePicture)]
pub fn generate_profile_picture(
    image_data: &[u8],
    target_width: u32,
) -> Result<ProfilePictureResult, JsValue> {
    if target_width == 0 {
        return Err(JsValue::from_str("target width must be greater than zero"));
    }

    let resized =
        load_image(image_data)?.resize_to_fill(target_width, target_width, FilterType::Triangle);
    let jpeg = encode_jpeg(&resized)?;

    Ok(ProfilePictureResult { img: jpeg })
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

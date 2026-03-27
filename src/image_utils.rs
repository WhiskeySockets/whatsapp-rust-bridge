use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

const JPEG_QUALITY: u8 = 50;

// ---------------------------------------------------------------------------
// Host-agnostic core functions (no JS types)
// ---------------------------------------------------------------------------

/// Extract a JPEG thumbnail from any supported image format.
pub fn extract_thumb(image_data: &[u8], width: u32) -> Result<ImageThumbResult, String> {
    if width == 0 {
        return Err("width must be greater than zero".into());
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

/// Generate a square profile picture as JPEG.
pub fn generate_profile_pic(
    image_data: &[u8],
    target_width: u32,
) -> Result<ProfilePictureResult, String> {
    if target_width == 0 {
        return Err("target width must be greater than zero".into());
    }
    let resized =
        load_image(image_data)?.resize_to_fill(target_width, target_width, FilterType::Triangle);
    let jpeg = encode_jpeg(&resized)?;
    Ok(ProfilePictureResult { img: jpeg })
}

/// Get image dimensions without full processing.
pub fn get_dimensions(image_data: &[u8]) -> Result<ImageDimensions, String> {
    let img = load_image(image_data)?;
    let (width, height) = img.dimensions();
    Ok(ImageDimensions { width, height })
}

/// Convert any image to WebP format.
pub fn convert_to_webp_bytes(image_data: &[u8]) -> Result<Vec<u8>, String> {
    let img = load_image(image_data)?;
    encode_format(&img, image::ImageFormat::WebP)
}

/// Process image with resize and format conversion options.
pub fn process(
    image_data: &[u8],
    options: &ProcessImageOptions,
) -> Result<ProcessImageResult, String> {
    let img = load_image(image_data)?;

    let processed = match (options.width, options.height) {
        (Some(w), Some(h)) => img.resize_exact(w, h, FilterType::Triangle),
        (Some(w), None) => img.resize(w, u32::MAX, FilterType::Triangle),
        (None, Some(h)) => img.resize(u32::MAX, h, FilterType::Triangle),
        (None, None) => img,
    };

    let (width, height) = processed.dimensions();
    let quality = options.quality.unwrap_or(80).clamp(1, 100);

    let buffer = match options.format {
        ImageFormat::Jpeg => encode_jpeg_quality(&processed, quality)?,
        ImageFormat::Png => encode_format(&processed, image::ImageFormat::Png)?,
        ImageFormat::WebP => encode_format(&processed, image::ImageFormat::WebP)?,
    };

    Ok(ProcessImageResult {
        buffer,
        width,
        height,
    })
}

fn load_image(image_data: &[u8]) -> Result<DynamicImage, String> {
    image::load_from_memory(image_data).map_err(|e| format!("Failed to load image: {e}"))
}

fn encode_jpeg(image: &DynamicImage) -> Result<Vec<u8>, String> {
    encode_jpeg_quality(image, JPEG_QUALITY)
}

fn encode_jpeg_quality(image: &DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut buffer = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut buffer, quality);
    encoder
        .encode_image(image)
        .map_err(|e| format!("Failed to encode JPEG: {e}"))?;
    Ok(buffer.into_inner())
}

fn encode_format(image: &DynamicImage, format: image::ImageFormat) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut buffer), format)
        .map_err(|e| format!("Failed to encode image: {e}"))?;
    Ok(buffer)
}

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

// ---------------------------------------------------------------------------
// WASM wrappers (thin delegation to core functions)
// ---------------------------------------------------------------------------

#[wasm_bindgen(js_name = extractImageThumb)]
pub fn extract_image_thumb(image_data: &[u8], width: u32) -> Result<ImageThumbResult, JsError> {
    extract_thumb(image_data, width).map_err(|e| JsError::new(&e))
}

#[wasm_bindgen(js_name = generateProfilePicture)]
pub fn generate_profile_picture(
    image_data: &[u8],
    target_width: u32,
) -> Result<ProfilePictureResult, JsError> {
    generate_profile_pic(image_data, target_width).map_err(|e| JsError::new(&e))
}

/// Output format for image processing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Jpeg,
    Png,
    WebP,
}

/// Options for image processing
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
pub struct ProcessImageOptions {
    /// Target width (optional, maintains aspect ratio if only width is set)
    pub width: Option<u32>,
    /// Target height (optional, maintains aspect ratio if only height is set)
    pub height: Option<u32>,
    /// Output format
    pub format: ImageFormat,
    /// Quality for lossy formats (JPEG, WebP). 1-100, default 80
    pub quality: Option<u8>,
}

/// Result of image processing
#[derive(Debug, Clone, Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct ProcessImageResult {
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub buffer: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Get image dimensions without full decoding
#[wasm_bindgen(js_name = getImageDimensions)]
pub fn get_image_dimensions(image_data: &[u8]) -> Result<ImageDimensions, JsError> {
    get_dimensions(image_data).map_err(|e| JsError::new(&e))
}

/// Convert any image to WebP format
#[wasm_bindgen(js_name = convertToWebP)]
pub fn convert_to_webp(image_data: Vec<u8>) -> Result<js_sys::Uint8Array, JsError> {
    let webp = convert_to_webp_bytes(&image_data).map_err(|e| JsError::new(&e))?;
    Ok(js_sys::Uint8Array::from(webp.as_slice()))
}

/// Process image with resize and format conversion options
#[wasm_bindgen(js_name = processImage)]
pub fn process_image(
    image_data: Vec<u8>,
    options: ProcessImageOptions,
) -> Result<ProcessImageResult, JsError> {
    process(&image_data, &options).map_err(|e| JsError::new(&e))
}

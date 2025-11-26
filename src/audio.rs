use js_sys::{ArrayBuffer, Reflect, Uint8Array};
use std::io::Cursor;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, Decoder, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{ReadableStream, ReadableStreamDefaultReader};

/// WhatsApp uses 64 buckets for visual waveforms.
const WAVEFORM_SAMPLES: usize = 64;

#[wasm_bindgen(js_name = generateAudioWaveform)]
pub fn generate_audio_waveform(audio_data: &[u8]) -> Result<Uint8Array, JsValue> {
    if audio_data.is_empty() {
        return Err(JsValue::from_str("Audio buffer is empty"));
    }

    let DecoderContext {
        mut format,
        mut decoder,
        track_id,
        ..
    } = prepare_decoder(audio_data)?;
    let mut samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(ref e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::NotFound
                ) =>
            {
                break;
            }
            Err(Error::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => {
                return Err(JsValue::from_str(&format!("Audio decode error: {e}")));
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                let spec = *audio_buf.spec();
                let channel_count = spec.channels.count();
                if channel_count == 0 {
                    continue;
                }

                let capacity = audio_buf.capacity() as u64;
                let mut sample_buf = SampleBuffer::<f32>::new(capacity, spec);
                sample_buf.copy_interleaved_ref(audio_buf);

                let data = sample_buf.samples();
                let frame_count = data.len() / channel_count;

                for frame_idx in 0..frame_count {
                    let mut sum = 0.0;
                    for channel in 0..channel_count {
                        let sample = data[frame_idx * channel_count + channel];
                        sum += sample;
                    }
                    samples.push(sum / channel_count as f32);
                }
            }
            Err(Error::IoError(ref e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::NotFound
                ) =>
            {
                break;
            }
            Err(Error::DecodeError(_)) => continue,
            Err(Error::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => {
                return Err(JsValue::from_str(&format!(
                    "Failed to decode audio frame: {e}"
                )));
            }
        }
    }

    if samples.is_empty() {
        return Err(JsValue::from_str("No audio samples decoded"));
    }

    let waveform = process_waveform(&samples, WAVEFORM_SAMPLES);
    Ok(Uint8Array::from(waveform.as_slice()))
}

#[wasm_bindgen(js_name = getAudioDuration, skip_typescript)]
pub async fn get_audio_duration(input: JsValue) -> Result<f64, JsValue> {
    let audio_bytes = normalize_audio_input(input).await?;
    compute_audio_duration(&audio_bytes)
}

#[wasm_bindgen(typescript_custom_section)]
const TS_AUDIO_DURATION: &str = r#"
export type AudioDurationInput =
    | Uint8Array
    | ArrayBuffer
    | ReadableStream<Uint8Array | ArrayBuffer | ArrayBufferView>;

export function getAudioDuration(input: AudioDurationInput): Promise<number>;
"#;

fn compute_audio_duration(audio_data: &[u8]) -> Result<f64, JsValue> {
    if audio_data.is_empty() {
        return Err(JsValue::from_str("Audio buffer is empty"));
    }

    let DecoderContext {
        mut format,
        mut decoder,
        track_id,
        sample_rate,
    } = prepare_decoder(audio_data)?;

    let sample_rate =
        sample_rate.ok_or_else(|| JsValue::from_str("Audio track missing sample rate"))?;

    let mut total_frames: u64 = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(ref e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::NotFound
                ) =>
            {
                break;
            }
            Err(Error::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => {
                return Err(JsValue::from_str(&format!("Audio decode error: {e}")));
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                total_frames += audio_buf.frames() as u64;
            }
            Err(Error::IoError(ref e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::NotFound
                ) =>
            {
                break;
            }
            Err(Error::DecodeError(_)) => continue,
            Err(Error::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => {
                return Err(JsValue::from_str(&format!(
                    "Failed to decode audio frame: {e}"
                )));
            }
        }
    }

    if total_frames == 0 {
        return Err(JsValue::from_str("No audio samples decoded"));
    }

    Ok(total_frames as f64 / sample_rate as f64)
}

fn process_waveform(samples: &[f32], target_bins: usize) -> Vec<u8> {
    if samples.is_empty() {
        return vec![0; target_bins];
    }

    let total_samples = samples.len();
    let chunk_size = (total_samples as f64 / target_bins as f64).max(1.0);

    let mut bins = Vec::with_capacity(target_bins);
    let mut max_val: f32 = 0.0;

    for i in 0..target_bins {
        let start = (i as f64 * chunk_size).floor() as usize;
        let end = (((i + 1) as f64 * chunk_size).floor() as usize).min(total_samples);

        if start >= end {
            bins.push(0.0);
            continue;
        }

        let mut sum = 0.0;
        for sample in &samples[start..end] {
            sum += sample.abs();
        }
        let avg = sum / (end - start) as f32;
        max_val = max_val.max(avg);
        bins.push(avg);
    }

    let multiplier = if max_val > 0.0 { 100.0 / max_val } else { 0.0 };
    bins.iter()
        .map(|val| (val * multiplier).clamp(0.0, 100.0) as u8)
        .collect()
}

async fn normalize_audio_input(input: JsValue) -> Result<Vec<u8>, JsValue> {
    if input.is_instance_of::<Uint8Array>() {
        let arr = Uint8Array::new(&input);
        return Ok(copy_uint8_array(arr));
    }

    if input.is_instance_of::<ArrayBuffer>() {
        let arr = Uint8Array::new(&input);
        return Ok(copy_uint8_array(arr));
    }

    if input.is_instance_of::<ReadableStream>() {
        let stream: ReadableStream = input.dyn_into()?;
        return read_stream(stream).await;
    }

    Err(JsValue::from_str(
        "Unsupported input type. Expected Uint8Array, ArrayBuffer, or ReadableStream",
    ))
}

fn copy_uint8_array(array: Uint8Array) -> Vec<u8> {
    let mut buffer = vec![0; array.length() as usize];
    array.copy_to(&mut buffer);
    buffer
}

async fn read_stream(stream: ReadableStream) -> Result<Vec<u8>, JsValue> {
    let reader = stream.get_reader();
    let reader: ReadableStreamDefaultReader = reader.dyn_into()?;
    read_from_reader(reader).await
}

async fn read_from_reader(reader: ReadableStreamDefaultReader) -> Result<Vec<u8>, JsValue> {
    let mut chunks: Vec<u8> = Vec::new();

    loop {
        let promise = reader.read();
        let result = JsFuture::from(promise).await?;

        let done = Reflect::get(&result, &JsValue::from_str("done"))?
            .as_bool()
            .unwrap_or(false);
        if done {
            break;
        }

        let value = Reflect::get(&result, &JsValue::from_str("value"))?;
        if !value.is_undefined() && !value.is_null() {
            let chunk = Uint8Array::new(&value);
            let chunk_len = chunk.length() as usize;
            let prev_len = chunks.len();
            chunks.resize(prev_len + chunk_len, 0);
            chunk.copy_to(&mut chunks[prev_len..]);
        }
    }

    reader.release_lock();
    Ok(chunks)
}

struct DecoderContext {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    sample_rate: Option<u32>,
}

fn prepare_decoder(audio_data: &[u8]) -> Result<DecoderContext, JsValue> {
    // Feed the raw bytes into Symphonia via an in-memory cursor.
    let cursor = Cursor::new(audio_data.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let hint = Hint::new();
    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();
    let decoder_opts = DecoderOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| JsValue::from_str(&format!("Failed to probe audio format: {e}")))?;

    let format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| JsValue::from_str("No supported audio track found"))?;

    let codec_params = track.codec_params.clone();
    let track_id = track.id;

    let decoder = symphonia::default::get_codecs()
        .make(&codec_params, &decoder_opts)
        .map_err(|e| JsValue::from_str(&format!("Failed to create decoder: {e}")))?;

    Ok(DecoderContext {
        format,
        decoder,
        track_id,
        sample_rate: codec_params.sample_rate,
    })
}

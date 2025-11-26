use js_sys::Uint8Array;
use std::io::Cursor;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use wasm_bindgen::prelude::*;

/// WhatsApp uses 64 buckets for visual waveforms.
const WAVEFORM_SAMPLES: usize = 64;

#[wasm_bindgen(js_name = generateAudioWaveform)]
pub fn generate_audio_waveform(audio_data: &[u8]) -> Result<Uint8Array, JsValue> {
    if audio_data.is_empty() {
        return Err(JsValue::from_str("Audio buffer is empty"));
    }

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

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| JsValue::from_str("No supported audio track found"))?;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .map_err(|e| JsValue::from_str(&format!("Failed to create decoder: {e}")))?;

    let track_id = track.id;
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

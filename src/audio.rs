use js_sys::{ArrayBuffer, Reflect, Uint8Array};
use std::io::Cursor;
use symphonia::core::audio::{AudioBuffer, AudioBufferRef, Signal};
use symphonia::core::codecs::{CODEC_TYPE_NULL, Decoder, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, Track};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::TimeBase;
use symphonia::core::{conv::IntoSample, sample::Sample};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{ReadableStream, ReadableStreamDefaultReader};

/// WhatsApp uses 64 buckets for visual waveforms.
const WAVEFORM_SAMPLES: usize = 64;
/// Aggregate raw samples into medium-sized chunks to keep memory bounded.
const WAVEFORM_CHUNK_SIZE: usize = 2048;

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
    let mut builder = WaveformBuilder::new(WAVEFORM_CHUNK_SIZE);

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
                accumulate_waveform_samples(&audio_buf, &mut builder);
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

    let chunks = builder.finish();

    if chunks.is_empty() {
        return Err(JsValue::from_str("No audio samples decoded"));
    }

    let waveform = build_waveform(&chunks, WAVEFORM_SAMPLES);
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

    let track = format
        .tracks()
        .iter()
        .find(|track| track.id == track_id)
        .cloned()
        .ok_or_else(|| JsValue::from_str("No supported audio track found"))?;

    if let Some(duration) = duration_from_track_metadata(&track) {
        return Ok(duration);
    }

    let mut stats = DurationAccumulator::default();

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

        stats.update(packet.ts(), packet.dur());
    }

    let ticks = stats
        .elapsed_ticks()
        .ok_or_else(|| JsValue::from_str("No audio samples decoded"))?;

    let duration = convert_ticks_to_seconds(
        ticks,
        track.codec_params.time_base,
        sample_rate.or(track.codec_params.sample_rate),
    )
    .ok_or_else(|| JsValue::from_str("Missing timing information for audio track"))?;

    Ok(duration)
}

fn build_waveform(chunks: &[WaveformChunk], target_bins: usize) -> Vec<u8> {
    if chunks.is_empty() || target_bins == 0 {
        return vec![0; target_bins];
    }

    let total_samples: u64 = chunks.iter().map(|chunk| chunk.count as u64).sum();
    if total_samples == 0 {
        return vec![0; target_bins];
    }

    let samples_per_bin = (total_samples as f64 / target_bins as f64).max(1.0);
    let mut bin_sums = vec![0.0f64; target_bins];
    let mut bin_counts = vec![0.0f64; target_bins];
    let mut bin_idx = 0usize;
    let mut bin_remaining = samples_per_bin;

    for chunk in chunks {
        if chunk.count == 0 {
            continue;
        }

        let chunk_total = f64::from(chunk.count);
        let mut chunk_remaining = chunk_total;

        while chunk_remaining > 0.0 && bin_idx < target_bins {
            let take = bin_remaining.min(chunk_remaining);
            let contribution = chunk.sum_abs * (take / chunk_total);
            bin_sums[bin_idx] += contribution;
            bin_counts[bin_idx] += take;
            chunk_remaining -= take;
            bin_remaining -= take;

            if bin_remaining <= f64::EPSILON {
                bin_idx += 1;
                bin_remaining = samples_per_bin;
            }
        }

        if bin_idx >= target_bins {
            break;
        }
    }

    let mut max_avg = 0.0f64;
    let mut averages = vec![0.0f64; target_bins];
    for i in 0..target_bins {
        if bin_counts[i] > 0.0 {
            let avg = bin_sums[i] / bin_counts[i];
            averages[i] = avg;
            if avg > max_avg {
                max_avg = avg;
            }
        }
    }

    if max_avg == 0.0 {
        return vec![0; target_bins];
    }

    averages
        .into_iter()
        .map(|avg| (avg * (100.0 / max_avg)).clamp(0.0, 100.0) as u8)
        .collect()
}

fn accumulate_waveform_samples(buffer: &AudioBufferRef<'_>, builder: &mut WaveformBuilder) {
    match buffer {
        AudioBufferRef::U8(buf) => accumulate_from_buffer(buf.as_ref(), builder),
        AudioBufferRef::U16(buf) => accumulate_from_buffer(buf.as_ref(), builder),
        AudioBufferRef::U24(buf) => accumulate_from_buffer(buf.as_ref(), builder),
        AudioBufferRef::U32(buf) => accumulate_from_buffer(buf.as_ref(), builder),
        AudioBufferRef::S8(buf) => accumulate_from_buffer(buf.as_ref(), builder),
        AudioBufferRef::S16(buf) => accumulate_from_buffer(buf.as_ref(), builder),
        AudioBufferRef::S24(buf) => accumulate_from_buffer(buf.as_ref(), builder),
        AudioBufferRef::S32(buf) => accumulate_from_buffer(buf.as_ref(), builder),
        AudioBufferRef::F32(buf) => accumulate_from_buffer(buf.as_ref(), builder),
        AudioBufferRef::F64(buf) => accumulate_from_buffer(buf.as_ref(), builder),
    }
}

fn accumulate_from_buffer<S>(buffer: &AudioBuffer<S>, builder: &mut WaveformBuilder)
where
    S: Sample + IntoSample<f32>,
{
    let channel_count = buffer.spec().channels.count();
    if channel_count == 0 {
        return;
    }

    let frames = buffer.frames();
    if frames == 0 {
        return;
    }

    let mut channel_slices: Vec<&[S]> = Vec::with_capacity(channel_count);
    for channel in 0..channel_count {
        channel_slices.push(buffer.chan(channel));
    }

    for frame_idx in 0..frames {
        let mut sum = 0.0f32;
        for plane in &channel_slices {
            sum += plane[frame_idx].into_sample();
        }

        let avg = sum / channel_count as f32;
        builder.ingest(f64::from(avg.abs()));
    }
}

struct WaveformBuilder {
    chunks: Vec<WaveformChunk>,
    chunk_sum: f64,
    chunk_count: u32,
    chunk_size: usize,
}

impl WaveformBuilder {
    fn new(chunk_size: usize) -> Self {
        WaveformBuilder {
            chunks: Vec::new(),
            chunk_sum: 0.0,
            chunk_count: 0,
            chunk_size,
        }
    }

    fn ingest(&mut self, sample: f64) {
        self.chunk_sum += sample;
        self.chunk_count += 1;

        if self.chunk_count as usize == self.chunk_size {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.chunk_count == 0 {
            return;
        }

        self.chunks.push(WaveformChunk {
            sum_abs: self.chunk_sum,
            count: self.chunk_count,
        });

        self.chunk_sum = 0.0;
        self.chunk_count = 0;
    }

    fn finish(mut self) -> Vec<WaveformChunk> {
        self.flush();
        self.chunks
    }
}

fn duration_from_track_metadata(track: &Track) -> Option<f64> {
    let codec_params = &track.codec_params;
    let frames = codec_params.n_frames?;

    convert_ticks_to_seconds(frames, codec_params.time_base, codec_params.sample_rate)
}

fn convert_ticks_to_seconds(
    ticks: u64,
    time_base: Option<TimeBase>,
    sample_rate: Option<u32>,
) -> Option<f64> {
    if ticks == 0 {
        return None;
    }

    if let Some(tb) = time_base {
        let time = tb.calc_time(ticks);
        return Some(time.seconds as f64 + time.frac);
    }

    sample_rate.map(|rate| ticks as f64 / rate as f64)
}

#[derive(Default)]
struct DurationAccumulator {
    first_ts: Option<u64>,
    max_end_ts: u64,
}

impl DurationAccumulator {
    fn update(&mut self, ts: u64, dur: u64) {
        if self.first_ts.is_none() {
            self.first_ts = Some(ts);
        }

        let end = ts.saturating_add(dur);
        if end > self.max_end_ts {
            self.max_end_ts = end;
        }
    }

    fn elapsed_ticks(&self) -> Option<u64> {
        let start = self.first_ts?;
        Some(self.max_end_ts.saturating_sub(start))
    }
}

struct WaveformChunk {
    sum_abs: f64,
    count: u32,
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

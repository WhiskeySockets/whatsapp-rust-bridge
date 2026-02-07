use js_sys::Uint8Array;
use wacore_binary::consts::NOISE_START_PATTERN as NOISE_MODE;
use wacore_binary::marshal::{marshal_ref, unmarshal_ref};
use wacore_binary::util::unpack;
use wacore_noise::framing::{FrameDecoder, encode_frame_into};
use wacore_noise::{NoiseCipher, NoiseHandshake, build_handshake_header};
use wasm_bindgen::prelude::*;

use crate::binary::{EncodingNode, decode_node, js_to_node_ref, set_result_descriptor};

/// NoiseSession implements the Noise_XX_25519_AESGCM_SHA256 protocol pattern
/// with combined binary encoding/decoding operations for reduced WASM boundary crossings.
#[wasm_bindgen]
pub struct NoiseSession {
    handshake: Option<NoiseHandshake>,
    enc_cipher: Option<NoiseCipher>,
    dec_cipher: Option<NoiseCipher>,
    read_counter: u32,
    write_counter: u32,
    is_finished: bool,
    intro_header: Option<Vec<u8>>,
    frame_decoder: FrameDecoder,
    encode_scratch: Vec<u8>,
}

#[wasm_bindgen]
impl NoiseSession {
    #[wasm_bindgen(constructor)]
    pub fn new(
        public_key: &[u8],
        noise_header: &[u8],
        routing_info: Option<Vec<u8>>,
    ) -> Result<NoiseSession, JsValue> {
        let mut handshake = NoiseHandshake::new(NOISE_MODE, noise_header)
            .map_err(|e| JsValue::from_str(&format!("NoiseHandshake init failed: {}", e)))?;

        handshake.authenticate(public_key);

        let (intro_header, _) = build_handshake_header(routing_info.as_deref());

        Ok(NoiseSession {
            handshake: Some(handshake),
            enc_cipher: None,
            dec_cipher: None,
            read_counter: 0,
            write_counter: 0,
            is_finished: false,
            intro_header: Some(intro_header),
            frame_decoder: FrameDecoder::new(),
            encode_scratch: Vec::with_capacity(4096),
        })
    }

    /// Updates the session hash with the given data (no-op after handshake).
    pub fn authenticate(&mut self, data: &[u8]) {
        if let Some(ref mut handshake) = self.handshake {
            handshake.authenticate(data);
        }
    }

    #[inline]
    fn encrypt_vec(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, JsValue> {
        if self.is_finished {
            let cipher = self
                .enc_cipher
                .as_ref()
                .ok_or_else(|| JsValue::from_str("Encryption cipher not initialized"))?;
            let counter = self.write_counter;
            self.write_counter += 1;
            cipher
                .encrypt_with_counter(counter, plaintext)
                .map_err(|e| JsValue::from_str(&format!("Encryption failed: {}", e)))
        } else {
            self.handshake
                .as_mut()
                .ok_or_else(|| JsValue::from_str("Handshake not initialized"))?
                .encrypt(plaintext)
                .map_err(|e| JsValue::from_str(&format!("Encryption failed: {}", e)))
        }
    }

    #[inline]
    fn decrypt_vec(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, JsValue> {
        if self.is_finished {
            let cipher = self
                .dec_cipher
                .as_ref()
                .ok_or_else(|| JsValue::from_str("Decryption cipher not initialized"))?;
            let counter = self.read_counter;
            self.read_counter += 1;
            cipher
                .decrypt_with_counter(counter, ciphertext)
                .map_err(|e| JsValue::from_str(&format!("Decryption failed: {}", e)))
        } else {
            self.handshake
                .as_mut()
                .ok_or_else(|| JsValue::from_str("Handshake not initialized"))?
                .decrypt(ciphertext)
                .map_err(|e| JsValue::from_str(&format!("Decryption failed: {}", e)))
        }
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Uint8Array, JsValue> {
        let ciphertext = self.encrypt_vec(plaintext)?;
        let result = Uint8Array::new_with_length(ciphertext.len() as u32);
        result.copy_from(&ciphertext);
        Ok(result)
    }

    /// Packed variant: writes ciphertext to DECODE_BUF and sets result descriptor.
    #[wasm_bindgen(js_name = encryptPacked)]
    pub fn encrypt_packed(&mut self, plaintext: &[u8]) -> Result<(), JsValue> {
        let ciphertext = self.encrypt_vec(plaintext)?;
        crate::binary::with_decode_buf(|buf| {
            buf.clear();
            buf.extend_from_slice(&ciphertext);
            set_result_descriptor(buf.as_ptr() as u32, buf.len() as u32);
        });
        Ok(())
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Uint8Array, JsValue> {
        let plaintext = self.decrypt_vec(ciphertext)?;
        let result = Uint8Array::new_with_length(plaintext.len() as u32);
        result.copy_from(&plaintext);
        Ok(result)
    }

    /// Packed variant: writes plaintext to DECODE_BUF and sets result descriptor.
    #[wasm_bindgen(js_name = decryptPacked)]
    pub fn decrypt_packed(&mut self, ciphertext: &[u8]) -> Result<(), JsValue> {
        let plaintext = self.decrypt_vec(ciphertext)?;
        crate::binary::with_decode_buf(|buf| {
            buf.clear();
            buf.extend_from_slice(&plaintext);
            set_result_descriptor(buf.as_ptr() as u32, buf.len() as u32);
        });
        Ok(())
    }

    #[wasm_bindgen(js_name = mixIntoKey)]
    pub fn mix_into_key(&mut self, data: &[u8]) -> Result<(), JsValue> {
        let handshake = self
            .handshake
            .as_mut()
            .ok_or_else(|| JsValue::from_str("NoiseHandshake not initialized"))?;
        handshake
            .mix_into_key(data)
            .map_err(|e| JsValue::from_str(&format!("mixIntoKey failed: {}", e)))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = finishInit)]
    pub fn finish_init(&mut self) -> Result<(), JsValue> {
        let handshake = self
            .handshake
            .take()
            .ok_or_else(|| JsValue::from_str("NoiseHandshake not initialized"))?;

        let (write_cipher, read_cipher) = handshake
            .finish()
            .map_err(|e| JsValue::from_str(&format!("finishInit failed: {}", e)))?;

        self.enc_cipher = Some(write_cipher);
        self.dec_cipher = Some(read_cipher);
        self.read_counter = 0;
        self.write_counter = 0;
        self.is_finished = true;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = isFinished)]
    pub fn is_finished(&self) -> bool {
        self.is_finished
    }

    #[wasm_bindgen(js_name = encodeFrameRaw)]
    pub fn encode_frame_raw(&mut self, data: &[u8]) -> Result<Uint8Array, JsValue> {
        self.encode_frame_raw_inner(data)?;

        let result = Uint8Array::new_with_length(self.encode_scratch.len() as u32);
        result.copy_from(&self.encode_scratch);
        Ok(result)
    }

    /// Packed variant: sets result descriptor instead of allocating Uint8Array.
    /// JS reads ptr+len directly from WASM memory.
    #[wasm_bindgen(js_name = encodeFrameRawPacked)]
    pub fn encode_frame_raw_packed(&mut self, data: &[u8]) -> Result<(), JsValue> {
        self.encode_frame_raw_inner(data)?;
        set_result_descriptor(
            self.encode_scratch.as_ptr() as u32,
            self.encode_scratch.len() as u32,
        );
        Ok(())
    }

    /// Reads data from shared INPUT_BUF — avoids passArray8ToWasm0 malloc+copy.
    #[wasm_bindgen(js_name = encodeFrameRawFromInputBuf)]
    pub fn encode_frame_raw_from_input_buf(&mut self, len: u32) -> Result<(), JsValue> {
        crate::binary::with_input_buf(|input| {
            let data = &input[..len as usize];
            self.encode_frame_raw_inner(data)?;
            set_result_descriptor(
                self.encode_scratch.as_ptr() as u32,
                self.encode_scratch.len() as u32,
            );
            Ok(())
        })
    }

    #[inline]
    fn encode_frame_raw_inner(&mut self, data: &[u8]) -> Result<(), JsValue> {
        let header = self.intro_header.take();
        if self.is_finished {
            let encrypted = self.encrypt_vec(data)?;
            encode_frame_into(&encrypted, header.as_deref(), &mut self.encode_scratch)
                .map_err(|e| JsValue::from_str(&format!("Frame encoding failed: {}", e)))?;
        } else {
            encode_frame_into(data, header.as_deref(), &mut self.encode_scratch)
                .map_err(|e| JsValue::from_str(&format!("Frame encoding failed: {}", e)))?;
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = encodeFrame)]
    pub fn encode_frame(&mut self, node: EncodingNode) -> Result<Uint8Array, JsValue> {
        let node_ref = js_to_node_ref(&node)?;
        let encoded_bytes = marshal_ref(&node_ref)
            .map_err(|e| JsValue::from_str(&format!("Marshal error: {}", e)))?;

        let encrypted = if self.is_finished {
            self.encrypt_vec(&encoded_bytes)?
        } else {
            encoded_bytes
        };

        let header = self.intro_header.take();
        encode_frame_into(&encrypted, header.as_deref(), &mut self.encode_scratch)
            .map_err(|e| JsValue::from_str(&format!("Frame encoding failed: {}", e)))?;

        let result = Uint8Array::new_with_length(self.encode_scratch.len() as u32);
        result.copy_from(&self.encode_scratch);
        Ok(result)
    }

    #[wasm_bindgen(js_name = decodeFrame)]
    pub fn decode_frame(&mut self, new_data: &[u8]) -> Result<js_sys::Array, JsValue> {
        self.frame_decoder.feed(new_data);
        let decoded_frames = js_sys::Array::new();

        while let Some(frame_data) = self.frame_decoder.decode_frame() {
            if self.is_finished {
                let decrypted_bytes = self.decrypt_vec(&frame_data)?;
                let node = decode_node(decrypted_bytes)?;
                decoded_frames.push(&node.into());
            } else {
                let result = Uint8Array::new_with_length(frame_data.len() as u32);
                result.copy_from(&frame_data);
                decoded_frames.push(&result.into());
            }
        }

        Ok(decoded_frames)
    }

    /// Packed decode path: writes decoded nodes as LNP to DECODE_BUF.
    /// Handshake: returns JS Array of raw Uint8Array frames.
    /// Post-handshake: returns node count (u32); packed nodes in WASM memory.
    #[wasm_bindgen(js_name = decodeFramePacked)]
    pub fn decode_frame_packed(&mut self, new_data: &[u8]) -> Result<JsValue, JsValue> {
        self.frame_decoder.feed(new_data);

        if !self.is_finished {
            let result_array = js_sys::Array::new();
            while let Some(frame_data) = self.frame_decoder.decode_frame() {
                let result = Uint8Array::new_with_length(frame_data.len() as u32);
                result.copy_from(&frame_data);
                result_array.push(&result.into());
            }
            set_result_descriptor(0, 0);
            return Ok(result_array.into());
        }

        // Post-handshake: decrypt + pack each frame directly into DECODE_BUF.
        // Process one frame at a time — no intermediate Vec<Vec<u8>>.
        crate::binary::with_decode_buf(|buf| {
            buf.clear();
            buf.extend_from_slice(&0u16.to_le_bytes()); // count placeholder
            let mut count: u16 = 0;

            while let Some(frame_data) = self.frame_decoder.decode_frame() {
                let decrypted = self.decrypt_vec(&frame_data)?;
                if decrypted.is_empty() {
                    continue;
                }
                let unpacked = unpack(&decrypted).map_err(|e| JsValue::from_str(&e.to_string()))?;
                let node_ref =
                    unmarshal_ref(&unpacked).map_err(|e| JsValue::from_str(&e.to_string()))?;
                crate::binary::write_packed_node(&node_ref, buf);
                count += 1;
            }

            buf[0..2].copy_from_slice(&count.to_le_bytes());
            set_result_descriptor(buf.as_ptr() as u32, buf.len() as u32);
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Packed handshake decode: writes raw frames to DECODE_BUF.
    /// Format: count:u16 + (len:u32 + raw_bytes)*
    /// Avoids js_sys::Array + Uint8Array FFI allocations.
    #[wasm_bindgen(js_name = decodeFrameHandshakePacked)]
    pub fn decode_frame_handshake_packed(&mut self, new_data: &[u8]) {
        self.frame_decoder.feed(new_data);
        self.drain_handshake_frames();
    }

    /// Combined clear + feed + decode for handshake mode.
    /// Saves one FFI call vs separate clearBuffer() + decodeFrameHandshakePacked().
    #[wasm_bindgen(js_name = decodeFrameFreshPacked)]
    pub fn decode_frame_fresh_packed(&mut self, new_data: &[u8]) {
        self.frame_decoder.clear();
        self.frame_decoder.feed(new_data);
        self.drain_handshake_frames();
    }

    /// Feed from shared INPUT_BUF — avoids passArray8ToWasm0 malloc+copy.
    #[wasm_bindgen(js_name = decodeFrameHandshakeFromInputBuf)]
    pub fn decode_frame_handshake_from_input_buf(&mut self, len: u32) {
        crate::binary::with_input_buf(|input| {
            self.frame_decoder.feed(&input[..len as usize]);
        });
        self.drain_handshake_frames();
    }

    #[inline]
    fn drain_handshake_frames(&mut self) {
        crate::binary::with_decode_buf(|buf| {
            buf.clear();
            buf.extend_from_slice(&0u16.to_le_bytes()); // count placeholder
            let mut count: u16 = 0;
            while let Some(frame_data) = self.frame_decoder.decode_frame() {
                buf.extend_from_slice(&(frame_data.len() as u32).to_le_bytes());
                buf.extend_from_slice(&frame_data);
                count += 1;
            }
            buf[0..2].copy_from_slice(&count.to_le_bytes());
            set_result_descriptor(buf.as_ptr() as u32, buf.len() as u32);
        });
    }

    #[wasm_bindgen(getter, js_name = bufferedBytes)]
    pub fn buffered_bytes(&self) -> usize {
        self.frame_decoder.buffered_len()
    }

    #[wasm_bindgen(js_name = clearBuffer)]
    pub fn clear_buffer(&mut self) {
        self.frame_decoder.clear();
    }

    #[wasm_bindgen(js_name = getHash)]
    pub fn get_hash(&self) -> Uint8Array {
        if let Some(ref handshake) = self.handshake {
            let hash = handshake.hash();
            let result = Uint8Array::new_with_length(hash.len() as u32);
            result.copy_from(hash);
            result
        } else {
            Uint8Array::new_with_length(0)
        }
    }

    #[wasm_bindgen(js_name = processHandshakeInit)]
    pub fn process_handshake_init(
        &mut self,
        server_ephemeral: &[u8],
        server_static_encrypted: &[u8],
        server_payload_encrypted: &[u8],
        private_key: &[u8],
    ) -> Result<Uint8Array, JsValue> {
        let handshake = self
            .handshake
            .as_mut()
            .ok_or_else(|| JsValue::from_str("NoiseHandshake not initialized"))?;

        handshake.authenticate(server_ephemeral);

        handshake
            .mix_shared_secret(private_key, server_ephemeral)
            .map_err(|e| JsValue::from_str(&format!("mix_shared_secret failed: {}", e)))?;

        let dec_static = handshake
            .decrypt(server_static_encrypted)
            .map_err(|e| JsValue::from_str(&format!("decrypt static failed: {}", e)))?;

        handshake
            .mix_shared_secret(private_key, &dec_static)
            .map_err(|e| JsValue::from_str(&format!("mix_shared_secret failed: {}", e)))?;

        let cert_payload = handshake
            .decrypt(server_payload_encrypted)
            .map_err(|e| JsValue::from_str(&format!("decrypt payload failed: {}", e)))?;

        let result = Uint8Array::new_with_length(cert_payload.len() as u32);
        result.copy_from(&cert_payload);
        Ok(result)
    }

    #[wasm_bindgen(js_name = processHandshakeFinish)]
    pub fn process_handshake_finish(
        &mut self,
        noise_public_key: &[u8],
        noise_private_key: &[u8],
        server_ephemeral: &[u8],
    ) -> Result<Uint8Array, JsValue> {
        let handshake = self
            .handshake
            .as_mut()
            .ok_or_else(|| JsValue::from_str("NoiseHandshake not initialized"))?;

        let encrypted_key = handshake
            .encrypt(noise_public_key)
            .map_err(|e| JsValue::from_str(&format!("encrypt failed: {}", e)))?;

        handshake
            .mix_shared_secret(noise_private_key, server_ephemeral)
            .map_err(|e| JsValue::from_str(&format!("mix_shared_secret failed: {}", e)))?;

        let result = Uint8Array::new_with_length(encrypted_key.len() as u32);
        result.copy_from(&encrypted_key);
        Ok(result)
    }
}

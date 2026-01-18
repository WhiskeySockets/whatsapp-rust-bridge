use js_sys::Uint8Array;
use wacore_noise::framing::{FrameDecoder, encode_frame_into};
use wacore_noise::{NoiseCipher, NoiseHandshake, build_handshake_header};
use wasm_bindgen::prelude::*;

use crate::binary::{EncodingNode, decode_node, encode_node};
use wacore_binary::consts::NOISE_START_PATTERN as NOISE_MODE;

/// NoiseSession implements the Noise_XX_25519_AESGCM_SHA256 protocol pattern
/// with combined binary encoding/decoding operations for reduced WASM boundary crossings.
///
/// Uses wacore-noise for the core Noise protocol operations:
/// - During handshake: NoiseHandshake handles hash/salt/cipher/counter + DH operations
/// - After handshake: NoiseCipher provides encrypt/decrypt with counter
#[wasm_bindgen]
pub struct NoiseSession {
    // Noise protocol state - during handshake (uses NoiseHandshake for DH support)
    handshake: Option<NoiseHandshake>,

    // Post-handshake ciphers
    enc_cipher: Option<NoiseCipher>,
    dec_cipher: Option<NoiseCipher>,
    read_counter: u32,
    write_counter: u32,
    is_finished: bool,

    // Frame state - intro header is built once using build_handshake_header
    intro_header: Option<Vec<u8>>,
    frame_decoder: FrameDecoder,
    // Scratch buffer for encoding frames (reused across calls)
    encode_scratch: Vec<u8>,
}

#[wasm_bindgen]
impl NoiseSession {
    /// Creates a new NoiseSession with the given public key and configuration.
    ///
    /// # Arguments
    /// * `public_key` - The local ephemeral public key (32 bytes)
    /// * `noise_header` - The noise protocol header bytes (used as prologue)
    /// * `routing_info` - Optional routing information for edge routing
    #[wasm_bindgen(constructor)]
    pub fn new(
        public_key: &[u8],
        noise_header: &[u8],
        routing_info: Option<Vec<u8>>,
    ) -> Result<NoiseSession, JsValue> {
        // Create NoiseHandshake with pattern and header (prologue)
        let mut handshake = NoiseHandshake::new(NOISE_MODE, noise_header)
            .map_err(|e| JsValue::from_str(&format!("NoiseHandshake init failed: {}", e)))?;

        // Authenticate the public key
        handshake.authenticate(public_key);

        // Build intro header using wacore-noise's build_handshake_header
        let (intro_header, _used_edge_routing) = build_handshake_header(routing_info.as_deref());

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

    /// Updates the session hash with the given data.
    /// This is a no-op after the handshake is finished.
    pub fn authenticate(&mut self, data: &[u8]) {
        if let Some(ref mut handshake) = self.handshake {
            handshake.authenticate(data);
        }
    }

    /// Encrypts the plaintext using the current encryption key.
    /// Returns the ciphertext with appended authentication tag.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Uint8Array, JsValue> {
        let ciphertext = if self.is_finished {
            // Use NoiseCipher after handshake
            let cipher = self
                .enc_cipher
                .as_ref()
                .ok_or_else(|| JsValue::from_str("Encryption cipher not initialized"))?;
            let counter = self.write_counter;
            self.write_counter += 1;
            cipher
                .encrypt_with_counter(counter, plaintext)
                .map_err(|e| JsValue::from_str(&format!("Encryption failed: {}", e)))?
        } else {
            // Use NoiseHandshake during handshake
            let handshake = self
                .handshake
                .as_mut()
                .ok_or_else(|| JsValue::from_str("NoiseHandshake not initialized"))?;
            handshake
                .encrypt(plaintext)
                .map_err(|e| JsValue::from_str(&format!("Encryption failed: {}", e)))?
        };

        let result = Uint8Array::new_with_length(ciphertext.len() as u32);
        result.copy_from(&ciphertext);
        Ok(result)
    }

    /// Decrypts the ciphertext using the current decryption key.
    /// The ciphertext should include the authentication tag at the end.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Uint8Array, JsValue> {
        let plaintext = if self.is_finished {
            // Use NoiseCipher after handshake
            let cipher = self
                .dec_cipher
                .as_ref()
                .ok_or_else(|| JsValue::from_str("Decryption cipher not initialized"))?;
            let counter = self.read_counter;
            self.read_counter += 1;
            cipher
                .decrypt_with_counter(counter, ciphertext)
                .map_err(|e| JsValue::from_str(&format!("Decryption failed: {}", e)))?
        } else {
            // Use NoiseHandshake during handshake
            let handshake = self
                .handshake
                .as_mut()
                .ok_or_else(|| JsValue::from_str("NoiseHandshake not initialized"))?;
            handshake
                .decrypt(ciphertext)
                .map_err(|e| JsValue::from_str(&format!("Decryption failed: {}", e)))?
        };

        let result = Uint8Array::new_with_length(plaintext.len() as u32);
        result.copy_from(&plaintext);
        Ok(result)
    }

    /// Mixes new key material into the session using HKDF.
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

    /// Finalizes the handshake and splits keys for bidirectional communication.
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

    /// Returns whether the handshake has been completed.
    #[wasm_bindgen(getter, js_name = isFinished)]
    pub fn is_finished(&self) -> bool {
        self.is_finished
    }

    /// Encodes raw data (without binary node encoding) into a frame.
    /// Used during handshake when data is not a binary node.
    #[wasm_bindgen(js_name = encodeFrameRaw)]
    pub fn encode_frame_raw(&mut self, data: &[u8]) -> Result<Uint8Array, JsValue> {
        // Encrypt if finished
        let encrypted = if self.is_finished {
            let enc = self.encrypt(data)?;
            enc.to_vec()
        } else {
            data.to_vec()
        };

        // Take intro header if not yet sent
        let header = self.intro_header.take();

        // Use shared framing from wacore-noise (reuses encode_scratch buffer)
        encode_frame_into(&encrypted, header.as_deref(), &mut self.encode_scratch)
            .map_err(|e| JsValue::from_str(&format!("Frame encoding failed: {}", e)))?;

        let result = Uint8Array::new_with_length(self.encode_scratch.len() as u32);
        result.copy_from(&self.encode_scratch);
        Ok(result)
    }

    /// Encodes a binary node into an encrypted frame.
    /// This is the combined operation: binary encode + noise encrypt + framing.
    /// Reduces WASM boundary crossings compared to separate operations.
    #[wasm_bindgen(js_name = encodeFrame)]
    pub fn encode_frame(&mut self, node: EncodingNode) -> Result<Uint8Array, JsValue> {
        // Step 1: Encode binary node
        let encoded = encode_node(node)?;
        let encoded_bytes = encoded.to_vec();

        // Step 2 & 3: Encrypt (if finished) and frame
        self.encode_frame_raw(&encoded_bytes)
    }

    /// Decodes incoming data and returns decoded frames as an array.
    /// This is the combined operation: frame parse + noise decrypt + binary decode.
    /// Reduces WASM boundary crossings compared to separate operations.
    ///
    /// Returns an array of decoded frames (BinaryNode after finishInit, Uint8Array during handshake).
    /// The caller should iterate over the returned array and process each frame.
    ///
    /// # Arguments
    /// * `new_data` - New data received from the network
    #[wasm_bindgen(js_name = decodeFrame)]
    pub fn decode_frame(&mut self, new_data: &[u8]) -> Result<js_sys::Array, JsValue> {
        // Feed new data to the frame decoder
        self.frame_decoder.feed(new_data);

        // Collect all decoded frames
        let decoded_frames = js_sys::Array::new();

        // Process complete frames using FrameDecoder from wacore-noise
        while let Some(frame_data) = self.frame_decoder.decode_frame() {
            // Decrypt and decode
            if self.is_finished {
                // Decrypt
                let decrypted = self.decrypt(&frame_data)?;
                let decrypted_bytes = decrypted.to_vec();

                // Decode binary node
                let node = decode_node(decrypted_bytes)?;
                decoded_frames.push(&node.into());
            } else {
                // During handshake, just return the raw frame as Uint8Array
                let result = Uint8Array::new_with_length(frame_data.len() as u32);
                result.copy_from(&frame_data);
                decoded_frames.push(&result.into());
            }
        }

        Ok(decoded_frames)
    }

    /// Returns the number of bytes currently buffered waiting for more data.
    #[wasm_bindgen(getter, js_name = bufferedBytes)]
    pub fn buffered_bytes(&self) -> usize {
        self.frame_decoder.buffered_len()
    }

    /// Clears the internal buffer.
    #[wasm_bindgen(js_name = clearBuffer)]
    pub fn clear_buffer(&mut self) {
        self.frame_decoder.clear();
    }

    /// Returns the current hash value (for debugging/testing).
    #[wasm_bindgen(js_name = getHash)]
    pub fn get_hash(&self) -> Uint8Array {
        if let Some(ref handshake) = self.handshake {
            let hash = handshake.hash();
            let result = Uint8Array::new_with_length(hash.len() as u32);
            result.copy_from(hash);
            result
        } else {
            // After handshake, hash is cleared (matches Baileys behavior)
            Uint8Array::new_with_length(0)
        }
    }

    // ==================== Handshake methods ====================

    /// Process the initial phase of the handshake.
    /// Performs: authenticate(ephemeral) -> mixSharedSecret -> decrypt(static) -> mixSharedSecret -> decrypt(payload)
    ///
    /// This combines 5 operations into a single WASM call, reducing boundary crossings.
    /// Uses NoiseHandshake.mix_shared_secret() which handles DH internally.
    ///
    /// # Arguments
    /// * `server_ephemeral` - Server's ephemeral public key (32 bytes)
    /// * `server_static_encrypted` - Encrypted server static key
    /// * `server_payload_encrypted` - Encrypted certificate payload
    /// * `private_key` - Local private key (32 bytes)
    ///
    /// # Returns
    /// The decrypted certificate payload for JS to decode and verify
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

        // 1. Authenticate server ephemeral key
        handshake.authenticate(server_ephemeral);

        // 2. Mix in shared secret from (our private, server ephemeral) - uses libsignal DH internally
        handshake
            .mix_shared_secret(private_key, server_ephemeral)
            .map_err(|e| JsValue::from_str(&format!("mix_shared_secret failed: {}", e)))?;

        // 3. Decrypt server's static public key
        let dec_static = handshake
            .decrypt(server_static_encrypted)
            .map_err(|e| JsValue::from_str(&format!("decrypt static failed: {}", e)))?;

        // 4. Mix in shared secret from (our private, server static) - uses libsignal DH internally
        handshake
            .mix_shared_secret(private_key, &dec_static)
            .map_err(|e| JsValue::from_str(&format!("mix_shared_secret failed: {}", e)))?;

        // 5. Decrypt certificate payload
        let cert_payload = handshake
            .decrypt(server_payload_encrypted)
            .map_err(|e| JsValue::from_str(&format!("decrypt payload failed: {}", e)))?;

        // Return cert payload for JS to decode protobuf and verify
        let result = Uint8Array::new_with_length(cert_payload.len() as u32);
        result.copy_from(&cert_payload);
        Ok(result)
    }

    /// Finish the handshake after certificate verification.
    /// Performs: encrypt(noiseKey.public) -> mixSharedSecret
    ///
    /// This combines 2 operations into a single WASM call.
    /// Uses NoiseHandshake.mix_shared_secret() which handles DH internally.
    ///
    /// # Arguments
    /// * `noise_public_key` - Local noise public key to encrypt and send
    /// * `noise_private_key` - Local noise private key (32 bytes)
    /// * `server_ephemeral` - Server's ephemeral public key (for final key mixing)
    ///
    /// # Returns
    /// The encrypted public key to send to server
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

        // 7. Encrypt our noise public key
        let encrypted_key = handshake
            .encrypt(noise_public_key)
            .map_err(|e| JsValue::from_str(&format!("encrypt failed: {}", e)))?;

        // 8. Mix in final shared secret from (noise private, server ephemeral) - uses libsignal DH internally
        handshake
            .mix_shared_secret(noise_private_key, server_ephemeral)
            .map_err(|e| JsValue::from_str(&format!("mix_shared_secret failed: {}", e)))?;

        let result = Uint8Array::new_with_length(encrypted_key.len() as u32);
        result.copy_from(&encrypted_key);
        Ok(result)
    }
}

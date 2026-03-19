use prost::Message;
use wasm_bindgen::prelude::*;

use waproto::whatsapp as wa;

/// Encode a WebMessageInfo protobuf from JSON to binary.
///
/// Takes a JSON object matching the WebMessageInfo schema and returns
/// the protobuf-encoded bytes. This replaces `protobufjs`'s encode.
#[wasm_bindgen(js_name = encodeWebMessageInfo)]
pub fn encode_web_message_info(json: JsValue) -> Result<Vec<u8>, JsValue> {
    let msg: wa::WebMessageInfo =
        serde_wasm_bindgen::from_value(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(msg.encode_to_vec())
}

/// Decode a WebMessageInfo protobuf from binary to JSON.
#[wasm_bindgen(js_name = decodeWebMessageInfo)]
pub fn decode_web_message_info(data: &[u8]) -> Result<JsValue, JsValue> {
    let msg = wa::WebMessageInfo::decode(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&msg)
}

/// Encode a Message protobuf from JSON to binary.
///
/// This is the inner message content (text, image, video, etc.).
#[wasm_bindgen(js_name = encodeMessage)]
pub fn encode_message(json: JsValue) -> Result<Vec<u8>, JsValue> {
    let msg: wa::Message =
        serde_wasm_bindgen::from_value(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(msg.encode_to_vec())
}

/// Decode a Message protobuf from binary to JSON.
#[wasm_bindgen(js_name = decodeMessage)]
pub fn decode_message(data: &[u8]) -> Result<JsValue, JsValue> {
    let msg = wa::Message::decode(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&msg)
}

/// Encode a HistorySync protobuf from JSON to binary.
#[wasm_bindgen(js_name = encodeHistorySync)]
pub fn encode_history_sync(json: JsValue) -> Result<Vec<u8>, JsValue> {
    let msg: wa::HistorySync =
        serde_wasm_bindgen::from_value(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(msg.encode_to_vec())
}

/// Decode a HistorySync protobuf from binary to JSON.
#[wasm_bindgen(js_name = decodeHistorySync)]
pub fn decode_history_sync(data: &[u8]) -> Result<JsValue, JsValue> {
    let msg = wa::HistorySync::decode(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&msg)
}

/// Encode a SyncActionData protobuf from JSON to binary.
#[wasm_bindgen(js_name = encodeSyncActionData)]
pub fn encode_sync_action_data(json: JsValue) -> Result<Vec<u8>, JsValue> {
    let msg: wa::SyncActionData =
        serde_wasm_bindgen::from_value(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(msg.encode_to_vec())
}

/// Decode a SyncActionData protobuf from binary to JSON.
#[wasm_bindgen(js_name = decodeSyncActionData)]
pub fn decode_sync_action_data(data: &[u8]) -> Result<JsValue, JsValue> {
    let msg = wa::SyncActionData::decode(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&msg)
}

/// Encode a ClientPayload protobuf from JSON to binary.
#[wasm_bindgen(js_name = encodeClientPayload)]
pub fn encode_client_payload(json: JsValue) -> Result<Vec<u8>, JsValue> {
    let msg: wa::ClientPayload =
        serde_wasm_bindgen::from_value(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(msg.encode_to_vec())
}

/// Decode a ClientPayload protobuf from binary to JSON.
#[wasm_bindgen(js_name = decodeClientPayload)]
pub fn decode_client_payload(data: &[u8]) -> Result<JsValue, JsValue> {
    let msg = wa::ClientPayload::decode(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&msg)
}

/// Encode an AdvSignedDeviceIdentity protobuf from JSON to binary.
#[wasm_bindgen(js_name = encodeAdvSignedDeviceIdentity)]
pub fn encode_adv_signed_device_identity(json: JsValue) -> Result<Vec<u8>, JsValue> {
    let msg: wa::AdvSignedDeviceIdentity =
        serde_wasm_bindgen::from_value(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(msg.encode_to_vec())
}

/// Decode an AdvSignedDeviceIdentity protobuf from binary to JSON.
#[wasm_bindgen(js_name = decodeAdvSignedDeviceIdentity)]
pub fn decode_adv_signed_device_identity(data: &[u8]) -> Result<JsValue, JsValue> {
    let msg =
        wa::AdvSignedDeviceIdentity::decode(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&msg)
}

/// Encode an AdvSignedKeyIndexList protobuf from JSON to binary.
#[wasm_bindgen(js_name = encodeAdvSignedKeyIndexList)]
pub fn encode_adv_signed_key_index_list(json: JsValue) -> Result<Vec<u8>, JsValue> {
    let msg: wa::AdvSignedKeyIndexList =
        serde_wasm_bindgen::from_value(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(msg.encode_to_vec())
}

/// Decode an AdvSignedKeyIndexList protobuf from binary to JSON.
#[wasm_bindgen(js_name = decodeAdvSignedKeyIndexList)]
pub fn decode_adv_signed_key_index_list(data: &[u8]) -> Result<JsValue, JsValue> {
    let msg =
        wa::AdvSignedKeyIndexList::decode(data).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js_value(&msg)
}

/// Serialize a value to JsValue. Uses BigInt for large integer types
/// (u64, i64) to avoid precision loss, unlike protobufjs which truncates to f64.
pub fn to_js_value<T: serde::Serialize>(val: &T) -> Result<JsValue, JsValue> {
    let serializer =
        serde_wasm_bindgen::Serializer::new().serialize_large_number_types_as_bigints(true);
    val.serialize(&serializer)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

macro_rules! proto_types {
    ($($name:literal => $type:ty),* $(,)?) => {
        /// Generic protobuf encode: takes a type name and JSON, returns binary.
        #[wasm_bindgen(js_name = encodeProto)]
        pub fn encode_proto(type_name: &str, json: JsValue) -> Result<Vec<u8>, JsValue> {
            match type_name {
                $($name => {
                    let msg: $type = serde_wasm_bindgen::from_value(json)
                        .map_err(|e| JsValue::from_str(&e.to_string()))?;
                    Ok(msg.encode_to_vec())
                })*
                _ => Err(JsValue::from_str(&format!("unknown proto type: {type_name}")))
            }
        }

        /// Generic protobuf decode: takes a type name and binary, returns JSON.
        #[wasm_bindgen(js_name = decodeProto)]
        pub fn decode_proto(type_name: &str, data: &[u8]) -> Result<JsValue, JsValue> {
            match type_name {
                $($name => {
                    let msg = <$type>::decode(data)
                        .map_err(|e| JsValue::from_str(&e.to_string()))?;
                    to_js_value(&msg)
                })*
                _ => Err(JsValue::from_str(&format!("unknown proto type: {type_name}")))
            }
        }
    }
}

proto_types! {
    "Message" => wa::Message,
    "WebMessageInfo" => wa::WebMessageInfo,
    "HistorySync" => wa::HistorySync,
    "SyncActionData" => wa::SyncActionData,
    "ClientPayload" => wa::ClientPayload,
    "AdvSignedDeviceIdentity" => wa::AdvSignedDeviceIdentity,
    "AdvSignedKeyIndexList" => wa::AdvSignedKeyIndexList,
    "AdvDeviceIdentity" => wa::AdvDeviceIdentity,
    "AdvSignedDeviceIdentityHmac" => wa::AdvSignedDeviceIdentityHmac,
    "HandshakeMessage" => wa::HandshakeMessage,
    "SyncdRecord" => wa::SyncdRecord,
    "SyncdMutation" => wa::SyncdMutation,
    "SyncdMutations" => wa::SyncdMutations,
    "SyncdPatch" => wa::SyncdPatch,
    "SyncdSnapshot" => wa::SyncdSnapshot,
    "ExitCode" => wa::ExitCode,
    "SyncActionValue" => wa::SyncActionValue,
    "DeviceProps" => wa::DeviceProps,
    "SenderKeyDistributionMessage" => wa::SenderKeyDistributionMessage,
    "SenderKeyMessage" => wa::SenderKeyMessage,
    "ServerErrorReceipt" => wa::ServerErrorReceipt,
    "CertChain" => wa::CertChain,
    "CertChain.NoiseCertificate" => wa::cert_chain::NoiseCertificate,
    "CertChain.NoiseCertificate.Details" => wa::cert_chain::noise_certificate::Details,
    "ExternalBlobReference" => wa::ExternalBlobReference,
    "LidMigrationMappingSyncPayload" => wa::LidMigrationMappingSyncPayload,
    "MediaRetryNotification" => wa::MediaRetryNotification,
    "VerifiedNameCertificate" => wa::VerifiedNameCertificate,
    "VerifiedNameCertificate.Details" => wa::verified_name_certificate::Details,
    "Message.PollVoteMessage" => wa::message::PollVoteMessage,
    "Message.EventResponseMessage" => wa::message::EventResponseMessage,
}

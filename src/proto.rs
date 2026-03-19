use prost::Message;
use wasm_bindgen::prelude::*;

use waproto::whatsapp as wa;

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

//! Verify serde-snake-case feature correctly deserializes oneof variants.

#[test]
fn test_native_flow_oneof_deserialization() {
    use waproto::whatsapp as wa;

    // This JSON mimics what to_snake_case_js produces from camelCase JS input
    let json = r#"{
        "interactive_message": {
            "header": { "title": "Test" },
            "body": { "text": "Hello" },
            "native_flow_message": {
                "buttons": [{ "name": "cta_url", "button_params_json": "{}" }],
                "message_version": 1
            }
        }
    }"#;

    let msg: wa::Message = serde_json::from_str(json).unwrap();
    let im = msg
        .interactive_message
        .as_ref()
        .expect("interactive_message should be Some");
    assert!(im.header.is_some(), "header should be set");
    assert!(im.body.is_some(), "body should be set");

    let oneof = im.interactive_message.as_ref();
    assert!(
        oneof.is_some(),
        "oneof should be Some — 'native_flow_message' should match NativeFlowMessage variant"
    );

    match oneof.unwrap() {
        wa::message::interactive_message::InteractiveMessage::NativeFlowMessage(nf) => {
            assert_eq!(nf.buttons.len(), 1);
            assert_eq!(nf.buttons[0].name.as_deref(), Some("cta_url"));
            assert_eq!(nf.buttons[0].button_params_json.as_deref(), Some("{}"));
        }
        other => panic!(
            "Expected NativeFlowMessage, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

#[test]
fn test_document_with_caption_interactive_roundtrip() {
    use prost::Message;
    use waproto::whatsapp as wa;

    let json = r#"{
        "document_with_caption_message": {
            "message": {
                "interactive_message": {
                    "header": { "title": "PIX" },
                    "body": { "text": "Pay here" },
                    "native_flow_message": {
                        "buttons": [{
                            "name": "payment_info",
                            "button_params_json": "{\"currency\":\"BRL\"}"
                        }],
                        "message_version": 1
                    }
                }
            }
        }
    }"#;

    let msg: wa::Message = serde_json::from_str(json).unwrap();

    // Verify structure
    let dwc = msg.document_with_caption_message.as_ref().expect("dwc");
    let inner = dwc.message.as_ref().expect("inner message");
    let im = inner.interactive_message.as_ref().expect("interactive");
    let oneof = im
        .interactive_message
        .as_ref()
        .expect("oneof should be Some");

    match oneof {
        wa::message::interactive_message::InteractiveMessage::NativeFlowMessage(nf) => {
            assert_eq!(nf.buttons[0].name.as_deref(), Some("payment_info"));
        }
        _ => panic!("wrong variant"),
    }

    // Verify prost encode roundtrip
    let bytes = msg.encode_to_vec();
    assert!(
        bytes.len() > 30,
        "should have substantial content, got {} bytes",
        bytes.len()
    );

    let decoded = wa::Message::decode(bytes.as_slice()).unwrap();
    let has_nf = decoded
        .document_with_caption_message
        .and_then(|d| d.message)
        .and_then(|m| m.interactive_message)
        .and_then(|i| i.interactive_message)
        .is_some();
    assert!(has_nf, "roundtrip should preserve nativeFlowMessage");
}

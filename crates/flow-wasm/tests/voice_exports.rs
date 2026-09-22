use flow_core::MAX_VOICE_TRANSCRIPT_BYTES;
use serde_json::{Value, json};

fn request(locale: &str, transcript: &str) -> Value {
    json!({
        "protocolVersion": 1,
        "locale": locale,
        "transcript": transcript,
        "sourceRevision": 9,
        "selection": {
            "anchor": {
                "nodeId": "00000000-0000-4000-8000-000000000901",
                "utf16Offset": 2,
                "affinity": "forward"
            },
            "focus": {
                "nodeId": "00000000-0000-4000-8000-000000000901",
                "utf16Offset": 8,
                "affinity": "forward"
            }
        }
    })
}

fn response(request: Value) -> Value {
    serde_json::from_str(&flow_wasm::resolve_voice_command(request.to_string()))
        .expect("voice response JSON")
}

#[test]
fn resolver_export_returns_versioned_rust_intent_without_transcript() {
    let response = response(request("en-US", "make bold"));
    assert_eq!(response["protocolVersion"], 1);
    assert_eq!(response["ok"], true);
    assert_eq!(response["intent"]["sourceRevision"], 9);
    assert_eq!(response["intent"]["action"]["type"], "setInlineMark");
    assert_eq!(response["intent"]["capability"]["family"], "setInlineMark");
    assert_eq!(response["error"], Value::Null);
    assert!(!response.to_string().contains("make bold"));
}

#[test]
fn resolver_export_reports_unsupported_ambiguous_and_future_requests() {
    let unsupported = response(request("en-US", "do something arbitrary"));
    assert_eq!(unsupported["ok"], false);
    assert_eq!(
        unsupported["error"]["code"],
        "FLOW_VOICE_COMMAND_UNSUPPORTED"
    );

    let ambiguous = response(request("en-US", "format"));
    assert_eq!(ambiguous["error"]["code"], "FLOW_VOICE_COMMAND_AMBIGUOUS");

    let mut future = request("en-US", "undo");
    future["protocolVersion"] = json!(2);
    let future_response = response(future);
    assert_eq!(
        future_response["error"]["code"],
        "FLOW_VOICE_PROTOCOL_UNSUPPORTED"
    );
    assert!(!future_response.to_string().contains("undo"));
}

#[test]
fn resolver_export_fails_closed_for_decode_and_size_limits() {
    let malformed: Value =
        serde_json::from_str(&flow_wasm::resolve_voice_command("not-json".to_owned()))
            .expect("malformed response JSON");
    assert_eq!(malformed["error"]["code"], "FLOW_VOICE_REQUEST_DECODE");

    let mut oversized = request("en-US", "undo");
    oversized["transcript"] = Value::String("x".repeat(MAX_VOICE_TRANSCRIPT_BYTES + 1));
    let transcript_limit = response(oversized);
    assert_eq!(
        transcript_limit["error"]["code"],
        "FLOW_VOICE_TRANSCRIPT_SIZE_LIMIT"
    );

    let request_limit = "x".repeat(16 * 1024 + 1);
    let request_limit_response: Value =
        serde_json::from_str(&flow_wasm::resolve_voice_command(request_limit))
            .expect("size-limit response JSON");
    assert_eq!(
        request_limit_response["error"]["code"],
        "FLOW_VOICE_REQUEST_SIZE_LIMIT"
    );
    assert!(!request_limit_response.to_string().contains('x'));
}

#[test]
fn resolver_export_rejects_unknown_fields_without_echoing_input() {
    let mut invalid = request("uk-UA", "скасувати");
    invalid["unexpected"] = json!("do not admit");
    let response = response(invalid);
    assert_eq!(response["error"]["code"], "FLOW_VOICE_REQUEST_DECODE");
    assert!(!response.to_string().contains("do not admit"));
}

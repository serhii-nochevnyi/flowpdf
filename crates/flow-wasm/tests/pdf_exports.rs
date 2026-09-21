use flow_core::{
    canonical::{canonical_bytes, canonical_hash},
    model::FlowDocument,
};
use serde_json::{Value, json};

fn export_request() -> (String, FlowDocument) {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample document");
    let canonical = canonical_bytes(&document).expect("canonical document");
    let request = json!({
        "protocolVersion": 1,
        "requestId": "pdf-export-test-1",
        "sourceRevision": document.revision,
        "sourceHash": canonical_hash(&canonical),
        "layoutSettingsFingerprint": "layout-settings-v1",
        "layoutResultHash": "layout-result-v1",
        "fontCatalogIdentity": "fonts-v1",
        "fontFaces": [],
        "hyphenationDataIdentity": "uk-v1",
        "canonicalJson": String::from_utf8(canonical).expect("utf8 canonical document"),
        "pages": [{
            "bounds": { "x": 0, "y": 0, "width": 38080, "height": 53888 }
        }],
        "outlines": [],
        "internalLinks": []
    });
    (
        serde_json::to_string(&request).expect("request json"),
        document,
    )
}

fn response_value(serialized: &str) -> Value {
    serde_json::from_str(serialized).expect("protocol response json")
}

#[test]
fn export_boundary_is_bounded_verifiable_and_recoverable() {
    let (request, document) = export_request();
    let first = flow_wasm::export_pdf(request.clone());
    let second = flow_wasm::export_pdf(request);
    assert_eq!(first, second);

    let response = response_value(&first);
    assert_eq!(response["ok"], true);
    assert!(
        !response["result"]["bytesHex"]
            .as_str()
            .unwrap_or_default()
            .is_empty()
    );
    assert!(
        !response["result"]["privateSourceStreamHex"]
            .as_str()
            .unwrap_or_default()
            .is_empty()
    );
    assert!(flow_wasm::verify_pdf_export_response(first.clone()));

    let private_stream = response["result"]["privateSourceStreamHex"]
        .as_str()
        .expect("private source stream")
        .to_owned();
    let recovered_request = json!({
        "protocolVersion": 1,
        "requestId": "pdf-recovery-test-1",
        "privateSourceStreamHex": private_stream,
        "expected": {}
    });
    let recovered = response_value(&flow_wasm::recover_owned_source(
        serde_json::to_string(&recovered_request).unwrap(),
    ));
    assert_eq!(recovered["ok"], true);
    assert_eq!(recovered["result"]["exact"], true);
    assert_eq!(
        recovered["result"]["documentId"],
        document.document_id.to_string()
    );
}

#[test]
fn export_and_recovery_reject_unknown_or_missing_payloads_without_partial_results() {
    let (request, _) = export_request();
    let mut unknown: Value = serde_json::from_str(&request).unwrap();
    unknown["unexpected"] = Value::String("not admitted".to_owned());
    let unknown_response = response_value(&flow_wasm::export_pdf(
        serde_json::to_string(&unknown).unwrap(),
    ));
    assert_eq!(unknown_response["ok"], false);
    assert_eq!(
        unknown_response["error"]["code"],
        "FLOW_PDF_EXPORT_REQUEST_DECODE"
    );
    assert!(unknown_response["result"].is_null());

    let missing = json!({
        "protocolVersion": 1,
        "requestId": "pdf-recovery-missing",
        "privateSourceStreamHex": null,
        "expected": {}
    });
    let missing_response = response_value(&flow_wasm::recover_owned_source(
        serde_json::to_string(&missing).unwrap(),
    ));
    assert_eq!(missing_response["ok"], false);
    assert_eq!(
        missing_response["error"]["code"],
        "FLOW_PDF_RECOVERY_PAYLOAD_MISSING"
    );
    assert!(missing_response["result"].is_null());
}

use flow_core::{
    LAYOUT_WASM_SCHEMA_VERSION, LayoutWasmRequest, LayoutWasmResponse, LayoutWasmViewport,
    MAX_LAYOUT_WASM_REQUEST_BYTES,
    canonical::canonical_bytes,
    layout::{FontCatalog, FontFace},
    model::{ContentNode, FlowDocument, NodeId},
};

const NOTO_SANS: &[u8] = include_bytes!("../../flow-core/data/NotoSans-Regular.ttf");

fn request() -> LayoutWasmRequest {
    let mut document = FlowDocument::deterministic_sample("en-US").expect("sample");
    let style_id = document.styles.first().map(|style| style.id.clone());
    document.content = vec![ContentNode::paragraph(
        NodeId::new("00000000-0000-4000-8000-000000000901").unwrap(),
        style_id,
        "Boundary secret must stay out of the derived response.".to_owned(),
    )];
    document.assets.clear();
    document.fields.clear();
    let canonical = canonical_bytes(&document).unwrap();
    let font = FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).unwrap();
    let catalog = FontCatalog::new(vec![font]).unwrap();

    LayoutWasmRequest {
        schema_version: LAYOUT_WASM_SCHEMA_VERSION,
        request_id: "layout-test-1".to_owned(),
        canonical_json: String::from_utf8(canonical.clone()).unwrap(),
        source_revision: document.revision,
        source_hash: flow_core::canonical::canonical_hash(&canonical),
        max_pages: 32,
        viewport: LayoutWasmViewport {
            first_page: 0,
            page_count: 4,
        },
        font_catalog_identity: catalog.identity().to_owned(),
        hyphenation_data_identity: None,
        expected_layout_settings_fingerprint: None,
        fonts: vec![flow_core::LayoutWasmFontInput {
            id: "noto-sans".to_owned(),
            family: "Noto Sans".to_owned(),
            face_index: 0,
            bytes: NOTO_SANS.to_vec(),
        }],
        ukrainian_hyphenation: None,
    }
}

fn response(input: &str) -> LayoutWasmResponse {
    serde_json::from_str(&flow_wasm::layout_document(input.to_owned())).unwrap()
}

#[test]
fn layout_export_is_closed_deterministic_and_does_not_leak_authored_payloads() {
    let input = serde_json::to_string(&request()).unwrap();
    let first = response(&input);
    let second = response(&input);

    assert!(first.ok, "unexpected layout error: {:?}", first.error);
    assert_eq!(first, second);
    assert!(first.result.is_some());
    let serialized = flow_wasm::layout_document(input);
    assert!(!serialized.contains("Boundary secret"));
    assert!(!serialized.contains("canonicalJson"));
    assert!(!serialized.contains("\"fonts\""));

    let valid_response = flow_wasm::layout_document(serde_json::to_string(&request()).unwrap());
    assert!(flow_wasm::verify_layout_response(valid_response));
    let mut forged: serde_json::Value = serde_json::from_str(&flow_wasm::layout_document(
        serde_json::to_string(&request()).unwrap(),
    ))
    .unwrap();
    forged["resultHash"] = serde_json::Value::String("forged".to_owned());
    assert!(!flow_wasm::verify_layout_response(
        serde_json::to_string(&forged).unwrap()
    ));
}

#[test]
fn layout_export_rejects_unknown_and_future_contracts_without_partial_state() {
    let mut unknown: serde_json::Value = serde_json::to_value(request()).unwrap();
    unknown["unexpected"] = serde_json::Value::String("not admitted".to_owned());
    let unknown_response = response(&serde_json::to_string(&unknown).unwrap());
    assert_eq!(
        unknown_response.error.unwrap().code,
        "FLOW_LAYOUT_WASM_REQUEST_DECODE"
    );
    assert!(!unknown_response.ok);
    assert!(unknown_response.result.is_none());

    let mut future: serde_json::Value = serde_json::to_value(request()).unwrap();
    future["schemaVersion"] = serde_json::Value::from(LAYOUT_WASM_SCHEMA_VERSION + 1);
    let future_response = response(&serde_json::to_string(&future).unwrap());
    assert_eq!(
        future_response.error.unwrap().code,
        "FLOW_LAYOUT_WASM_VERSION_UNSUPPORTED"
    );
    assert!(future_response.result.is_none());
}

#[test]
fn layout_export_rejects_identity_and_revision_mismatches() {
    let mut identity: serde_json::Value = serde_json::to_value(request()).unwrap();
    identity["fontCatalogIdentity"] = serde_json::Value::String("forged".to_owned());
    let identity_response = response(&serde_json::to_string(&identity).unwrap());
    assert_eq!(
        identity_response.error.unwrap().code,
        "FLOW_LAYOUT_WASM_IDENTITY_MISMATCH"
    );

    let mut stale: serde_json::Value = serde_json::to_value(request()).unwrap();
    stale["sourceHash"] = serde_json::Value::String("flowpdf:blake3:v1:stale".to_owned());
    let stale_response = response(&serde_json::to_string(&stale).unwrap());
    assert_eq!(
        stale_response.error.unwrap().code,
        "FLOW_LAYOUT_PAGINATION_REQUEST_INVALID"
    );
}

#[test]
fn layout_export_rejects_oversized_serialized_requests() {
    let oversized = " ".repeat(MAX_LAYOUT_WASM_REQUEST_BYTES + 1);
    let result = response(&oversized);
    assert_eq!(
        result.error.unwrap().code,
        "FLOW_LAYOUT_WASM_REQUEST_SIZE_LIMIT"
    );
    assert!(!result.ok);
    assert!(result.result.is_none());
}

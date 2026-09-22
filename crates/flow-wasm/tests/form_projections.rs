use flow_core::{
    FORM_PROJECTION_WASM_SCHEMA_VERSION, FormProjectionWasmRequest, FormSessionState,
    LayoutWasmFontInput, LayoutWasmRequest, LayoutWasmViewport,
    canonical::canonical_bytes,
    canonical::canonical_hash,
    layout::{FontCatalog, FontFace},
    model::{
        Affinity, ContentNode, FieldAnchorState, FieldValue, FlowDocument, LogicalPosition, NodeId,
    },
};
use serde_json::{Value, json};

const NOTO_SANS: &[u8] = include_bytes!("../../flow-core/data/NotoSans-Regular.ttf");

fn node_id(value: u32) -> NodeId {
    NodeId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("valid node id")
}

fn form_document() -> FlowDocument {
    let mut document = FlowDocument::deterministic_sample("en-US").expect("sample document");
    let node = node_id(9_901);
    let style_id = document.styles.first().map(|style| style.id.clone());
    document.content = vec![ContentNode::paragraph(
        node.clone(),
        style_id,
        "Hello form fields for the projection boundary.".to_owned(),
    )];
    document.assets.clear();
    for field in &mut document.fields {
        field.anchor = FieldAnchorState::GraphemeSafe {
            original: LogicalPosition {
                node_id: node.clone(),
                utf16_offset: 6.into(),
                affinity: Affinity::Forward,
            },
        };
    }
    document
}

fn layout_request(document: &FlowDocument, request_id: &str) -> LayoutWasmRequest {
    let canonical = canonical_bytes(document).expect("canonical document");
    let catalog = FontCatalog::new(vec![
        FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).expect("font face"),
    ])
    .expect("font catalog");
    LayoutWasmRequest {
        schema_version: flow_core::LAYOUT_WASM_SCHEMA_VERSION,
        request_id: request_id.to_owned(),
        canonical_json: String::from_utf8(canonical.clone()).expect("canonical utf8"),
        source_revision: document.revision,
        source_hash: canonical_hash(&canonical),
        max_pages: 32,
        viewport: LayoutWasmViewport {
            first_page: 0,
            page_count: 4,
        },
        font_catalog_identity: catalog.identity().to_owned(),
        hyphenation_data_identity: None,
        expected_layout_settings_fingerprint: None,
        fonts: vec![LayoutWasmFontInput {
            id: "noto-sans".to_owned(),
            family: "Noto Sans".to_owned(),
            face_index: 0,
            bytes: NOTO_SANS.to_vec(),
        }],
        ukrainian_hyphenation: None,
    }
}

fn projection_request(
    document: &FlowDocument,
    request_id: &str,
    session: Option<FormSessionState>,
) -> String {
    serde_json::to_string(&FormProjectionWasmRequest {
        schema_version: FORM_PROJECTION_WASM_SCHEMA_VERSION,
        request_id: request_id.to_owned(),
        layout_request_json: serde_json::to_string(&layout_request(
            document,
            &format!("layout-{request_id}"),
        ))
        .expect("layout request json"),
        session,
    })
    .expect("projection request json")
}

fn response_value(serialized: &str) -> Value {
    serde_json::from_str(serialized).expect("projection response json")
}

#[test]
fn projection_boundary_is_deterministic_session_aware_and_plan_bound() {
    let document = form_document();
    let source_hash = canonical_hash(&canonical_bytes(&document).expect("canonical document"));
    let default_request = projection_request(&document, "form-projection-default", None);
    let first = flow_wasm::project_form_widgets(default_request.clone());
    let second = flow_wasm::project_form_widgets(default_request);
    assert_eq!(first, second);
    assert!(flow_wasm::verify_form_projection_response(first.clone()));
    let default_response = response_value(&first);
    assert_eq!(default_response["ok"], true);
    assert_eq!(default_response["result"]["sourceHash"], source_hash);
    assert_eq!(
        default_response["result"]["projection"]["widgets"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert!(default_response["result"]["formPlan"].is_object());

    let text_field = document
        .fields
        .iter()
        .find(|field| matches!(field.kind, flow_core::model::FieldKind::Text { .. }))
        .expect("sample text field");
    let session = FormSessionState::from_document(&document)
        .expect("session")
        .set_value(&document, &text_field.id, FieldValue::text("Alice"))
        .expect("session value");
    let filled = response_value(&flow_wasm::project_form_widgets(projection_request(
        &document,
        "form-projection-filled",
        Some(session),
    )));
    assert_eq!(filled["ok"], true);
    assert_eq!(filled["result"]["sourceRevision"], document.revision);
    assert_eq!(
        filled["result"]["projection"]["widgets"][0]["value"]["value"],
        "Alice"
    );
    assert_eq!(
        filled["result"]["formPlan"]["fields"][0]["value"]["text"]["value"],
        "Alice"
    );
}

#[test]
fn review_and_integrity_failures_are_closed_without_partial_results() {
    let mut review_document = form_document();
    let original = review_document.fields[0].anchor.original().clone();
    review_document.fields[0].anchor = FieldAnchorState::LegacyInvalid {
        original,
        reason: flow_core::model::LegacyAnchorReason::MissingNode,
    };
    let review_serialized = flow_wasm::project_form_widgets(projection_request(
        &review_document,
        "form-projection-review",
        None,
    ));
    assert!(flow_wasm::verify_form_projection_response(
        review_serialized.clone()
    ));
    let review = response_value(&review_serialized);
    assert_eq!(review["ok"], true);
    assert_eq!(review["result"]["formPlan"], Value::Null);
    assert_eq!(
        review["result"]["projection"]["review"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let mut unknown: Value = serde_json::from_str(&projection_request(
        &form_document(),
        "form-projection-unknown",
        None,
    ))
    .expect("request value");
    unknown["unexpected"] = Value::Bool(true);
    let unknown_response = response_value(&flow_wasm::project_form_widgets(
        serde_json::to_string(&unknown).expect("unknown request json"),
    ));
    assert_eq!(unknown_response["ok"], false);
    assert_eq!(unknown_response["result"], Value::Null);

    let mut nested_mismatch: Value = serde_json::from_str(&projection_request(
        &form_document(),
        "form-projection-nested-mismatch",
        None,
    ))
    .expect("nested request value");
    let mut nested_layout: Value = serde_json::from_str(
        nested_mismatch["layoutRequestJson"]
            .as_str()
            .expect("nested layout request"),
    )
    .expect("nested layout value");
    nested_layout["sourceHash"] = json!("forged");
    nested_mismatch["layoutRequestJson"] =
        json!(serde_json::to_string(&nested_layout).expect("nested layout json"));
    let nested_response = response_value(&flow_wasm::project_form_widgets(
        serde_json::to_string(&nested_mismatch).expect("nested mismatch request json"),
    ));
    assert_eq!(nested_response["ok"], false);
    assert_eq!(nested_response["result"], Value::Null);

    let document = form_document();
    let session = FormSessionState::from_document(&document).expect("session");
    let mut stale_document = document.clone();
    stale_document.revision += 1;
    let stale_response = response_value(&flow_wasm::project_form_widgets(projection_request(
        &stale_document,
        "form-projection-stale-session",
        Some(session),
    )));
    assert_eq!(stale_response["ok"], false);
    assert_eq!(stale_response["result"], Value::Null);
    assert_eq!(
        stale_response["error"]["code"],
        "FLOW_FORM_SESSION_REVISION_STALE"
    );

    let valid = flow_wasm::project_form_widgets(projection_request(
        &document,
        "form-projection-forge",
        None,
    ));
    let mut forged = response_value(&valid);
    forged["resultHash"] = json!("forged");
    assert!(!flow_wasm::verify_form_projection_response(
        serde_json::to_string(&forged).expect("forged response json"),
    ));
}

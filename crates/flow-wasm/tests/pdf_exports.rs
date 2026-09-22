use flow_core::{
    PdfFormPlan, build_display_list, build_pdf_form_plan,
    canonical::{canonical_bytes, canonical_hash},
    layout::{FontCatalog, FontFace, PaginationRequest},
    model::{Affinity, ContentNode, FieldAnchorState, FlowDocument, LogicalPosition, NodeId},
    paginate_document, resolve_form_widgets,
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
        "Hello form fields for the owned PDF envelope.".to_owned(),
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

fn form_plan(document: &FlowDocument) -> PdfFormPlan {
    let catalog = FontCatalog::new(vec![
        FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).expect("font face"),
    ])
    .expect("font catalog");
    let request = PaginationRequest::for_document(document).expect("pagination request");
    let pagination = paginate_document(document, &request, &catalog, None).expect("pagination");
    let display = build_display_list(document, &pagination, &catalog, None).expect("display list");
    let projection = resolve_form_widgets(document, &display).expect("form projection");
    build_pdf_form_plan(document, &projection).expect("form plan")
}

fn form_export_request() -> (String, FlowDocument, PdfFormPlan) {
    let document = form_document();
    let plan = form_plan(&document);
    let canonical = canonical_bytes(&document).expect("canonical document");
    let selected = plan.fields[0].field_id.as_str().to_owned();
    let request = json!({
        "protocolVersion": 1,
        "requestId": "pdf-form-flatten-test-1",
        "sourceRevision": document.revision,
        "sourceHash": canonical_hash(&canonical),
        "layoutSettingsFingerprint": "layout-settings-v1",
        "layoutResultHash": plan.display_list_hash,
        "fontCatalogIdentity": "fonts-v1",
        "fontFaces": [],
        "hyphenationDataIdentity": null,
        "canonicalJson": String::from_utf8(canonical).expect("canonical utf8"),
        "pages": [{
            "bounds": { "x": 0, "y": 0, "width": 38080, "height": 53888 }
        }],
        "outlines": [],
        "internalLinks": [],
        "formPlan": serde_json::to_value(&plan).expect("form plan json"),
        "flattenedFieldIds": [selected]
    });
    (
        serde_json::to_string(&request).expect("request json"),
        document,
        plan,
    )
}

fn decode_hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).expect("hex high");
            let low = (pair[1] as char).to_digit(16).expect("hex low");
            ((high << 4) | low) as u8
        })
        .collect()
}

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

#[test]
fn form_flattening_crosses_the_wasm_boundary_and_recovers_source() {
    let (request, document, plan) = form_export_request();
    let first = flow_wasm::export_pdf(request.clone());
    let second = flow_wasm::export_pdf(request.clone());
    assert_eq!(first, second);

    let response = response_value(&first);
    assert_eq!(response["ok"], true);
    assert!(flow_wasm::verify_pdf_export_response(first.clone()));
    let partial_bytes = decode_hex(response["result"]["bytesHex"].as_str().expect("pdf bytes"));
    let partial_pdf = String::from_utf8_lossy(&partial_bytes);
    assert!(partial_pdf.contains("/AcroForm"));
    assert_eq!(partial_pdf.matches("/AP").count(), 5);
    assert!(partial_pdf.contains("1 0 0 1"));
    assert_eq!(
        response["result"]["manifest"]["options"]["flattenedFieldIds"][0],
        plan.fields[0].field_id.as_str()
    );

    let private_stream = response["result"]["privateSourceStreamHex"]
        .as_str()
        .expect("private source stream");
    let recovered_request = json!({
        "protocolVersion": 1,
        "requestId": "pdf-form-recovery-test-1",
        "privateSourceStreamHex": private_stream,
        "expected": {}
    });
    let recovered = response_value(&flow_wasm::recover_owned_source(
        serde_json::to_string(&recovered_request).expect("recovery json"),
    ));
    assert_eq!(recovered["ok"], true);
    assert_eq!(recovered["result"]["exact"], true);
    assert_eq!(
        recovered["result"]["canonicalHash"],
        canonical_hash(&canonical_bytes(&document).expect("canonical bytes"))
    );

    let mut all_request: Value = serde_json::from_str(&request).expect("form request json");
    all_request["requestId"] = Value::String("pdf-form-flatten-all".to_owned());
    all_request["flattenedFieldIds"] = json!(
        plan.fields
            .iter()
            .map(|field| field.field_id.as_str())
            .rev()
            .collect::<Vec<_>>()
    );
    let all_response = response_value(&flow_wasm::export_pdf(
        serde_json::to_string(&all_request).expect("all form request json"),
    ));
    assert_eq!(all_response["ok"], true);
    let all_bytes = decode_hex(
        all_response["result"]["bytesHex"]
            .as_str()
            .expect("all pdf bytes"),
    );
    let all_pdf = String::from_utf8_lossy(&all_bytes);
    assert!(!all_pdf.contains("/AcroForm"));
    assert!(!all_pdf.contains("/Widget"));
    assert!(!all_pdf.contains("/AP"));
    assert!(all_pdf.contains("/Helv"));

    let mut unknown: Value = serde_json::from_str(&request).expect("form request json");
    unknown["flattenedFieldIds"] = json!(["missing-field"]);
    let unknown_response = response_value(&flow_wasm::export_pdf(
        serde_json::to_string(&unknown).expect("unknown form request json"),
    ));
    assert_eq!(unknown_response["ok"], false);
    assert_eq!(
        unknown_response["error"]["code"],
        "FLOW_PDF_FORM_FLATTENING_INVALID"
    );
    assert!(unknown_response["result"].is_null());

    let mut duplicate: Value = serde_json::from_str(&request).expect("form request json");
    duplicate["flattenedFieldIds"] = json!([
        plan.fields[0].field_id.as_str(),
        plan.fields[0].field_id.as_str()
    ]);
    let duplicate_response = response_value(&flow_wasm::export_pdf(
        serde_json::to_string(&duplicate).expect("duplicate form request json"),
    ));
    assert_eq!(duplicate_response["ok"], false);
    assert_eq!(
        duplicate_response["error"]["code"],
        "FLOW_PDF_FORM_FLATTENING_INVALID"
    );

    let (bare_request, _) = export_request();
    let mut planless: Value = serde_json::from_str(&bare_request).expect("bare request json");
    planless["flattenedFieldIds"] = json!(["missing-field"]);
    let planless_response = response_value(&flow_wasm::export_pdf(
        serde_json::to_string(&planless).expect("planless request json"),
    ));
    assert_eq!(planless_response["ok"], false);
    assert_eq!(
        planless_response["error"]["code"],
        "FLOW_PDF_FORM_FLATTENING_REQUIRES_PLAN"
    );
    assert!(planless_response["result"].is_null());
}

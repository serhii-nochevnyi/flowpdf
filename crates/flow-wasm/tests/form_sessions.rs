use flow_core::{
    ApiResponse, FORM_SESSION_PROTOCOL_VERSION, FormSessionAction, FormSessionRequest,
    FormSessionResponse,
    canonical::canonical_bytes,
    model::{FieldValue, FlowDocument},
};
use serde_json::{Value, json};

fn request(
    document: &FlowDocument,
    session: Option<FormSessionResponse>,
    action: FormSessionAction,
) -> String {
    let session = session.map(|response| response.session);
    serde_json::to_string(&FormSessionRequest {
        protocol_version: FORM_SESSION_PROTOCOL_VERSION,
        canonical_json: String::from_utf8(canonical_bytes(document).expect("canonical bytes"))
            .expect("canonical UTF-8"),
        session,
        action,
    })
    .expect("request JSON")
}

fn response(serialized: &str) -> ApiResponse<FormSessionResponse> {
    serde_json::from_str(serialized).expect("form session response JSON")
}

#[test]
fn string_boundary_accepts_start_set_and_clear_without_canonical_mutation() {
    let document = FlowDocument::deterministic_sample("en-US").expect("sample document");
    let start = response(&flow_wasm::apply_form_session_json(request(
        &document,
        None,
        FormSessionAction::Start,
    )));
    assert!(start.ok);
    let started = start.value.expect("started response");
    let field_id = document.fields[0].id.clone();

    let filled = response(&flow_wasm::apply_form_session_json(request(
        &document,
        Some(started),
        FormSessionAction::SetValue {
            field_id: field_id.clone(),
            value: FieldValue::text("Alice"),
        },
    )));
    assert!(filled.ok);
    let filled_response = filled.value.expect("filled response");
    assert_eq!(filled_response.session.generation, 1);
    assert_eq!(document.revision, 1);

    let cleared = response(&flow_wasm::apply_form_session_json(request(
        &document,
        Some(filled_response),
        FormSessionAction::ClearValue { field_id },
    )));
    assert!(cleared.ok);
    let cleared_response = cleared.value.expect("cleared response");
    assert_eq!(cleared_response.session.generation, 2);
    assert!(cleared_response.session.overrides.is_empty());
}

#[test]
fn string_boundary_rejects_unknown_fields_stale_sessions_and_malformed_json() {
    let document = FlowDocument::deterministic_sample("en-US").expect("sample document");
    let start = response(&flow_wasm::apply_form_session_json(request(
        &document,
        None,
        FormSessionAction::Start,
    )));
    let mut session = start.value.expect("started response");
    session.session.source_revision += 1;
    let stale = response(&flow_wasm::apply_form_session_json(request(
        &document,
        Some(session),
        FormSessionAction::Validate,
    )));
    assert!(!stale.ok);
    assert_eq!(
        stale.error.expect("stale error").code,
        "FLOW_FORM_SESSION_REVISION_STALE"
    );

    let malformed: Value =
        serde_json::from_str(&flow_wasm::apply_form_session_json("{}".to_owned()))
            .expect("malformed response JSON");
    assert_eq!(malformed["ok"], json!(false));
    assert_eq!(malformed["error"]["code"], json!("FLOW_DECODE_ERROR"));
}

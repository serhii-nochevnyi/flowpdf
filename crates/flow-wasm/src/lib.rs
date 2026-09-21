//! Narrow serialized WebAssembly boundary for FlowPDF.

#![forbid(unsafe_code)]

use flow_core::{
    ApiResponse, ApplyCommandRequest, AssetStageRequest, AssetStageResponse, AuditedRecoverRequest,
    AuditedRecoverResult, CommandKind, CreateSampleRequest, EditorSessionRequest,
    EditorSessionResponse, EditorViewDto, EditorViewRequest, MigrateDocumentRequest,
    MigrateDocumentResult, OperationResult, PlanPersistenceCommitRequest,
    PlanStandaloneAuditRequest, RecoverRequest, RecoverResult, store::PlannedPersistenceCommit,
};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

const SUPPORTED_OLDER_FIXTURE: &str = include_str!("../../../fixtures/flowdoc/older.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OpenDocumentRequest {
    fixture: OpenFixture,
    migration_id: flow_core::model::CommandId,
    issued_at: String,
    assets: Vec<flow_core::store::AssetRecord>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum OpenFixture {
    SupportedOlder,
}

#[wasm_bindgen]
pub fn create_sample(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<CreateSampleRequest>(request) {
        Ok(request) => flow_core::create_sample(request),
        Err(_) => flow_core::decode_failure::<OperationResult>(),
    };
    serialize_response(&response)
}

#[wasm_bindgen]
pub fn apply_command(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<ApplyCommandRequest>(request) {
        Ok(request) => flow_core::apply_command(request),
        Err(_) => flow_core::decode_failure::<OperationResult>(),
    };
    serialize_response(&response)
}

/// Binary-only ingress for bounded PNG/JPEG staging. The semantic command
/// path receives only the opaque receipt returned by this function.
#[wasm_bindgen]
pub fn stage_asset(bytes: &[u8], request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<AssetStageRequest>(request) {
        Ok(request) => flow_core::stage_asset(request, bytes.to_vec()),
        Err(_) => flow_core::decode_failure::<AssetStageResponse>(),
    };
    serialize_response(&response)
}

/// Applies one immutable, revision-bound editor-session action. The response
/// contains no persistence commit and never exposes a mutable Rust handle.
#[wasm_bindgen]
pub fn apply_editor_session(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<EditorSessionRequest>(request) {
        Ok(request) => flow_core::apply_editor_session(request),
        Err(_) => flow_core::decode_failure::<EditorSessionResponse>(),
    };
    serialize_response(&response)
}

/// Revalidates and returns the immutable Rust-owned editor view projection.
#[wasm_bindgen]
pub fn query_editor_view(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<EditorViewRequest>(request) {
        Ok(request) => flow_core::query_editor_view(request),
        Err(_) => flow_core::decode_failure::<EditorViewDto>(),
    };
    serialize_response(&response)
}

/// Applies an undo through the same typed command service as every other
/// modality while rejecting a mismatched command DTO at the boundary.
#[wasm_bindgen]
pub fn undo(request: JsValue) -> JsValue {
    apply_history_command(request, HistoryCommand::Undo)
}

/// Applies a redo through the same typed command service as every other
/// modality while rejecting a mismatched command DTO at the boundary.
#[wasm_bindgen]
pub fn redo(request: JsValue) -> JsValue {
    apply_history_command(request, HistoryCommand::Redo)
}

#[wasm_bindgen]
pub fn migrate_document(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<MigrateDocumentRequest>(request) {
        Ok(request) => flow_core::migrate_document(request),
        Err(_) => flow_core::decode_failure::<MigrateDocumentResult>(),
    };
    serialize_response(&response)
}

/// Opens supported canonical bytes through Rust's schema registry. The browser
/// supplies bytes and assets but never performs a migration itself.
#[wasm_bindgen]
pub fn open_document(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<OpenDocumentRequest>(request) {
        Ok(OpenDocumentRequest {
            fixture: OpenFixture::SupportedOlder,
            migration_id,
            issued_at,
            assets,
        }) => flow_core::migrate_document(MigrateDocumentRequest {
            canonical_json: SUPPORTED_OLDER_FIXTURE
                .strip_suffix('\n')
                .unwrap_or(SUPPORTED_OLDER_FIXTURE)
                .to_owned(),
            migration_id,
            issued_at,
            assets,
        }),
        Err(_) => flow_core::decode_failure::<MigrateDocumentResult>(),
    };
    serialize_response(&response)
}

#[wasm_bindgen]
pub fn plan_persistence_commit(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<PlanPersistenceCommitRequest>(request) {
        Ok(request) => flow_core::plan_persistence_commit(request),
        Err(_) => flow_core::decode_failure::<PlannedPersistenceCommit>(),
    };
    serialize_response(&response)
}

/// Returns the Rust-owned physical record plan used by storage adapters.
#[wasm_bindgen]
pub fn commit_record(request: JsValue) -> JsValue {
    plan_persistence_commit(request)
}

/// Authorizes a core-produced audit-only write without exposing document
/// mutation or trusting the browser to reproduce audit validation rules.
#[wasm_bindgen]
pub fn plan_standalone_audit(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<PlanStandaloneAuditRequest>(request) {
        Ok(request) => flow_core::plan_standalone_audit(request),
        Err(_) => flow_core::decode_failure::<flow_core::AuditRecord>(),
    };
    serialize_response(&response)
}

#[wasm_bindgen]
pub fn recover_document(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<RecoverRequest>(request) {
        Ok(request) => flow_core::recover(request),
        Err(_) => flow_core::decode_failure::<RecoverResult>(),
    };
    serialize_response(&response)
}

/// Queries the exact verified durable view without exposing a mutable core
/// handle to JavaScript.
#[wasm_bindgen]
pub fn query_document(request: JsValue) -> JsValue {
    recover_document(request)
}

#[wasm_bindgen]
pub fn recover_document_audited(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<AuditedRecoverRequest>(request) {
        Ok(request) => flow_core::recover_audited(request),
        Err(_) => flow_core::decode_failure::<AuditedRecoverResult>(),
    };
    serialize_response(&response)
}

/// Runs one bounded, immutable layout request. The browser/worker exchanges
/// JSON only; Rust owns canonical decoding, font/data admission, pagination,
/// diagnostics, and result hashing. No mutable layout handle or raw font
/// bytes are returned to JavaScript.
#[wasm_bindgen]
pub fn layout_document(request_json: String) -> String {
    flow_core::layout_wasm_response_json(&request_json)
}

/// Verifies the Rust-owned result hash and response envelope before a worker
/// may publish a layout result.
#[wasm_bindgen]
pub fn verify_layout_response(response_json: String) -> bool {
    flow_core::verify_layout_wasm_response_json(&response_json)
}

#[derive(Clone, Copy)]
enum HistoryCommand {
    Undo,
    Redo,
}

fn apply_history_command(request: JsValue, expected: HistoryCommand) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<ApplyCommandRequest>(request) {
        Ok(request)
            if matches!(
                (&request.command.kind, expected),
                (CommandKind::Undo, HistoryCommand::Undo)
                    | (CommandKind::Redo, HistoryCommand::Redo)
            ) =>
        {
            flow_core::apply_command(request)
        }
        Ok(_) | Err(_) => flow_core::decode_failure::<OperationResult>(),
    };
    serialize_response(&response)
}

fn serialize_response<T: serde::Serialize>(response: &ApiResponse<T>) -> JsValue {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    serde::Serialize::serialize(response, &serializer)
        .expect("serializing a fixed response DTO cannot fail")
}

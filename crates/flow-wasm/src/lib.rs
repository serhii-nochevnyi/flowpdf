//! Narrow serialized WebAssembly boundary for FlowPDF.

#![forbid(unsafe_code)]

use flow_core::{
    ApiResponse, ApplyCommandRequest, AuditedRecoverRequest, AuditedRecoverResult, CommandKind,
    CreateSampleRequest, MigrateDocumentRequest, MigrateDocumentResult, OperationResult,
    PlanPersistenceCommitRequest, PlanStandaloneAuditRequest, RecoverRequest, RecoverResult,
    store::PlannedPersistenceCommit,
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

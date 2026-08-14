//! Narrow serialized WebAssembly boundary for FlowPDF.

#![forbid(unsafe_code)]

use flow_core::{
    ApiResponse, ApplyCommandRequest, AuditedRecoverRequest, AuditedRecoverResult,
    CreateSampleRequest, MigrateDocumentRequest, MigrateDocumentResult, OperationResult,
    PlanPersistenceCommitRequest, RecoverRequest, RecoverResult, store::PlannedPersistenceCommit,
};
use wasm_bindgen::prelude::*;

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

#[wasm_bindgen]
pub fn migrate_document(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<MigrateDocumentRequest>(request) {
        Ok(request) => flow_core::migrate_document(request),
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

#[wasm_bindgen]
pub fn recover_document(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<RecoverRequest>(request) {
        Ok(request) => flow_core::recover(request),
        Err(_) => flow_core::decode_failure::<RecoverResult>(),
    };
    serialize_response(&response)
}

#[wasm_bindgen]
pub fn recover_document_audited(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<AuditedRecoverRequest>(request) {
        Ok(request) => flow_core::recover_audited(request),
        Err(_) => flow_core::decode_failure::<AuditedRecoverResult>(),
    };
    serialize_response(&response)
}

fn serialize_response<T: serde::Serialize>(response: &ApiResponse<T>) -> JsValue {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    serde::Serialize::serialize(response, &serializer)
        .expect("serializing a fixed response DTO cannot fail")
}

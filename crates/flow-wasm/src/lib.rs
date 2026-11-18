//! Narrow serialized WebAssembly boundary for FlowPDF.

#![forbid(unsafe_code)]

use flow_core::pdf::{
    NATIVE_MAX_NOTE_BYTES, NATIVE_REQUEST_HISTORY, NativeBinding, NativeEditLimits,
    NativeNoteCommand, NativePdfNoteScene, NativePdfSession, NativeRequestLookup, PdfObjectRef,
};
use flow_core::{
    ApiResponse, ApplyCommandRequest, AssetStageRequest, AssetStageResponse, AuditedRecoverRequest,
    AuditedRecoverResult, CommandKind, CreateSampleRequest, EditorSessionRequest,
    EditorSessionResponse, EditorViewDto, EditorViewRequest, FontCatalog, FontFace,
    FontFaceIdentity, FormSessionRequest, FormSessionResponse, LayoutWasmFontInput,
    MigrateDocumentRequest, MigrateDocumentResult, OperationResult, PdfDocumentCandidate,
    PdfDocumentSummary, PdfExportManifest, PdfExportOptions, PdfExportRequest,
    PdfFontManifestIdentity, PdfFormPlan, PdfInternalLink, PdfMetadataOptions, PdfOcrCandidate,
    PdfOutlineEntry, PdfPagePlan, PdfReaderLimits, PdfReconstructionRequest,
    PdfReconstructionResult, PdfRecoveryExpectation, PdfReproducibilityInputs, PdfReviewDecision,
    PdfScene, PdfSceneRequest, PlanPersistenceCommitRequest, PlanStandaloneAuditRequest,
    RecoverRequest, RecoverResult, UkrainianHyphenation, VOICE_PROTOCOL_VERSION,
    VoiceCommandRequest, VoiceIntent, accept_pdf_reconstruction as core_accept_pdf_reconstruction,
    export_pdf as core_export_pdf, open_pdf, read_pdf_scene, reconstruct_pdf_scene,
    recover_owned_source as core_recover_owned_source, store::PlannedPersistenceCommit,
    verify_pdf_document_candidate, verify_pdf_reconstruction_result,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::VecDeque, io::Write};
use wasm_bindgen::prelude::*;

const SUPPORTED_OLDER_FIXTURE: &str = include_str!("../../../fixtures/flowdoc/older.json");
const PDF_PROTOCOL_VERSION: u32 = 1;
const MAX_PDF_PROTOCOL_REQUEST_BYTES: usize = 96 * 1024 * 1024;
const MAX_PDF_PROTOCOL_HEX_BYTES: usize = flow_core::MAX_PDF_SOURCE_STREAM_BYTES;
const PDF_READER_PROTOCOL_VERSION: u32 = 1;
const MAX_PDF_READER_PROTOCOL_REQUEST_BYTES: usize = 136 * 1024 * 1024;
const MAX_PDF_READER_INPUT_BYTES: usize = 64 * 1024 * 1024;
const PDF_RECONSTRUCTION_PROTOCOL_VERSION: u32 = 1;
const MAX_PDF_RECONSTRUCTION_PROTOCOL_REQUEST_BYTES: usize = 144 * 1024 * 1024;
const MAX_PDF_RECONSTRUCTION_RESULT_BYTES: usize = 64 * 1024 * 1024;
const FONT_CATALOG_PROTOCOL_VERSION: u32 = 1;
const MAX_FONT_CATALOG_PROTOCOL_REQUEST_BYTES: usize = flow_core::MAX_LAYOUT_WASM_REQUEST_BYTES;
const HYPHENATION_PROTOCOL_VERSION: u32 = 1;
const MAX_HYPHENATION_PROTOCOL_REQUEST_BYTES: usize = flow_core::MAX_LAYOUT_WASM_FONT_DATA_BYTES;
const MAX_VOICE_PROTOCOL_REQUEST_BYTES: usize = 16 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FontCatalogWireRequest {
    protocol_version: u32,
    fonts: Vec<LayoutWasmFontInput>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FontCatalogWireResponse {
    protocol_version: u32,
    ok: bool,
    identity: Option<String>,
    faces: Option<Vec<FontFaceIdentity>>,
    error: Option<PdfProtocolError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HyphenationWireRequest {
    protocol_version: u32,
    bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HyphenationWireResponse {
    protocol_version: u32,
    ok: bool,
    identity: Option<String>,
    error: Option<PdfProtocolError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceWireError {
    code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceWireResponse {
    protocol_version: u32,
    ok: bool,
    intent: Option<VoiceIntent>,
    error: Option<VoiceWireError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PdfPageWire {
    bounds: flow_core::LayoutRect,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PdfExportWireRequest {
    protocol_version: u32,
    request_id: String,
    source_revision: u32,
    source_hash: String,
    layout_settings_fingerprint: String,
    #[serde(default)]
    layout_result_hash: Option<String>,
    #[serde(default)]
    font_catalog_identity: Option<String>,
    #[serde(default)]
    font_faces: Vec<PdfFontManifestIdentity>,
    #[serde(default)]
    hyphenation_data_identity: Option<String>,
    canonical_json: String,
    pages: Vec<PdfPageWire>,
    #[serde(default)]
    producer: Option<String>,
    #[serde(default)]
    metadata: Option<PdfMetadataOptions>,
    #[serde(default)]
    outlines: Vec<PdfOutlineEntry>,
    #[serde(default)]
    internal_links: Vec<PdfInternalLink>,
    #[serde(default)]
    form_plan: Option<PdfFormPlan>,
    #[serde(default)]
    flattened_field_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PdfRecoveryWireRequest {
    protocol_version: u32,
    request_id: String,
    #[serde(default)]
    private_source_stream_hex: Option<String>,
    #[serde(default)]
    expected: PdfRecoveryExpectationWire,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PdfRecoveryExpectationWire {
    #[serde(default)]
    document_id: Option<String>,
    #[serde(default)]
    revision: Option<u32>,
    #[serde(default)]
    canonical_hash: Option<String>,
    #[serde(default)]
    export_fingerprint: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PdfExportWireResult {
    source_revision: u32,
    source_hash: String,
    layout_settings_fingerprint: String,
    export_fingerprint: String,
    byte_hash: String,
    bytes_hex: String,
    private_source_stream_hex: String,
    manifest: PdfExportManifest,
    support_report: flow_core::PdfSupportReport,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PdfRecoveryWireResult {
    exact: bool,
    canonical_json: String,
    canonical_hash: String,
    document_id: String,
    revision: u32,
    manifest: PdfExportManifest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PdfReaderWireRequest {
    protocol_version: u32,
    request_id: String,
    bytes_hex: String,
    first_page: u32,
    page_count: u32,
    #[serde(default)]
    limits: Option<PdfReaderLimits>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PdfReaderWireResult {
    summary: PdfDocumentSummary,
    scene: PdfScene,
    result_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PdfReconstructionWireRequest {
    protocol_version: u32,
    request_id: String,
    source_hash: String,
    scene: PdfScene,
    locale: String,
    #[serde(default)]
    ocr_candidates: Vec<PdfOcrCandidate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PdfReconstructionAcceptWireRequest {
    protocol_version: u32,
    request_id: String,
    result: PdfReconstructionResult,
    #[serde(default)]
    decisions: Vec<PdfReviewDecision>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PdfReconstructionWireResult {
    result: PdfReconstructionResult,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PdfReconstructionAcceptWireResult {
    candidate: PdfDocumentCandidate,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PdfProtocolError {
    code: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PdfProtocolResponse<T> {
    protocol_version: u32,
    request_id: String,
    ok: bool,
    result: Option<T>,
    error: Option<PdfProtocolError>,
}

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

/// Applies one immutable, revision-bound noncanonical form-session action.
#[wasm_bindgen]
pub fn apply_form_session(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<FormSessionRequest>(request) {
        Ok(request) => flow_core::apply_form_session(request),
        Err(_) => flow_core::decode_failure::<FormSessionResponse>(),
    };
    serialize_response(&response)
}

/// String-only form-session adapter used by workers and deterministic tests.
#[wasm_bindgen]
pub fn apply_form_session_json(request_json: String) -> String {
    let response = match serde_json::from_str::<FormSessionRequest>(&request_json) {
        Ok(request) => flow_core::apply_form_session(request),
        Err(_) => flow_core::decode_failure::<FormSessionResponse>(),
    };
    serialize_json_response(&response)
}

/// Resolves one bounded command-mode speech transcript through Rust's exact
/// locale allowlist. The response never echoes or persists the transcript.
#[wasm_bindgen]
pub fn resolve_voice_command(request_json: String) -> String {
    if request_json.len() > MAX_VOICE_PROTOCOL_REQUEST_BYTES {
        return serialize_voice_response(VoiceWireResponse {
            protocol_version: VOICE_PROTOCOL_VERSION,
            ok: false,
            intent: None,
            error: Some(VoiceWireError {
                code: "FLOW_VOICE_REQUEST_SIZE_LIMIT".to_owned(),
            }),
        });
    }
    let request = match serde_json::from_str::<VoiceCommandRequest>(&request_json) {
        Ok(request) => request,
        Err(_) => {
            return serialize_voice_response(VoiceWireResponse {
                protocol_version: VOICE_PROTOCOL_VERSION,
                ok: false,
                intent: None,
                error: Some(VoiceWireError {
                    code: "FLOW_VOICE_REQUEST_DECODE".to_owned(),
                }),
            });
        }
    };
    match flow_core::resolve_voice_command(request) {
        Ok(intent) => serialize_voice_response(VoiceWireResponse {
            protocol_version: VOICE_PROTOCOL_VERSION,
            ok: true,
            intent: Some(intent),
            error: None,
        }),
        Err(error) => serialize_voice_response(VoiceWireResponse {
            protocol_version: VOICE_PROTOCOL_VERSION,
            ok: false,
            intent: None,
            error: Some(VoiceWireError {
                code: error.code().to_owned(),
            }),
        }),
    }
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

/// Returns the exact Rust-owned identity for one bounded ordered font
/// catalog. The browser uses this only to construct a matching layout
/// request; font bytes never return through the response.
#[wasm_bindgen]
pub fn font_catalog_identity(request_json: String) -> String {
    if request_json.len() > MAX_FONT_CATALOG_PROTOCOL_REQUEST_BYTES {
        return serialize_font_catalog_response(FontCatalogWireResponse {
            protocol_version: FONT_CATALOG_PROTOCOL_VERSION,
            ok: false,
            identity: None,
            faces: None,
            error: Some(PdfProtocolError {
                code: "FLOW_FONT_CATALOG_REQUEST_SIZE_LIMIT".to_owned(),
            }),
        });
    }
    let request = match serde_json::from_str::<FontCatalogWireRequest>(&request_json) {
        Ok(request) => request,
        Err(_) => {
            return serialize_font_catalog_response(FontCatalogWireResponse {
                protocol_version: FONT_CATALOG_PROTOCOL_VERSION,
                ok: false,
                identity: None,
                faces: None,
                error: Some(PdfProtocolError {
                    code: "FLOW_FONT_CATALOG_REQUEST_DECODE".to_owned(),
                }),
            });
        }
    };
    if request.protocol_version != FONT_CATALOG_PROTOCOL_VERSION {
        return serialize_font_catalog_response(FontCatalogWireResponse {
            protocol_version: FONT_CATALOG_PROTOCOL_VERSION,
            ok: false,
            identity: None,
            faces: None,
            error: Some(PdfProtocolError {
                code: "FLOW_FONT_CATALOG_PROTOCOL_VERSION".to_owned(),
            }),
        });
    }
    let faces = match request
        .fonts
        .into_iter()
        .map(|font| FontFace::new(font.id, font.family, font.face_index, font.bytes))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(faces) => faces,
        Err(error) => {
            return serialize_font_catalog_response(FontCatalogWireResponse {
                protocol_version: FONT_CATALOG_PROTOCOL_VERSION,
                ok: false,
                identity: None,
                faces: None,
                error: Some(PdfProtocolError {
                    code: error.code().to_owned(),
                }),
            });
        }
    };
    let catalog = match FontCatalog::new(faces) {
        Ok(catalog) => catalog,
        Err(error) => {
            return serialize_font_catalog_response(FontCatalogWireResponse {
                protocol_version: FONT_CATALOG_PROTOCOL_VERSION,
                ok: false,
                identity: None,
                faces: None,
                error: Some(PdfProtocolError {
                    code: error.code().to_owned(),
                }),
            });
        }
    };
    serialize_font_catalog_response(FontCatalogWireResponse {
        protocol_version: FONT_CATALOG_PROTOCOL_VERSION,
        ok: true,
        identity: Some(catalog.identity().to_owned()),
        faces: Some(
            catalog
                .faces()
                .iter()
                .map(|face| face.identity().clone())
                .collect(),
        ),
        error: None,
    })
}

/// Validates one bounded pinned Ukrainian dictionary and returns its BLAKE3
/// identity. The browser uses the identity with the same bytes in every
/// layout request; dictionary bytes never return through this response.
#[wasm_bindgen]
pub fn hyphenation_data_identity(request_json: String) -> String {
    if request_json.len() > MAX_HYPHENATION_PROTOCOL_REQUEST_BYTES {
        return serialize_hyphenation_response(HyphenationWireResponse {
            protocol_version: HYPHENATION_PROTOCOL_VERSION,
            ok: false,
            identity: None,
            error: Some(PdfProtocolError {
                code: "FLOW_HYPHENATION_REQUEST_SIZE_LIMIT".to_owned(),
            }),
        });
    }
    let request = match serde_json::from_str::<HyphenationWireRequest>(&request_json) {
        Ok(request) => request,
        Err(_) => {
            return serialize_hyphenation_response(HyphenationWireResponse {
                protocol_version: HYPHENATION_PROTOCOL_VERSION,
                ok: false,
                identity: None,
                error: Some(PdfProtocolError {
                    code: "FLOW_HYPHENATION_REQUEST_DECODE".to_owned(),
                }),
            });
        }
    };
    if request.protocol_version != HYPHENATION_PROTOCOL_VERSION {
        return serialize_hyphenation_response(HyphenationWireResponse {
            protocol_version: HYPHENATION_PROTOCOL_VERSION,
            ok: false,
            identity: None,
            error: Some(PdfProtocolError {
                code: "FLOW_HYPHENATION_PROTOCOL_VERSION".to_owned(),
            }),
        });
    }
    if UkrainianHyphenation::from_bincode("validated", &request.bytes).is_err() {
        return serialize_hyphenation_response(HyphenationWireResponse {
            protocol_version: HYPHENATION_PROTOCOL_VERSION,
            ok: false,
            identity: None,
            error: Some(PdfProtocolError {
                code: "FLOW_LAYOUT_HYPHENATION_INVALID".to_owned(),
            }),
        });
    }
    serialize_hyphenation_response(HyphenationWireResponse {
        protocol_version: HYPHENATION_PROTOCOL_VERSION,
        ok: true,
        identity: Some(format!("blake3:{}", blake3::hash(&request.bytes).to_hex())),
        error: None,
    })
}

/// Derives a revision/hash-bound form projection from an opaque Rust layout
/// request. The browser supplies no page coordinates or display-list data.
#[wasm_bindgen]
pub fn project_form_widgets(request_json: String) -> String {
    flow_core::form_projection_wasm_response_json(&request_json)
}

/// Verifies the complete Rust-owned form projection response before browser
/// code may publish widget geometry or carry its optional plan to export.
#[wasm_bindgen]
pub fn verify_form_projection_response(response_json: String) -> bool {
    flow_core::verify_form_projection_wasm_response_json(&response_json)
}

/// Runs a bounded PDF export request through a string-only JSON boundary.
/// PDF bytes and the private source stream are hex-encoded only at this final
/// adapter; Rust remains the sole owner of PDF construction and validation.
#[wasm_bindgen]
pub fn export_pdf(request_json: String) -> String {
    if request_json.len() > MAX_PDF_PROTOCOL_REQUEST_BYTES {
        return serialize_pdf_response(PdfProtocolResponse::<PdfExportWireResult> {
            protocol_version: PDF_PROTOCOL_VERSION,
            request_id: "size-limit".to_owned(),
            ok: false,
            result: None,
            error: Some(PdfProtocolError {
                code: "FLOW_PDF_EXPORT_REQUEST_SIZE_LIMIT".to_owned(),
            }),
        });
    }
    let request = match serde_json::from_str::<PdfExportWireRequest>(&request_json) {
        Ok(request) => request,
        Err(_) => {
            return serialize_pdf_response(PdfProtocolResponse::<PdfExportWireResult> {
                protocol_version: PDF_PROTOCOL_VERSION,
                request_id: "decode-error".to_owned(),
                ok: false,
                result: None,
                error: Some(PdfProtocolError {
                    code: "FLOW_PDF_EXPORT_REQUEST_DECODE".to_owned(),
                }),
            });
        }
    };
    let request_id = request.request_id.clone();
    let result = execute_pdf_export(request);
    match result {
        Ok(result) => serialize_pdf_response(PdfProtocolResponse {
            protocol_version: PDF_PROTOCOL_VERSION,
            request_id,
            ok: true,
            result: Some(result),
            error: None,
        }),
        Err(code) => serialize_pdf_response(PdfProtocolResponse::<PdfExportWireResult> {
            protocol_version: PDF_PROTOCOL_VERSION,
            request_id,
            ok: false,
            result: None,
            error: Some(PdfProtocolError { code }),
        }),
    }
}

/// Verifies the Rust-produced PDF response before a browser worker publishes
/// it.  It checks the encoded byte hash and bounded private stream size.
#[wasm_bindgen]
pub fn verify_pdf_export_response(response_json: String) -> bool {
    let Ok(response) =
        serde_json::from_str::<PdfProtocolResponse<PdfExportWireResult>>(&response_json)
    else {
        return false;
    };
    if response.protocol_version != PDF_PROTOCOL_VERSION || !response.ok || response.error.is_some()
    {
        return false;
    }
    let Some(result) = response.result else {
        return false;
    };
    let Ok(bytes) = decode_hex(&result.bytes_hex, flow_core::MAX_PDF_OUTPUT_BYTES) else {
        return false;
    };
    let Ok(private_stream) = decode_hex(
        &result.private_source_stream_hex,
        MAX_PDF_PROTOCOL_HEX_BYTES,
    ) else {
        return false;
    };
    blake3::hash(&bytes).to_hex().as_str() == result.byte_hash
        && !private_stream.is_empty()
        && result.source_revision == result.manifest.source_revision
        && result.source_hash == result.manifest.source_hash
}

/// Reads a bounded page window through the string-only WASM boundary. The
/// browser supplies only hexadecimal PDF bytes and a request window; Rust
/// owns parsing, limits, provenance, and the derived scene.
#[wasm_bindgen]
pub fn read_pdf(request_json: String) -> String {
    if request_json.len() > MAX_PDF_READER_PROTOCOL_REQUEST_BYTES {
        return serialize_pdf_reader_response(PdfProtocolResponse::<PdfReaderWireResult> {
            protocol_version: PDF_READER_PROTOCOL_VERSION,
            request_id: "size-limit".to_owned(),
            ok: false,
            result: None,
            error: Some(PdfProtocolError {
                code: "FLOW_PDF_READER_REQUEST_SIZE_LIMIT".to_owned(),
            }),
        });
    }
    let request = match serde_json::from_str::<PdfReaderWireRequest>(&request_json) {
        Ok(request) => request,
        Err(_) => {
            return serialize_pdf_reader_response(PdfProtocolResponse::<PdfReaderWireResult> {
                protocol_version: PDF_READER_PROTOCOL_VERSION,
                request_id: "decode-error".to_owned(),
                ok: false,
                result: None,
                error: Some(PdfProtocolError {
                    code: "FLOW_PDF_READER_REQUEST_DECODE".to_owned(),
                }),
            });
        }
    };
    let request_id = request.request_id.clone();
    match execute_pdf_reader(request) {
        Ok(result) => serialize_pdf_reader_response(PdfProtocolResponse {
            protocol_version: PDF_READER_PROTOCOL_VERSION,
            request_id,
            ok: true,
            result: Some(result),
            error: None,
        }),
        Err(code) => serialize_pdf_reader_response(PdfProtocolResponse::<PdfReaderWireResult> {
            protocol_version: PDF_READER_PROTOCOL_VERSION,
            request_id,
            ok: false,
            result: None,
            error: Some(PdfProtocolError { code }),
        }),
    }
}

/// Recomputes the scene hash and checks the duplicated summary identities
/// before a worker or browser panel publishes a reader result.
#[wasm_bindgen]
pub fn verify_pdf_reader_response(response_json: String) -> bool {
    let Ok(response) =
        serde_json::from_str::<PdfProtocolResponse<PdfReaderWireResult>>(&response_json)
    else {
        return false;
    };
    if response.protocol_version != PDF_READER_PROTOCOL_VERSION
        || !response.ok
        || response.error.is_some()
    {
        return false;
    }
    let Some(result) = response.result else {
        return false;
    };
    if result.result_hash.trim().is_empty()
        || result.result_hash != result.scene.result_hash
        || result.summary.source_hash != result.scene.source_hash
        || result.summary.page_count != result.scene.page_count
        || result.scene.schema_version != flow_core::PDF_SCENE_SCHEMA_VERSION
        || result.summary.schema_version != flow_core::PDF_READER_SCHEMA_VERSION
        || result.scene.pages.len() > result.summary.page_count as usize
        || result.scene.first_page > result.summary.page_count
    {
        return false;
    }
    let mut unhashed_scene = result.scene;
    unhashed_scene.result_hash.clear();
    let Ok(bytes) = serde_json::to_vec(&unhashed_scene) else {
        return false;
    };
    blake3::hash(&bytes).to_hex().as_str() == result.result_hash
}

/// Reconstructs a verified reader scene through a bounded string-only
/// boundary. The returned candidate contains no PDF bytes or provider handle.
#[wasm_bindgen]
pub fn reconstruct_pdf(request_json: String) -> String {
    if request_json.len() > MAX_PDF_RECONSTRUCTION_PROTOCOL_REQUEST_BYTES {
        return serialize_pdf_reconstruction_response(PdfProtocolResponse::<
            PdfReconstructionWireResult,
        > {
            protocol_version: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
            request_id: "size-limit".to_owned(),
            ok: false,
            result: None,
            error: Some(PdfProtocolError {
                code: "FLOW_PDF_RECONSTRUCTION_REQUEST_SIZE_LIMIT".to_owned(),
            }),
        });
    }
    let request = match serde_json::from_str::<PdfReconstructionWireRequest>(&request_json) {
        Ok(request) => request,
        Err(_) => {
            return serialize_pdf_reconstruction_response(PdfProtocolResponse::<
                PdfReconstructionWireResult,
            > {
                protocol_version: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
                request_id: "decode-error".to_owned(),
                ok: false,
                result: None,
                error: Some(PdfProtocolError {
                    code: "FLOW_PDF_RECONSTRUCTION_REQUEST_DECODE".to_owned(),
                }),
            });
        }
    };
    let request_id = request.request_id.clone();
    match execute_pdf_reconstruction(request) {
        Ok(result) => serialize_pdf_reconstruction_response(PdfProtocolResponse {
            protocol_version: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
            request_id,
            ok: true,
            result: Some(PdfReconstructionWireResult { result }),
            error: None,
        }),
        Err(code) => serialize_pdf_reconstruction_response(PdfProtocolResponse::<
            PdfReconstructionWireResult,
        > {
            protocol_version: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
            request_id,
            ok: false,
            result: None,
            error: Some(PdfProtocolError { code }),
        }),
    }
}

/// Verifies a Rust-produced reconstruction response before browser publication.
#[wasm_bindgen]
pub fn verify_pdf_reconstruction_response(response_json: String) -> bool {
    let Ok(response) =
        serde_json::from_str::<PdfProtocolResponse<PdfReconstructionWireResult>>(&response_json)
    else {
        return false;
    };
    if response.protocol_version != PDF_RECONSTRUCTION_PROTOCOL_VERSION
        || response.request_id.trim().is_empty()
        || !response.ok
        || response.error.is_some()
    {
        return false;
    }
    let Some(result) = response.result else {
        return false;
    };
    verify_pdf_reconstruction_result(&result.result)
}

/// Applies explicit review decisions to a reconstruction candidate through the
/// same closed string-only boundary.
#[wasm_bindgen]
pub fn accept_pdf_reconstruction(request_json: String) -> String {
    if request_json.len() > MAX_PDF_RECONSTRUCTION_PROTOCOL_REQUEST_BYTES {
        return serialize_pdf_reconstruction_accept_response(PdfProtocolResponse::<
            PdfReconstructionAcceptWireResult,
        > {
            protocol_version: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
            request_id: "size-limit".to_owned(),
            ok: false,
            result: None,
            error: Some(PdfProtocolError {
                code: "FLOW_PDF_RECONSTRUCTION_ACCEPT_REQUEST_SIZE_LIMIT".to_owned(),
            }),
        });
    }
    let request = match serde_json::from_str::<PdfReconstructionAcceptWireRequest>(&request_json) {
        Ok(request) => request,
        Err(_) => {
            return serialize_pdf_reconstruction_accept_response(PdfProtocolResponse::<
                PdfReconstructionAcceptWireResult,
            > {
                protocol_version: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
                request_id: "decode-error".to_owned(),
                ok: false,
                result: None,
                error: Some(PdfProtocolError {
                    code: "FLOW_PDF_RECONSTRUCTION_ACCEPT_REQUEST_DECODE".to_owned(),
                }),
            });
        }
    };
    let request_id = request.request_id.clone();
    match execute_pdf_reconstruction_accept(request) {
        Ok(candidate) => serialize_pdf_reconstruction_accept_response(PdfProtocolResponse {
            protocol_version: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
            request_id,
            ok: true,
            result: Some(PdfReconstructionAcceptWireResult { candidate }),
            error: None,
        }),
        Err(code) => serialize_pdf_reconstruction_accept_response(PdfProtocolResponse::<
            PdfReconstructionAcceptWireResult,
        > {
            protocol_version: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
            request_id,
            ok: false,
            result: None,
            error: Some(PdfProtocolError { code }),
        }),
    }
}

/// Verifies the canonical identity and external provenance of an accepted
/// review candidate.
#[wasm_bindgen]
pub fn verify_pdf_reconstruction_accept_response(response_json: String) -> bool {
    let Ok(response) = serde_json::from_str::<PdfProtocolResponse<PdfReconstructionAcceptWireResult>>(
        &response_json,
    ) else {
        return false;
    };
    if response.protocol_version != PDF_RECONSTRUCTION_PROTOCOL_VERSION
        || response.request_id.trim().is_empty()
        || !response.ok
        || response.error.is_some()
    {
        return false;
    }
    response
        .result
        .is_some_and(|result| verify_pdf_document_candidate(&result.candidate))
}

/// Recovers an exact owned source through a bounded string-only request.
#[wasm_bindgen]
pub fn recover_owned_source(request_json: String) -> String {
    if request_json.len() > MAX_PDF_PROTOCOL_REQUEST_BYTES {
        return serialize_pdf_response(PdfProtocolResponse::<PdfRecoveryWireResult> {
            protocol_version: PDF_PROTOCOL_VERSION,
            request_id: "size-limit".to_owned(),
            ok: false,
            result: None,
            error: Some(PdfProtocolError {
                code: "FLOW_PDF_RECOVERY_REQUEST_SIZE_LIMIT".to_owned(),
            }),
        });
    }
    let request = match serde_json::from_str::<PdfRecoveryWireRequest>(&request_json) {
        Ok(request) => request,
        Err(_) => {
            return serialize_pdf_response(PdfProtocolResponse::<PdfRecoveryWireResult> {
                protocol_version: PDF_PROTOCOL_VERSION,
                request_id: "decode-error".to_owned(),
                ok: false,
                result: None,
                error: Some(PdfProtocolError {
                    code: "FLOW_PDF_RECOVERY_REQUEST_DECODE".to_owned(),
                }),
            });
        }
    };
    let request_id = request.request_id.clone();
    let result = execute_pdf_recovery(request);
    match result {
        Ok(result) => serialize_pdf_response(PdfProtocolResponse {
            protocol_version: PDF_PROTOCOL_VERSION,
            request_id,
            ok: true,
            result: Some(result),
            error: None,
        }),
        Err(code) => serialize_pdf_response(PdfProtocolResponse::<PdfRecoveryWireResult> {
            protocol_version: PDF_PROTOCOL_VERSION,
            request_id,
            ok: false,
            result: None,
            error: Some(PdfProtocolError { code }),
        }),
    }
}

fn execute_pdf_export(request: PdfExportWireRequest) -> Result<PdfExportWireResult, String> {
    if request.protocol_version != PDF_PROTOCOL_VERSION {
        return Err("FLOW_PDF_PROTOCOL_VERSION".to_owned());
    }
    if request.request_id.trim().is_empty() || request.request_id.len() > 128 {
        return Err("FLOW_PDF_REQUEST_ID_INVALID".to_owned());
    }
    if request.pages.is_empty() {
        return Err("FLOW_PDF_PAGE_PLAN_EMPTY".to_owned());
    }
    let form_plan = request.form_plan;
    let flattened_field_ids = request.flattened_field_ids;
    let canonical_json = request.canonical_json;
    let pages = request
        .pages
        .into_iter()
        .map(|page| PdfPagePlan::empty(page.bounds.width, page.bounds.height))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.code().to_owned())?;
    let options = PdfExportOptions {
        producer: request
            .producer
            .unwrap_or_else(|| PdfExportOptions::default().producer),
        metadata: request.metadata.unwrap_or_default(),
        outlines: request.outlines,
        internal_links: request.internal_links,
        flattened_field_ids,
    };
    let inputs = PdfReproducibilityInputs {
        layout_result_hash: request.layout_result_hash,
        font_catalog_identity: request.font_catalog_identity,
        font_faces: request.font_faces,
        hyphenation_identity: request.hyphenation_data_identity,
    };
    let request = PdfExportRequest::new(
        request.source_revision,
        request.source_hash,
        request.layout_settings_fingerprint,
        pages,
        options,
    )
    .map_err(|error| error.code().to_owned())?
    .with_reproducibility_inputs(inputs)
    .map_err(|error| error.code().to_owned())?;
    let request = if let Some(form_plan) = form_plan {
        request
            .with_form_plan(form_plan)
            .map_err(|error| error.code().to_owned())?
    } else {
        request
    }
    .with_source_payload(canonical_json.into_bytes())
    .map_err(|error| error.code().to_owned())?;
    let result = core_export_pdf(&request).map_err(|error| error.code().to_owned())?;
    Ok(PdfExportWireResult {
        source_revision: result.source_revision,
        source_hash: result.source_hash,
        layout_settings_fingerprint: result.layout_settings_fingerprint,
        export_fingerprint: result.export_fingerprint,
        byte_hash: result.byte_hash,
        bytes_hex: encode_hex(&result.bytes),
        private_source_stream_hex: encode_hex(&result.private_source_stream),
        manifest: result.manifest,
        support_report: result.support_report,
    })
}

fn execute_pdf_reader(request: PdfReaderWireRequest) -> Result<PdfReaderWireResult, String> {
    if request.protocol_version != PDF_READER_PROTOCOL_VERSION {
        return Err("FLOW_PDF_READER_PROTOCOL_VERSION".to_owned());
    }
    if request.request_id.trim().is_empty() || request.request_id.len() > 128 {
        return Err("FLOW_PDF_REQUEST_ID_INVALID".to_owned());
    }
    if request.page_count == 0 {
        return Err("FLOW_PDF_READER_PAGE_WINDOW_INVALID".to_owned());
    }
    let limits = request.limits.unwrap_or_default();
    if limits.max_input_bytes > MAX_PDF_READER_INPUT_BYTES {
        return Err("FLOW_PDF_READER_INPUT_LIMIT_INVALID".to_owned());
    }
    let bytes = decode_hex(
        &request.bytes_hex,
        limits.max_input_bytes.min(MAX_PDF_READER_INPUT_BYTES),
    )
    .map_err(|_| "FLOW_PDF_READER_BYTES_INVALID".to_owned())?;
    let reader = open_pdf(&bytes, limits).map_err(|error| error.code().to_owned())?;
    let summary = reader.summary().map_err(|error| error.code().to_owned())?;
    let scene_request = PdfSceneRequest {
        first_page: request.first_page,
        page_count: request.page_count,
    };
    let scene = read_pdf_scene(&reader, &scene_request).map_err(|error| error.code().to_owned())?;
    let result_hash = scene.result_hash.clone();
    Ok(PdfReaderWireResult {
        summary,
        scene,
        result_hash,
    })
}

fn execute_pdf_reconstruction(
    request: PdfReconstructionWireRequest,
) -> Result<PdfReconstructionResult, String> {
    if request.protocol_version != PDF_RECONSTRUCTION_PROTOCOL_VERSION {
        return Err("FLOW_PDF_RECONSTRUCTION_PROTOCOL_VERSION".to_owned());
    }
    if request.request_id.trim().is_empty() || request.request_id.len() > 128 {
        return Err("FLOW_PDF_REQUEST_ID_INVALID".to_owned());
    }
    reconstruct_pdf_scene(&PdfReconstructionRequest {
        source_hash: request.source_hash,
        scene: request.scene,
        locale: request.locale,
        ocr_candidates: request.ocr_candidates,
    })
    .map_err(|error| error.code().to_owned())
}

fn execute_pdf_reconstruction_accept(
    request: PdfReconstructionAcceptWireRequest,
) -> Result<PdfDocumentCandidate, String> {
    if request.protocol_version != PDF_RECONSTRUCTION_PROTOCOL_VERSION {
        return Err("FLOW_PDF_RECONSTRUCTION_PROTOCOL_VERSION".to_owned());
    }
    if request.request_id.trim().is_empty() || request.request_id.len() > 128 {
        return Err("FLOW_PDF_REQUEST_ID_INVALID".to_owned());
    }
    if !verify_pdf_reconstruction_result(&request.result) {
        return Err("FLOW_PDF_RECONSTRUCTION_RESULT_INVALID".to_owned());
    }
    core_accept_pdf_reconstruction(&request.result, &request.decisions)
        .map_err(|error| error.code().to_owned())
}

fn execute_pdf_recovery(request: PdfRecoveryWireRequest) -> Result<PdfRecoveryWireResult, String> {
    if request.protocol_version != PDF_PROTOCOL_VERSION {
        return Err("FLOW_PDF_PROTOCOL_VERSION".to_owned());
    }
    if request.request_id.trim().is_empty() || request.request_id.len() > 128 {
        return Err("FLOW_PDF_REQUEST_ID_INVALID".to_owned());
    }
    let private_stream = request
        .private_source_stream_hex
        .as_deref()
        .map(|value| decode_hex(value, MAX_PDF_PROTOCOL_HEX_BYTES))
        .transpose()
        .map_err(|_| "FLOW_PDF_RECOVERY_HEX_INVALID".to_owned())?;
    let document_id = request
        .expected
        .document_id
        .map(flow_core::model::DocumentId::new)
        .transpose()
        .map_err(|_| "FLOW_PDF_RECOVERY_DOCUMENT_ID_INVALID".to_owned())?;
    let recovered = core_recover_owned_source(
        private_stream.as_deref(),
        &PdfRecoveryExpectation {
            document_id,
            revision: request.expected.revision,
            canonical_hash: request.expected.canonical_hash,
            export_fingerprint: request.expected.export_fingerprint,
        },
    )
    .map_err(|error| error.code().to_owned())?;
    Ok(PdfRecoveryWireResult {
        exact: recovered.exact,
        canonical_json: recovered.canonical_json,
        canonical_hash: recovered.canonical_hash,
        document_id: recovered.document.document_id.to_string(),
        revision: recovered.document.revision,
        manifest: recovered.manifest,
    })
}

fn serialize_pdf_response<T: Serialize>(response: PdfProtocolResponse<T>) -> String {
    serde_json::to_string(&response).expect("serializing a closed PDF protocol cannot fail")
}

fn serialize_pdf_reader_response(response: PdfProtocolResponse<PdfReaderWireResult>) -> String {
    serde_json::to_string(&response).expect("serializing a closed PDF reader protocol cannot fail")
}

fn serialize_pdf_reconstruction_response<T: Serialize>(response: PdfProtocolResponse<T>) -> String {
    serialize_bounded_pdf_response(response, "FLOW_PDF_RECONSTRUCTION_RESULT_SIZE_LIMIT")
}

fn serialize_pdf_reconstruction_accept_response<T: Serialize>(
    response: PdfProtocolResponse<T>,
) -> String {
    serialize_bounded_pdf_response(response, "FLOW_PDF_RECONSTRUCTION_ACCEPT_RESULT_SIZE_LIMIT")
}

fn serialize_bounded_pdf_response<T: Serialize>(
    response: PdfProtocolResponse<T>,
    size_code: &str,
) -> String {
    let bytes =
        serde_json::to_vec(&response).expect("serializing a closed PDF protocol cannot fail");
    if bytes.len() <= MAX_PDF_RECONSTRUCTION_RESULT_BYTES {
        return String::from_utf8(bytes).expect("PDF protocol JSON is UTF-8");
    }
    serde_json::to_string(&PdfProtocolResponse::<T> {
        protocol_version: response.protocol_version,
        request_id: response.request_id,
        ok: false,
        result: None,
        error: Some(PdfProtocolError {
            code: size_code.to_owned(),
        }),
    })
    .expect("serializing a bounded PDF protocol error cannot fail")
}

fn serialize_font_catalog_response(response: FontCatalogWireResponse) -> String {
    serde_json::to_string(&response)
        .expect("serializing a closed font-catalog protocol cannot fail")
}

fn serialize_hyphenation_response(response: HyphenationWireResponse) -> String {
    serde_json::to_string(&response).expect("serializing a closed hyphenation protocol cannot fail")
}

fn serialize_voice_response(response: VoiceWireResponse) -> String {
    serde_json::to_string(&response).expect("serializing a closed voice protocol cannot fail")
}

fn serialize_json_response<T: Serialize>(response: &ApiResponse<T>) -> String {
    serde_json::to_string(response).expect("serializing a fixed response DTO cannot fail")
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for &byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    output
}

fn decode_hex(value: &str, maximum_bytes: usize) -> Result<Vec<u8>, ()> {
    if !value.len().is_multiple_of(2) || value.len() / 2 > maximum_bytes {
        return Err(());
    }
    let mut output = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn hex_digit(value: u8) -> Result<u8, ()> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(()),
    }
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

const NATIVE_PDF_VERSION: u32 = 1;
const NATIVE_PDF_BYTES: usize = 8 * 1024 * 1024;
const NATIVE_PDF_REQUEST: usize = 2 * NATIVE_PDF_BYTES + 4096;
const NATIVE_PDF_COMMAND: usize = 128 * 1024;
const NATIVE_PDF_RESULT: usize = 64 * 1024 * 1024;
const NATIVE_PDF_SESSIONS: usize = 2;
const NATIVE_PDF_RETAINED: usize = 8 * 1024 * 1024;
const MAX_WIRE_ID: u64 = (1 << 53) - 1;
const MAX_NATIVE_REVISION: u64 = MAX_WIRE_ID - (1 << 48);
const NATIVE_DECODE: &str = "FLOW_PDF_NATIVE_REQUEST_DECODE";
const NATIVE_BINDING: &str = "FLOW_PDF_NATIVE_BINDING_MISMATCH";
const NATIVE_REVISION: &str = "FLOW_PDF_NATIVE_REVISION_MISMATCH";
const NATIVE_RECONCILE: &str = "FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED";
const NATIVE_BUDGET: &str = "FLOW_PDF_NATIVE_BUDGET_EXHAUSTED";

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativeOperation {
    Open,
    Command,
    Snapshot,
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativeStatus {
    Opened,
    Applied,
    Accepted,
    Unseen,
    Evicted,
    Closed,
    Failed,
}

#[derive(Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum NativeRequest {
    Open {
        protocol_version: u32,
        request_id: u64,
        epoch: [u8; 16],
        origin_store_hash: String,
        bytes_hex: String,
    },
    Command {
        protocol_version: u32,
        request_id: u64,
        binding: NativeBinding,
        origin_store_hash: String,
        output_hash: [u8; 32],
        command: NativeNoteCommand,
    },
    Snapshot {
        protocol_version: u32,
        request_id: u64,
        binding: NativeBinding,
        origin_store_hash: String,
    },
    Close {
        protocol_version: u32,
        request_id: u64,
        binding: NativeBinding,
        origin_store_hash: String,
    },
}

impl NativeRequest {
    fn identity(&self) -> (NativeOperation, u32, u64, Option<NativeBinding>, &str) {
        match self {
            Self::Open {
                protocol_version,
                request_id,
                origin_store_hash,
                ..
            } => (
                NativeOperation::Open,
                *protocol_version,
                *request_id,
                None,
                origin_store_hash,
            ),
            Self::Command {
                protocol_version,
                request_id,
                binding,
                origin_store_hash,
                ..
            } => (
                NativeOperation::Command,
                *protocol_version,
                *request_id,
                Some(*binding),
                origin_store_hash,
            ),
            Self::Snapshot {
                protocol_version,
                request_id,
                binding,
                origin_store_hash,
            } => (
                NativeOperation::Snapshot,
                *protocol_version,
                *request_id,
                Some(*binding),
                origin_store_hash,
            ),
            Self::Close {
                protocol_version,
                request_id,
                binding,
                origin_store_hash,
            } => (
                NativeOperation::Close,
                *protocol_version,
                *request_id,
                Some(*binding),
                origin_store_hash,
            ),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeWireLimits {
    identity_version: u32,
    count_unit: NativeCountUnit,
    request_bytes: usize,
    command_bytes: usize,
    result_bytes: usize,
    pdf_bytes: usize,
    note_bytes: usize,
    request_history: usize,
    sessions: usize,
    retained_bytes: usize,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum NativeCountUnit {
    Utf8Bytes,
}

fn native_wire_limits() -> NativeWireLimits {
    NativeWireLimits {
        identity_version: 1,
        count_unit: NativeCountUnit::Utf8Bytes,
        request_bytes: NATIVE_PDF_REQUEST,
        command_bytes: NATIVE_PDF_COMMAND,
        result_bytes: NATIVE_PDF_RESULT,
        pdf_bytes: NATIVE_PDF_BYTES,
        note_bytes: NATIVE_MAX_NOTE_BYTES,
        request_history: NATIVE_REQUEST_HISTORY,
        sessions: NATIVE_PDF_SESSIONS,
        retained_bytes: NATIVE_PDF_SESSIONS * NATIVE_PDF_RETAINED,
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeWireSnapshot<S> {
    binding: NativeBinding,
    origin_store_hash: String,
    output_hash: [u8; 32],
    consumed_request: u64,
    scene: S,
    output_hex: String,
}

#[derive(Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    bound(deserialize = "S: Deserialize<'de>")
)]
struct NativeResponse<S> {
    protocol_version: u32,
    operation: NativeOperation,
    request_id: u64,
    #[serde(deserialize_with = "native_required_option")]
    binding: Option<NativeBinding>,
    #[serde(deserialize_with = "native_required_option")]
    origin_store_hash: Option<String>,
    status: NativeStatus,
    limits: NativeWireLimits,
    #[serde(deserialize_with = "native_required_option")]
    snapshot: Option<NativeWireSnapshot<S>>,
    #[serde(deserialize_with = "native_required_option")]
    code: Option<String>,
    result_hash: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeVerifiedScene {
    output_hash: [u8; 32],
    scene_hash: [u8; 32],
    pages: Vec<NativeVerifiedPage>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeVerifiedPage {
    id: u32,
    object: PdfObjectRef,
    #[serde(deserialize_with = "native_required_option")]
    geometry: Option<NativeVerifiedGeometry>,
    can_add_note: bool,
    annotations: Vec<NativeVerifiedAnnotation>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeVerifiedGeometry {
    bounds: [i64; 4],
    rotation: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum NativeVerifiedAnnotation {
    PlainNote {
        id: u64,
        object: PdfObjectRef,
        contents: String,
        rect: [i64; 4],
    },
    ReadOnly {
        #[serde(deserialize_with = "native_required_option")]
        object: Option<PdfObjectRef>,
        profile: NativeVerifiedProfile,
        #[serde(deserialize_with = "native_required_option")]
        rect: Option<[i64; 4]>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativeVerifiedProfile {
    Text,
    Appearance,
    Link,
    Widget,
    Other,
}

struct NativeRegistryEntry {
    session: NativePdfSession,
    origin_store_hash: String,
    base_outputs: VecDeque<(u64, [u8; 32])>,
}

#[derive(Default)]
struct NativeRegistry {
    sessions: Vec<NativeRegistryEntry>,
    closed: VecDeque<(NativeBinding, String, u64)>,
}

thread_local! {
    static NATIVE_REGISTRY: RefCell<NativeRegistry> = RefCell::new(NativeRegistry::default());
}

fn native_same_session(left: NativeBinding, right: NativeBinding) -> bool {
    left.session_id == right.session_id
        && left.origin_hash == right.origin_hash
        && left.epoch == right.epoch
}

fn native_hash_text(value: &str) -> bool {
    value.len() == 64 && native_lower_hex(value)
}

fn native_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn native_decode_pdf(value: &str) -> Result<Vec<u8>, ()> {
    if value.is_empty() || value.len() > 2 * NATIVE_PDF_BYTES || !native_lower_hex(value) {
        return Err(());
    }
    decode_hex(value, NATIVE_PDF_BYTES)
}

fn native_binding_valid(binding: NativeBinding) -> bool {
    binding.session_id > 0
        && binding.session_id <= MAX_WIRE_ID
        && binding.revision <= MAX_NATIVE_REVISION
}

fn native_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

struct NativeHashWriter(blake3::Hasher, usize);

impl Write for NativeHashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > NATIVE_PDF_RESULT - self.1 {
            return Err(std::io::Error::other("native result limit"));
        }
        self.1 += bytes.len();
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct NativeBoundedJson(Vec<u8>);

impl Write for NativeBoundedJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > NATIVE_PDF_RESULT - self.0.len() {
            return Err(std::io::Error::other("native result limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn native_result_hash(response: &impl Serialize) -> Result<String, serde_json::Error> {
    let mut writer = NativeHashWriter(blake3::Hasher::new(), 0);
    writer.0.update(b"flowpdf:native-wire:v1");
    serde_json::to_writer(&mut writer, response)?;
    Ok(writer.0.finalize().to_hex().to_string())
}

impl NativeRegistry {
    fn execute<'a>(
        &'a mut self,
        request: NativeRequest,
        response: &mut NativeResponse<&'a NativePdfNoteScene>,
    ) -> Result<(), &'static str> {
        let (_, _, request_id, binding, store_hash) = request.identity();
        if let NativeRequest::Open {
            epoch,
            bytes_hex,
            origin_store_hash,
            ..
        } = request
        {
            if self.sessions.len() == NATIVE_PDF_SESSIONS {
                return Err(NATIVE_BUDGET);
            }
            let bytes = native_decode_pdf(&bytes_hex).map_err(|()| NATIVE_DECODE)?;
            let limits = NativeEditLimits {
                max_source_bytes: NATIVE_PDF_BYTES,
                max_output_bytes: NATIVE_PDF_BYTES,
                max_retained_bytes: NATIVE_PDF_RETAINED
                    - (NATIVE_REQUEST_HISTORY * size_of::<(u64, [u8; 32])>() + 1024),
                ..NativeEditLimits::default()
            };
            let session = NativePdfSession::open_scoped(&bytes, limits, epoch)
                .map_err(|error| error.code())?;
            if !native_binding_valid(session.snapshot().binding) {
                return Err(NATIVE_BUDGET);
            }
            self.sessions.push(NativeRegistryEntry {
                session,
                origin_store_hash,
                base_outputs: VecDeque::with_capacity(NATIVE_REQUEST_HISTORY),
            });
            let entry = self.sessions.last().expect("session just inserted");
            response.binding = Some(entry.session.snapshot().binding);
            response.status = NativeStatus::Opened;
            response.snapshot = Some(entry.wire_snapshot());
            return Ok(());
        }
        let binding = binding.ok_or(NATIVE_BINDING)?;
        let index = self.sessions.iter().position(|entry| {
            native_same_session(entry.session.snapshot().binding, binding)
                && entry.origin_store_hash == store_hash
        });
        if matches!(request, NativeRequest::Close { .. }) {
            let consumed_request = if let Some(index) = index {
                let current = self.sessions[index].session.snapshot();
                if current.binding != binding {
                    return Err(NATIVE_REVISION);
                }
                current.consumed_request
            } else {
                self.closed
                    .iter()
                    .find(|(closed, hash, _)| *closed == binding && hash == store_hash)
                    .map(|(_, _, consumed_request)| *consumed_request)
                    .ok_or(NATIVE_BINDING)?
            };
            if request_id <= consumed_request {
                return Err("FLOW_PDF_NATIVE_REQUEST_INVALID");
            }
            if let Some(index) = index {
                let entry = self.sessions.swap_remove(index);
                if self.closed.len() == NATIVE_PDF_SESSIONS {
                    self.closed.pop_front();
                }
                self.closed
                    .push_back((binding, entry.origin_store_hash, consumed_request));
            }
            response.status = NativeStatus::Closed;
            return Ok(());
        }

        let entry = &mut self.sessions[index.ok_or(NATIVE_BINDING)?];
        let current = entry.session.snapshot();
        response.status = match request {
            NativeRequest::Command {
                output_hash,
                command,
                ..
            } => {
                entry.apply(binding, request_id, output_hash, &command)?;
                NativeStatus::Applied
            }
            NativeRequest::Snapshot { .. } => {
                match entry
                    .session
                    .lookup(binding, request_id)
                    .map_err(|error| error.code())?
                {
                    NativeRequestLookup::Accepted(_) => NativeStatus::Accepted,
                    NativeRequestLookup::Unseen => NativeStatus::Unseen,
                    NativeRequestLookup::ReconciliationRequired
                        if binding.revision < current.binding.revision =>
                    {
                        NativeStatus::Evicted
                    }
                    NativeRequestLookup::ReconciliationRequired => return Err(NATIVE_REVISION),
                }
            }
            _ => return Err(NATIVE_DECODE),
        };
        response.snapshot = Some(entry.wire_snapshot());
        Ok(())
    }
}

impl NativeRegistryEntry {
    fn apply(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        output_hash: [u8; 32],
        command: &NativeNoteCommand,
    ) -> Result<(), &'static str> {
        let current = self.session.snapshot();
        let previous_revision = current.binding.revision;
        let expected_output = match self
            .session
            .lookup(binding, request_id)
            .map_err(|error| error.code())?
        {
            NativeRequestLookup::Unseen if previous_revision == MAX_NATIVE_REVISION => {
                return Err(NATIVE_BUDGET);
            }
            NativeRequestLookup::Unseen => current.output_hash,
            NativeRequestLookup::Accepted(_) => self
                .base_outputs
                .iter()
                .find(|(id, _)| *id == request_id)
                .map(|(_, output)| *output)
                .ok_or(NATIVE_RECONCILE)?,
            NativeRequestLookup::ReconciliationRequired => return Err(NATIVE_RECONCILE),
        };
        if output_hash != expected_output {
            return Err(NATIVE_BINDING);
        }
        let receipt = self
            .session
            .apply(binding, request_id, command)
            .map_err(|error| error.code())?;
        if receipt.binding.revision < previous_revision {
            return Err(NATIVE_RECONCILE);
        }
        if receipt.binding.revision > previous_revision {
            if self.base_outputs.len() == NATIVE_REQUEST_HISTORY {
                self.base_outputs.pop_front();
            }
            self.base_outputs.push_back((request_id, output_hash));
        }
        Ok(())
    }

    fn wire_snapshot(&self) -> NativeWireSnapshot<&NativePdfNoteScene> {
        let snapshot = self.session.snapshot();
        NativeWireSnapshot {
            binding: snapshot.binding,
            origin_store_hash: self.origin_store_hash.clone(),
            output_hash: snapshot.output_hash,
            consumed_request: snapshot.consumed_request,
            scene: &snapshot.scene,
            output_hex: encode_hex(self.session.output_bytes()),
        }
    }
}

fn native_response<S>(operation: NativeOperation) -> NativeResponse<S> {
    NativeResponse {
        protocol_version: NATIVE_PDF_VERSION,
        operation,
        request_id: 0,
        binding: None,
        origin_store_hash: None,
        status: NativeStatus::Failed,
        limits: native_wire_limits(),
        snapshot: None,
        code: Some(NATIVE_DECODE.to_owned()),
        result_hash: String::new(),
    }
}

fn serialize_native_response<S: Serialize>(mut response: NativeResponse<S>) -> String {
    fn encode<S: Serialize>(response: &mut NativeResponse<S>) -> Result<String, serde_json::Error> {
        response.result_hash = native_result_hash(response)?;
        let mut output = NativeBoundedJson(Vec::new());
        serde_json::to_writer(&mut output, response)?;
        Ok(String::from_utf8(output.0).expect("JSON is UTF-8"))
    }

    encode(&mut response).unwrap_or_else(|_| {
        response.status = NativeStatus::Failed;
        response.snapshot = None;
        response.code = Some(NATIVE_BUDGET.to_owned());
        response.result_hash.clear();
        encode(&mut response).expect("bounded native error")
    })
}

fn native_dispatch(operation: NativeOperation, json: &str) -> String {
    let mut response = native_response::<&NativePdfNoteScene>(operation);
    let limit = if operation == NativeOperation::Open {
        NATIVE_PDF_REQUEST
    } else {
        NATIVE_PDF_COMMAND
    };
    if json.len() > limit {
        response.code = Some(NATIVE_BUDGET.to_owned());
        return serialize_native_response(response);
    }
    let Ok(request) = serde_json::from_str::<NativeRequest>(json) else {
        return serialize_native_response(response);
    };
    let (kind, version, request_id, binding, hash) = request.identity();
    if kind != operation
        || version != NATIVE_PDF_VERSION
        || request_id == 0
        || request_id > MAX_WIRE_ID
        || !native_hash_text(hash)
        || binding.is_some_and(|binding| !native_binding_valid(binding))
    {
        return serialize_native_response(response);
    }
    let origin_store_hash = hash.to_owned();
    NATIVE_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let mut response = native_response(operation);
        response.request_id = request_id;
        response.binding = binding;
        response.origin_store_hash = Some(origin_store_hash);
        response.code = registry
            .execute(request, &mut response)
            .err()
            .map(str::to_owned);
        serialize_native_response(response)
    })
}

#[wasm_bindgen]
pub fn native_pdf_open(request_json: String) -> String {
    native_dispatch(NativeOperation::Open, &request_json)
}

#[wasm_bindgen]
pub fn native_pdf_command(request_json: String) -> String {
    native_dispatch(NativeOperation::Command, &request_json)
}

#[wasm_bindgen]
pub fn native_pdf_snapshot(request_json: String) -> String {
    native_dispatch(NativeOperation::Snapshot, &request_json)
}

#[wasm_bindgen]
pub fn native_pdf_close(request_json: String) -> String {
    native_dispatch(NativeOperation::Close, &request_json)
}

impl NativeVerifiedScene {
    fn verify(&self) -> Option<()> {
        let mut writer = NativeHashWriter(blake3::Hasher::new(), 0);
        writer.0.update(b"flowpdf:accepted-note-scene:v2");
        writer.0.update(&self.output_hash);
        for (index, page) in self.pages.iter().enumerate() {
            if page.id as usize != index {
                return None;
            }
            serde_json::to_writer(
                &mut writer,
                &(page.object, &page.geometry, page.can_add_note),
            )
            .ok()?;
            for annotation in &page.annotations {
                match annotation {
                    NativeVerifiedAnnotation::PlainNote {
                        id,
                        object,
                        contents,
                        rect,
                    } => {
                        if *id > MAX_WIRE_ID {
                            return None;
                        }
                        serde_json::to_writer(&mut writer, &(object, rect, contents))
                    }
                    NativeVerifiedAnnotation::ReadOnly {
                        object,
                        profile,
                        rect,
                    } => serde_json::to_writer(&mut writer, &(object, profile, rect)),
                }
                .ok()?;
            }
        }
        (writer.0.finalize().as_bytes() == &self.scene_hash).then_some(())
    }
}

fn verify_native_snapshot(response: &NativeResponse<NativeVerifiedScene>) -> Option<()> {
    let snapshot = response.snapshot.as_ref()?;
    let binding = response.binding?;
    if !native_binding_valid(snapshot.binding)
        || !native_same_session(snapshot.binding, binding)
        || Some(&snapshot.origin_store_hash) != response.origin_store_hash.as_ref()
        || snapshot.consumed_request > MAX_WIRE_ID
        || snapshot.output_hash != snapshot.scene.output_hash
        || (snapshot.binding.revision == 0) != (snapshot.consumed_request == 0)
        || (snapshot.binding.revision == 0 && snapshot.binding.origin_hash != snapshot.output_hash)
    {
        return None;
    }
    let revision = snapshot.binding.revision;
    let base = binding.revision;
    let consumed = snapshot.consumed_request;
    let request = response.request_id;
    let correlated = match (response.operation, response.status) {
        (NativeOperation::Open, NativeStatus::Opened) => revision == 0 && base == 0,
        (NativeOperation::Command, NativeStatus::Applied) => {
            revision == base + 1 && consumed == request
        }
        (NativeOperation::Snapshot, NativeStatus::Accepted) => {
            revision > base && consumed >= request
        }
        (NativeOperation::Snapshot, NativeStatus::Evicted) => revision > base && consumed > request,
        (NativeOperation::Snapshot, NativeStatus::Unseen) => revision == base && consumed < request,
        _ => false,
    };
    if !correlated {
        return None;
    }
    let bytes = native_decode_pdf(&snapshot.output_hex).ok()?;
    if blake3::hash(&bytes).as_bytes() != &snapshot.output_hash {
        return None;
    }
    snapshot.scene.verify()
}

#[wasm_bindgen]
pub fn verify_native_pdf_response(response_json: String) -> bool {
    if response_json.len() > NATIVE_PDF_RESULT {
        return false;
    }
    let Ok(mut response) =
        serde_json::from_str::<NativeResponse<NativeVerifiedScene>>(&response_json)
    else {
        return false;
    };
    let supplied = std::mem::take(&mut response.result_hash);
    if response.protocol_version != NATIVE_PDF_VERSION
        || response.limits != native_wire_limits()
        || response.request_id > MAX_WIRE_ID
        || response
            .binding
            .is_some_and(|binding| !native_binding_valid(binding))
        || response
            .origin_store_hash
            .as_ref()
            .is_some_and(|hash| !native_hash_text(hash))
        || !native_hash_text(&supplied)
    {
        return false;
    }
    let binding_present = response.binding.is_some();
    let store_present = response.origin_store_hash.is_some();
    let valid = match (response.operation, response.status) {
        (_, NativeStatus::Failed) => {
            response.snapshot.is_none()
                && response.code.as_ref().is_some_and(|code| {
                    matches!(
                        code.as_str(),
                        NATIVE_DECODE
                            | NATIVE_BINDING
                            | NATIVE_REVISION
                            | NATIVE_RECONCILE
                            | NATIVE_BUDGET
                            | "FLOW_PDF_NATIVE_LIMITS_INVALID"
                            | "FLOW_PDF_NATIVE_UNSUPPORTED"
                            | "FLOW_PDF_NATIVE_GRAPH_MALFORMED"
                            | "FLOW_PDF_NATIVE_NOTE_INVALID"
                            | "FLOW_PDF_NATIVE_REQUEST_INVALID"
                            | "FLOW_PDF_NATIVE_REQUEST_CONFLICT"
                            | "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE"
                            | "FLOW_PDF_NATIVE_POSTCONDITION_FAILED"
                    )
                })
                && if response.request_id == 0 {
                    !binding_present && !store_present
                } else {
                    store_present
                        && (binding_present == (response.operation != NativeOperation::Open))
                }
        }
        (NativeOperation::Close, NativeStatus::Closed) => {
            response.request_id > 0
                && binding_present
                && store_present
                && response.snapshot.is_none()
                && response.code.is_none()
        }
        _ => {
            response.request_id > 0
                && response.code.is_none()
                && verify_native_snapshot(&response).is_some()
        }
    };
    valid && native_result_hash(&response).is_ok_and(|actual| actual == supplied)
}

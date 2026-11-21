//! Narrow serialized WebAssembly boundary for FlowPDF.

#![forbid(unsafe_code)]

use flow_core::pdf::native::{
    NATIVE_MAX_FIELD_NAME_BYTES, NATIVE_MAX_FIELD_TEXT_BYTES, NATIVE_MAX_GRAPHIC_POINTS,
    NATIVE_MAX_IMAGE_BYTES, NATIVE_MAX_IMAGE_DIMENSION, NATIVE_MAX_IMAGE_PIXELS,
    NATIVE_MAX_STROKE_WIDTH, NATIVE_PREVIEW_MAX_DIMENSION, NATIVE_PREVIEW_MAX_PIXELS,
    NATIVE_REDACTION_VALIDATOR_PROFILE, NATIVE_REDACTION_VALIDATOR_VERSION, NativeFieldCommand,
    NativeFieldRefusal, NativeFieldView, NativeObjectCommand, NativeObjectKind,
    NativeObjectOperation, NativeObjectPage, NativeObjectRefusal, NativePageCommand,
    NativePageState, NativeRedactionCandidateStatus, NativeRedactionMark,
    NativeRedactionMarkAction, NativeRedactionPageEvidence, NativeTextCommand, NativeTextPage,
    NativeTextRefusal, NativeVerificationLane, NativeVerificationStatus,
    VersionedNativeRedactionEvidence,
};
use flow_core::pdf::{
    NATIVE_MAX_NOTE_BYTES, NATIVE_REQUEST_HISTORY, NativeBinding, NativeEditError,
    NativeEditLimits, NativeNoteCommand, NativePdfNoteScene, NativePdfSession, NativeRequestLookup,
    NativeSessionError, PdfObjectRef,
};
use flow_core::{
    ApiResponse, ApplyCommandRequest, AssetStageRequest, AssetStageResponse, AuditedRecoverRequest,
    AuditedRecoverResult, CommandKind, CreateSampleRequest, EditorSessionRequest,
    EditorSessionResponse, EditorViewDto, EditorViewRequest, FontCatalog, FontFace,
    FontFaceIdentity, FormSessionRequest, FormSessionResponse, LayoutWasmFontInput,
    MigrateDocumentRequest, MigrateDocumentResult, OperationResult, PdfDocumentCandidate,
    PdfDocumentSummary, PdfExportManifest, PdfExportOptions, PdfExportRequest,
    PdfFontManifestIdentity, PdfFormPlan, PdfInternalLink, PdfMetadataOptions, PdfOcrCandidate,
    PdfOutlineEntry, PdfPagePlan, PdfReadError, PdfReaderLimits, PdfReconstructionRequest,
    PdfReconstructionResult, PdfRecoveryExpectation, PdfReproducibilityInputs, PdfReviewDecision,
    PdfScene, PdfSceneRequest, PlanPersistenceCommitRequest, PlanStandaloneAuditRequest,
    RecoverRequest, RecoverResult, UkrainianHyphenation, VOICE_PROTOCOL_VERSION,
    VoiceCommandRequest, VoiceIntent, accept_pdf_reconstruction as core_accept_pdf_reconstruction,
    export_pdf as core_export_pdf, open_pdf, read_pdf_scene, reconstruct_pdf_scene,
    recover_owned_source as core_recover_owned_source, store::PlannedPersistenceCommit,
    verify_pdf_document_candidate, verify_pdf_reconstruction_result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
const NATIVE_IMPORT_REFUSED: &str = "FLOW_PDF_NATIVE_IMPORT_REFUSED";
const NATIVE_DECODE: &str = "FLOW_PDF_NATIVE_REQUEST_DECODE";
const NATIVE_BINDING: &str = "FLOW_PDF_NATIVE_BINDING_MISMATCH";
const NATIVE_REVISION: &str = "FLOW_PDF_NATIVE_REVISION_MISMATCH";
const NATIVE_RECONCILE: &str = "FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED";
const NATIVE_BUDGET: &str = "FLOW_PDF_NATIVE_BUDGET_EXHAUSTED";
const NATIVE_POSTCONDITION: &str = "FLOW_PDF_NATIVE_POSTCONDITION_FAILED";
const NATIVE_REDACTION_BUSY: &str = "FLOW_PDF_NATIVE_REDACTION_BUSY";
const NATIVE_REDACTION_UNAVAILABLE: &str = "FLOW_PDF_NATIVE_REDACTION_CANDIDATE_UNAVAILABLE";
const NATIVE_REDACTION_UNVERIFIED: &str = "FLOW_PDF_NATIVE_REDACTION_UNVERIFIED";

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativeOperation {
    Open,
    Command,
    Snapshot,
    Close,
    Redaction,
    Preview,
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
    Redaction,
    Published,
    Preview,
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
        command: NativeWireCommand,
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
    Redaction {
        protocol_version: u32,
        request_id: u64,
        binding: NativeBinding,
        origin_store_hash: String,
        accepted_hash: [u8; 32],
        action: NativeWireRedactionAction,
    },
    Preview {
        protocol_version: u32,
        request_id: u64,
        binding: NativeBinding,
        origin_store_hash: String,
        output_hash: [u8; 32],
        page_id: u64,
    },
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum NativeWireRedactionAction {
    AddMark {
        page_id: u64,
        rect: [i64; 4],
    },
    RemoveMark {
        page_id: u64,
        rect: [i64; 4],
    },
    ClearMarks {},
    Stage {},
    Status {
        #[serde(deserialize_with = "native_required_option")]
        candidate: Option<NativeWireCandidate>,
    },
    Verify {
        candidate: NativeWireCandidate,
    },
    Publish {
        candidate: NativeWireCandidate,
    },
    ReadPublished {
        candidate: NativeWireCandidate,
    },
    Cancel {
        candidate: NativeWireCandidate,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativeRedactionActionKind {
    AddMark,
    RemoveMark,
    ClearMarks,
    Stage,
    Status,
    Verify,
    Publish,
    ReadPublished,
    Cancel,
}

impl NativeWireRedactionAction {
    fn kind(&self) -> NativeRedactionActionKind {
        use NativeRedactionActionKind as Kind;
        match self {
            Self::AddMark { .. } => Kind::AddMark,
            Self::RemoveMark { .. } => Kind::RemoveMark,
            Self::ClearMarks {} => Kind::ClearMarks,
            Self::Stage {} => Kind::Stage,
            Self::Status { .. } => Kind::Status,
            Self::Verify { .. } => Kind::Verify,
            Self::Publish { .. } => Kind::Publish,
            Self::ReadPublished { .. } => Kind::ReadPublished,
            Self::Cancel { .. } => Kind::Cancel,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeWireCandidate {
    hash: [u8; 32],
    validator_version: u32,
    validator_profile: String,
    affected_pages: Vec<u64>,
}

impl NativeWireCandidate {
    fn new(hash: [u8; 32], pages: &[NativeRedactionPageEvidence]) -> Self {
        Self {
            hash,
            validator_version: NATIVE_REDACTION_VALIDATOR_VERSION,
            validator_profile: NATIVE_REDACTION_VALIDATOR_PROFILE.to_owned(),
            affected_pages: pages.iter().map(|page| page.page_id).collect(),
        }
    }

    fn valid(&self) -> bool {
        self.validator_version == NATIVE_REDACTION_VALIDATOR_VERSION
            && self.validator_profile == NATIVE_REDACTION_VALIDATOR_PROFILE
            && (1..=256).contains(&self.affected_pages.len())
            && self.affected_pages.iter().enumerate().all(|(i, id)| {
                (1..=MAX_WIRE_ID).contains(id) && !self.affected_pages[..i].contains(id)
            })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativeWireVerificationStatus {
    Pending,
    Passed,
    Failed,
    Unavailable,
    Inconclusive,
}

impl From<NativeVerificationStatus> for NativeWireVerificationStatus {
    fn from(status: NativeVerificationStatus) -> Self {
        match status {
            NativeVerificationStatus::Pending => Self::Pending,
            NativeVerificationStatus::Passed => Self::Passed,
            NativeVerificationStatus::Failed => Self::Failed,
            NativeVerificationStatus::Unavailable => Self::Unavailable,
            NativeVerificationStatus::Inconclusive => Self::Inconclusive,
        }
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeWireRedactionCheck {
    status: NativeWireVerificationStatus,
    covered_pages: usize,
}

impl NativeWireRedactionCheck {
    fn new(
        lane: Option<&NativeVerificationLane>,
        affected: &[NativeRedactionPageEvidence],
    ) -> Self {
        use NativeWireVerificationStatus::*;
        match lane {
            None => Self {
                status: Pending,
                covered_pages: 0,
            },
            Some(lane) if lane.status() == NativeVerificationStatus::Passed => {
                if lane.covered_pages() == affected {
                    Self {
                        status: Passed,
                        covered_pages: affected.len(),
                    }
                } else {
                    Self {
                        status: Inconclusive,
                        covered_pages: 0,
                    }
                }
            }
            Some(lane) => Self {
                status: lane.status().into(),
                covered_pages: 0,
            },
        }
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeWireRedactionVerification {
    status: NativeWireVerificationStatus,
    structure: NativeWireRedactionCheck,
    text: NativeWireRedactionCheck,
    raster: NativeWireRedactionCheck,
}

impl NativeWireRedactionVerification {
    fn new(
        status: NativeVerificationStatus,
        evidence: Option<&VersionedNativeRedactionEvidence>,
        affected: &[NativeRedactionPageEvidence],
    ) -> Self {
        Self {
            status: status.into(),
            structure: NativeWireRedactionCheck::new(
                evidence.map(|value| value.structure()),
                affected,
            ),
            text: NativeWireRedactionCheck::new(evidence.map(|value| value.text()), affected),
            raster: NativeWireRedactionCheck::new(evidence.map(|value| value.raster()), affected),
        }
    }

    fn valid(&self, pages: usize) -> bool {
        use NativeWireVerificationStatus::*;
        let checks = [&self.structure, &self.text, &self.raster];
        if !checks
            .iter()
            .all(|check| check.covered_pages == if check.status == Passed { pages } else { 0 })
        {
            return false;
        }
        let statuses = checks.map(|check| check.status);
        match self.status {
            Pending => statuses == [Pending; 3],
            Passed => statuses == [Passed; 3],
            Failed => true,
            Unavailable => !statuses.contains(&Failed) && statuses.contains(&Unavailable),
            Inconclusive => {
                !statuses.contains(&Failed)
                    && !statuses.contains(&Unavailable)
                    && statuses != [Passed; 3]
                    && statuses != [Pending; 3]
            }
        }
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeWireRedaction {
    action: NativeRedactionActionKind,
    accepted_hash: [u8; 32],
    mark_count: usize,
    #[serde(deserialize_with = "native_required_option")]
    candidate: Option<NativeWireCandidate>,
    #[serde(deserialize_with = "native_required_option")]
    verification: Option<NativeWireRedactionVerification>,
    published: bool,
}

impl NativeWireRedaction {
    fn new(action: NativeRedactionActionKind, accepted_hash: [u8; 32], mark_count: usize) -> Self {
        Self {
            action,
            accepted_hash,
            mark_count,
            candidate: None,
            verification: None,
            published: false,
        }
    }

    fn set_candidate(&mut self, status: NativeRedactionCandidateStatus<'_>) {
        self.candidate = Some(NativeWireCandidate::new(
            status.identity.candidate_hash,
            status.affected_pages,
        ));
        self.verification = Some(NativeWireRedactionVerification::new(
            status.status(),
            status.evidence,
            status.affected_pages,
        ));
    }

    fn terminal_receipt(&self, binding: NativeBinding, request_id: u64) -> Option<blake3::Hash> {
        let mut writer = NativeHashWriter(blake3::Hasher::new(), 0);
        serde_json::to_writer(&mut writer, &(binding, request_id, self)).ok()?;
        Some(writer.0.finalize())
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NativeWireCommand {
    Note(NativeNoteCommand),
    Text(NativeTextCommand),
    Object(NativeObjectCommand),
    Page(NativePageCommand),
    Field(NativeFieldCommand),
}

#[derive(Deserialize)]
struct NativeCommandBounds {
    #[serde(rename = "command")]
    _command: NativeFieldInputBounds,
}

#[derive(Deserialize)]
struct NativeFieldInputBounds {
    #[serde(
        default,
        rename = "name",
        deserialize_with = "native_check_string_bound::<_, NATIVE_MAX_FIELD_NAME_BYTES>"
    )]
    _name: (),
    #[serde(
        default,
        rename = "value",
        deserialize_with = "native_check_string_bound::<_, NATIVE_MAX_FIELD_TEXT_BYTES>"
    )]
    _value: (),
}

fn native_check_string_bound<'de, D: serde::Deserializer<'de>, const LIMIT: usize>(
    deserializer: D,
) -> Result<(), D::Error> {
    struct Check<const LIMIT: usize>;
    impl<const LIMIT: usize> serde::de::Visitor<'_> for Check<LIMIT> {
        type Value = ();
        fn expecting(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(out, "a string of at most {LIMIT} UTF-8 bytes")
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<(), E> {
            if value.len() > LIMIT {
                return Err(E::custom("field input exceeds its byte limit"));
            }
            Ok(())
        }
    }
    deserializer.deserialize_str(Check::<LIMIT>)
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
            Self::Redaction {
                protocol_version,
                request_id,
                binding,
                origin_store_hash,
                ..
            } => (
                NativeOperation::Redaction,
                *protocol_version,
                *request_id,
                Some(*binding),
                origin_store_hash,
            ),
            Self::Preview {
                protocol_version,
                request_id,
                binding,
                origin_store_hash,
                ..
            } => (
                NativeOperation::Preview,
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
    image_bytes: usize,
    image_pixels: u64,
    image_dimension: u32,
    graphic_points: usize,
    graphic_stroke_width: i64,
    image_formats: [NativeImageFormat; 2],
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum NativeCountUnit {
    Utf8Bytes,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum NativeImageFormat {
    Png,
    Jpeg,
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
        image_bytes: NATIVE_MAX_IMAGE_BYTES,
        image_pixels: NATIVE_MAX_IMAGE_PIXELS,
        image_dimension: NATIVE_MAX_IMAGE_DIMENSION,
        graphic_points: NATIVE_MAX_GRAPHIC_POINTS,
        graphic_stroke_width: NATIVE_MAX_STROKE_WIDTH,
        image_formats: [NativeImageFormat::Png, NativeImageFormat::Jpeg],
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativeRedactionAdmission {
    Supported,
    Unavailable,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeWireSnapshot<S> {
    binding: NativeBinding,
    origin_store_hash: String,
    output_hash: [u8; 32],
    consumed_request: u64,
    page_state: NativePageState,
    redaction_admission: NativeRedactionAdmission,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fields: Vec<NativeFieldView>,
    scene: S,
    output_hex: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeWirePreview {
    output_hash: [u8; 32],
    page_id: u64,
    crop_box: [i64; 4],
    rotation: u16,
    width: usize,
    height: usize,
    rgb_hex: String,
}

impl NativeWirePreview {
    fn valid(&self, binding: NativeBinding) -> bool {
        let [l, b, r, t] = self.crop_box;
        if !(1..=MAX_WIRE_ID).contains(&self.page_id)
            || (binding.revision == 0 && binding.origin_hash != self.output_hash)
            || !self
                .crop_box
                .iter()
                .all(|v| (-1_000_000_000..=1_000_000_000).contains(v))
            || l >= r
            || b >= t
            || !matches!(self.rotation, 0 | 90 | 180 | 270)
        {
            return false;
        }
        let (w, h) = if matches!(self.rotation, 90 | 270) {
            (t - b, r - l)
        } else {
            (r - l, t - b)
        };
        // @contract: One RGB pixel per half point, rounded up at the crop edges.
        self.width == ((w + 499) / 500) as usize
            && self.height == ((h + 499) / 500) as usize
            && self.width <= NATIVE_PREVIEW_MAX_DIMENSION
            && self.height <= NATIVE_PREVIEW_MAX_DIMENSION
            && self.width * self.height <= NATIVE_PREVIEW_MAX_PIXELS
            && self.rgb_hex.len() == self.width * self.height * 6
            && native_lower_hex(&self.rgb_hex)
    }

    fn render_evidence(&self, binding: NativeBinding, request_id: u64) -> Option<blake3::Hash> {
        let mut writer = NativeHashWriter(blake3::Hasher::new(), 0);
        serde_json::to_writer(&mut writer, &(binding, request_id, self)).ok()?;
        Some(writer.0.finalize())
    }

    fn matches_live_session(
        &self,
        binding: NativeBinding,
        request_id: u64,
        origin_store_hash: &str,
    ) -> bool {
        NATIVE_REGISTRY.with(|registry| {
            registry.borrow().sessions.iter().any(|entry| {
                let current = entry.session.snapshot();
                current.binding == binding
                    && entry.origin_store_hash == origin_store_hash
                    && current.output_hash == self.output_hash
                    && entry.last_preview.is_some_and(|evidence| {
                        self.render_evidence(binding, request_id) == Some(evidence)
                    })
            })
        })
    }
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "native_present_option"
    )]
    redaction: Option<NativeWireRedaction>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "native_present_option"
    )]
    preview: Option<NativeWirePreview>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    text_pages: Vec<NativeTextPage>,
    object_pages: Vec<NativeObjectPage>,
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
    // @contract: One receipt per live session. Replay expires on revision advance or close.
    open_request_id: u64,
    base_outputs: VecDeque<(u64, [u8; 32])>,
    last_preview: Option<blake3::Hash>,
    last_cancel: Option<blake3::Hash>,
    last_empty_status: Option<blake3::Hash>,
}

#[derive(Default)]
struct NativeRegistry {
    sessions: Vec<NativeRegistryEntry>,
    closed: VecDeque<NativeClosedSession>,
}

struct NativeClosedSession {
    binding: NativeBinding,
    origin_store_hash: String,
    request_id: u64,
    consumed_request: u64,
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

fn native_present_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
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

struct NativeBoundedJson {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for NativeBoundedJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit - self.bytes.len() {
            return Err(std::io::Error::other("native result limit"));
        }
        self.bytes.extend_from_slice(bytes);
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

fn native_session_error_code(
    error: NativeSessionError,
    reader_refusal: &'static str,
) -> &'static str {
    match error {
        NativeSessionError::Graph(NativeEditError::Reader(
            PdfReadError::InputSizeLimit
            | PdfReadError::ObjectLimit
            | PdfReadError::NestingLimit
            | PdfReadError::TokenLimit
            | PdfReadError::StreamLimit
            | PdfReadError::DecodedStreamLimit
            | PdfReadError::PageCountLimit
            | PdfReadError::NumericLimit
            | PdfReadError::ImageLimit,
        )) => NATIVE_BUDGET,
        NativeSessionError::Graph(NativeEditError::Reader(_)) => reader_refusal,
        error => error.code(),
    }
}

fn native_edit_limits() -> NativeEditLimits {
    NativeEditLimits {
        max_source_bytes: NATIVE_PDF_BYTES,
        max_output_bytes: NATIVE_PDF_BYTES,
        max_retained_bytes: NATIVE_PDF_RETAINED
            - (NATIVE_REQUEST_HISTORY * size_of::<(u64, [u8; 32])>() + 1024),
        ..NativeEditLimits::default()
    }
}

impl NativeRegistry {
    fn open(
        &mut self,
        request_id: u64,
        epoch: [u8; 16],
        origin_store_hash: String,
        bytes_hex: &str,
        result_limit: usize,
    ) -> Result<String, &'static str> {
        let receipt = self.sessions.iter().find(|entry| {
            entry.open_request_id == request_id
                && entry.session.snapshot().binding.epoch == epoch
                && entry.origin_store_hash == origin_store_hash
        });
        if receipt.is_none() && self.sessions.len() == NATIVE_PDF_SESSIONS {
            return Err(NATIVE_BUDGET);
        }
        let bytes = native_decode_pdf(bytes_hex).map_err(|()| NATIVE_DECODE)?;
        if encode_hex(&Sha256::digest(&bytes)) != origin_store_hash {
            return Err(NATIVE_BINDING);
        }
        if let Some(entry) = receipt {
            if entry.session.snapshot().binding.revision != 0 {
                return Err(NATIVE_RECONCILE);
            }
            return entry.encode_open_response(result_limit);
        }
        let session = NativePdfSession::open_scoped(&bytes, native_edit_limits(), epoch)
            .map_err(|error| native_session_error_code(error, NATIVE_IMPORT_REFUSED))?;
        if !native_binding_valid(session.snapshot().binding) {
            return Err(NATIVE_BUDGET);
        }
        let entry = NativeRegistryEntry {
            session,
            origin_store_hash,
            open_request_id: request_id,
            base_outputs: VecDeque::with_capacity(NATIVE_REQUEST_HISTORY),
            last_preview: None,
            last_cancel: None,
            last_empty_status: None,
        };
        let reply = entry.encode_open_response(result_limit)?;
        self.sessions.push(entry);
        Ok(reply)
    }

    fn execute<'a>(
        &'a mut self,
        request: NativeRequest,
        response: &mut NativeResponse<&'a NativePdfNoteScene>,
    ) -> Result<(), &'static str> {
        let (_, _, request_id, binding, store_hash) = request.identity();
        let binding = binding.ok_or(NATIVE_BINDING)?;
        let index = self.sessions.iter().position(|entry| {
            native_same_session(entry.session.snapshot().binding, binding)
                && entry.origin_store_hash == store_hash
        });
        if matches!(request, NativeRequest::Close { .. }) {
            if let Some(index) = index {
                let entry = &self.sessions[index];
                entry
                    .session
                    .check_close(binding, request_id)
                    .map_err(|error| error.code())?;
                let consumed_request = entry.session.snapshot().consumed_request;
                let entry = self.sessions.swap_remove(index);
                if self.closed.len() == NATIVE_PDF_SESSIONS {
                    self.closed.pop_front();
                }
                self.closed.push_back(NativeClosedSession {
                    binding,
                    origin_store_hash: entry.origin_store_hash,
                    request_id,
                    consumed_request,
                });
            } else {
                let closed = self
                    .closed
                    .iter()
                    .find(|closed| {
                        closed.binding == binding && closed.origin_store_hash == store_hash
                    })
                    .ok_or(NATIVE_BINDING)?;
                if request_id <= closed.consumed_request {
                    return Err("FLOW_PDF_NATIVE_REQUEST_INVALID");
                }
                if request_id != closed.request_id {
                    return Err(NATIVE_BINDING);
                }
            }
            response.status = NativeStatus::Closed;
            return Ok(());
        }

        let entry = &mut self.sessions[index.ok_or(NATIVE_BINDING)?];
        response.status = match request {
            NativeRequest::Preview {
                output_hash,
                page_id,
                ..
            } => {
                let raster = entry
                    .session
                    .accepted_page_raster(binding, output_hash, page_id)
                    .map_err(|error| error.code())?;
                let preview = NativeWirePreview {
                    output_hash,
                    page_id,
                    crop_box: raster.crop_box,
                    rotation: raster.rotation,
                    width: raster.width,
                    height: raster.height,
                    rgb_hex: encode_hex(&raster.rgb),
                };
                entry.last_preview = Some(
                    preview
                        .render_evidence(binding, request_id)
                        .ok_or(NATIVE_BUDGET)?,
                );
                response.preview = Some(preview);
                response.status = NativeStatus::Preview;
                return Ok(());
            }
            NativeRequest::Redaction {
                accepted_hash,
                action,
                ..
            } => {
                let redaction = entry.redact(binding, request_id, accepted_hash, action)?;
                response.status = if matches!(
                    redaction.action,
                    NativeRedactionActionKind::Publish | NativeRedactionActionKind::ReadPublished
                ) {
                    response.snapshot = Some(entry.wire_snapshot());
                    NativeStatus::Published
                } else {
                    NativeStatus::Redaction
                };
                response.redaction = Some(redaction);
                return Ok(());
            }
            NativeRequest::Command {
                output_hash,
                command,
                ..
            } => {
                entry.apply(binding, request_id, output_hash, &command)?;
                NativeStatus::Applied
            }
            NativeRequest::Snapshot { .. } => entry.snapshot_status(binding, request_id)?,
            _ => return Err(NATIVE_DECODE),
        };
        response.snapshot = Some(entry.wire_snapshot());
        Ok(())
    }
}

impl NativeRegistryEntry {
    fn snapshot_status(
        &self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<NativeStatus, &'static str> {
        let revision = self.session.snapshot().binding.revision;
        match self
            .session
            .lookup(binding, request_id)
            .map_err(|error| error.code())?
        {
            NativeRequestLookup::Accepted(_) => Ok(NativeStatus::Accepted),
            NativeRequestLookup::Unseen => Ok(NativeStatus::Unseen),
            NativeRequestLookup::ReconciliationRequired
                if revision.saturating_sub(binding.revision) > NATIVE_REQUEST_HISTORY as u64 =>
            {
                Ok(NativeStatus::Evicted)
            }
            NativeRequestLookup::ReconciliationRequired => Err(NATIVE_REVISION),
        }
    }

    fn encode_open_response(&self, result_limit: usize) -> Result<String, &'static str> {
        let snapshot = self.wire_snapshot();
        let mut response = native_response(NativeOperation::Open);
        response.request_id = self.open_request_id;
        response.binding = Some(snapshot.binding);
        response.origin_store_hash = Some(self.origin_store_hash.clone());
        response.status = NativeStatus::Opened;
        response.snapshot = Some(snapshot);
        response.code = None;
        encode_native_response(&mut response, result_limit).map_err(|_| NATIVE_BUDGET)
    }

    fn redact(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        accepted_hash: [u8; 32],
        action: NativeWireRedactionAction,
    ) -> Result<NativeWireRedaction, &'static str> {
        use NativeWireRedactionAction::*;
        let current = self.session.snapshot();
        let mut result = NativeWireRedaction::new(
            action.kind(),
            accepted_hash,
            self.session.redaction_marks().len(),
        );
        if current.binding != binding {
            let expected = match &action {
                Status {
                    candidate: Some(candidate),
                }
                | Publish { candidate }
                | ReadPublished { candidate } => candidate,
                _ => return Err(NATIVE_REVISION),
            };
            self.published_redaction(binding, request_id, &mut result, expected)?;
            return Ok(result);
        }
        if current.output_hash != accepted_hash {
            return Err(NATIVE_BINDING);
        }
        let map_error = |error| native_session_error_code(error, NATIVE_POSTCONDITION);
        let mark_action = match action {
            AddMark { page_id, rect } => Some(NativeRedactionMarkAction::Add { page_id, rect }),
            RemoveMark { page_id, rect } => {
                Some(NativeRedactionMarkAction::Remove { page_id, rect })
            }
            ClearMarks {} => Some(NativeRedactionMarkAction::Clear),
            _ => None,
        };
        if let Some(mark_action) = mark_action {
            if self
                .session
                .is_redaction_mark_replay(binding, request_id, accepted_hash, mark_action)
                .map_err(map_error)?
            {
                return Ok(result);
            }
            if self.session.redaction_candidate_status().is_some() {
                return Err(NATIVE_REDACTION_BUSY);
            }
            if self
                .session
                .lookup(binding, request_id)
                .map_err(map_error)?
                != NativeRequestLookup::Unseen
            {
                return Err(NATIVE_RECONCILE);
            }
            match mark_action {
                NativeRedactionMarkAction::Add { page_id, rect } => {
                    let geometry = self
                        .session
                        .redaction_mark_geometry(page_id)
                        .map_err(map_error)?;
                    let mark = NativeRedactionMark {
                        page_id,
                        geometry,
                        rect,
                    };
                    self.session
                        .add_redaction_mark(binding, request_id, mark)
                        .map_err(map_error)?;
                }
                NativeRedactionMarkAction::Remove { page_id, rect } => {
                    let mark = self
                        .session
                        .redaction_marks()
                        .iter()
                        .find(|mark| mark.page_id == page_id && mark.rect == rect)
                        .copied()
                        .ok_or_else(|| map_error(NativeSessionError::InvalidRedactionMark))?;
                    self.session
                        .remove_redaction_mark(binding, request_id, mark)
                        .map_err(map_error)?;
                }
                NativeRedactionMarkAction::Clear => self
                    .session
                    .clear_redaction_marks(binding, request_id)
                    .map_err(map_error)?,
            }
            self.last_cancel = None;
            self.last_empty_status = None;
            result.mark_count = self.session.redaction_marks().len();
            return Ok(result);
        }
        if request_id <= current.consumed_request
            || self
                .session
                .last_redaction_mark()
                .is_some_and(|last| request_id <= last.request_id)
        {
            return Err(NativeSessionError::InvalidRequest.code());
        }
        match &action {
            Stage {} => {
                if binding.revision == MAX_NATIVE_REVISION {
                    return Err(NATIVE_BUDGET);
                }
                self.session
                    .stage_redaction(binding, request_id)
                    .map_err(map_error)?;
                let status = self.bound_candidate(binding, request_id, None)?;
                if status.status() != NativeVerificationStatus::Pending {
                    return Err(NATIVE_RECONCILE);
                }
                result.set_candidate(status);
                self.last_cancel = None;
                self.last_empty_status = None;
            }
            Status { candidate } => {
                if self.session.redaction_candidate_status().is_some() {
                    result.set_candidate(self.bound_candidate(
                        binding,
                        request_id,
                        candidate.as_ref(),
                    )?);
                } else {
                    self.last_empty_status = result.terminal_receipt(binding, request_id);
                }
            }
            Verify { candidate } => {
                self.bound_candidate(binding, request_id, Some(candidate))?;
                self.session
                    .verify_redaction(binding, request_id)
                    .map_err(map_error)?;
                result.set_candidate(self.bound_candidate(binding, request_id, Some(candidate))?);
            }
            Publish { candidate } => {
                self.bound_candidate(binding, request_id, Some(candidate))?;
                self.session
                    .publish_redaction(binding, request_id)
                    .map_err(map_error)?;
                self.published_redaction(binding, request_id, &mut result, candidate)?;
            }
            ReadPublished { .. } => return Err(NATIVE_REDACTION_UNVERIFIED),
            Cancel { candidate } => {
                self.bound_candidate(binding, request_id, Some(candidate))?;
                result.candidate = Some(candidate.clone());
                self.session
                    .cancel_redaction(binding, request_id)
                    .map_err(map_error)?;
                self.last_cancel = result.terminal_receipt(binding, request_id);
            }
            _ => return Err(NATIVE_DECODE),
        }
        Ok(result)
    }

    fn bound_candidate(
        &self,
        binding: NativeBinding,
        request_id: u64,
        expected: Option<&NativeWireCandidate>,
    ) -> Result<NativeRedactionCandidateStatus<'_>, &'static str> {
        let status = self
            .session
            .redaction_candidate_status()
            .ok_or(NATIVE_REDACTION_UNAVAILABLE)?;
        let identity = status.identity;
        if identity.binding != binding
            || identity.request_id != request_id
            || identity.accepted_hash != self.session.snapshot().output_hash
            || identity.validator_version != NATIVE_REDACTION_VALIDATOR_VERSION
            || identity.validator_profile != NATIVE_REDACTION_VALIDATOR_PROFILE
            || expected.is_some_and(|candidate| {
                *candidate
                    != NativeWireCandidate::new(identity.candidate_hash, status.affected_pages)
            })
        {
            return Err(NATIVE_BINDING);
        }
        Ok(status)
    }

    fn published_redaction(
        &self,
        binding: NativeBinding,
        request_id: u64,
        result: &mut NativeWireRedaction,
        expected: &NativeWireCandidate,
    ) -> Result<(), &'static str> {
        let current = self.session.snapshot();
        if current.binding.revision != binding.revision + 1
            || current.consumed_request != request_id
        {
            return Err(NATIVE_REVISION);
        }
        let evidence = self
            .session
            .accepted_redaction_evidence()
            .ok_or(NATIVE_REDACTION_UNVERIFIED)?;
        let identity = evidence.identity().ok_or(NATIVE_REDACTION_UNVERIFIED)?;
        if identity.binding() != binding
            || identity.request_id() != request_id
            || identity.accepted_hash() != result.accepted_hash
            || identity.candidate_hash() != current.output_hash
            || identity.validator_version() != expected.validator_version
            || identity.validator_profile() != expected.validator_profile
            || *expected != NativeWireCandidate::new(current.output_hash, identity.affected_pages())
            || !evidence.checks_passed()
        {
            return Err(NATIVE_BINDING);
        }
        result.mark_count = 0;
        result.candidate = Some(expected.clone());
        result.verification = Some(NativeWireRedactionVerification::new(
            NativeVerificationStatus::Passed,
            Some(evidence),
            identity.affected_pages(),
        ));
        result.published = true;
        Ok(())
    }

    fn matches_live_redaction(
        &self,
        binding: NativeBinding,
        request_id: u64,
        redaction: &NativeWireRedaction,
    ) -> bool {
        use NativeRedactionActionKind::*;
        if redaction.published {
            let Some(candidate) = redaction.candidate.as_ref() else {
                return false;
            };
            let mut expected =
                NativeWireRedaction::new(redaction.action, redaction.accepted_hash, 0);
            return self
                .published_redaction(binding, request_id, &mut expected, candidate)
                .is_ok()
                && expected == *redaction;
        }

        let current = self.session.snapshot();
        if current.binding != binding
            || current.output_hash != redaction.accepted_hash
            || self.session.redaction_marks().len() != redaction.mark_count
        {
            return false;
        }
        match redaction.action {
            AddMark | RemoveMark | ClearMarks => {
                self.session.last_redaction_mark().is_some_and(|last| {
                    let kind = match last.action {
                        NativeRedactionMarkAction::Add { .. } => AddMark,
                        NativeRedactionMarkAction::Remove { .. } => RemoveMark,
                        NativeRedactionMarkAction::Clear => ClearMarks,
                    };
                    last.binding == binding
                        && last.request_id == request_id
                        && last.accepted_hash == redaction.accepted_hash
                        && kind == redaction.action
                })
            }
            Cancel | Status if redaction.verification.is_none() => {
                let receipt = if redaction.action == Cancel {
                    self.last_cancel
                } else {
                    self.last_empty_status
                };
                self.session.redaction_candidate_status().is_none()
                    && receipt.is_some_and(|receipt| {
                        redaction.terminal_receipt(binding, request_id) == Some(receipt)
                    })
            }
            Stage | Status | Verify => self
                .bound_candidate(binding, request_id, redaction.candidate.as_ref())
                .is_ok_and(|status| {
                    redaction.verification
                        == Some(NativeWireRedactionVerification::new(
                            status.status(),
                            status.evidence,
                            status.affected_pages,
                        ))
                }),
            _ => false,
        }
    }

    fn apply(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        output_hash: [u8; 32],
        command: &NativeWireCommand,
    ) -> Result<(), &'static str> {
        if self.session.redaction_candidate_status().is_some() {
            return Err(NATIVE_REDACTION_BUSY);
        }
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
            NativeRequestLookup::Unseen
                if self
                    .session
                    .last_redaction_mark()
                    .is_some_and(|last| request_id <= last.request_id) =>
            {
                return Err(NATIVE_RECONCILE);
            }
            NativeRequestLookup::Unseen => current.output_hash,
            NativeRequestLookup::Accepted(_) => {
                // @contract: Snapshot history also records redaction publications.
                self.session
                    .ordinary_command_receipt(binding, request_id)
                    .map_err(|error| error.code())?
                    .ok_or(NATIVE_RECONCILE)?;
                self.base_outputs
                    .iter()
                    .find(|(id, _)| *id == request_id)
                    .map(|(_, output)| *output)
                    .ok_or(NATIVE_RECONCILE)?
            }
            NativeRequestLookup::ReconciliationRequired => return Err(NATIVE_RECONCILE),
        };
        if output_hash != expected_output {
            return Err(NATIVE_BINDING);
        }
        let receipt = match command {
            NativeWireCommand::Note(command) => self.session.apply(binding, request_id, command),
            NativeWireCommand::Text(command) => {
                self.session.apply_text(binding, request_id, command)
            }
            NativeWireCommand::Field(command) => {
                self.session.apply_field(binding, request_id, command)
            }
            NativeWireCommand::Page(command) => {
                self.session.apply_page(binding, request_id, command)
            }
            NativeWireCommand::Object(command) => {
                self.session.apply_object(binding, request_id, command)
            }
        }
        .map_err(|error| native_session_error_code(error, NATIVE_POSTCONDITION))?;
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
            page_state: snapshot.page_state.clone(),
            redaction_admission: if self.session.redaction_admitted() {
                NativeRedactionAdmission::Supported
            } else {
                NativeRedactionAdmission::Unavailable
            },
            fields: snapshot.fields.clone(),
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
        redaction: None,
        preview: None,
        code: Some(NATIVE_DECODE.to_owned()),
        result_hash: String::new(),
    }
}

fn encode_native_response<S: Serialize>(
    response: &mut NativeResponse<S>,
    limit: usize,
) -> Result<String, serde_json::Error> {
    response.result_hash = native_result_hash(response)?;
    let mut output = NativeBoundedJson {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, response)?;
    Ok(String::from_utf8(output.bytes).expect("JSON is UTF-8"))
}

fn serialize_native_response<S: Serialize>(mut response: NativeResponse<S>) -> String {
    encode_native_response(&mut response, NATIVE_PDF_RESULT).unwrap_or_else(|_| {
        response.status = NativeStatus::Failed;
        response.snapshot = None;
        response.redaction = None;
        response.preview = None;
        response.code = Some(NATIVE_BUDGET.to_owned());
        response.result_hash.clear();
        encode_native_response(&mut response, NATIVE_PDF_RESULT).expect("bounded native error")
    })
}

fn native_dispatch(operation: NativeOperation, json: &str) -> String {
    native_dispatch_with_result_limit(operation, json, NATIVE_PDF_RESULT)
}

fn native_dispatch_with_result_limit(
    operation: NativeOperation,
    json: &str,
    open_result_limit: usize,
) -> String {
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
    // @security: Check field string sizes before the tagged command decoder owns their values.
    if operation == NativeOperation::Command
        && serde_json::from_str::<NativeCommandBounds>(json).is_err()
    {
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
        response.code = match request {
            NativeRequest::Open {
                epoch,
                origin_store_hash,
                bytes_hex,
                ..
            } => match registry.open(
                request_id,
                epoch,
                origin_store_hash,
                &bytes_hex,
                open_result_limit,
            ) {
                Ok(reply) => return reply,
                Err(code) => Some(code.to_owned()),
            },
            _ => registry
                .execute(request, &mut response)
                .err()
                .map(str::to_owned),
        };
        serialize_native_response(response)
    })
}

#[wasm_bindgen]
pub fn native_pdf_open(request_json: String) -> String {
    native_dispatch(NativeOperation::Open, &request_json)
}

#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub fn native_pdf_open_with_result_limit(request_json: String, limit: usize) -> String {
    native_dispatch_with_result_limit(
        NativeOperation::Open,
        &request_json,
        limit.min(NATIVE_PDF_RESULT),
    )
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

#[wasm_bindgen]
pub fn native_pdf_redaction(request_json: String) -> String {
    native_dispatch(NativeOperation::Redaction, &request_json)
}

#[wasm_bindgen]
pub fn native_pdf_preview(request_json: String) -> String {
    native_dispatch(NativeOperation::Preview, &request_json)
}

impl NativeVerifiedScene {
    fn verify(&self) -> Option<()> {
        use NativeObjectKind::{Graphic, Image};
        use NativeObjectOperation::{Move, Remove, Replace};

        if self
            .object_pages
            .iter()
            .flat_map(|page| &page.objects)
            .any(|object| {
                !matches!(
                    (object.kind, object.operations.as_slice()),
                    (_, [Replace, Move, Remove])
                        | (Graphic, [Move, Remove] | [Remove])
                        | (Image, [Replace, Remove])
                )
            })
        {
            return None;
        }
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
        if !self.text_pages.is_empty() {
            writer.0.update(b"flowpdf:native-text-pages:v1");
            serde_json::to_writer(&mut writer, &self.text_pages).ok()?;
        }
        writer.0.update(b"flowpdf:native-object-pages:v1");
        serde_json::to_writer(&mut writer, &self.object_pages).ok()?;
        (writer.0.finalize().as_bytes() == &self.scene_hash).then_some(())
    }
}

fn verify_native_snapshot(
    response: &NativeResponse<NativeVerifiedScene>,
    require_live_session: bool,
) -> Option<()> {
    let snapshot = response.snapshot.as_ref()?;
    let binding = response.binding?;
    if !native_binding_valid(snapshot.binding)
        || !native_same_session(snapshot.binding, binding)
        || Some(&snapshot.origin_store_hash) != response.origin_store_hash.as_ref()
        || snapshot.consumed_request > MAX_WIRE_ID
        || snapshot.binding.revision > snapshot.consumed_request
        || snapshot.output_hash != snapshot.scene.output_hash
        || (snapshot.binding.revision == 0) != (snapshot.consumed_request == 0)
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
        (NativeOperation::Redaction, NativeStatus::Published) => {
            revision == base + 1 && consumed == request
        }
        (NativeOperation::Snapshot, NativeStatus::Accepted) => {
            (revision == base + 1 && consumed == request)
                || (revision > base + 1 && consumed > request)
        }
        (NativeOperation::Snapshot, NativeStatus::Evicted) => {
            revision.saturating_sub(base) > NATIVE_REQUEST_HISTORY as u64 && consumed > request
        }
        (NativeOperation::Snapshot, NativeStatus::Unseen) => revision == base && consumed < request,
        _ => false,
    };
    if !correlated {
        return None;
    }
    let bytes = native_decode_pdf(&snapshot.output_hex).ok()?;
    if blake3::hash(&bytes).as_bytes() != &snapshot.output_hash
        || (revision == 0 && encode_hex(&Sha256::digest(&bytes)) != snapshot.origin_store_hash)
    {
        return None;
    }
    snapshot.scene.verify()?;
    let claimed_notes: Vec<_> = snapshot
        .scene
        .pages
        .iter()
        .flat_map(|page| {
            page.annotations
                .iter()
                .filter_map(move |annotation| match annotation {
                    NativeVerifiedAnnotation::PlainNote { object, id, .. } => {
                        Some((page.id, *object, *id))
                    }
                    _ => None,
                })
        })
        .collect();
    NATIVE_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let entry = registry
            .sessions
            .iter()
            .find(|entry| native_same_session(entry.session.snapshot().binding, snapshot.binding));
        if (require_live_session && entry.is_none())
            || entry.is_some_and(|entry| {
                entry.origin_store_hash != snapshot.origin_store_hash
                    || entry.session.snapshot().consumed_request != consumed
                    || (response.operation == NativeOperation::Open
                        && entry.open_request_id != request)
                    || (require_live_session
                        && response.operation == NativeOperation::Snapshot
                        && entry.snapshot_status(binding, request).ok() != Some(response.status))
            })
        {
            return None;
        }
        if require_live_session
            && matches!(
                (response.operation, response.status),
                (NativeOperation::Command, NativeStatus::Applied)
            )
        {
            let receipt = entry?
                .session
                .ordinary_command_receipt(binding, request)
                .ok()??;
            if receipt.binding != snapshot.binding
                || receipt.request_id != request
                || receipt.output_hash != snapshot.output_hash
                || receipt.scene_hash != snapshot.scene.scene_hash
            {
                return None;
            }
        }
        // @contract: Wire imports already fit the redaction origin ceiling.
        NativePdfSession::verify_page_and_field_projection_with_objects(
            &bytes,
            native_edit_limits(),
            snapshot.binding,
            snapshot.scene.scene_hash,
            Some(&snapshot.scene.object_pages),
            &snapshot.page_state,
            &snapshot.fields,
            &claimed_notes,
            matches!(
                snapshot.redaction_admission,
                NativeRedactionAdmission::Supported
            ),
            entry.map(|entry| &entry.session),
        )
        .ok()?
        .then_some(())
    })
}

fn verify_native_live_redaction(response: &NativeResponse<NativeVerifiedScene>) -> Option<()> {
    let redaction = response.redaction.as_ref()?;
    let binding = response.binding?;
    let origin_store_hash = response.origin_store_hash.as_deref()?;
    NATIVE_REGISTRY
        .with(|registry| {
            registry.borrow().sessions.iter().any(|entry| {
                entry.origin_store_hash == origin_store_hash
                    && native_same_session(entry.session.snapshot().binding, binding)
                    && entry.matches_live_redaction(binding, response.request_id, redaction)
            })
        })
        .then_some(())
}

fn verify_native_redaction_response(
    response: &NativeResponse<NativeVerifiedScene>,
    require_live_session: bool,
) -> Option<()> {
    use NativeRedactionActionKind::*;
    use NativeWireVerificationStatus::{Passed, Pending};
    let redaction = response.redaction.as_ref()?;
    let binding = response.binding?;
    response.origin_store_hash.as_ref()?;
    if response.request_id == 0
        || response.code.is_some()
        || redaction.mark_count > 1024
        || (binding.revision == 0 && redaction.accepted_hash != binding.origin_hash)
    {
        return None;
    }
    if redaction.action == Cancel {
        let candidate = redaction.candidate.as_ref()?;
        return (response.status == NativeStatus::Redaction
            && response.snapshot.is_none()
            && !redaction.published
            && redaction.verification.is_none()
            && candidate.valid()
            && candidate.affected_pages.len() <= redaction.mark_count)
            .then_some(());
    }
    let verdict = match (&redaction.candidate, &redaction.verification) {
        (Some(candidate), Some(verification))
            if candidate.valid()
                && verification.valid(candidate.affected_pages.len())
                && if redaction.published {
                    redaction.mark_count == 0
                } else {
                    candidate.affected_pages.len() <= redaction.mark_count
                } =>
        {
            Some(verification.status)
        }
        (None, None) => None,
        _ => return None,
    };
    if redaction.published && verdict != Some(Passed) {
        return None;
    }
    if response.status == NativeStatus::Published {
        let candidate = redaction.candidate.as_ref()?;
        let snapshot = response.snapshot.as_ref()?;
        return (matches!(redaction.action, Publish | ReadPublished)
            && redaction.published
            && snapshot.output_hash == candidate.hash
            && snapshot
                .page_state
                .order
                .iter()
                .copied()
                .filter(|id| candidate.affected_pages.contains(id))
                .eq(candidate.affected_pages.iter().copied())
            && verify_native_snapshot(response, require_live_session).is_some())
        .then_some(());
    }
    if response.snapshot.is_some() {
        return None;
    }
    let valid = match redaction.action {
        AddMark | RemoveMark | ClearMarks => {
            !redaction.published
                && verdict.is_none()
                && (redaction.action != ClearMarks || redaction.mark_count == 0)
        }
        Stage => !redaction.published && verdict == Some(Pending),
        Verify => !redaction.published && verdict.is_some_and(|status| status != Pending),
        Status => true,
        Publish | ReadPublished | Cancel => false,
    };
    valid.then_some(())
}

#[wasm_bindgen]
pub fn verify_native_pdf_response(response_json: String) -> bool {
    verify_native_pdf_response_inner(&response_json, false)
}

#[wasm_bindgen]
pub fn verify_native_pdf_worker_response(response_json: String) -> bool {
    verify_native_pdf_response_inner(&response_json, true)
}

fn verify_native_pdf_response_inner(response_json: &str, require_live_session: bool) -> bool {
    if response_json.len() > NATIVE_PDF_RESULT {
        return false;
    }
    let Ok(mut response) =
        serde_json::from_str::<NativeResponse<NativeVerifiedScene>>(response_json)
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
    if response.redaction.is_some()
        != (response.operation == NativeOperation::Redaction
            && response.status != NativeStatus::Failed)
        || response.preview.is_some()
            != (response.operation == NativeOperation::Preview
                && response.status == NativeStatus::Preview)
    {
        return false;
    }
    let valid = match (response.operation, response.status) {
        (_, NativeStatus::Failed) => {
            response.snapshot.is_none()
                && response.code.as_ref().is_some_and(|code| {
                    if matches!(
                        code.as_str(),
                        NATIVE_REDACTION_BUSY
                            | NATIVE_REDACTION_UNAVAILABLE
                            | NATIVE_REDACTION_UNVERIFIED
                            | "FLOW_PDF_NATIVE_REDACTION_MARK_INVALID"
                    ) {
                        return response.request_id > 0
                            && (response.operation == NativeOperation::Redaction
                                || (response.operation == NativeOperation::Command
                                    && code == NATIVE_REDACTION_BUSY));
                    }
                    if code == NATIVE_IMPORT_REFUSED {
                        return response.operation == NativeOperation::Open
                            && response.request_id > 0;
                    }
                    if code == NativeSessionError::PageGeometry.code() {
                        return response.operation == NativeOperation::Command
                            && response.request_id > 0;
                    }
                    if [
                        NativeFieldRefusal::Profile,
                        NativeFieldRefusal::Reference,
                        NativeFieldRefusal::Appearance,
                        NativeFieldRefusal::Geometry,
                        NativeFieldRefusal::Glyph,
                    ]
                    .iter()
                    .any(|reason| reason.code() == code)
                    {
                        return response.operation == NativeOperation::Command
                            && response.request_id > 0;
                    }
                    if [
                        NativeObjectRefusal::State,
                        NativeObjectRefusal::Geometry,
                        NativeObjectRefusal::Asset,
                        NativeObjectRefusal::Profile,
                    ]
                    .iter()
                    .any(|reason| reason.code() == code)
                    {
                        return response.operation == NativeOperation::Command
                            && response.request_id > 0;
                    }
                    if [
                        NativeTextRefusal::Font,
                        NativeTextRefusal::Mapping,
                        NativeTextRefusal::Layout,
                        NativeTextRefusal::Ownership,
                        NativeTextRefusal::Overflow,
                    ]
                    .iter()
                    .any(|reason| reason.code() == code)
                    {
                        return response.operation == NativeOperation::Command
                            && response.request_id > 0;
                    }
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
                            | "FLOW_PDF_NATIVE_FINAL_PAGE"
                            | NATIVE_POSTCONDITION
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
                && response.snapshot.is_none()
                && response.code.is_none()
                && response
                    .binding
                    .zip(response.origin_store_hash.as_deref())
                    .is_some_and(|(binding, store_hash)| {
                        !require_live_session
                            || NATIVE_REGISTRY.with(|registry| {
                                registry.borrow().closed.iter().any(|closed| {
                                    closed.binding == binding
                                        && closed.origin_store_hash == store_hash
                                        && closed.request_id == response.request_id
                                })
                            })
                    })
        }
        (NativeOperation::Redaction, NativeStatus::Redaction | NativeStatus::Published) => {
            verify_native_redaction_response(&response, require_live_session).is_some()
                && (!require_live_session || verify_native_live_redaction(&response).is_some())
        }
        (NativeOperation::Preview, NativeStatus::Preview) => {
            response.request_id > 0
                && response.snapshot.is_none()
                && response.code.is_none()
                && response
                    .binding
                    .zip(response.origin_store_hash.as_deref())
                    .zip(response.preview.as_ref())
                    .is_some_and(|((binding, store_hash), preview)| {
                        preview.valid(binding)
                            && (!require_live_session
                                || preview.matches_live_session(
                                    binding,
                                    response.request_id,
                                    store_hash,
                                ))
                    })
        }
        _ => {
            response.request_id > 0
                && response.code.is_none()
                && verify_native_snapshot(&response, require_live_session).is_some()
        }
    };
    valid && native_result_hash(&response).is_ok_and(|actual| actual == supplied)
}

#[cfg(test)]
mod native_wire_tests {
    use super::*;

    #[test]
    fn nested_reader_failures_use_verifiable_native_codes() {
        let binding = NativeBinding {
            epoch: [1; 16],
            session_id: 1,
            origin_hash: [2; 32],
            revision: 0,
        };
        for (operation, reader_refusal) in [
            (NativeOperation::Open, NATIVE_IMPORT_REFUSED),
            (NativeOperation::Command, NATIVE_POSTCONDITION),
        ] {
            for (reader_error, expected) in [
                (PdfReadError::DecodedStreamLimit, NATIVE_BUDGET),
                (PdfReadError::TokenLimit, NATIVE_BUDGET),
                (PdfReadError::Syntax, reader_refusal),
            ] {
                let mut response = native_response::<NativeVerifiedScene>(operation);
                response.request_id = 2;
                response.binding = (operation == NativeOperation::Command).then_some(binding);
                response.origin_store_hash = Some("ab".repeat(32));
                let error = NativeEditError::Reader(reader_error).into();
                let code = native_session_error_code(error, reader_refusal);
                assert_eq!(code, expected);
                response.code = Some(code.to_owned());
                assert!(verify_native_pdf_response(serialize_native_response(
                    response
                )));
            }
        }
    }
}

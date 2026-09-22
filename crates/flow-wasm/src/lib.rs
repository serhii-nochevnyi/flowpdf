//! Narrow serialized WebAssembly boundary for FlowPDF.

#![forbid(unsafe_code)]

use flow_core::{
    ApiResponse, ApplyCommandRequest, AssetStageRequest, AssetStageResponse, AuditedRecoverRequest,
    AuditedRecoverResult, CommandKind, CreateSampleRequest, EditorSessionRequest,
    EditorSessionResponse, EditorViewDto, EditorViewRequest, FontCatalog, FontFace,
    FontFaceIdentity, FormSessionRequest, FormSessionResponse, LayoutWasmFontInput,
    MigrateDocumentRequest, MigrateDocumentResult, OperationResult, PdfExportManifest,
    PdfExportOptions, PdfExportRequest, PdfFontManifestIdentity, PdfFormPlan, PdfInternalLink,
    PdfMetadataOptions, PdfOutlineEntry, PdfPagePlan, PdfRecoveryExpectation,
    PdfReproducibilityInputs, PlanPersistenceCommitRequest, PlanStandaloneAuditRequest,
    RecoverRequest, RecoverResult, UkrainianHyphenation, VOICE_PROTOCOL_VERSION,
    VoiceCommandRequest, VoiceIntent, export_pdf as core_export_pdf,
    recover_owned_source as core_recover_owned_source, store::PlannedPersistenceCommit,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

const SUPPORTED_OLDER_FIXTURE: &str = include_str!("../../../fixtures/flowdoc/older.json");
const PDF_PROTOCOL_VERSION: u32 = 1;
const MAX_PDF_PROTOCOL_REQUEST_BYTES: usize = 96 * 1024 * 1024;
const MAX_PDF_PROTOCOL_HEX_BYTES: usize = flow_core::MAX_PDF_SOURCE_STREAM_BYTES;
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
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
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

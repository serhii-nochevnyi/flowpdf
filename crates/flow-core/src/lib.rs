//! Canonical, deterministic FlowPDF document semantics.
//!
//! JavaScript transports the DTOs in this module, but it never receives a
//! mutable document handle. Creation, commands, hashing, recovery validation,
//! provenance, and audit redaction all stay on this side of the WASM boundary.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 1;
const MAX_CANONICAL_BYTES: usize = 4 * 1024 * 1024;
const SAMPLE_DOCUMENT_ID: &str = "00000000-0000-4000-8000-000000000001";
const SAMPLE_PARAGRAPH_ID: &str = "00000000-0000-4000-8000-000000000101";
const SAMPLE_CREATE_COMMAND_ID: &str = "00000000-0000-4000-8000-000000000201";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FlowDocument {
    pub schema_version: u32,
    pub document_id: String,
    pub revision: u32,
    pub locale: String,
    pub page_settings: PageSettings,
    pub styles: Vec<StyleDefinition>,
    pub content: Vec<ContentNode>,
    pub assets: Vec<AssetDescriptor>,
    pub fields: Vec<FieldDescriptor>,
    pub provenance: CreateProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageSettings {
    pub page_size: PageSize,
    pub orientation: PageOrientation,
    pub margins_millimetres: PageMargins,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PageSize {
    A4,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PageOrientation {
    Portrait,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageMargins {
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
    pub left: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StyleDefinition {
    pub id: String,
    pub name: String,
    pub font_family: String,
    pub font_size_points: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContentNode {
    pub id: String,
    pub kind: ContentNodeKind,
    pub style_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ContentNodeKind {
    Paragraph,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AssetDescriptor {
    pub id: String,
    pub content_hash: String,
    pub media_type: String,
    pub byte_length: u32,
    pub alt_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FieldDescriptor {
    pub id: String,
    pub name: String,
    pub field_type: FieldType,
    pub anchor: LogicalPosition,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FieldType {
    SingleLineText,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreateProvenance {
    pub kind: ProvenanceKind,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProvenanceKind {
    LocalSample,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogicalPosition {
    pub node_id: String,
    pub utf16_offset: u32,
    pub affinity: Affinity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Affinity {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreateSampleRequest {
    pub requested_locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCommandRequest {
    pub canonical_json: String,
    pub command: CommandDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandDto {
    pub command_id: String,
    pub base_revision: u32,
    pub modality: SourceModality,
    pub issued_at: String,
    pub kind: CommandKind,
    pub target: LogicalPosition,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SourceModality {
    Ui,
    Keyboard,
    Voice,
    Api,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CommandKind {
    InsertText,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRecord {
    pub document_id: String,
    pub revision: u32,
    pub schema_version: u32,
    pub canonical_json: String,
    pub canonical_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransactionRecord {
    pub transaction_id: String,
    pub document_id: String,
    pub command_id: String,
    pub base_revision: u32,
    pub new_revision: u32,
    pub command_type: String,
    pub modality: SourceModality,
    pub before_hash: String,
    pub after_hash: String,
    pub forward_operations: Vec<Operation>,
    pub inverse_operations: Vec<Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Operation {
    CreateDocument,
    DeleteDocument,
    InsertText {
        target: LogicalPosition,
        text: String,
    },
    DeleteText {
        target: LogicalPosition,
        utf16_length: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    pub audit_id: String,
    pub document_id: String,
    pub command_id: String,
    pub base_revision: u32,
    pub new_revision: u32,
    pub timestamp: String,
    pub command_type: String,
    pub modality: SourceModality,
    pub outcome: AuditOutcome,
    pub error_code: Option<String>,
    pub safe_metadata: Vec<SafeMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuditOutcome {
    Applied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SafeMetadata {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersistenceCommit {
    pub replace_existing: bool,
    pub snapshot: SnapshotRecord,
    pub transaction: TransactionRecord,
    pub audit: AuditRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub canonical_json: String,
    pub canonical_hash: String,
    pub document_id: String,
    pub revision: u32,
    pub next_command_target: LogicalPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectorView {
    pub document_id: String,
    pub schema_version: u32,
    pub revision: u32,
    pub canonical_hash: String,
    pub locale: String,
    pub document_summary: String,
    pub provenance: String,
    pub audit: Vec<AuditRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub session: SessionDto,
    pub view: InspectorView,
    pub commit: PersistenceCommit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoverRequest {
    pub snapshot: SnapshotRecord,
    pub transactions: Vec<TransactionRecord>,
    pub audits: Vec<AuditRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoverResult {
    pub session: SessionDto,
    pub view: InspectorView,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse<T> {
    pub ok: bool,
    pub value: Option<T>,
    pub error: Option<ErrorDto>,
}

impl<T> ApiResponse<T> {
    #[must_use]
    pub fn success(value: T) -> Self {
        Self {
            ok: true,
            value: Some(value),
            error: None,
        }
    }

    #[must_use]
    pub fn failure(error: CoreError) -> Self {
        Self {
            ok: false,
            value: None,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDto {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    #[error("The boundary request could not be decoded")]
    Decode,
    #[error("The canonical payload is not valid JSON")]
    InvalidCanonicalJson,
    #[error("The canonical payload is not in the deterministic representation")]
    NonCanonicalPayload,
    #[error("The canonical payload exceeds the bounded size")]
    PayloadTooLarge,
    #[error("The document schema version is unsupported")]
    UnsupportedSchema,
    #[error("The document invariant is invalid")]
    InvalidDocument,
    #[error("The command uses a stale base revision")]
    StaleRevision,
    #[error("The command identifier is invalid")]
    InvalidCommandId,
    #[error("The command target does not exist")]
    InvalidTarget,
    #[error("The command range is invalid")]
    InvalidRange,
    #[error("The persisted record set is incomplete or discontinuous")]
    RecoveryGap,
    #[error("The persisted record hash does not match canonical content")]
    HashMismatch,
    #[error("The persisted audit record violates the redaction allowlist")]
    UnsafeAuditRecord,
}

impl From<CoreError> for ErrorDto {
    fn from(error: CoreError) -> Self {
        let code = match error {
            CoreError::Decode => "FLOW_DECODE_ERROR",
            CoreError::InvalidCanonicalJson => "FLOW_INVALID_CANONICAL_JSON",
            CoreError::NonCanonicalPayload => "FLOW_NON_CANONICAL_PAYLOAD",
            CoreError::PayloadTooLarge => "FLOW_PAYLOAD_TOO_LARGE",
            CoreError::UnsupportedSchema => "FLOW_UNSUPPORTED_SCHEMA",
            CoreError::InvalidDocument => "FLOW_INVALID_DOCUMENT",
            CoreError::StaleRevision => "FLOW_STALE_REVISION",
            CoreError::InvalidCommandId => "FLOW_INVALID_COMMAND_ID",
            CoreError::InvalidTarget => "FLOW_INVALID_TARGET",
            CoreError::InvalidRange => "FLOW_INVALID_RANGE",
            CoreError::RecoveryGap => "FLOW_RECOVERY_GAP",
            CoreError::HashMismatch => "FLOW_HASH_MISMATCH",
            CoreError::UnsafeAuditRecord => "FLOW_UNSAFE_AUDIT_RECORD",
        };
        Self {
            code: code.to_owned(),
            message: error.to_string(),
        }
    }
}

#[must_use]
pub fn decode_failure<T>() -> ApiResponse<T> {
    ApiResponse::failure(CoreError::Decode)
}

#[must_use]
pub fn create_sample(request: CreateSampleRequest) -> ApiResponse<OperationResult> {
    match create_sample_inner(request) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => ApiResponse::failure(error),
    }
}

fn create_sample_inner(request: CreateSampleRequest) -> Result<OperationResult, CoreError> {
    if request.requested_locale != "uk-UA" && request.requested_locale != "en-US" {
        return Err(CoreError::InvalidDocument);
    }

    let document = FlowDocument {
        schema_version: SCHEMA_VERSION,
        document_id: SAMPLE_DOCUMENT_ID.to_owned(),
        revision: 1,
        locale: request.requested_locale,
        page_settings: PageSettings {
            page_size: PageSize::A4,
            orientation: PageOrientation::Portrait,
            margins_millimetres: PageMargins {
                top: 20,
                right: 20,
                bottom: 20,
                left: 20,
            },
        },
        styles: vec![StyleDefinition {
            id: "00000000-0000-4000-8000-000000000301".to_owned(),
            name: "Body".to_owned(),
            font_family: "Noto Sans".to_owned(),
            font_size_points: 12,
        }],
        content: vec![ContentNode {
            id: SAMPLE_PARAGRAPH_ID.to_owned(),
            kind: ContentNodeKind::Paragraph,
            style_id: "00000000-0000-4000-8000-000000000301".to_owned(),
            text: "Український тестовий документ. English sample document.".to_owned(),
        }],
        assets: vec![AssetDescriptor {
            id: "00000000-0000-4000-8000-000000000401".to_owned(),
            content_hash: format!("blake3:{}", "0".repeat(64)),
            media_type: "image/png".to_owned(),
            byte_length: 0,
            alt_text: "Sample asset descriptor".to_owned(),
        }],
        fields: vec![FieldDescriptor {
            id: "00000000-0000-4000-8000-000000000501".to_owned(),
            name: "sample-name".to_owned(),
            field_type: FieldType::SingleLineText,
            anchor: LogicalPosition {
                node_id: SAMPLE_PARAGRAPH_ID.to_owned(),
                utf16_offset: 0,
                affinity: Affinity::Forward,
            },
            required: false,
        }],
        provenance: CreateProvenance {
            kind: ProvenanceKind::LocalSample,
            created_at: "2026-08-14T00:00:00Z".to_owned(),
        },
    };
    validate_document(&document)?;

    let canonical_json = canonical_json(&document)?;
    let canonical_hash = canonical_hash(canonical_json.as_bytes());
    let transaction = TransactionRecord {
        transaction_id: SAMPLE_CREATE_COMMAND_ID.to_owned(),
        document_id: document.document_id.clone(),
        command_id: SAMPLE_CREATE_COMMAND_ID.to_owned(),
        base_revision: 0,
        new_revision: 1,
        command_type: "createSample".to_owned(),
        modality: SourceModality::System,
        before_hash: "none".to_owned(),
        after_hash: canonical_hash.clone(),
        forward_operations: vec![Operation::CreateDocument],
        inverse_operations: vec![Operation::DeleteDocument],
    };
    let audit = safe_audit(
        SAMPLE_CREATE_COMMAND_ID,
        &document,
        0,
        "createSample",
        SourceModality::System,
        "2026-08-14T00:00:00Z",
    );
    operation_result(document, canonical_json, canonical_hash, transaction, audit)
}

#[must_use]
pub fn apply_command(request: ApplyCommandRequest) -> ApiResponse<OperationResult> {
    match apply_command_inner(request) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => ApiResponse::failure(error),
    }
}

fn apply_command_inner(request: ApplyCommandRequest) -> Result<OperationResult, CoreError> {
    let mut document = decode_canonical(&request.canonical_json)?;
    let before_hash = canonical_hash(request.canonical_json.as_bytes());
    let command = request.command;

    if Uuid::parse_str(&command.command_id).is_err() {
        return Err(CoreError::InvalidCommandId);
    }
    if command.base_revision != document.revision {
        return Err(CoreError::StaleRevision);
    }
    if command.text.is_empty() || command.text.len() > 4096 {
        return Err(CoreError::InvalidRange);
    }

    let node = document
        .content
        .iter_mut()
        .find(|node| node.id == command.target.node_id)
        .ok_or(CoreError::InvalidTarget)?;
    let byte_offset = utf16_to_byte_offset(&node.text, command.target.utf16_offset)
        .ok_or(CoreError::InvalidRange)?;
    node.text.insert_str(byte_offset, &command.text);

    let base_revision = document.revision;
    document.revision = document
        .revision
        .checked_add(1)
        .ok_or(CoreError::InvalidDocument)?;
    validate_document(&document)?;

    let canonical_json = canonical_json(&document)?;
    let canonical_hash = canonical_hash(canonical_json.as_bytes());
    let inserted_utf16_length =
        u32::try_from(command.text.encode_utf16().count()).map_err(|_| CoreError::InvalidRange)?;
    let transaction = TransactionRecord {
        transaction_id: command.command_id.clone(),
        document_id: document.document_id.clone(),
        command_id: command.command_id.clone(),
        base_revision,
        new_revision: document.revision,
        command_type: command_type(&command.kind).to_owned(),
        modality: command.modality.clone(),
        before_hash,
        after_hash: canonical_hash.clone(),
        forward_operations: vec![Operation::InsertText {
            target: command.target.clone(),
            text: command.text,
        }],
        inverse_operations: vec![Operation::DeleteText {
            target: command.target,
            utf16_length: inserted_utf16_length,
        }],
    };
    let audit = safe_audit(
        &command.command_id,
        &document,
        base_revision,
        command_type(&command.kind),
        command.modality,
        &command.issued_at,
    );
    operation_result(document, canonical_json, canonical_hash, transaction, audit)
}

#[must_use]
pub fn recover(request: RecoverRequest) -> ApiResponse<RecoverResult> {
    match recover_inner(request) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => ApiResponse::failure(error),
    }
}

fn recover_inner(mut request: RecoverRequest) -> Result<RecoverResult, CoreError> {
    let document = decode_canonical(&request.snapshot.canonical_json)?;
    if request.snapshot.document_id != document.document_id
        || request.snapshot.revision != document.revision
        || request.snapshot.schema_version != document.schema_version
    {
        return Err(CoreError::InvalidDocument);
    }
    let computed_hash = canonical_hash(request.snapshot.canonical_json.as_bytes());
    if computed_hash != request.snapshot.canonical_hash {
        return Err(CoreError::HashMismatch);
    }

    request
        .transactions
        .sort_by_key(|record| record.new_revision);
    let mut expected_base = 0;
    let mut previous_hash = "none".to_owned();
    for record in &request.transactions {
        if record.document_id != document.document_id {
            continue;
        }
        if record.base_revision != expected_base
            || record.new_revision != expected_base + 1
            || record.before_hash != previous_hash
        {
            return Err(CoreError::RecoveryGap);
        }
        expected_base = record.new_revision;
        previous_hash.clone_from(&record.after_hash);
    }
    if expected_base != document.revision || previous_hash != computed_hash {
        return Err(CoreError::RecoveryGap);
    }

    request
        .audits
        .retain(|audit| audit.document_id == document.document_id);
    request.audits.sort_by_key(|audit| audit.new_revision);
    for audit in &request.audits {
        validate_audit(audit)?;
    }

    let session = session_dto(
        &document,
        request.snapshot.canonical_json,
        computed_hash.clone(),
    )?;
    let view = inspector_view(&document, computed_hash, request.audits);
    Ok(RecoverResult { session, view })
}

fn operation_result(
    document: FlowDocument,
    canonical_json: String,
    canonical_hash: String,
    transaction: TransactionRecord,
    audit: AuditRecord,
) -> Result<OperationResult, CoreError> {
    let snapshot = SnapshotRecord {
        document_id: document.document_id.clone(),
        revision: document.revision,
        schema_version: document.schema_version,
        canonical_json: canonical_json.clone(),
        canonical_hash: canonical_hash.clone(),
    };
    let session = session_dto(&document, canonical_json, canonical_hash.clone())?;
    let view = inspector_view(&document, canonical_hash, vec![audit.clone()]);
    Ok(OperationResult {
        session,
        view,
        commit: PersistenceCommit {
            replace_existing: transaction.base_revision == 0,
            snapshot,
            transaction,
            audit,
        },
    })
}

fn session_dto(
    document: &FlowDocument,
    canonical_json: String,
    canonical_hash: String,
) -> Result<SessionDto, CoreError> {
    let node = document.content.first().ok_or(CoreError::InvalidDocument)?;
    let utf16_offset =
        u32::try_from(node.text.encode_utf16().count()).map_err(|_| CoreError::InvalidDocument)?;
    Ok(SessionDto {
        canonical_json,
        canonical_hash,
        document_id: document.document_id.clone(),
        revision: document.revision,
        next_command_target: LogicalPosition {
            node_id: node.id.clone(),
            utf16_offset,
            affinity: Affinity::Forward,
        },
    })
}

fn inspector_view(
    document: &FlowDocument,
    canonical_hash: String,
    audit: Vec<AuditRecord>,
) -> InspectorView {
    let document_summary = document
        .content
        .iter()
        .map(|node| node.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    InspectorView {
        document_id: document.document_id.clone(),
        schema_version: document.schema_version,
        revision: document.revision,
        canonical_hash,
        locale: document.locale.clone(),
        document_summary,
        provenance: "localSample".to_owned(),
        audit,
    }
}

fn safe_audit(
    command_id: &str,
    document: &FlowDocument,
    base_revision: u32,
    command_type: &str,
    modality: SourceModality,
    timestamp: &str,
) -> AuditRecord {
    AuditRecord {
        audit_id: command_id.to_owned(),
        document_id: document.document_id.clone(),
        command_id: command_id.to_owned(),
        base_revision,
        new_revision: document.revision,
        timestamp: timestamp.chars().take(64).collect(),
        command_type: command_type.to_owned(),
        modality,
        outcome: AuditOutcome::Applied,
        error_code: None,
        safe_metadata: vec![SafeMetadata {
            key: "schemaVersion".to_owned(),
            value: document.schema_version.to_string(),
        }],
    }
}

fn validate_audit(audit: &AuditRecord) -> Result<(), CoreError> {
    if audit.command_type != "createSample" && audit.command_type != "insertText" {
        return Err(CoreError::UnsafeAuditRecord);
    }
    if audit.timestamp.len() > 64 || audit.safe_metadata.len() > 8 {
        return Err(CoreError::UnsafeAuditRecord);
    }
    for metadata in &audit.safe_metadata {
        if metadata.key != "schemaVersion" || metadata.value != SCHEMA_VERSION.to_string() {
            return Err(CoreError::UnsafeAuditRecord);
        }
    }
    Ok(())
}

fn validate_document(document: &FlowDocument) -> Result<(), CoreError> {
    if document.schema_version != SCHEMA_VERSION {
        return Err(CoreError::UnsupportedSchema);
    }
    if Uuid::parse_str(&document.document_id).is_err()
        || document.content.is_empty()
        || document.locale.len() > 32
    {
        return Err(CoreError::InvalidDocument);
    }
    for node in &document.content {
        if Uuid::parse_str(&node.id).is_err() || node.text.len() > MAX_CANONICAL_BYTES {
            return Err(CoreError::InvalidDocument);
        }
    }
    Ok(())
}

fn decode_canonical(input: &str) -> Result<FlowDocument, CoreError> {
    if input.len() > MAX_CANONICAL_BYTES {
        return Err(CoreError::PayloadTooLarge);
    }
    let document: FlowDocument =
        serde_json::from_str(input).map_err(|_| CoreError::InvalidCanonicalJson)?;
    validate_document(&document)?;
    let encoded = canonical_json(&document)?;
    if encoded != input {
        return Err(CoreError::NonCanonicalPayload);
    }
    Ok(document)
}

fn canonical_json(document: &FlowDocument) -> Result<String, CoreError> {
    serde_json::to_string(document).map_err(|_| CoreError::InvalidDocument)
}

fn canonical_hash(bytes: &[u8]) -> String {
    format!("flowpdf:blake3:v1:{}", blake3::hash(bytes).to_hex())
}

fn command_type(kind: &CommandKind) -> &'static str {
    match kind {
        CommandKind::InsertText => "insertText",
    }
}

fn utf16_to_byte_offset(value: &str, utf16_offset: u32) -> Option<usize> {
    let requested = usize::try_from(utf16_offset).ok()?;
    if requested == 0 {
        return Some(0);
    }
    let mut consumed = 0;
    for (byte_index, character) in value.char_indices() {
        if consumed == requested {
            return Some(byte_index);
        }
        consumed += character.len_utf16();
        if consumed > requested {
            return None;
        }
    }
    (consumed == requested).then_some(value.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn success<T>(response: ApiResponse<T>) -> T {
        assert!(response.ok, "unexpected error: {:?}", response.error);
        response.value.expect("successful response has a value")
    }

    #[test]
    fn sample_command_and_recovery_preserve_the_canonical_hash() {
        let created = success(create_sample(CreateSampleRequest {
            requested_locale: "uk-UA".to_owned(),
        }));
        let command_id = "00000000-0000-4000-8000-000000000202".to_owned();
        let applied = success(apply_command(ApplyCommandRequest {
            canonical_json: created.session.canonical_json,
            command: CommandDto {
                command_id,
                base_revision: created.session.revision,
                modality: SourceModality::Ui,
                issued_at: "2026-08-14T00:00:01Z".to_owned(),
                kind: CommandKind::InsertText,
                target: created.session.next_command_target,
                text: " — typed mutation".to_owned(),
            },
        }));

        let recovered = success(recover(RecoverRequest {
            snapshot: applied.commit.snapshot.clone(),
            transactions: vec![created.commit.transaction, applied.commit.transaction],
            audits: vec![created.commit.audit, applied.commit.audit],
        }));
        assert_eq!(
            recovered.session.canonical_hash,
            applied.session.canonical_hash
        );
        assert_eq!(recovered.session.revision, 2);
        assert!(recovered.view.document_summary.contains("Український"));
        assert!(recovered.view.document_summary.contains("typed mutation"));
        assert!(
            !serde_json::to_string(&recovered.view.audit)
                .expect("audit serializes")
                .contains("typed mutation")
        );
    }

    #[test]
    fn stale_command_is_atomic() {
        let created = success(create_sample(CreateSampleRequest {
            requested_locale: "uk-UA".to_owned(),
        }));
        let before_hash = created.session.canonical_hash.clone();
        let response = apply_command(ApplyCommandRequest {
            canonical_json: created.session.canonical_json,
            command: CommandDto {
                command_id: "00000000-0000-4000-8000-000000000203".to_owned(),
                base_revision: 0,
                modality: SourceModality::Ui,
                issued_at: "2026-08-14T00:00:01Z".to_owned(),
                kind: CommandKind::InsertText,
                target: created.session.next_command_target,
                text: "must not appear".to_owned(),
            },
        });
        assert!(!response.ok);
        assert_eq!(response.error.expect("error").code, "FLOW_STALE_REVISION");
        assert_eq!(before_hash, created.session.canonical_hash);
    }

    #[test]
    fn utf16_offsets_do_not_split_surrogate_pairs() {
        assert_eq!(utf16_to_byte_offset("A😀Б", 1), Some(1));
        assert_eq!(utf16_to_byte_offset("A😀Б", 2), None);
        assert_eq!(utf16_to_byte_offset("A😀Б", 3), Some(5));
    }
}

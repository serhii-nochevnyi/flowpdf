//! Canonical, deterministic FlowPDF semantics and persistence DTOs.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

pub mod anchor;
pub mod asset;
pub mod audit;
pub mod canonical;
pub mod capability;
pub mod editor_view;
pub mod model;
pub mod provenance;
pub mod schema;
pub mod store;
pub mod transaction;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use anchor::Utf16Offset;
pub use asset::{
    AssetError, AssetStageRequest, AssetStageResponse, AssetStagingStore, MAX_AUTHORED_ALT_BYTES,
    MAX_IMAGE_DECODED_BYTES, MAX_IMAGE_DIMENSION, MAX_IMAGE_ENCODED_BYTES, MAX_IMAGE_PIXELS,
    MAX_STAGING_BYTES_PER_SESSION, MAX_STAGING_RECEIPTS_PER_SESSION, STAGING_RECEIPT_TTL_SECONDS,
};
use audit::{
    AuditAction, AuditCommandKind, AuditErrorCode, AuditMetadata, AuditTimestamp,
    AuditValidationError,
};
pub use audit::{AuditEvent as AuditRecord, AuditOutcome};
use canonical::{canonical_bytes, canonical_hash, decode_canonical};
pub use capability::{
    CommandCapability, CommandParityContract, ConfirmationPolicy, FutureVoiceBinding,
    MutationFamily, RiskLevel, RouteBinding, UndoPolicy, command_capabilities,
    command_capability_for_command_kind, command_capability_for_family,
    command_capability_for_voice_intent, command_parity_contract, command_with_modality,
};
pub use editor_view::{
    CapabilityDto, ConfirmationKindDto, ConfirmationMetadataDto, DirectionalSelection,
    EditorBlockViewDto, EditorCapability, EditorDocumentViewDto, EditorFieldReviewDto,
    EditorFieldReviewStatusDto, EditorFieldValueSummaryDto, EditorFieldViewDto,
    EditorSessionAction, EditorSessionError, EditorSessionRequest, EditorSessionResponse,
    EditorSessionState, EditorTextSpanDto, EditorViewDto, EditorViewRequest,
    FormattingProjectionDto, FormattingState, ImageBlockViewDto, ImageLimitsDto,
    StructuralPlacementDto, TableCellFocusDto, TableLimitsDto,
};
use model::{
    Affinity, CommandId, DocumentId, FlowDocument, LogicalPosition, MigrationHop, Provenance,
    SCHEMA_VERSION,
};
use provenance::{ProvenanceError, RevisionHash, RevisionProvenance};
use schema::{DocumentLimits, LimitKind, MigrationRegistry, MigrationReport, SchemaError};
pub use transaction::{
    Command as CommandDto, CommandKind, HistoryState, Operation, SourceModality,
    StructuralPlacement, Transaction as TransactionRecord,
};
use transaction::{
    CommandError, EditorState, HistoryEffect, HistoryEntry, TransactionService, replay_forward,
    replay_inverse, semantic_hash, validate_private_preimages,
};

const SAMPLE_CREATE_COMMAND_ID: &str = "00000000-0000-4000-8000-000000000201";
pub const RECORD_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSampleRequest {
    pub requested_locale: String,
    pub issued_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyCommandRequest {
    pub canonical_json: String,
    pub history: HistoryState,
    pub command: CommandDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrateDocumentRequest {
    pub canonical_json: String,
    pub migration_id: CommandId,
    pub issued_at: String,
    pub assets: Vec<store::AssetRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrateDocumentResult {
    pub canonical_json: String,
    pub canonical_hash: String,
    pub provenance: Provenance,
    pub revision_provenance: RevisionProvenance,
    pub report: MigrationReport,
    pub commit: Option<MigrationPersistenceCommit>,
    pub editor: EditorSessionResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotRecord {
    pub record_format_version: u32,
    pub document_id: DocumentId,
    pub revision: u32,
    pub schema_version: u32,
    pub canonical_json: String,
    pub canonical_hash: String,
    pub history: HistoryState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_boundary: Option<MigrationBoundaryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationBoundaryRecord {
    pub record_format_version: u32,
    pub migration_id: CommandId,
    pub issued_at: String,
    pub document_id: DocumentId,
    pub revision: u32,
    pub source_schema_version: u32,
    pub current_schema_version: u32,
    pub source_canonical_hash: String,
    pub migrated_canonical_hash: String,
    pub hops: Vec<MigrationHop>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationPersistenceCommit {
    pub snapshot: SnapshotRecord,
    pub audit: AuditRecord,
    pub assets: Vec<store::AssetRecord>,
    pub source: store::MigrationSourceRecord,
}

impl MigrationBoundaryRecord {
    /// Validates the immutable migration-lineage pointer carried by every
    /// current-schema checkpoint descended from a migration. The boundary
    /// revision/hash describe the original migration output, not the later
    /// checkpoint currently being validated.
    pub(crate) fn validate_against_current(
        &self,
        document: &FlowDocument,
    ) -> Result<(), CoreError> {
        let Provenance::Migrated {
            source_schema_version,
            current_schema_version,
            hops,
            ..
        } = &document.provenance
        else {
            return Err(CoreError::RecoveryGap);
        };
        AuditTimestamp::parse(self.issued_at.clone())?;
        RevisionHash::parse(self.source_canonical_hash.clone())?;
        RevisionHash::parse(self.migrated_canonical_hash.clone())?;
        if self.record_format_version != RECORD_FORMAT_VERSION
            || self.document_id != document.document_id
            || self.revision > document.revision
            || self.source_schema_version != *source_schema_version
            || self.current_schema_version != *current_schema_version
            || self.current_schema_version != document.schema_version
            || self.source_canonical_hash == self.migrated_canonical_hash
            || self.hops != *hops
        {
            return Err(CoreError::RecoveryGap);
        }
        Ok(())
    }

    fn validate_migration_output(
        &self,
        document: &FlowDocument,
        migrated_hash: &str,
    ) -> Result<(), CoreError> {
        self.validate_against_current(document)?;
        if self.revision != document.revision || self.migrated_canonical_hash != migrated_hash {
            return Err(CoreError::RecoveryGap);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistenceCommit {
    pub replace_existing: bool,
    pub snapshot: SnapshotRecord,
    pub transaction: TransactionRecord,
    pub audit: AuditRecord,
    pub assets: Vec<store::AssetRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDto {
    pub canonical_json: String,
    pub canonical_hash: String,
    pub document_id: DocumentId,
    pub revision: u32,
    pub next_command_target: Option<LogicalPosition>,
    pub history: HistoryState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InspectorView {
    pub document_id: DocumentId,
    pub schema_version: u32,
    pub revision: u32,
    pub canonical_hash: String,
    pub locale: String,
    pub content_node_count: u32,
    pub field_count: u32,
    pub asset_count: u32,
    pub revision_provenance: RevisionProvenance,
    pub audit: Vec<AuditRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationResult {
    pub session: SessionDto,
    pub view: InspectorView,
    pub editor: EditorSessionResponse,
    pub commit: PersistenceCommit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoverRequest {
    pub snapshots: Vec<SnapshotRecord>,
    pub transactions: Vec<TransactionRecord>,
    pub audits: Vec<AuditRecord>,
    pub assets: Vec<store::AssetRecord>,
    pub sources: Vec<store::MigrationSourceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanPersistenceCommitRequest {
    pub records: RecoverRequest,
    pub commit: PersistenceCommit,
    pub reason: store::SnapshotReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanStandaloneAuditRequest {
    pub records: RecoverRequest,
    pub derivation: StandaloneAuditDerivation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum StandaloneAuditDerivation {
    CommandFailure {
        request: Box<ApplyCommandRequest>,
        attempt_id: CommandId,
    },
    Recovery {
        audit_context: RecoveryAuditContext,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoverResult {
    pub session: SessionDto,
    pub view: InspectorView,
    pub editor: EditorSessionResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryAuditContext {
    pub attempt_id: CommandId,
    pub document_id: DocumentId,
    pub expected_revision: u32,
    pub issued_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditedRecoverRequest {
    pub records: RecoverRequest,
    pub audit_context: RecoveryAuditContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditedRecoverResult {
    pub recovered: RecoverResult,
    pub audit: AuditRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

    #[must_use]
    pub fn failure_with_audit(error: CoreError, audit: Option<AuditRecord>) -> Self {
        Self {
            ok: false,
            value: None,
            error: Some(ErrorDto {
                code: error.code().to_owned(),
                message: error.to_string(),
                audit,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErrorDto {
    pub code: String,
    pub message: String,
    pub audit: Option<AuditRecord>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    #[error("The boundary request could not be decoded")]
    Decode,
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error(transparent)]
    Asset(#[from] asset::AssetError),
    #[error(transparent)]
    Command(#[from] CommandError),
    #[error(transparent)]
    EditorSession(#[from] EditorSessionError),
    #[error("The persisted record set is incomplete or discontinuous")]
    RecoveryGap,
    #[error("The persisted record hash does not match canonical content")]
    HashMismatch,
    #[error("The persisted audit record violates the redaction allowlist")]
    UnsafeAuditRecord,
    #[error(transparent)]
    Audit(#[from] AuditValidationError),
    #[error(transparent)]
    Provenance(#[from] ProvenanceError),
    #[error(transparent)]
    Store(#[from] store::StoreError),
}

impl CoreError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Decode => "FLOW_DECODE_ERROR",
            Self::Schema(error) => error.code(),
            Self::Asset(error) => error.code(),
            Self::Command(error) => error.code(),
            Self::EditorSession(error) => error.code(),
            Self::RecoveryGap => "FLOW_RECOVERY_GAP",
            Self::HashMismatch => "FLOW_HASH_MISMATCH",
            Self::UnsafeAuditRecord | Self::Audit(_) => "FLOW_UNSAFE_AUDIT_RECORD",
            Self::Provenance(_) => "FLOW_PROVENANCE_INVALID",
            Self::Store(error) => error.code(),
        }
    }
}

impl From<CoreError> for ErrorDto {
    fn from(error: CoreError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
            audit: None,
        }
    }
}

#[must_use]
pub fn decode_failure<T>() -> ApiResponse<T> {
    ApiResponse::failure(CoreError::Decode)
}

/// Applies a noncanonical, Rust-owned editor-session action. This function
/// never creates a document revision, transaction, audit entry, or persistence
/// plan; it returns a new immutable session/view projection instead.
#[must_use]
pub fn apply_editor_session(request: EditorSessionRequest) -> ApiResponse<EditorSessionResponse> {
    match apply_editor_session_inner(request) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => ApiResponse::failure(error),
    }
}

fn apply_editor_session_inner(
    request: EditorSessionRequest,
) -> Result<EditorSessionResponse, CoreError> {
    let document = decode_canonical(request.canonical_json.as_bytes())?;
    Ok(editor_view::apply_action(
        request.session,
        &document,
        request.action,
    )?)
}

/// Revalidates and projects an immutable Rust-owned editor session without
/// mutating the canonical document or session generation.
#[must_use]
pub fn query_editor_view(request: EditorViewRequest) -> ApiResponse<EditorViewDto> {
    match query_editor_view_inner(request) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => ApiResponse::failure(error),
    }
}

fn query_editor_view_inner(request: EditorViewRequest) -> Result<EditorViewDto, CoreError> {
    let document = decode_canonical(request.canonical_json.as_bytes())?;
    Ok(editor_view::project_view(&document, &request.session)?)
}

#[must_use]
pub fn create_sample(request: CreateSampleRequest) -> ApiResponse<OperationResult> {
    match create_sample_inner(request) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => ApiResponse::failure(error),
    }
}

/// Validates and stages one bounded PNG/JPEG payload. The returned receipt is
/// ephemeral and revision-bound; it is intentionally not a document or
/// persistence record.
#[must_use]
pub fn stage_asset(request: AssetStageRequest, bytes: Vec<u8>) -> ApiResponse<AssetStageResponse> {
    match asset::stage_asset(request, bytes) {
        Ok(response) => ApiResponse::success(response),
        Err(error) => ApiResponse::failure(error.into()),
    }
}

fn create_sample_inner(request: CreateSampleRequest) -> Result<OperationResult, CoreError> {
    AuditTimestamp::parse(request.issued_at.clone())?;
    let document =
        FlowDocument::deterministic_sample_at(&request.requested_locale, &request.issued_at)?;
    let canonical_json = canonical_string(&document)?;
    let canonical_hash = canonical_hash(canonical_json.as_bytes());
    let command_id = CommandId::new(SAMPLE_CREATE_COMMAND_ID)?;
    let history = HistoryState {
        entries: Vec::new(),
        cursor: 0,
        seen_command_ids: vec![command_id.clone()],
    };
    let transaction = TransactionRecord {
        record_format_version: RECORD_FORMAT_VERSION,
        transaction_id: command_id.clone(),
        document_id: document.document_id.clone(),
        schema_version: document.schema_version,
        command_id,
        base_revision: 0,
        new_revision: 1,
        command_type: "createSample".to_owned(),
        modality: SourceModality::System,
        issued_at: request.issued_at.clone(),
        before_hash: "none".to_owned(),
        after_hash: canonical_hash.clone(),
        forward_operations: vec![Operation::CreateDocument],
        inverse_operations: vec![Operation::DeleteDocument],
        anchor_mapping: crate::anchor::AnchorMapping::identity(),
        history_effect: HistoryEffect::Create,
    };
    let audit = safe_audit(
        SAMPLE_CREATE_COMMAND_ID,
        &document,
        0,
        "createSample",
        SourceModality::System,
        &request.issued_at,
    )?;
    operation_result(
        document,
        history,
        canonical_json,
        canonical_hash,
        transaction,
        audit,
        None,
        Vec::new(),
    )
}

#[must_use]
pub fn apply_command(request: ApplyCommandRequest) -> ApiResponse<OperationResult> {
    match apply_command_inner(request.clone()) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => {
            let audit =
                rejected_command_audit(&request, &error, request.command.command_id.clone());
            ApiResponse::failure_with_audit(error, audit)
        }
    }
}

/// Runs the same pure migration registry used by native callers through a
/// serialized DTO boundary suitable for WebAssembly.
#[must_use]
pub fn migrate_document(request: MigrateDocumentRequest) -> ApiResponse<MigrateDocumentResult> {
    match migrate_document_inner(request) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => ApiResponse::failure(error),
    }
}

/// Selects the physical checkpoint set using the locked Rust-owned default
/// policy. Browser/native adapters persist this DTO verbatim and never decide
/// whether a snapshot is due.
#[must_use]
pub fn plan_persistence_commit(
    request: PlanPersistenceCommitRequest,
) -> ApiResponse<store::PlannedPersistenceCommit> {
    match store::CommitPlanner::new(store::SnapshotPolicy::default()).plan(
        &request.records,
        request.commit,
        request.reason,
    ) {
        Ok(commit) => ApiResponse::success(commit),
        Err(error) => ApiResponse::failure(error.into()),
    }
}

/// Authorizes a privacy-minimized, non-mutating audit record against the
/// durable record image. Physical adapters persist only the returned record,
/// preserving Rust ownership of audit semantics while keeping the write
/// idempotent and independent from the document head.
#[must_use]
pub fn plan_standalone_audit(request: PlanStandaloneAuditRequest) -> ApiResponse<AuditRecord> {
    match derive_standalone_audit(&request.records, request.derivation).and_then(|audit| {
        store::validate_standalone_audit(&request.records, &audit)?;
        let mut candidate = request.records;
        if !candidate.audits.iter().any(|existing| existing == &audit) {
            candidate.audits.push(audit.clone());
        }
        preflight_recovery_records(&candidate)?;
        Ok(audit)
    }) {
        Ok(audit) => ApiResponse::success(audit),
        Err(error) => ApiResponse::failure(error),
    }
}

fn derive_standalone_audit(
    records: &RecoverRequest,
    derivation: StandaloneAuditDerivation,
) -> Result<AuditRecord, CoreError> {
    match derivation {
        StandaloneAuditDerivation::CommandFailure {
            request,
            attempt_id,
        } => {
            if attempt_id == request.command.command_id {
                return Err(CoreError::UnsafeAuditRecord);
            }
            match apply_command_inner(*request.clone()) {
                Ok(_) => Err(CoreError::UnsafeAuditRecord),
                Err(error) => rejected_command_audit(&request, &error, attempt_id)
                    .ok_or(CoreError::UnsafeAuditRecord),
            }
        }
        StandaloneAuditDerivation::Recovery { audit_context } => {
            let error = match recover_inner(records.clone()) {
                Ok(recovered)
                    if recovered.session.document_id == audit_context.document_id
                        && recovered.session.revision == audit_context.expected_revision =>
                {
                    None
                }
                Ok(_) => Some(CoreError::RecoveryGap),
                Err(error) => Some(error),
            };
            recovery_audit(&audit_context, error.as_ref()).ok_or(CoreError::UnsafeAuditRecord)
        }
    }
}

fn migrate_document_inner(
    request: MigrateDocumentRequest,
) -> Result<MigrateDocumentResult, CoreError> {
    let source_canonical_hash = canonical_hash(request.canonical_json.as_bytes());
    let outcome = MigrationRegistry::current().migrate(request.canonical_json.as_bytes())?;
    store::validate_asset_records(&outcome.document, &request.assets)?;
    let canonical_json = String::from_utf8(outcome.canonical_bytes.clone())
        .map_err(|_| SchemaError::serialization())?;
    let mut revision_provenance =
        RevisionProvenance::from_document(&outcome.document, outcome.canonical_hash.clone())?;
    let commit = if outcome.report.requires_new_snapshot {
        revision_provenance = revision_provenance
            .with_source_hashes(vec![RevisionHash::parse(source_canonical_hash.clone())?])?;
        let source_hash_for_record = source_canonical_hash.clone();
        let boundary = MigrationBoundaryRecord {
            record_format_version: RECORD_FORMAT_VERSION,
            migration_id: request.migration_id.clone(),
            issued_at: request.issued_at.clone(),
            document_id: outcome.document.document_id.clone(),
            revision: outcome.document.revision,
            source_schema_version: outcome.report.source_schema_version,
            current_schema_version: outcome.report.current_schema_version,
            source_canonical_hash,
            migrated_canonical_hash: outcome.canonical_hash.clone(),
            hops: outcome.report.hops.clone(),
        };
        boundary.validate_migration_output(&outcome.document, &outcome.canonical_hash)?;
        let snapshot = SnapshotRecord {
            record_format_version: RECORD_FORMAT_VERSION,
            document_id: outcome.document.document_id.clone(),
            revision: outcome.document.revision,
            schema_version: outcome.document.schema_version,
            canonical_json: canonical_json.clone(),
            canonical_hash: outcome.canonical_hash.clone(),
            history: HistoryState::default(),
            migration_boundary: Some(boundary),
        };
        EditorState::with_history(outcome.document.clone(), snapshot.history.clone())?;
        let audit = AuditRecord::success(
            request.migration_id.clone(),
            outcome.document.document_id.clone(),
            request.migration_id.clone(),
            request.migration_id,
            outcome.document.revision,
            outcome.document.revision,
            u64::from(outcome.document.revision),
            AuditTimestamp::parse(request.issued_at)?,
            AuditAction::Migration,
            SourceModality::System,
            vec![AuditMetadata::Migration {
                from_schema_version: outcome.report.source_schema_version,
                to_schema_version: outcome.report.current_schema_version,
            }],
        )?;
        let source = store::MigrationSourceRecord {
            record_format_version: RECORD_FORMAT_VERSION,
            document_id: outcome.document.document_id.clone(),
            schema_version: outcome.report.source_schema_version,
            canonical_json: request.canonical_json,
            canonical_hash: source_hash_for_record,
        };
        let commit = MigrationPersistenceCommit {
            snapshot,
            audit,
            assets: request.assets,
            source,
        };
        preflight_recovery_records(&RecoverRequest {
            snapshots: vec![commit.snapshot.clone()],
            transactions: Vec::new(),
            audits: vec![commit.audit.clone()],
            assets: commit.assets.clone(),
            sources: vec![commit.source.clone()],
        })?;
        Some(commit)
    } else {
        None
    };
    let editor = editor_response(&outcome.document, None)?;
    Ok(MigrateDocumentResult {
        canonical_json,
        canonical_hash: outcome.canonical_hash,
        provenance: outcome.document.provenance.clone(),
        revision_provenance,
        report: outcome.report,
        commit,
        editor,
    })
}

fn apply_command_inner(request: ApplyCommandRequest) -> Result<OperationResult, CoreError> {
    let document = decode_canonical(request.canonical_json.as_bytes())?;
    let state = EditorState::with_history(document, request.history)?;
    if state.canonical_hash() != canonical_hash(request.canonical_json.as_bytes()) {
        return Err(CoreError::HashMismatch);
    }
    let command = request.command;
    let applied = TransactionService::apply(&state, command.clone())?;
    let document = applied.state.document().clone();
    let history = applied.state.history().clone();
    let canonical_json = canonical_string(&document)?;
    let resulting_hash = applied.state.canonical_hash().to_owned();
    let audit = safe_audit(
        command.command_id.as_str(),
        &document,
        command.base_revision,
        &applied.transaction.command_type,
        command.modality,
        &command.issued_at,
    )?;
    operation_result(
        document,
        history,
        canonical_json,
        resulting_hash,
        applied.transaction,
        audit,
        applied.selection,
        applied.asset_records,
    )
}

fn rejected_command_audit(
    request: &ApplyCommandRequest,
    error: &CoreError,
    attempt_id: CommandId,
) -> Option<AuditRecord> {
    let document = decode_canonical(request.canonical_json.as_bytes()).ok()?;
    let code = AuditErrorCode::from_stable_code(error.code())?;
    let command_id = request.command.command_id.clone();
    AuditRecord::failure(
        attempt_id,
        document.document_id,
        command_id.clone(),
        command_id,
        document.revision,
        document.revision,
        u64::from(document.revision) + 1,
        AuditTimestamp::parse(request.command.issued_at.clone()).ok()?,
        AuditAction::Command {
            command_kind: audit_command_kind(&request.command.kind),
        },
        request.command.modality.clone(),
        code,
        vec![AuditMetadata::SchemaVersion {
            value: document.schema_version,
        }],
    )
    .ok()
}

const fn audit_command_kind(kind: &CommandKind) -> AuditCommandKind {
    match kind {
        CommandKind::InsertText { .. } => AuditCommandKind::InsertText,
        CommandKind::ReplaceText { .. } => AuditCommandKind::ReplaceText,
        CommandKind::ReplaceSelection { .. } => AuditCommandKind::ReplaceSelection,
        CommandKind::DeleteText { .. } => AuditCommandKind::DeleteText,
        CommandKind::SetNodeStyle { .. } => AuditCommandKind::SetNodeStyle,
        CommandKind::InsertNode { .. } => AuditCommandKind::InsertNode,
        CommandKind::DeleteNode { .. } => AuditCommandKind::DeleteNode,
        CommandKind::SplitTextBlock { .. } => AuditCommandKind::SplitTextBlock,
        CommandKind::MergeTextBlocks { .. } => AuditCommandKind::MergeTextBlocks,
        CommandKind::DeleteSubtree { .. } => AuditCommandKind::DeleteSubtree,
        CommandKind::SetInlineMarks { .. } => AuditCommandKind::SetInlineMarks,
        CommandKind::SetInlineMark { .. } => AuditCommandKind::SetInlineMark,
        CommandKind::SetBlockAttributes { .. } => AuditCommandKind::SetBlockAttributes,
        CommandKind::SetBlockStyle { .. } => AuditCommandKind::SetBlockStyle,
        CommandKind::SetListKind { .. } => AuditCommandKind::SetListKind,
        CommandKind::ContinueListItem { .. } => AuditCommandKind::ContinueListItem,
        CommandKind::ExitListItem { .. } => AuditCommandKind::ExitListItem,
        CommandKind::IndentListItem { .. } => AuditCommandKind::IndentListItem,
        CommandKind::OutdentListItem { .. } => AuditCommandKind::OutdentListItem,
        CommandKind::InsertPageBreak { .. } => AuditCommandKind::InsertPageBreak,
        CommandKind::RemovePageBreak { .. } => AuditCommandKind::RemovePageBreak,
        CommandKind::InsertTable { .. } => AuditCommandKind::InsertTable,
        CommandKind::InsertImage { .. } => AuditCommandKind::InsertImage,
        CommandKind::ReplaceImage { .. } => AuditCommandKind::ReplaceImage,
        CommandKind::SetImageAccessibility { .. } => AuditCommandKind::SetImageAccessibility,
        CommandKind::RemoveImage { .. } => AuditCommandKind::RemoveImage,
        CommandKind::AddTableRow { .. } => AuditCommandKind::AddTableRow,
        CommandKind::RemoveTableRow { .. } => AuditCommandKind::RemoveTableRow,
        CommandKind::AddTableColumn { .. } => AuditCommandKind::AddTableColumn,
        CommandKind::RemoveTableColumn { .. } => AuditCommandKind::RemoveTableColumn,
        CommandKind::SetTableHeaderRow { .. } => AuditCommandKind::SetTableHeaderRow,
        CommandKind::RemoveTable { .. } => AuditCommandKind::RemoveTable,
        CommandKind::SetField { .. } => AuditCommandKind::SetField,
        CommandKind::Batch { .. } => AuditCommandKind::Batch,
        CommandKind::Undo => AuditCommandKind::Undo,
        CommandKind::Redo => AuditCommandKind::Redo,
    }
}

#[must_use]
pub fn recover(request: RecoverRequest) -> ApiResponse<RecoverResult> {
    match recover_inner(request) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => ApiResponse::failure(error),
    }
}

#[must_use]
pub fn recover_audited(request: AuditedRecoverRequest) -> ApiResponse<AuditedRecoverResult> {
    match recover_inner(request.records) {
        Ok(recovered) => {
            if recovered.session.document_id != request.audit_context.document_id
                || recovered.session.revision != request.audit_context.expected_revision
            {
                let error = CoreError::RecoveryGap;
                let audit = recovery_audit(&request.audit_context, Some(&error));
                return ApiResponse::failure_with_audit(error, audit);
            }
            match recovery_audit(&request.audit_context, None) {
                Some(audit) => ApiResponse::success(AuditedRecoverResult { recovered, audit }),
                None => ApiResponse::failure(CoreError::UnsafeAuditRecord),
            }
        }
        Err(error) => {
            let audit = recovery_audit(&request.audit_context, Some(&error));
            ApiResponse::failure_with_audit(error, audit)
        }
    }
}

fn recovery_audit(
    context: &RecoveryAuditContext,
    error: Option<&CoreError>,
) -> Option<AuditRecord> {
    let sequence = u64::from(context.expected_revision) + 1;
    let timestamp = AuditTimestamp::parse(context.issued_at.clone()).ok()?;
    match error {
        None => AuditRecord::success(
            context.attempt_id.clone(),
            context.document_id.clone(),
            context.attempt_id.clone(),
            context.attempt_id.clone(),
            context.expected_revision,
            context.expected_revision,
            sequence,
            timestamp,
            AuditAction::Recovery,
            SourceModality::System,
            vec![AuditMetadata::RecoveryVerified],
        )
        .ok(),
        Some(error) => {
            let code = AuditErrorCode::from_stable_code(error.code()).unwrap_or({
                if matches!(error, CoreError::HashMismatch) {
                    AuditErrorCode::HashMismatch
                } else {
                    AuditErrorCode::SchemaInvalid
                }
            });
            AuditRecord::failure(
                context.attempt_id.clone(),
                context.document_id.clone(),
                context.attempt_id.clone(),
                context.attempt_id.clone(),
                context.expected_revision,
                context.expected_revision,
                sequence,
                timestamp,
                AuditAction::Recovery,
                SourceModality::System,
                code,
                Vec::new(),
            )
            .ok()
        }
    }
}

fn recover_inner(request: RecoverRequest) -> Result<RecoverResult, CoreError> {
    recover_inner_with_checkpoint(request).map(|(recovered, _)| recovered)
}

/// Returns the revision of the snapshot that actually passed validation, so
/// durability policy never trusts metadata from a newer rejected candidate.
pub(crate) fn recover_inner_with_checkpoint(
    request: RecoverRequest,
) -> Result<(RecoverResult, u32), CoreError> {
    preflight_recovery_records(&request)?;
    let snapshots = normalize_snapshots(request.snapshots)?;
    let document_id = snapshots
        .first()
        .map(|snapshot| snapshot.document_id.clone())
        .ok_or(CoreError::RecoveryGap)?;
    let records = normalize_transactions(request.transactions, &document_id)?;
    let sources = normalize_migration_sources(request.sources, &document_id)?;
    let mut first_error = None;
    for snapshot in &snapshots {
        match recover_from_snapshot(
            snapshot,
            &records,
            &request.audits,
            &request.assets,
            &sources,
        ) {
            Ok(result) => return Ok((result, snapshot.revision)),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    Err(first_error.unwrap_or(CoreError::RecoveryGap))
}

fn recover_from_snapshot(
    snapshot: &SnapshotRecord,
    records: &[TransactionRecord],
    audits: &[AuditRecord],
    assets: &[store::AssetRecord],
    sources: &[store::MigrationSourceRecord],
) -> Result<RecoverResult, CoreError> {
    let (mut document, mut history, _) = validate_snapshot(snapshot)?;
    let migration_output =
        validate_migration_source(&document, snapshot.migration_boundary.as_ref(), sources)?;
    store::validate_asset_records(&document, assets)?;
    validate_snapshot_head(&document, &history, records, migration_output.as_ref())?;

    let checkpoint_revision = document.revision;
    for record in records
        .iter()
        .filter(|record| record.new_revision > checkpoint_revision)
    {
        if record.schema_version != document.schema_version {
            return Err(CoreError::RecoveryGap);
        }
        let before = document.clone();
        document = replay_forward(&before, record).map_err(|_| CoreError::RecoveryGap)?;
        replay_history_effect(&mut history, record, &before, &document)?;
    }
    store::validate_asset_records(&document, assets)?;
    history.validate().map_err(|_| CoreError::RecoveryGap)?;
    let recovered_state = EditorState::with_history(document.clone(), history.clone())
        .map_err(|_| CoreError::RecoveryGap)?;
    let computed_hash = recovered_state.canonical_hash().to_owned();
    let canonical_json = canonical_string(&document)?;
    if canonical_hash(canonical_json.as_bytes()) != computed_hash {
        return Err(CoreError::HashMismatch);
    }

    let audits = normalize_and_bind_audits(
        audits,
        records,
        &document,
        snapshot.migration_boundary.as_ref(),
    )?;
    let session = session_dto(&document, history, canonical_json, computed_hash.clone())?;
    let view = inspector_view(
        &document,
        computed_hash,
        snapshot
            .migration_boundary
            .as_ref()
            .map(|boundary| boundary.source_canonical_hash.as_str()),
        audits,
    )?;
    let editor = editor_response(&document, None)?;
    Ok(RecoverResult {
        session,
        view,
        editor,
    })
}

pub(crate) fn preflight_recovery_records(request: &RecoverRequest) -> Result<(), CoreError> {
    let record_count = request
        .snapshots
        .len()
        .checked_add(request.transactions.len())
        .and_then(|count| count.checked_add(request.audits.len()))
        .and_then(|count| count.checked_add(request.assets.len()))
        .and_then(|count| count.checked_add(request.sources.len()))
        .ok_or_else(SchemaError::invalid_document)?;
    DocumentLimits::V1.check_recovery(record_count, 0)?;

    let mut replay_bytes = 0_usize;
    for snapshot in &request.snapshots {
        DocumentLimits::V1.check(LimitKind::CanonicalBytes, snapshot.canonical_json.len())?;
        replay_bytes = checked_record_bytes(replay_bytes, snapshot)?;
    }
    for transaction in &request.transactions {
        validate_private_preimages(transaction).map_err(|_| CoreError::RecoveryGap)?;
        let bytes = serde_json::to_vec(transaction).map_err(|_| SchemaError::serialization())?;
        DocumentLimits::V1.check_transaction(
            transaction.forward_operations.len() + transaction.inverse_operations.len(),
            bytes.len(),
        )?;
        replay_bytes = replay_bytes
            .checked_add(bytes.len())
            .ok_or_else(SchemaError::invalid_document)?;
        DocumentLimits::V1.check_recovery(0, replay_bytes)?;
    }
    for audit in &request.audits {
        replay_bytes = checked_record_bytes(replay_bytes, audit)?;
    }
    for asset in &request.assets {
        DocumentLimits::V1.check(LimitKind::RecoveryBytes, asset.bytes.len())?;
        replay_bytes = checked_record_bytes(replay_bytes, asset)?;
    }
    for source in &request.sources {
        DocumentLimits::V1.check(LimitKind::CanonicalBytes, source.canonical_json.len())?;
        replay_bytes = checked_record_bytes(replay_bytes, source)?;
    }
    DocumentLimits::V1.check_recovery(record_count, replay_bytes)?;
    Ok(())
}

fn checked_record_bytes<T: Serialize>(total: usize, record: &T) -> Result<usize, CoreError> {
    let bytes = serde_json::to_vec(record).map_err(|_| SchemaError::serialization())?;
    let total = total
        .checked_add(bytes.len())
        .ok_or_else(SchemaError::invalid_document)?;
    DocumentLimits::V1.check_recovery(0, total)?;
    Ok(total)
}

fn normalize_snapshots(
    mut snapshots: Vec<SnapshotRecord>,
) -> Result<Vec<SnapshotRecord>, CoreError> {
    if snapshots.is_empty() {
        return Err(CoreError::RecoveryGap);
    }
    if snapshots
        .iter()
        .any(|snapshot| snapshot.schema_version > SCHEMA_VERSION)
    {
        return Err(CoreError::RecoveryGap);
    }
    snapshots.sort_by(|left, right| {
        right
            .schema_version
            .cmp(&left.schema_version)
            .then_with(|| right.revision.cmp(&left.revision))
            .then_with(|| left.canonical_hash.cmp(&right.canonical_hash))
    });
    let declared_document_id = snapshots[0].document_id.clone();
    if snapshots
        .iter()
        .any(|snapshot| snapshot.document_id != declared_document_id)
    {
        return Err(CoreError::RecoveryGap);
    }
    let mut identities = BTreeMap::<(DocumentId, u32, u32), SnapshotRecord>::new();
    for snapshot in snapshots {
        let identity = (
            snapshot.document_id.clone(),
            snapshot.schema_version,
            snapshot.revision,
        );
        match identities.get(&identity) {
            Some(existing) if existing == &snapshot => {}
            Some(_) => return Err(CoreError::RecoveryGap),
            None => {
                identities.insert(identity, snapshot);
            }
        }
    }
    let mut normalized = identities.into_values().collect::<Vec<_>>();
    normalized.sort_by(|left, right| {
        right
            .schema_version
            .cmp(&left.schema_version)
            .then_with(|| right.revision.cmp(&left.revision))
            .then_with(|| left.canonical_hash.cmp(&right.canonical_hash))
    });
    Ok(normalized)
}

fn normalize_migration_sources(
    sources: Vec<store::MigrationSourceRecord>,
    document_id: &DocumentId,
) -> Result<Vec<store::MigrationSourceRecord>, CoreError> {
    let mut identities = BTreeMap::<(u32, String), store::MigrationSourceRecord>::new();
    for source in sources {
        if source.record_format_version != RECORD_FORMAT_VERSION
            || &source.document_id != document_id
            || source.canonical_hash != canonical_hash(source.canonical_json.as_bytes())
        {
            return Err(CoreError::RecoveryGap);
        }
        let identity = (source.schema_version, source.canonical_hash.clone());
        match identities.get(&identity) {
            Some(existing) if existing == &source => {}
            Some(_) => return Err(CoreError::RecoveryGap),
            None => {
                identities.insert(identity, source);
            }
        }
    }
    Ok(identities.into_values().collect())
}

pub(crate) fn validate_migration_source(
    document: &FlowDocument,
    boundary: Option<&MigrationBoundaryRecord>,
    sources: &[store::MigrationSourceRecord],
) -> Result<Option<FlowDocument>, CoreError> {
    let Some(boundary) = boundary else {
        return if sources.is_empty() {
            Ok(None)
        } else {
            Err(CoreError::RecoveryGap)
        };
    };
    boundary.validate_against_current(document)?;
    if sources.iter().any(|source| {
        source.record_format_version != RECORD_FORMAT_VERSION
            || source.document_id != document.document_id
            || source.canonical_hash != canonical_hash(source.canonical_json.as_bytes())
    }) {
        return Err(CoreError::RecoveryGap);
    }
    let mut matching = None;
    for source in sources.iter().filter(|source| {
        source.document_id == document.document_id
            && source.schema_version == boundary.source_schema_version
            && source.canonical_hash == boundary.source_canonical_hash
    }) {
        match matching {
            Some(existing) if existing != source => return Err(CoreError::RecoveryGap),
            Some(_) => {}
            None => matching = Some(source),
        }
    }
    let matching = matching.ok_or(CoreError::RecoveryGap)?;
    let migrated = MigrationRegistry::current()
        .migrate(matching.canonical_json.as_bytes())
        .map_err(|_| CoreError::RecoveryGap)?;
    if migrated.report.source_schema_version != boundary.source_schema_version
        || migrated.report.current_schema_version != boundary.current_schema_version
        || migrated.report.hops != boundary.hops
        || migrated.canonical_hash != boundary.migrated_canonical_hash
    {
        return Err(CoreError::RecoveryGap);
    }
    boundary.validate_migration_output(&migrated.document, &migrated.canonical_hash)?;
    Ok(Some(migrated.document))
}

fn validate_snapshot(
    snapshot: &SnapshotRecord,
) -> Result<(FlowDocument, HistoryState, String), CoreError> {
    if snapshot.record_format_version != RECORD_FORMAT_VERSION {
        return Err(CoreError::RecoveryGap);
    }
    let document = decode_canonical(snapshot.canonical_json.as_bytes())?;
    if snapshot.document_id != document.document_id
        || snapshot.revision != document.revision
        || snapshot.schema_version != document.schema_version
    {
        return Err(SchemaError::invalid_document().into());
    }
    let computed_hash = canonical_hash(snapshot.canonical_json.as_bytes());
    if computed_hash != snapshot.canonical_hash {
        return Err(CoreError::HashMismatch);
    }
    match (&document.provenance, &snapshot.migration_boundary) {
        (Provenance::LocalSample { .. }, None) => {}
        (Provenance::Migrated { .. }, Some(boundary)) => {
            boundary.validate_against_current(&document)?;
        }
        _ => return Err(CoreError::RecoveryGap),
    }
    snapshot.history.validate().map_err(|error| match error {
        CommandError::Schema(schema) => CoreError::Schema(schema),
        _ => CoreError::RecoveryGap,
    })?;
    let state = EditorState::with_history(document.clone(), snapshot.history.clone())
        .map_err(|_| CoreError::RecoveryGap)?;
    if state.canonical_hash() != computed_hash {
        return Err(CoreError::HashMismatch);
    }
    Ok((document, snapshot.history.clone(), computed_hash))
}

fn normalize_transactions(
    transactions: Vec<TransactionRecord>,
    document_id: &DocumentId,
) -> Result<Vec<TransactionRecord>, CoreError> {
    let mut by_id = BTreeMap::<String, TransactionRecord>::new();
    for record in transactions {
        if record.record_format_version != RECORD_FORMAT_VERSION
            || record.schema_version != SCHEMA_VERSION
            || &record.document_id != document_id
        {
            return Err(CoreError::RecoveryGap);
        }
        match by_id.get(record.transaction_id.as_str()) {
            Some(existing) if existing == &record => continue,
            Some(_) => return Err(CoreError::RecoveryGap),
            None => {
                by_id.insert(record.transaction_id.as_str().to_owned(), record);
            }
        }
    }
    let mut records = by_id.into_values().collect::<Vec<_>>();
    records.sort_by(|left, right| {
        left.new_revision
            .cmp(&right.new_revision)
            .then_with(|| left.transaction_id.cmp(&right.transaction_id))
    });
    for pair in records.windows(2) {
        if pair[0].new_revision == pair[1].new_revision {
            return Err(CoreError::RecoveryGap);
        }
    }
    Ok(records)
}

fn validate_snapshot_head(
    document: &FlowDocument,
    history: &HistoryState,
    records: &[TransactionRecord],
    migration_output: Option<&FlowDocument>,
) -> Result<(), CoreError> {
    if let Provenance::Migrated { .. } = document.provenance {
        let migration_output = migration_output.ok_or(CoreError::RecoveryGap)?;
        if migration_output.document_id != document.document_id
            || migration_output.schema_version != document.schema_version
            || migration_output.provenance != document.provenance
            || migration_output.revision > document.revision
        {
            return Err(CoreError::RecoveryGap);
        }
        if records.iter().any(|record| {
            record.schema_version == document.schema_version
                && record.new_revision <= migration_output.revision
        }) {
            // A migration snapshot is the first current-schema durable
            // boundary. Current-schema transactions cannot exist behind it;
            // accepting them would create unaudited ghost history.
            return Err(CoreError::RecoveryGap);
        }

        let checkpoint_records = records
            .iter()
            .filter(|record| {
                record.schema_version == document.schema_version
                    && record.new_revision > migration_output.revision
                    && record.new_revision <= document.revision
            })
            .collect::<Vec<_>>();
        let expected_count = document
            .revision
            .checked_sub(migration_output.revision)
            .and_then(|count| usize::try_from(count).ok())
            .ok_or(CoreError::RecoveryGap)?;
        if checkpoint_records.len() != expected_count {
            return Err(CoreError::RecoveryGap);
        }
        for (index, record) in checkpoint_records.iter().enumerate() {
            let offset = u32::try_from(index).map_err(|_| CoreError::RecoveryGap)?;
            let base_revision = migration_output
                .revision
                .checked_add(offset)
                .ok_or(CoreError::RecoveryGap)?;
            let new_revision = base_revision.checked_add(1).ok_or(CoreError::RecoveryGap)?;
            if record.base_revision != base_revision
                || record.new_revision != new_revision
                || (index == 0
                    && record.before_hash != canonical_hash(&canonical_bytes(migration_output)?))
                || (index > 0 && checkpoint_records[index - 1].after_hash != record.before_hash)
            {
                return Err(CoreError::RecoveryGap);
            }
        }

        let mut reversed = document.clone();
        for record in checkpoint_records.iter().rev() {
            reversed = replay_inverse(&reversed, record).map_err(|_| CoreError::RecoveryGap)?;
        }
        if reversed != *migration_output {
            return Err(CoreError::RecoveryGap);
        }

        let mut replayed = migration_output.clone();
        let mut rebuilt_history = HistoryState::default();
        for record in checkpoint_records {
            let before = replayed.clone();
            replayed = replay_forward(&before, record).map_err(|_| CoreError::RecoveryGap)?;
            replay_history_effect(&mut rebuilt_history, record, &before, &replayed)?;
        }
        if replayed != *document || rebuilt_history != *history {
            return Err(CoreError::RecoveryGap);
        }
        return Ok(());
    }
    if migration_output.is_some() {
        return Err(CoreError::RecoveryGap);
    }

    let checkpoint_records = records
        .iter()
        .filter(|record| {
            record.schema_version == document.schema_version
                && record.new_revision <= document.revision
        })
        .collect::<Vec<_>>();
    let expected_count = usize::try_from(document.revision).map_err(|_| CoreError::RecoveryGap)?;
    if checkpoint_records.len() != expected_count {
        return Err(CoreError::RecoveryGap);
    }
    for (index, record) in checkpoint_records.iter().enumerate() {
        let new_revision = u32::try_from(index + 1).map_err(|_| CoreError::RecoveryGap)?;
        if record.base_revision != new_revision - 1 || record.new_revision != new_revision {
            return Err(CoreError::RecoveryGap);
        }
        if index > 0 && checkpoint_records[index - 1].after_hash != record.before_hash {
            return Err(CoreError::RecoveryGap);
        }
    }

    let create = *checkpoint_records.first().ok_or(CoreError::RecoveryGap)?;
    let mut base_document = document.clone();
    for record in checkpoint_records.iter().skip(1).rev() {
        base_document =
            replay_inverse(&base_document, record).map_err(|_| CoreError::RecoveryGap)?;
    }
    let mut rebuilt_history = HistoryState {
        entries: Vec::new(),
        cursor: 0,
        seen_command_ids: vec![create.command_id.clone()],
    };
    validate_create_record(&base_document, &rebuilt_history, create)?;

    let mut replayed = base_document;
    for record in checkpoint_records.iter().skip(1) {
        let before = replayed.clone();
        replayed = replay_forward(&before, record).map_err(|_| CoreError::RecoveryGap)?;
        replay_history_effect(&mut rebuilt_history, record, &before, &replayed)?;
    }
    if replayed != *document || rebuilt_history != *history {
        return Err(CoreError::RecoveryGap);
    }
    Ok(())
}

fn validate_create_record(
    document: &FlowDocument,
    history: &HistoryState,
    create: &TransactionRecord,
) -> Result<(), CoreError> {
    if create.record_format_version != RECORD_FORMAT_VERSION
        || create.transaction_id != create.command_id
        || create.document_id != document.document_id
        || create.schema_version != document.schema_version
        || create.base_revision != 0
        || create.new_revision != 1
        || create.before_hash != "none"
        || create.after_hash != canonical_hash(&canonical_bytes(document)?)
        || create.command_type != "createSample"
        || create.modality != SourceModality::System
        || create.forward_operations != [Operation::CreateDocument]
        || create.inverse_operations != [Operation::DeleteDocument]
        || create.anchor_mapping != crate::anchor::AnchorMapping::identity()
        || create.history_effect != HistoryEffect::Create
        || !history.entries.is_empty()
        || history.cursor != 0
        || history.seen_command_ids != [create.command_id.clone()]
    {
        return Err(CoreError::RecoveryGap);
    }
    Ok(())
}

fn normalize_and_bind_audits(
    audits: &[AuditRecord],
    transactions: &[TransactionRecord],
    document: &FlowDocument,
    migration_boundary: Option<&MigrationBoundaryRecord>,
) -> Result<Vec<AuditRecord>, CoreError> {
    let mut by_id = BTreeMap::<String, AuditRecord>::new();
    for audit in audits {
        validate_audit(audit)?;
        if audit.document_id() != &document.document_id || audit.new_revision() > document.revision
        {
            return Err(CoreError::RecoveryGap);
        }
        let audit_id = audit.audit_id().as_str().to_owned();
        match by_id.get(&audit_id) {
            Some(existing) if existing == audit => continue,
            Some(_) => return Err(CoreError::RecoveryGap),
            None => {
                by_id.insert(audit_id, audit.clone());
            }
        }
    }
    let mut audits = by_id.into_values().collect::<Vec<_>>();
    audit::sort_events(&mut audits);
    for audit in &audits {
        match (audit.action(), audit.outcome()) {
            (AuditAction::Create | AuditAction::Command { .. }, AuditOutcome::Success) => {
                let transaction = transactions
                    .iter()
                    .find(|record| &record.transaction_id == audit.transaction_id())
                    .ok_or(CoreError::RecoveryGap)?;
                if audit.audit_id() != &transaction.command_id
                    || audit.command_id() != &transaction.command_id
                    || audit.base_revision() != transaction.base_revision
                    || audit.new_revision() != transaction.new_revision
                    || audit.durable_sequence() != u64::from(transaction.new_revision)
                    || audit.transaction_type() != transaction.command_type
                    || audit.modality() != &transaction.modality
                    || audit.timestamp().as_str() != transaction.issued_at
                    || audit.metadata()
                        != [AuditMetadata::SchemaVersion {
                            value: transaction.schema_version,
                        }]
                {
                    return Err(CoreError::RecoveryGap);
                }
            }
            (AuditAction::Command { .. }, AuditOutcome::Failure { .. }) => {
                let has_revision_anchor = transactions.iter().any(|transaction| {
                    transaction.document_id == *audit.document_id()
                        && transaction.new_revision == audit.new_revision()
                }) || migration_boundary.is_some_and(|boundary| {
                    boundary.document_id == *audit.document_id()
                        && boundary.revision == audit.new_revision()
                });
                if audit.audit_id() == audit.command_id()
                    || audit.transaction_id() != audit.command_id()
                    || audit.durable_sequence() != u64::from(audit.new_revision()) + 1
                    || audit.metadata()
                        != [AuditMetadata::SchemaVersion {
                            value: document.schema_version,
                        }]
                    || !has_revision_anchor
                {
                    return Err(CoreError::RecoveryGap);
                }
            }
            (AuditAction::Recovery, AuditOutcome::Success | AuditOutcome::Failure { .. }) => {
                let has_revision_anchor = transactions.iter().any(|transaction| {
                    transaction.document_id == *audit.document_id()
                        && transaction.new_revision == audit.new_revision()
                }) || migration_boundary.is_some_and(|boundary| {
                    boundary.document_id == *audit.document_id()
                        && boundary.revision == audit.new_revision()
                });
                if audit.audit_id() != audit.command_id()
                    || audit.transaction_id() != audit.command_id()
                    || audit.durable_sequence() != u64::from(audit.new_revision()) + 1
                    || !has_revision_anchor
                {
                    return Err(CoreError::RecoveryGap);
                }
            }
            (AuditAction::Migration, AuditOutcome::Success) => {
                let boundary = migration_boundary.ok_or(CoreError::RecoveryGap)?;
                if audit.audit_id() != &boundary.migration_id
                    || audit.command_id() != &boundary.migration_id
                    || audit.transaction_id() != &boundary.migration_id
                    || audit.base_revision() != boundary.revision
                    || audit.new_revision() != boundary.revision
                    || audit.durable_sequence() != u64::from(boundary.revision)
                    || audit.transaction_type() != "migration"
                    || audit.modality() != &SourceModality::System
                    || audit.timestamp().as_str() != boundary.issued_at
                    || audit.metadata()
                        != [AuditMetadata::Migration {
                            from_schema_version: boundary.source_schema_version,
                            to_schema_version: boundary.current_schema_version,
                        }]
                {
                    return Err(CoreError::RecoveryGap);
                }
            }
            _ => return Err(CoreError::RecoveryGap),
        }
    }
    for transaction in transactions.iter().filter(|record| {
        record.schema_version == document.schema_version && record.new_revision <= document.revision
    }) {
        if !audits.iter().any(|audit| {
            audit.transaction_id() == &transaction.transaction_id
                && matches!(audit.outcome(), AuditOutcome::Success)
                && matches!(
                    audit.action(),
                    AuditAction::Create | AuditAction::Command { .. }
                )
        }) {
            return Err(CoreError::RecoveryGap);
        }
    }
    if let Some(boundary) = migration_boundary {
        let migration_count = audits
            .iter()
            .filter(|audit| {
                audit.audit_id() == &boundary.migration_id
                    && matches!(audit.action(), AuditAction::Migration)
                    && matches!(audit.outcome(), AuditOutcome::Success)
            })
            .count();
        if migration_count != 1 {
            return Err(CoreError::RecoveryGap);
        }
    }
    Ok(audits)
}

pub(crate) fn replay_history_effect(
    history: &mut HistoryState,
    record: &TransactionRecord,
    before: &FlowDocument,
    after: &FlowDocument,
) -> Result<(), CoreError> {
    if history.seen_command_ids.contains(&record.command_id) {
        return Err(CoreError::RecoveryGap);
    }
    let before_semantic_hash = semantic_hash(before).map_err(|_| CoreError::RecoveryGap)?;
    let after_semantic_hash = semantic_hash(after).map_err(|_| CoreError::RecoveryGap)?;
    let cursor = usize::try_from(history.cursor).map_err(|_| CoreError::RecoveryGap)?;

    match &record.history_effect {
        HistoryEffect::Commit { entry_command_id } => {
            if entry_command_id != &record.command_id
                || ![
                    "insertText",
                    "replaceText",
                    "replaceSelection",
                    "deleteText",
                    "setNodeStyle",
                    "insertNode",
                    "deleteNode",
                    "splitTextBlock",
                    "mergeTextBlocks",
                    "deleteSubtree",
                    "setInlineMarks",
                    "setInlineMark",
                    "setBlockAttributes",
                    "setBlockStyle",
                    "setListKind",
                    "continueListItem",
                    "exitListItem",
                    "indentListItem",
                    "outdentListItem",
                    "insertPageBreak",
                    "removePageBreak",
                    "insertTable",
                    "insertImage",
                    "replaceImage",
                    "setImageAccessibility",
                    "removeImage",
                    "addTableRow",
                    "removeTableRow",
                    "addTableColumn",
                    "removeTableColumn",
                    "setTableHeaderRow",
                    "removeTable",
                    "setField",
                    "batch",
                ]
                .contains(&record.command_type.as_str())
            {
                return Err(CoreError::RecoveryGap);
            }
            history.entries.truncate(cursor);
            history.entries.push(HistoryEntry {
                command_id: record.command_id.clone(),
                forward_operations: record.forward_operations.clone(),
                inverse_operations: record.inverse_operations.clone(),
                before_semantic_hash,
                after_semantic_hash,
            });
            history.cursor =
                u32::try_from(history.entries.len()).map_err(|_| CoreError::RecoveryGap)?;
        }
        HistoryEffect::Undo { entry_command_id } => {
            if record.command_type != "undo" {
                return Err(CoreError::RecoveryGap);
            }
            let entry = cursor
                .checked_sub(1)
                .and_then(|index| history.entries.get(index))
                .ok_or(CoreError::RecoveryGap)?;
            if &entry.command_id != entry_command_id
                || record.forward_operations != entry.inverse_operations
                || record.inverse_operations != entry.forward_operations
                || before_semantic_hash != entry.after_semantic_hash
                || after_semantic_hash != entry.before_semantic_hash
            {
                return Err(CoreError::RecoveryGap);
            }
            history.cursor = history
                .cursor
                .checked_sub(1)
                .ok_or(CoreError::RecoveryGap)?;
        }
        HistoryEffect::Redo { entry_command_id } => {
            if record.command_type != "redo" {
                return Err(CoreError::RecoveryGap);
            }
            let entry = history.entries.get(cursor).ok_or(CoreError::RecoveryGap)?;
            if &entry.command_id != entry_command_id
                || record.forward_operations != entry.forward_operations
                || record.inverse_operations != entry.inverse_operations
                || before_semantic_hash != entry.before_semantic_hash
                || after_semantic_hash != entry.after_semantic_hash
            {
                return Err(CoreError::RecoveryGap);
            }
            history.cursor = history
                .cursor
                .checked_add(1)
                .ok_or(CoreError::RecoveryGap)?;
        }
        HistoryEffect::Create => return Err(CoreError::RecoveryGap),
    }
    history.seen_command_ids.push(record.command_id.clone());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn operation_result(
    document: FlowDocument,
    history: HistoryState,
    canonical_json: String,
    canonical_hash: String,
    transaction: TransactionRecord,
    audit: AuditRecord,
    selection: Option<DirectionalSelection>,
    mut asset_records: Vec<store::AssetRecord>,
) -> Result<OperationResult, CoreError> {
    let snapshot = SnapshotRecord {
        record_format_version: RECORD_FORMAT_VERSION,
        document_id: document.document_id.clone(),
        revision: document.revision,
        schema_version: document.schema_version,
        canonical_json: canonical_json.clone(),
        canonical_hash: canonical_hash.clone(),
        history: history.clone(),
        migration_boundary: None,
    };
    for descriptor in document
        .assets
        .iter()
        .filter(|descriptor| descriptor.byte_length == 0)
    {
        if !asset_records
            .iter()
            .any(|record| record.content_hash == descriptor.content_hash)
        {
            asset_records.push(store::AssetRecord {
                record_format_version: RECORD_FORMAT_VERSION,
                content_hash: descriptor.content_hash.clone(),
                bytes: Vec::new(),
            });
        }
    }
    let replace_existing = transaction.base_revision == 0;
    let session = session_dto(&document, history, canonical_json, canonical_hash.clone())?;
    let view = inspector_view(&document, canonical_hash, None, vec![audit.clone()])?;
    let editor = editor_response(&document, selection)?;
    Ok(OperationResult {
        session,
        view,
        editor,
        commit: PersistenceCommit {
            replace_existing,
            snapshot,
            transaction,
            audit,
            assets: asset_records,
        },
    })
}

fn editor_response(
    document: &FlowDocument,
    selection: Option<DirectionalSelection>,
) -> Result<EditorSessionResponse, CoreError> {
    let session = match selection {
        Some(selection) => EditorSessionState::from_document_with_selection(document, selection)?,
        None => EditorSessionState::from_document(document)?,
    };
    let view = session.view_for_document(document);
    Ok(EditorSessionResponse { session, view })
}

fn session_dto(
    document: &FlowDocument,
    history: HistoryState,
    canonical_json: String,
    canonical_hash: String,
) -> Result<SessionDto, CoreError> {
    let next_command_target = document
        .content
        .iter()
        .find(|node| node.legacy_text().is_some())
        .map(|node| {
            let utf16_offset = u32::try_from(node.text().encode_utf16().count())
                .map(Utf16Offset::new)
                .map_err(|_| CommandError::InvalidRange)?;
            Ok::<_, CommandError>(LogicalPosition {
                node_id: node.id.clone(),
                utf16_offset,
                affinity: Affinity::Forward,
            })
        })
        .transpose()?;
    Ok(SessionDto {
        canonical_json,
        canonical_hash,
        document_id: document.document_id.clone(),
        revision: document.revision,
        next_command_target,
        history,
    })
}

fn inspector_view(
    document: &FlowDocument,
    canonical_hash: String,
    source_canonical_hash: Option<&str>,
    audit: Vec<AuditRecord>,
) -> Result<InspectorView, CoreError> {
    let content_node_count =
        u32::try_from(document.content.len()).map_err(|_| SchemaError::invalid_document())?;
    let field_count =
        u32::try_from(document.fields.len()).map_err(|_| SchemaError::invalid_document())?;
    let asset_count =
        u32::try_from(document.assets.len()).map_err(|_| SchemaError::invalid_document())?;
    let mut revision_provenance = RevisionProvenance::from_document(document, &canonical_hash)?;
    if let Some(source_canonical_hash) = source_canonical_hash {
        revision_provenance = revision_provenance
            .with_source_hashes(vec![RevisionHash::parse(source_canonical_hash.to_owned())?])?;
    }
    Ok(InspectorView {
        document_id: document.document_id.clone(),
        schema_version: document.schema_version,
        revision: document.revision,
        canonical_hash,
        locale: document.locale.clone(),
        content_node_count,
        field_count,
        asset_count,
        revision_provenance,
        audit,
    })
}

fn safe_audit(
    command_id: &str,
    document: &FlowDocument,
    base_revision: u32,
    command_type: &str,
    modality: SourceModality,
    timestamp: &str,
) -> Result<AuditRecord, CoreError> {
    let command_id = CommandId::new(command_id)?;
    let action = match command_type {
        "createSample" => AuditAction::Create,
        "recovery" => AuditAction::Recovery,
        value => AuditAction::Command {
            command_kind: AuditCommandKind::from_transaction_type(value)
                .ok_or(CoreError::UnsafeAuditRecord)?,
        },
    };
    Ok(AuditRecord::success(
        command_id.clone(),
        document.document_id.clone(),
        command_id.clone(),
        command_id,
        base_revision,
        document.revision,
        u64::from(document.revision),
        AuditTimestamp::parse(timestamp.to_owned())?,
        action,
        modality,
        vec![AuditMetadata::SchemaVersion {
            value: document.schema_version,
        }],
    )?)
}

fn validate_audit(audit: &AuditRecord) -> Result<(), CoreError> {
    audit.validate()?;
    if audit.transaction_id() != audit.command_id() || audit.timestamp().as_str().is_empty() {
        return Err(CoreError::UnsafeAuditRecord);
    }
    Ok(())
}

fn canonical_string(document: &FlowDocument) -> Result<String, CoreError> {
    String::from_utf8(canonical_bytes(document)?).map_err(|_| SchemaError::serialization().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn success<T>(response: ApiResponse<T>) -> T {
        assert!(response.ok, "unexpected error: {:?}", response.error);
        response.value.expect("successful response has a value")
    }

    #[test]
    fn empty_and_image_only_documents_publish_a_null_command_target() {
        for keep_image in [false, true] {
            let mut document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
            document.fields.clear();
            document
                .content
                .retain(|node| keep_image && node.asset_id().is_some());
            if !keep_image {
                document.assets.clear();
            }
            let canonical_json = canonical_string(&document).expect("canonical document");
            let session = session_dto(
                &document,
                HistoryState::default(),
                canonical_json.clone(),
                canonical_hash(canonical_json.as_bytes()),
            )
            .expect("session DTO");

            assert_eq!(session.next_command_target, None);
            assert_eq!(
                serde_json::to_value(&session).expect("session JSON")["nextCommandTarget"],
                serde_json::Value::Null
            );
        }
    }

    #[test]
    fn sample_command_and_recovery_preserve_the_canonical_hash() {
        let created = success(create_sample(CreateSampleRequest {
            requested_locale: "uk-UA".to_owned(),
            issued_at: "2026-08-14T00:00:00Z".to_owned(),
        }));
        let command_id =
            CommandId::new("00000000-0000-4000-8000-000000000202").expect("command id");
        let history = created.session.history.clone();
        let target = created
            .session
            .next_command_target
            .clone()
            .expect("sample command target");
        let applied = success(apply_command(ApplyCommandRequest {
            canonical_json: created.session.canonical_json,
            history,
            command: CommandDto {
                command_id,
                base_revision: created.session.revision,
                modality: SourceModality::Ui,
                issued_at: "2026-08-14T00:00:01Z".to_owned(),
                kind: CommandKind::InsertText {
                    target,
                    text: " — typed mutation".to_owned(),
                },
            },
        }));

        let recovered = success(recover(RecoverRequest {
            snapshots: vec![created.commit.snapshot, applied.commit.snapshot.clone()],
            transactions: vec![created.commit.transaction, applied.commit.transaction],
            audits: vec![created.commit.audit, applied.commit.audit],
            assets: created.commit.assets,
            sources: Vec::new(),
        }));
        assert_eq!(
            recovered.session.canonical_hash,
            applied.session.canonical_hash
        );
        assert_eq!(recovered.session.revision, 2);
        assert_eq!(recovered.view.content_node_count, 3);
        assert!(
            !serde_json::to_string(&recovered.view)
                .expect("inspector serializes")
                .contains("typed mutation")
        );
        assert!(
            !serde_json::to_string(&recovered.view)
                .expect("inspector serializes")
                .contains("Український")
        );
    }

    #[test]
    fn migration_dto_returns_the_registry_bytes_hash_and_provenance() {
        let older = include_str!("../../../fixtures/flowdoc/older.json")
            .strip_suffix('\n')
            .unwrap_or(include_str!("../../../fixtures/flowdoc/older.json"));
        let migrated = include_str!("../../../fixtures/flowdoc/schema-v2-migrated.json")
            .strip_suffix('\n')
            .unwrap_or(include_str!(
                "../../../fixtures/flowdoc/schema-v2-migrated.json"
            ));
        let result = success(migrate_document(MigrateDocumentRequest {
            canonical_json: older.to_owned(),
            migration_id: CommandId::new("00000000-0000-4000-8000-000000000204")
                .expect("migration id"),
            issued_at: "2026-08-14T00:00:03Z".to_owned(),
            assets: vec![store::AssetRecord {
                record_format_version: RECORD_FORMAT_VERSION,
                content_hash:
                    "blake3:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
                        .to_owned(),
                bytes: Vec::new(),
            }],
        }));

        assert_eq!(result.canonical_json, migrated);
        assert_eq!(
            result.canonical_hash,
            include_str!("../../../fixtures/flowdoc/schema-v2-migrated.hash").trim()
        );
        assert!(matches!(result.provenance, Provenance::Migrated { .. }));
        assert_eq!(result.report.source_schema_version, 0);
        assert!(result.report.requires_new_snapshot);
        let provenance_json =
            serde_json::to_string(&result.revision_provenance).expect("provenance");
        assert!(provenance_json.contains(&canonical_hash(older.as_bytes())));

        use crate::store::DocumentStore;
        let commit = result.commit.clone().expect("migration commit");
        let mut missing_boundary = commit.clone();
        missing_boundary.snapshot.migration_boundary = None;
        let rejected = recover(RecoverRequest {
            snapshots: vec![missing_boundary.snapshot],
            transactions: Vec::new(),
            audits: vec![missing_boundary.audit],
            assets: missing_boundary.assets,
            sources: vec![missing_boundary.source],
        });
        assert!(!rejected.ok);
        assert_eq!(
            rejected.error.expect("boundary error").code,
            "FLOW_RECOVERY_GAP"
        );

        let mut store = store::InMemoryDocumentStore::default();
        store
            .commit_migration_atomic(commit)
            .expect("atomic migration boundary");
        let recovered = success(recover(store.load_records().expect("migration records")));
        assert_eq!(recovered.session.canonical_hash, result.canonical_hash);
    }

    #[test]
    fn stale_command_is_atomic() {
        let created = success(create_sample(CreateSampleRequest {
            requested_locale: "uk-UA".to_owned(),
            issued_at: "2026-08-14T00:00:00Z".to_owned(),
        }));
        let before_hash = created.session.canonical_hash.clone();
        let history = created.session.history.clone();
        let target = created
            .session
            .next_command_target
            .clone()
            .expect("sample command target");
        let response = apply_command(ApplyCommandRequest {
            canonical_json: created.session.canonical_json,
            history,
            command: CommandDto {
                command_id: CommandId::new("00000000-0000-4000-8000-000000000203")
                    .expect("command id"),
                base_revision: 0,
                modality: SourceModality::Ui,
                issued_at: "2026-08-14T00:00:01Z".to_owned(),
                kind: CommandKind::InsertText {
                    target,
                    text: "must not appear".to_owned(),
                },
            },
        });
        assert!(!response.ok);
        let error = response.error.expect("error");
        assert_eq!(error.code, "FLOW_STALE_REVISION");
        let audit_json = serde_json::to_string(&error.audit.expect("safe failure audit"))
            .expect("audit serialization");
        assert!(audit_json.contains("staleRevision"));
        assert!(!audit_json.contains("must not appear"));
        assert_eq!(before_hash, created.session.canonical_hash);
    }

    #[test]
    fn utf16_offsets_do_not_split_surrogate_pairs() {
        assert_eq!(schema::utf16_to_byte_offset("A😀Б", 1), Some(1));
        assert_eq!(schema::utf16_to_byte_offset("A😀Б", 2), None);
        assert_eq!(schema::utf16_to_byte_offset("A😀Б", 3), Some(5));
    }
}

//! Canonical, deterministic FlowPDF semantics and persistence DTOs.

#![forbid(unsafe_code)]

pub mod anchor;
pub mod canonical;
pub mod model;
pub mod schema;
pub mod transaction;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use anchor::Utf16Offset;
use canonical::{canonical_bytes, canonical_hash, decode_canonical};
use model::{
    Affinity, CommandId, ContentNodeKind, DocumentId, FlowDocument, LogicalPosition, Provenance,
    SCHEMA_VERSION,
};
use schema::{DocumentLimits, MigrationRegistry, MigrationReport, SchemaError};
pub use transaction::{
    Command as CommandDto, CommandKind, HistoryState, Operation, SourceModality,
    Transaction as TransactionRecord,
};
use transaction::{CommandError, EditorState, HistoryEffect, TransactionService};

const SAMPLE_CREATE_COMMAND_ID: &str = "00000000-0000-4000-8000-000000000201";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSampleRequest {
    pub requested_locale: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrateDocumentResult {
    pub canonical_json: String,
    pub canonical_hash: String,
    pub provenance: Provenance,
    pub report: MigrationReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotRecord {
    pub document_id: DocumentId,
    pub revision: u32,
    pub schema_version: u32,
    pub canonical_json: String,
    pub canonical_hash: String,
    pub history: HistoryState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditRecord {
    pub audit_id: String,
    pub document_id: DocumentId,
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SafeMetadata {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistenceCommit {
    pub replace_existing: bool,
    pub snapshot: SnapshotRecord,
    pub transaction: TransactionRecord,
    pub audit: AuditRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDto {
    pub canonical_json: String,
    pub canonical_hash: String,
    pub document_id: DocumentId,
    pub revision: u32,
    pub next_command_target: LogicalPosition,
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
    pub document_summary: String,
    pub provenance: String,
    pub audit: Vec<AuditRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationResult {
    pub session: SessionDto,
    pub view: InspectorView,
    pub commit: PersistenceCommit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoverRequest {
    pub snapshot: SnapshotRecord,
    pub transactions: Vec<TransactionRecord>,
    pub audits: Vec<AuditRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoverResult {
    pub session: SessionDto,
    pub view: InspectorView,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErrorDto {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    #[error("The boundary request could not be decoded")]
    Decode,
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error(transparent)]
    Command(#[from] CommandError),
    #[error("The persisted record set is incomplete or discontinuous")]
    RecoveryGap,
    #[error("The persisted record hash does not match canonical content")]
    HashMismatch,
    #[error("The persisted audit record violates the redaction allowlist")]
    UnsafeAuditRecord,
}

impl CoreError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Decode => "FLOW_DECODE_ERROR",
            Self::Schema(error) => error.code(),
            Self::Command(error) => error.code(),
            Self::RecoveryGap => "FLOW_RECOVERY_GAP",
            Self::HashMismatch => "FLOW_HASH_MISMATCH",
            Self::UnsafeAuditRecord => "FLOW_UNSAFE_AUDIT_RECORD",
        }
    }
}

impl From<CoreError> for ErrorDto {
    fn from(error: CoreError) -> Self {
        Self {
            code: error.code().to_owned(),
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
    let document = FlowDocument::deterministic_sample(&request.requested_locale)?;
    let canonical_json = canonical_string(&document)?;
    let canonical_hash = canonical_hash(canonical_json.as_bytes());
    let command_id = CommandId::new(SAMPLE_CREATE_COMMAND_ID)?;
    let history = HistoryState {
        entries: Vec::new(),
        cursor: 0,
        seen_command_ids: vec![command_id.clone()],
    };
    let transaction = TransactionRecord {
        transaction_id: command_id.clone(),
        document_id: document.document_id.clone(),
        command_id,
        base_revision: 0,
        new_revision: 1,
        command_type: "createSample".to_owned(),
        modality: SourceModality::System,
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
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
        "2026-08-14T00:00:00Z",
    );
    operation_result(
        document,
        history,
        canonical_json,
        canonical_hash,
        transaction,
        audit,
    )
}

#[must_use]
pub fn apply_command(request: ApplyCommandRequest) -> ApiResponse<OperationResult> {
    match apply_command_inner(request) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => ApiResponse::failure(error),
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

fn migrate_document_inner(
    request: MigrateDocumentRequest,
) -> Result<MigrateDocumentResult, CoreError> {
    let outcome = MigrationRegistry::current().migrate(request.canonical_json.as_bytes())?;
    let canonical_json =
        String::from_utf8(outcome.canonical_bytes).map_err(|_| SchemaError::serialization())?;
    Ok(MigrateDocumentResult {
        canonical_json,
        canonical_hash: outcome.canonical_hash,
        provenance: outcome.document.provenance,
        report: outcome.report,
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
    );
    operation_result(
        document,
        history,
        canonical_json,
        resulting_hash,
        applied.transaction,
        audit,
    )
}

#[must_use]
pub fn recover(request: RecoverRequest) -> ApiResponse<RecoverResult> {
    match recover_inner(request) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => ApiResponse::failure(error),
    }
}

fn recover_inner(mut request: RecoverRequest) -> Result<RecoverResult, CoreError> {
    let replay_bytes = request
        .transactions
        .iter()
        .try_fold(0_usize, |total, record| {
            let bytes = serde_json::to_vec(record).map_err(|_| SchemaError::serialization())?;
            DocumentLimits::V1.check_transaction(
                record.forward_operations.len() + record.inverse_operations.len(),
                bytes.len(),
            )?;
            total
                .checked_add(bytes.len())
                .ok_or_else(SchemaError::invalid_document)
        })?;
    DocumentLimits::V1.check_recovery(request.transactions.len(), replay_bytes)?;

    let document = decode_canonical(request.snapshot.canonical_json.as_bytes())?;
    if request.snapshot.document_id != document.document_id
        || request.snapshot.revision != document.revision
        || request.snapshot.schema_version != document.schema_version
    {
        return Err(SchemaError::invalid_document().into());
    }
    let computed_hash = canonical_hash(request.snapshot.canonical_json.as_bytes());
    if computed_hash != request.snapshot.canonical_hash {
        return Err(CoreError::HashMismatch);
    }
    let history = request.snapshot.history.clone();
    let recovered_state = EditorState::with_history(document.clone(), history.clone())?;
    if recovered_state.canonical_hash() != computed_hash {
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
    let mut transaction_command_ids = request
        .transactions
        .iter()
        .filter(|record| record.document_id == document.document_id)
        .map(|record| record.command_id.clone())
        .collect::<Vec<_>>();
    let mut history_command_ids = history.seen_command_ids.clone();
    transaction_command_ids.sort();
    history_command_ids.sort();
    if transaction_command_ids != history_command_ids {
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
        history,
        request.snapshot.canonical_json,
        computed_hash.clone(),
    )?;
    let view = inspector_view(&document, computed_hash, request.audits);
    Ok(RecoverResult { session, view })
}

fn operation_result(
    document: FlowDocument,
    history: HistoryState,
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
        history: history.clone(),
    };
    let replace_existing = transaction.base_revision == 0;
    let session = session_dto(&document, history, canonical_json, canonical_hash.clone())?;
    let view = inspector_view(&document, canonical_hash, vec![audit.clone()]);
    Ok(OperationResult {
        session,
        view,
        commit: PersistenceCommit {
            replace_existing,
            snapshot,
            transaction,
            audit,
        },
    })
}

fn session_dto(
    document: &FlowDocument,
    history: HistoryState,
    canonical_json: String,
    canonical_hash: String,
) -> Result<SessionDto, CoreError> {
    let node = document
        .content
        .iter()
        .find(|node| node.kind == ContentNodeKind::Paragraph)
        .ok_or(CommandError::InvalidTarget)?;
    let utf16_offset = u32::try_from(node.text.encode_utf16().count())
        .map(Utf16Offset::new)
        .map_err(|_| CommandError::InvalidRange)?;
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
        history,
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
        .filter(|node| node.kind == ContentNodeKind::Paragraph)
        .map(|node| node.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let provenance = match document.provenance {
        Provenance::LocalSample { .. } => "localSample",
        Provenance::Migrated { .. } => "migrated",
    };
    InspectorView {
        document_id: document.document_id.clone(),
        schema_version: document.schema_version,
        revision: document.revision,
        canonical_hash,
        locale: document.locale.clone(),
        document_summary,
        provenance: provenance.to_owned(),
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
    if ![
        "createSample",
        "insertText",
        "replaceText",
        "deleteText",
        "setNodeStyle",
        "insertNode",
        "deleteNode",
        "setField",
        "batch",
        "undo",
        "redo",
    ]
    .contains(&audit.command_type.as_str())
    {
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
    fn sample_command_and_recovery_preserve_the_canonical_hash() {
        let created = success(create_sample(CreateSampleRequest {
            requested_locale: "uk-UA".to_owned(),
        }));
        let command_id =
            CommandId::new("00000000-0000-4000-8000-000000000202").expect("command id");
        let history = created.session.history.clone();
        let target = created.session.next_command_target.clone();
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
    fn migration_dto_returns_the_registry_bytes_hash_and_provenance() {
        let older = include_str!("../../../fixtures/flowdoc/older.json")
            .strip_suffix('\n')
            .unwrap_or(include_str!("../../../fixtures/flowdoc/older.json"));
        let migrated = include_str!("../../../fixtures/flowdoc/migrated.json")
            .strip_suffix('\n')
            .unwrap_or(include_str!("../../../fixtures/flowdoc/migrated.json"));
        let result = success(migrate_document(MigrateDocumentRequest {
            canonical_json: older.to_owned(),
        }));

        assert_eq!(result.canonical_json, migrated);
        assert_eq!(
            result.canonical_hash,
            include_str!("../../../fixtures/flowdoc/migrated.hash").trim()
        );
        assert!(matches!(result.provenance, Provenance::Migrated { .. }));
        assert_eq!(result.report.source_schema_version, 0);
        assert!(result.report.requires_new_snapshot);
    }

    #[test]
    fn stale_command_is_atomic() {
        let created = success(create_sample(CreateSampleRequest {
            requested_locale: "uk-UA".to_owned(),
        }));
        let before_hash = created.session.canonical_hash.clone();
        let history = created.session.history.clone();
        let target = created.session.next_command_target.clone();
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
        assert_eq!(response.error.expect("error").code, "FLOW_STALE_REVISION");
        assert_eq!(before_hash, created.session.canonical_hash);
    }

    #[test]
    fn utf16_offsets_do_not_split_surrogate_pairs() {
        assert_eq!(schema::utf16_to_byte_offset("A😀Б", 1), Some(1));
        assert_eq!(schema::utf16_to_byte_offset("A😀Б", 2), None);
        assert_eq!(schema::utf16_to_byte_offset("A😀Б", 3), Some(5));
    }
}

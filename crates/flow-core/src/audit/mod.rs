//! Privacy-minimized, typed audit projections.
//!
//! These records intentionally have no document text, command arguments,
//! storage payloads, audio, transcript, or arbitrary metadata field.  They
//! are durable diagnostic facts, not a second document representation.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    model::{CommandId, DocumentId},
    transaction::SourceModality,
};

const MAX_METADATA_ENTRIES: usize = 4;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuditValidationError {
    #[error("Audit timestamps must be compact UTC RFC 3339 seconds")]
    InvalidTimestamp,
    #[error("Audit durable sequence must be non-zero")]
    InvalidDurableSequence,
    #[error("Audit revision links are inconsistent with the outcome")]
    InvalidRevisionLink,
    #[error("Audit metadata is duplicated or exceeds the allowlist")]
    InvalidMetadata,
    #[error("Audit action and outcome are not a supported durable fact")]
    InvalidActionOutcome,
    #[error("Audit metadata is not valid for the action and outcome")]
    InvalidMetadataContext,
    #[error("Audit record format is unsupported")]
    UnsupportedRecordFormat,
}

/// A fixed-format timestamp prevents a caller from placing arbitrary prose in
/// an audit diagnostic field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "String", into = "String")]
pub struct AuditTimestamp(String);

impl AuditTimestamp {
    pub fn parse(value: impl Into<String>) -> Result<Self, AuditValidationError> {
        let value = value.into();
        if !crate::schema::is_compact_utc_timestamp(&value) {
            return Err(AuditValidationError::InvalidTimestamp);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AuditTimestamp {
    type Error = AuditValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<AuditTimestamp> for String {
    fn from(value: AuditTimestamp) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuditCommandKind {
    InsertText,
    ReplaceText,
    ReplaceSelection,
    DeleteText,
    SetNodeStyle,
    InsertNode,
    DeleteNode,
    SplitTextBlock,
    MergeTextBlocks,
    DeleteSubtree,
    SetField,
    Batch,
    Undo,
    Redo,
}

impl AuditCommandKind {
    #[must_use]
    pub fn from_transaction_type(value: &str) -> Option<Self> {
        Some(match value {
            "insertText" => Self::InsertText,
            "replaceText" => Self::ReplaceText,
            "replaceSelection" => Self::ReplaceSelection,
            "deleteText" => Self::DeleteText,
            "setNodeStyle" => Self::SetNodeStyle,
            "insertNode" => Self::InsertNode,
            "deleteNode" => Self::DeleteNode,
            "splitTextBlock" => Self::SplitTextBlock,
            "mergeTextBlocks" => Self::MergeTextBlocks,
            "deleteSubtree" => Self::DeleteSubtree,
            "setField" => Self::SetField,
            "batch" => Self::Batch,
            "undo" => Self::Undo,
            "redo" => Self::Redo,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn transaction_type(&self) -> &'static str {
        match self {
            Self::InsertText => "insertText",
            Self::ReplaceText => "replaceText",
            Self::ReplaceSelection => "replaceSelection",
            Self::DeleteText => "deleteText",
            Self::SetNodeStyle => "setNodeStyle",
            Self::InsertNode => "insertNode",
            Self::DeleteNode => "deleteNode",
            Self::SplitTextBlock => "splitTextBlock",
            Self::MergeTextBlocks => "mergeTextBlocks",
            Self::DeleteSubtree => "deleteSubtree",
            Self::SetField => "setField",
            Self::Batch => "batch",
            Self::Undo => "undo",
            Self::Redo => "redo",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AuditAction {
    Create,
    Command { command_kind: AuditCommandKind },
    Migration,
    Recovery,
}

impl AuditAction {
    #[must_use]
    pub const fn transaction_type(&self) -> &'static str {
        match self {
            Self::Create => "createSample",
            Self::Command { command_kind } => command_kind.transaction_type(),
            Self::Migration => "migration",
            Self::Recovery => "recovery",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuditErrorCode {
    StaleRevision,
    DuplicateCommand,
    InvalidTarget,
    InvalidRange,
    InvalidUtf16Boundary,
    InvalidGraphemeBoundary,
    InvalidAtomicPosition,
    InvalidPositionNodeKind,
    PositionNodeMismatch,
    PositionTextLimit,
    AnchorInvalidated,
    BrokenInvariant,
    NoOp,
    UndoEmpty,
    RedoEmpty,
    HistoryConflict,
    IncompatibleStructure,
    InvalidContainer,
    InvalidPreimage,
    AnchorPreimageLimit,
    RecoveryGap,
    HashMismatch,
    SchemaInvalid,
}

impl AuditErrorCode {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::StaleRevision => "FLOW_STALE_REVISION",
            Self::DuplicateCommand => "FLOW_DUPLICATE_COMMAND",
            Self::InvalidTarget => "FLOW_INVALID_TARGET",
            Self::InvalidRange => "FLOW_INVALID_RANGE",
            Self::InvalidUtf16Boundary => "FLOW_INVALID_UTF16_BOUNDARY",
            Self::InvalidGraphemeBoundary => "FLOW_INVALID_GRAPHEME_BOUNDARY",
            Self::InvalidAtomicPosition => "FLOW_INVALID_ATOMIC_POSITION",
            Self::InvalidPositionNodeKind => "FLOW_INVALID_POSITION_NODE_KIND",
            Self::PositionNodeMismatch => "FLOW_POSITION_NODE_MISMATCH",
            Self::PositionTextLimit => "FLOW_POSITION_TEXT_LIMIT",
            Self::AnchorInvalidated => "FLOW_ANCHOR_INVALIDATED",
            Self::BrokenInvariant => "FLOW_BROKEN_INVARIANT",
            Self::NoOp => "FLOW_NO_OP",
            Self::UndoEmpty => "FLOW_UNDO_EMPTY",
            Self::RedoEmpty => "FLOW_REDO_EMPTY",
            Self::HistoryConflict => "FLOW_HISTORY_CONFLICT",
            Self::IncompatibleStructure => "FLOW_INCOMPATIBLE_STRUCTURE",
            Self::InvalidContainer => "FLOW_INVALID_CONTAINER",
            Self::InvalidPreimage => "FLOW_INVALID_PREIMAGE",
            Self::AnchorPreimageLimit => "FLOW_LIMIT_ANCHOR_PREIMAGE",
            Self::RecoveryGap => "FLOW_RECOVERY_GAP",
            Self::HashMismatch => "FLOW_HASH_MISMATCH",
            Self::SchemaInvalid => "FLOW_SCHEMA_INVALID",
        }
    }

    #[must_use]
    pub fn from_stable_code(value: &str) -> Option<Self> {
        Some(match value {
            "FLOW_STALE_REVISION" => Self::StaleRevision,
            "FLOW_DUPLICATE_COMMAND" => Self::DuplicateCommand,
            "FLOW_INVALID_TARGET" => Self::InvalidTarget,
            "FLOW_INVALID_RANGE" => Self::InvalidRange,
            "FLOW_INVALID_UTF16_BOUNDARY" => Self::InvalidUtf16Boundary,
            "FLOW_INVALID_GRAPHEME_BOUNDARY" => Self::InvalidGraphemeBoundary,
            "FLOW_INVALID_ATOMIC_POSITION" => Self::InvalidAtomicPosition,
            "FLOW_INVALID_POSITION_NODE_KIND" => Self::InvalidPositionNodeKind,
            "FLOW_POSITION_NODE_MISMATCH" => Self::PositionNodeMismatch,
            "FLOW_POSITION_TEXT_LIMIT" => Self::PositionTextLimit,
            "FLOW_ANCHOR_INVALIDATED" => Self::AnchorInvalidated,
            "FLOW_BROKEN_INVARIANT" => Self::BrokenInvariant,
            "FLOW_NO_OP" => Self::NoOp,
            "FLOW_UNDO_EMPTY" => Self::UndoEmpty,
            "FLOW_REDO_EMPTY" => Self::RedoEmpty,
            "FLOW_HISTORY_CONFLICT" => Self::HistoryConflict,
            "FLOW_INCOMPATIBLE_STRUCTURE" => Self::IncompatibleStructure,
            "FLOW_INVALID_CONTAINER" => Self::InvalidContainer,
            "FLOW_INVALID_PREIMAGE" => Self::InvalidPreimage,
            "FLOW_LIMIT_ANCHOR_PREIMAGE" => Self::AnchorPreimageLimit,
            "FLOW_RECOVERY_GAP" => Self::RecoveryGap,
            "FLOW_HASH_MISMATCH" => Self::HashMismatch,
            "FLOW_SCHEMA_INVALID" => Self::SchemaInvalid,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AuditOutcome {
    Success,
    Failure { code: AuditErrorCode },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AuditMetadata {
    SchemaVersion {
        value: u32,
    },
    Migration {
        from_schema_version: u32,
        to_schema_version: u32,
    },
    RecoveryVerified,
}

impl AuditMetadata {
    const fn discriminant(&self) -> u8 {
        match self {
            Self::SchemaVersion { .. } => 0,
            Self::Migration { .. } => 1,
            Self::RecoveryVerified => 2,
        }
    }
}

/// Ordered, redacted facts emitted with a durable commit or a non-mutating
/// command failure. Fields are private so callers cannot construct a shadow
/// text/analytics record by adding ad-hoc values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditEvent {
    record_format_version: u32,
    audit_id: CommandId,
    document_id: DocumentId,
    transaction_id: CommandId,
    command_id: CommandId,
    base_revision: u32,
    new_revision: u32,
    durable_sequence: u64,
    timestamp: AuditTimestamp,
    action: AuditAction,
    modality: SourceModality,
    outcome: AuditOutcome,
    metadata: Vec<AuditMetadata>,
}

impl AuditEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn success(
        audit_id: CommandId,
        document_id: DocumentId,
        transaction_id: CommandId,
        command_id: CommandId,
        base_revision: u32,
        new_revision: u32,
        durable_sequence: u64,
        timestamp: AuditTimestamp,
        action: AuditAction,
        modality: SourceModality,
        metadata: Vec<AuditMetadata>,
    ) -> Result<Self, AuditValidationError> {
        Self::new(
            audit_id,
            document_id,
            transaction_id,
            command_id,
            base_revision,
            new_revision,
            durable_sequence,
            timestamp,
            action,
            modality,
            AuditOutcome::Success,
            metadata,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn failure(
        audit_id: CommandId,
        document_id: DocumentId,
        transaction_id: CommandId,
        command_id: CommandId,
        base_revision: u32,
        new_revision: u32,
        durable_sequence: u64,
        timestamp: AuditTimestamp,
        action: AuditAction,
        modality: SourceModality,
        code: AuditErrorCode,
        metadata: Vec<AuditMetadata>,
    ) -> Result<Self, AuditValidationError> {
        Self::new(
            audit_id,
            document_id,
            transaction_id,
            command_id,
            base_revision,
            new_revision,
            durable_sequence,
            timestamp,
            action,
            modality,
            AuditOutcome::Failure { code },
            metadata,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        audit_id: CommandId,
        document_id: DocumentId,
        transaction_id: CommandId,
        command_id: CommandId,
        base_revision: u32,
        new_revision: u32,
        durable_sequence: u64,
        timestamp: AuditTimestamp,
        action: AuditAction,
        modality: SourceModality,
        outcome: AuditOutcome,
        metadata: Vec<AuditMetadata>,
    ) -> Result<Self, AuditValidationError> {
        if durable_sequence == 0 {
            return Err(AuditValidationError::InvalidDurableSequence);
        }
        validate_action_semantics(&action, &outcome, base_revision, new_revision, &metadata)?;
        validate_metadata(&metadata)?;
        Ok(Self {
            record_format_version: crate::RECORD_FORMAT_VERSION,
            audit_id,
            document_id,
            transaction_id,
            command_id,
            base_revision,
            new_revision,
            durable_sequence,
            timestamp,
            action,
            modality,
            outcome,
            metadata,
        })
    }

    #[must_use]
    pub const fn durable_sequence(&self) -> u64 {
        self.durable_sequence
    }

    #[must_use]
    pub const fn record_format_version(&self) -> u32 {
        self.record_format_version
    }

    #[must_use]
    pub fn audit_id(&self) -> &CommandId {
        &self.audit_id
    }

    #[must_use]
    pub fn document_id(&self) -> &DocumentId {
        &self.document_id
    }

    #[must_use]
    pub fn transaction_id(&self) -> &CommandId {
        &self.transaction_id
    }

    #[must_use]
    pub fn command_id(&self) -> &CommandId {
        &self.command_id
    }

    #[must_use]
    pub const fn base_revision(&self) -> u32 {
        self.base_revision
    }

    #[must_use]
    pub const fn new_revision(&self) -> u32 {
        self.new_revision
    }

    #[must_use]
    pub fn timestamp(&self) -> &AuditTimestamp {
        &self.timestamp
    }

    #[must_use]
    pub const fn action(&self) -> &AuditAction {
        &self.action
    }

    #[must_use]
    pub const fn modality(&self) -> &SourceModality {
        &self.modality
    }

    #[must_use]
    pub const fn outcome(&self) -> &AuditOutcome {
        &self.outcome
    }

    #[must_use]
    pub fn metadata(&self) -> &[AuditMetadata] {
        &self.metadata
    }

    #[must_use]
    pub const fn transaction_type(&self) -> &'static str {
        self.action.transaction_type()
    }

    pub fn validate(&self) -> Result<(), AuditValidationError> {
        if self.record_format_version != crate::RECORD_FORMAT_VERSION {
            return Err(AuditValidationError::UnsupportedRecordFormat);
        }
        if self.durable_sequence == 0 {
            return Err(AuditValidationError::InvalidDurableSequence);
        }
        validate_action_semantics(
            &self.action,
            &self.outcome,
            self.base_revision,
            self.new_revision,
            &self.metadata,
        )?;
        validate_metadata(&self.metadata)
    }
}

impl fmt::Display for AuditEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "audit event #{}", self.durable_sequence)
    }
}

/// Sorts without coalescing: adjacent equal-type outcomes remain distinct.
pub fn sort_events(events: &mut [AuditEvent]) {
    events.sort_by(|left, right| {
        left.durable_sequence
            .cmp(&right.durable_sequence)
            .then_with(|| left.new_revision.cmp(&right.new_revision))
            .then_with(|| left.timestamp.cmp(&right.timestamp))
            .then_with(|| left.audit_id.cmp(&right.audit_id))
    });
}

fn validate_metadata(metadata: &[AuditMetadata]) -> Result<(), AuditValidationError> {
    if metadata.len() > MAX_METADATA_ENTRIES {
        return Err(AuditValidationError::InvalidMetadata);
    }
    let mut seen = [false; 3];
    for item in metadata {
        let index = usize::from(item.discriminant());
        if seen[index] {
            return Err(AuditValidationError::InvalidMetadata);
        }
        seen[index] = true;
    }
    Ok(())
}

fn validate_action_semantics(
    action: &AuditAction,
    outcome: &AuditOutcome,
    base_revision: u32,
    new_revision: u32,
    metadata: &[AuditMetadata],
) -> Result<(), AuditValidationError> {
    match (action, outcome) {
        (AuditAction::Create, AuditOutcome::Success) => {
            if base_revision != 0 || new_revision != 1 {
                return Err(AuditValidationError::InvalidRevisionLink);
            }
            require_schema_metadata(metadata)
        }
        (AuditAction::Command { .. }, AuditOutcome::Success) => {
            if base_revision.checked_add(1) != Some(new_revision) {
                return Err(AuditValidationError::InvalidRevisionLink);
            }
            require_schema_metadata(metadata)
        }
        (AuditAction::Command { .. }, AuditOutcome::Failure { .. }) => {
            if new_revision != base_revision {
                return Err(AuditValidationError::InvalidRevisionLink);
            }
            require_schema_metadata(metadata)
        }
        (AuditAction::Migration, AuditOutcome::Success) => {
            if new_revision != base_revision {
                return Err(AuditValidationError::InvalidRevisionLink);
            }
            require_migration_metadata(metadata)
        }
        (AuditAction::Recovery, AuditOutcome::Success) => {
            if new_revision != base_revision {
                return Err(AuditValidationError::InvalidRevisionLink);
            }
            require_exact_metadata(metadata, |item| {
                matches!(item, AuditMetadata::RecoveryVerified)
            })
        }
        (AuditAction::Recovery, AuditOutcome::Failure { .. }) => {
            if new_revision != base_revision {
                return Err(AuditValidationError::InvalidRevisionLink);
            }
            if metadata.is_empty() {
                Ok(())
            } else {
                Err(AuditValidationError::InvalidMetadataContext)
            }
        }
        _ => Err(AuditValidationError::InvalidActionOutcome),
    }
}

fn require_schema_metadata(metadata: &[AuditMetadata]) -> Result<(), AuditValidationError> {
    require_exact_metadata(
        metadata,
        |item| matches!(item, AuditMetadata::SchemaVersion { value } if *value > 0),
    )
}

fn require_migration_metadata(metadata: &[AuditMetadata]) -> Result<(), AuditValidationError> {
    require_exact_metadata(metadata, |item| {
        matches!(
            item,
            AuditMetadata::Migration {
                from_schema_version,
                to_schema_version,
            } if from_schema_version < to_schema_version
        )
    })
}

fn require_exact_metadata(
    metadata: &[AuditMetadata],
    predicate: impl Fn(&AuditMetadata) -> bool,
) -> Result<(), AuditValidationError> {
    if metadata.len() == 1 && predicate(&metadata[0]) {
        Ok(())
    } else {
        Err(AuditValidationError::InvalidMetadataContext)
    }
}

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
}

/// A fixed-format timestamp prevents a caller from placing arbitrary prose in
/// an audit diagnostic field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "String", into = "String")]
pub struct AuditTimestamp(String);

impl AuditTimestamp {
    pub fn parse(value: impl Into<String>) -> Result<Self, AuditValidationError> {
        let value = value.into();
        let bytes = value.as_bytes();
        let punctuation = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':'), (19, b'Z')];
        if bytes.len() != 20
            || punctuation.iter().any(|(index, expected)| bytes[*index] != *expected)
            || bytes.iter().enumerate().any(|(index, byte)| {
                ![4, 7, 10, 13, 16, 19].contains(&index) && !byte.is_ascii_digit()
            })
        {
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
    Create,
    InsertText,
    ReplaceText,
    DeleteText,
    SetNodeStyle,
    InsertNode,
    DeleteNode,
    SetField,
    Batch,
    Undo,
    Redo,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum AuditAction {
    Create,
    Command { kind: AuditCommandKind },
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuditErrorCode {
    StaleRevision,
    DuplicateCommand,
    InvalidTarget,
    InvalidRange,
    InvalidUtf16Boundary,
    AnchorInvalidated,
    BrokenInvariant,
    UndoEmpty,
    RedoEmpty,
    HistoryConflict,
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
            Self::AnchorInvalidated => "FLOW_ANCHOR_INVALIDATED",
            Self::BrokenInvariant => "FLOW_BROKEN_INVARIANT",
            Self::UndoEmpty => "FLOW_UNDO_EMPTY",
            Self::RedoEmpty => "FLOW_REDO_EMPTY",
            Self::HistoryConflict => "FLOW_HISTORY_CONFLICT",
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
            "FLOW_ANCHOR_INVALIDATED" => Self::AnchorInvalidated,
            "FLOW_BROKEN_INVARIANT" => Self::BrokenInvariant,
            "FLOW_UNDO_EMPTY" => Self::UndoEmpty,
            "FLOW_REDO_EMPTY" => Self::RedoEmpty,
            "FLOW_HISTORY_CONFLICT" => Self::HistoryConflict,
            "FLOW_RECOVERY_GAP" => Self::RecoveryGap,
            "FLOW_HASH_MISMATCH" => Self::HashMismatch,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum AuditOutcome {
    Success,
    Failure { code: AuditErrorCode },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum AuditMetadata {
    SchemaVersion { value: u32 },
    Migration { from_schema_version: u32, to_schema_version: u32 },
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
        match &outcome {
            AuditOutcome::Success if new_revision <= base_revision => {
                return Err(AuditValidationError::InvalidRevisionLink);
            }
            AuditOutcome::Failure if new_revision != base_revision => {
                return Err(AuditValidationError::InvalidRevisionLink);
            }
            _ => {}
        }
        validate_metadata(&metadata)?;
        Ok(Self {
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
    pub fn audit_id(&self) -> &CommandId {
        &self.audit_id
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
        left
            .durable_sequence
            .cmp(&right.durable_sequence)
            .then_with(|| left.new_revision.cmp(&right.new_revision))
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

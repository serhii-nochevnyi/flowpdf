//! Typed commands, immutable transactions, and deterministic undo/redo.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    anchor::{
        AnchorError, AnchorMapResult, AnchorMapping, AnchorTransformation, EditorPositionError,
        NodePositionMap, ResolvedPosition, Utf16Offset, resolve_utf16_offset, utf16_length,
    },
    asset::{self, RedeemedAsset},
    canonical::{canonical_bytes, canonical_hash},
    editor_view::DirectionalSelection,
    model::{
        Affinity, AssetDescriptor, BlockAttributes, BlockKind, BlockStyle, CommandId, ContentNode,
        FieldAnchorState, FieldDescriptor, FieldId, FlowDocument, ImageAccessibility, InlineMark,
        InlineRun, ListKind, LogicalPosition, MarkSet, NodeId, ParagraphAttrs, StyleId,
        TombstoneToken,
    },
    schema::{
        DocumentLimits, MAX_TABLE_CELLS, MAX_TABLE_COLUMNS, MAX_TABLE_ROWS, SchemaError,
        validate_document,
    },
};

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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Command {
    pub command_id: CommandId,
    pub base_revision: u32,
    pub modality: SourceModality,
    pub issued_at: String,
    pub kind: CommandKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CommandKind {
    InsertText {
        target: LogicalPosition,
        text: String,
    },
    ReplaceText {
        range: TextRange,
        text: String,
    },
    ReplaceSelection {
        selection: DirectionalSelection,
        text: String,
    },
    DeleteText {
        range: TextRange,
    },
    SetNodeStyle {
        node_id: NodeId,
        style_id: Option<StyleId>,
    },
    InsertNode {
        index: u32,
        node: ContentNode,
    },
    DeleteNode {
        node_id: NodeId,
    },
    SplitTextBlock {
        node_id: NodeId,
        utf16_offset: Utf16Offset,
        new_node_id: NodeId,
    },
    MergeTextBlocks {
        first_node_id: NodeId,
        second_node_id: NodeId,
    },
    DeleteSubtree {
        node_id: NodeId,
    },
    SetInlineMarks {
        selection: DirectionalSelection,
        marks: MarkSet,
    },
    SetInlineMark {
        selection: DirectionalSelection,
        mark: InlineMark,
    },
    SetBlockAttributes {
        selection: DirectionalSelection,
        attributes: BlockAttributes,
    },
    SetBlockStyle {
        selection: DirectionalSelection,
        style: BlockStyle,
    },
    SetListKind {
        selection: DirectionalSelection,
        kind: ListKind,
    },
    ContinueListItem {
        selection: DirectionalSelection,
    },
    ExitListItem {
        selection: DirectionalSelection,
    },
    IndentListItem {
        item_id: NodeId,
    },
    OutdentListItem {
        item_id: NodeId,
    },
    InsertPageBreak {
        placement: StructuralPlacement,
    },
    RemovePageBreak {
        page_break_id: NodeId,
    },
    InsertTable {
        placement: StructuralPlacement,
        rows: u32,
        columns: u32,
        header_row: bool,
    },
    InsertImage {
        placement: StructuralPlacement,
        session_id: String,
        receipt: String,
        accessibility: ImageAccessibility,
    },
    ReplaceImage {
        image_node_id: NodeId,
        session_id: String,
        receipt: String,
        accessibility: ImageAccessibility,
    },
    SetImageAccessibility {
        image_node_id: NodeId,
        accessibility: ImageAccessibility,
    },
    RemoveImage {
        image_node_id: NodeId,
        confirmed: bool,
    },
    AddTableRow {
        selection: DirectionalSelection,
    },
    RemoveTableRow {
        selection: DirectionalSelection,
    },
    AddTableColumn {
        selection: DirectionalSelection,
    },
    RemoveTableColumn {
        selection: DirectionalSelection,
    },
    SetTableHeaderRow {
        table_id: NodeId,
        enabled: bool,
    },
    RemoveTable {
        table_id: NodeId,
        confirmed: bool,
    },
    InsertField {
        field: FieldDescriptor,
    },
    SetField {
        field_id: FieldId,
        field: FieldDescriptor,
    },
    RemoveField {
        field_id: FieldId,
        confirmed: bool,
    },
    Batch {
        mutations: Vec<Mutation>,
    },
    Undo,
    Redo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Mutation {
    InsertText {
        target: LogicalPosition,
        text: String,
    },
    ReplaceText {
        range: TextRange,
        text: String,
    },
    ReplaceSelection {
        selection: DirectionalSelection,
        text: String,
    },
    DeleteText {
        range: TextRange,
    },
    SetNodeStyle {
        node_id: NodeId,
        style_id: Option<StyleId>,
    },
    InsertNode {
        index: u32,
        node: ContentNode,
    },
    DeleteNode {
        node_id: NodeId,
    },
    SplitTextBlock {
        node_id: NodeId,
        utf16_offset: Utf16Offset,
        new_node_id: NodeId,
    },
    MergeTextBlocks {
        first_node_id: NodeId,
        second_node_id: NodeId,
    },
    DeleteSubtree {
        node_id: NodeId,
    },
    SetInlineMarks {
        selection: DirectionalSelection,
        marks: MarkSet,
    },
    SetInlineMark {
        selection: DirectionalSelection,
        mark: InlineMark,
    },
    SetBlockAttributes {
        selection: DirectionalSelection,
        attributes: BlockAttributes,
    },
    SetBlockStyle {
        selection: DirectionalSelection,
        style: BlockStyle,
    },
    SetListKind {
        selection: DirectionalSelection,
        kind: ListKind,
    },
    ContinueListItem {
        selection: DirectionalSelection,
    },
    ExitListItem {
        selection: DirectionalSelection,
    },
    IndentListItem {
        item_id: NodeId,
    },
    OutdentListItem {
        item_id: NodeId,
    },
    InsertPageBreak {
        placement: StructuralPlacement,
    },
    RemovePageBreak {
        page_break_id: NodeId,
    },
    InsertTable {
        placement: StructuralPlacement,
        rows: u32,
        columns: u32,
        header_row: bool,
    },
    InsertImage {
        placement: StructuralPlacement,
        session_id: String,
        receipt: String,
        accessibility: ImageAccessibility,
    },
    ReplaceImage {
        image_node_id: NodeId,
        session_id: String,
        receipt: String,
        accessibility: ImageAccessibility,
    },
    SetImageAccessibility {
        image_node_id: NodeId,
        accessibility: ImageAccessibility,
    },
    RemoveImage {
        image_node_id: NodeId,
        confirmed: bool,
    },
    AddTableRow {
        selection: DirectionalSelection,
    },
    RemoveTableRow {
        selection: DirectionalSelection,
    },
    AddTableColumn {
        selection: DirectionalSelection,
    },
    RemoveTableColumn {
        selection: DirectionalSelection,
    },
    SetTableHeaderRow {
        table_id: NodeId,
        enabled: bool,
    },
    RemoveTable {
        table_id: NodeId,
        confirmed: bool,
    },
    InsertField {
        field: FieldDescriptor,
    },
    SetField {
        field_id: FieldId,
        field: FieldDescriptor,
    },
    RemoveField {
        field_id: FieldId,
        confirmed: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextRange {
    pub start: LogicalPosition,
    pub end: LogicalPosition,
}

/// A structural insertion boundary. The parent is `None` for the document
/// root; otherwise it names the semantic container whose child vector is
/// being addressed. The index is an insertion point, not a DOM position.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StructuralPlacement {
    pub parent_id: Option<NodeId>,
    pub index: u32,
}

impl TextRange {
    #[must_use]
    pub fn collapsed(position: LogicalPosition) -> Self {
        Self {
            start: position.clone(),
            end: position,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Operation {
    CreateDocument,
    DeleteDocument,
    ReplaceText {
        range: TextRange,
        expected_text: String,
        replacement: String,
    },
    SetNodeStyle {
        node_id: NodeId,
        expected_style_id: Option<StyleId>,
        style_id: Option<StyleId>,
    },
    InsertNode {
        index: u32,
        node: ContentNode,
    },
    DeleteNode {
        index: u32,
        node: ContentNode,
    },
    ReplaceChildren {
        container_id: Option<NodeId>,
        expected_children: Vec<ContentNode>,
        replacement_children: Vec<ContentNode>,
        anchor_mapping: AnchorMapping,
        field_updates: Vec<FieldUpdate>,
        preimage: Option<AnchorPreimage>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        expected_assets: Vec<AssetDescriptor>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        replacement_assets: Vec<AssetDescriptor>,
    },
    SetField {
        index: u32,
        expected_field: Box<FieldDescriptor>,
        field: Box<FieldDescriptor>,
    },
    InsertField {
        index: u32,
        field: Box<FieldDescriptor>,
    },
    RemoveField {
        index: u32,
        field: Box<FieldDescriptor>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldUpdate {
    pub index: u32,
    pub expected: Box<FieldDescriptor>,
    pub replacement: Box<FieldDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum AnchorOwner {
    Field(FieldId),
    SelectionAnchor,
    SelectionFocus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnchorPreimageEntry {
    pub owner: AnchorOwner,
    pub position: LogicalPosition,
}

/// Transaction-private deletion proof. It is carried by the executable
/// transaction operation so replay can reject missing, duplicated, forged, or
/// over-budget owner state before publishing a recovered document. The public
/// editor view never projects this type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnchorPreimage {
    pub root_node_id: NodeId,
    pub deleted_node_ids: Vec<NodeId>,
    pub tombstone: TombstoneToken,
    pub owners: Vec<AnchorPreimageEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transaction {
    pub record_format_version: u32,
    pub transaction_id: CommandId,
    pub document_id: crate::model::DocumentId,
    pub schema_version: u32,
    pub command_id: CommandId,
    pub base_revision: u32,
    pub new_revision: u32,
    pub command_type: String,
    pub modality: SourceModality,
    pub issued_at: String,
    pub before_hash: String,
    pub after_hash: String,
    pub forward_operations: Vec<Operation>,
    pub inverse_operations: Vec<Operation>,
    pub anchor_mapping: AnchorMapping,
    pub history_effect: HistoryEffect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum HistoryEffect {
    Create,
    Commit { entry_command_id: CommandId },
    Undo { entry_command_id: CommandId },
    Redo { entry_command_id: CommandId },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryEntry {
    pub command_id: CommandId,
    pub forward_operations: Vec<Operation>,
    pub inverse_operations: Vec<Operation>,
    pub before_semantic_hash: String,
    pub after_semantic_hash: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryState {
    pub entries: Vec<HistoryEntry>,
    pub cursor: u32,
    pub seen_command_ids: Vec<CommandId>,
}

impl HistoryState {
    pub fn validate(&self) -> Result<(), CommandError> {
        let cursor = usize::try_from(self.cursor).map_err(|_| CommandError::HistoryConflict)?;
        if cursor > self.entries.len() {
            return Err(CommandError::HistoryConflict);
        }
        let mut seen = BTreeSet::new();
        for id in &self.seen_command_ids {
            if Uuid::parse_str(id.as_str()).is_err() || !seen.insert(id.as_str()) {
                return Err(CommandError::HistoryConflict);
            }
        }
        DocumentLimits::V1.check_recovery(self.seen_command_ids.len(), 0)?;
        if self.entries.len() > self.seen_command_ids.len() {
            return Err(CommandError::HistoryConflict);
        }
        let mut entry_ids = BTreeSet::new();
        let mut total_bytes = self
            .seen_command_ids
            .len()
            .checked_mul(36)
            .ok_or(CommandError::HistoryConflict)?;
        let mut previous_after: Option<&str> = None;
        for entry in &self.entries {
            if !seen.contains(entry.command_id.as_str())
                || !entry_ids.insert(entry.command_id.as_str())
                || entry.forward_operations.is_empty()
                || entry.inverse_operations.is_empty()
                || !is_versioned_hash(&entry.before_semantic_hash)
                || !is_versioned_hash(&entry.after_semantic_hash)
                || previous_after.is_some_and(|hash| hash != entry.before_semantic_hash)
            {
                return Err(CommandError::HistoryConflict);
            }
            previous_after = Some(&entry.after_semantic_hash);
            let entry_bytes = serde_json::to_vec(entry)
                .map_err(|_| SchemaError::serialization())?
                .len();
            DocumentLimits::V1.check_transaction(
                entry.forward_operations.len() + entry.inverse_operations.len(),
                entry_bytes,
            )?;
            total_bytes = total_bytes
                .checked_add(entry_bytes)
                .ok_or(CommandError::HistoryConflict)?;
        }
        DocumentLimits::V1.check_recovery(self.seen_command_ids.len(), total_bytes)?;
        Ok(())
    }

    fn has_seen(&self, command_id: &CommandId) -> bool {
        self.seen_command_ids.contains(command_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorState {
    document: FlowDocument,
    history: HistoryState,
    canonical_hash: String,
}

impl EditorState {
    pub fn new(document: FlowDocument) -> Result<Self, CommandError> {
        Self::with_history(document, HistoryState::default())
    }

    pub fn with_history(
        document: FlowDocument,
        history: HistoryState,
    ) -> Result<Self, CommandError> {
        validate_document(&document)?;
        history.validate()?;
        let bytes = canonical_bytes(&document)?;
        validate_history_against_document(&document, &history)?;
        Ok(Self {
            canonical_hash: canonical_hash(&bytes),
            document,
            history,
        })
    }

    #[must_use]
    pub const fn document(&self) -> &FlowDocument {
        &self.document
    }

    #[must_use]
    pub const fn history(&self) -> &HistoryState {
        &self.history
    }

    #[must_use]
    pub fn canonical_hash(&self) -> &str {
        &self.canonical_hash
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedCommand {
    pub state: EditorState,
    pub transaction: Transaction,
    pub selection: Option<DirectionalSelection>,
    pub asset_records: Vec<crate::store::AssetRecord>,
}

pub struct TransactionService;

impl TransactionService {
    pub fn apply(state: &EditorState, command: Command) -> Result<AppliedCommand, CommandError> {
        validate_command_envelope(state, &command)?;
        match &command.kind {
            CommandKind::Undo => apply_undo(state, command),
            CommandKind::Redo => apply_redo(state, command),
            _ => apply_mutation(state, command),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error("The command identifier is invalid")]
    InvalidCommandId,
    #[error("The command identifier was already applied")]
    DuplicateCommand,
    #[error("The command uses a stale base revision")]
    StaleRevision,
    #[error("The command target does not exist")]
    InvalidTarget,
    #[error("The command range is invalid")]
    InvalidRange,
    #[error("The UTF-16 position splits a surrogate pair")]
    InvalidUtf16Boundary,
    #[error(transparent)]
    Position(#[from] EditorPositionError),
    #[error("The command would invalidate an anchor without an explicit mapping")]
    AnchorInvalidated,
    #[error("The command would violate a document or operation invariant")]
    BrokenInvariant,
    #[error("The command would not change the document")]
    NoOp,
    #[error("There is no committed mutation to undo")]
    UndoEmpty,
    #[error("There is no reverted mutation to redo")]
    RedoEmpty,
    #[error("The history cursor or stored operation set is inconsistent")]
    HistoryConflict,
    #[error("The structural text targets are not adjacent or compatible")]
    IncompatibleStructure,
    #[error("The structural container is not valid for this operation")]
    InvalidContainer,
    #[error("The transaction-private anchor preimage is invalid")]
    InvalidPreimage,
    #[error("The operation would exceed the bounded anchor-preimage budget")]
    PreimageLimit,
    #[error("The formatting value is outside the closed authoring vocabulary")]
    InvalidFormatting,
    #[error("The list structure is not valid for this operation")]
    InvalidListStructure,
    #[error("The list nesting depth would exceed the authoring limit")]
    ListDepthExceeded,
    #[error("The table dimensions are outside the bounded authoring limits")]
    TableBoundsExceeded,
    #[error("The structural container is not a table cell or table")]
    InvalidTableStructure,
    #[error("The destructive structural action requires explicit confirmation")]
    ConfirmationRequired,
    #[error(transparent)]
    Asset(#[from] crate::asset::AssetError),
}

impl CommandError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Schema(error) => error.code(),
            Self::InvalidCommandId => "FLOW_INVALID_COMMAND_ID",
            Self::DuplicateCommand => "FLOW_DUPLICATE_COMMAND",
            Self::StaleRevision => "FLOW_STALE_REVISION",
            Self::InvalidTarget => "FLOW_INVALID_TARGET",
            Self::InvalidRange => "FLOW_INVALID_RANGE",
            Self::InvalidUtf16Boundary => "FLOW_INVALID_UTF16_BOUNDARY",
            Self::Position(error) => error.code(),
            Self::AnchorInvalidated => "FLOW_ANCHOR_INVALIDATED",
            Self::BrokenInvariant => "FLOW_BROKEN_INVARIANT",
            Self::NoOp => "FLOW_NO_OP",
            Self::UndoEmpty => "FLOW_UNDO_EMPTY",
            Self::RedoEmpty => "FLOW_REDO_EMPTY",
            Self::HistoryConflict => "FLOW_HISTORY_CONFLICT",
            Self::IncompatibleStructure => "FLOW_INCOMPATIBLE_STRUCTURE",
            Self::InvalidContainer => "FLOW_INVALID_CONTAINER",
            Self::InvalidPreimage => "FLOW_INVALID_PREIMAGE",
            Self::PreimageLimit => "FLOW_LIMIT_ANCHOR_PREIMAGE",
            Self::InvalidFormatting => "FLOW_INVALID_FORMATTING",
            Self::InvalidListStructure => "FLOW_INVALID_LIST_STRUCTURE",
            Self::ListDepthExceeded => "FLOW_LIMIT_LIST_DEPTH",
            Self::TableBoundsExceeded => "FLOW_LIMIT_TABLE",
            Self::InvalidTableStructure => "FLOW_INVALID_TABLE_STRUCTURE",
            Self::ConfirmationRequired => "FLOW_CONFIRMATION_REQUIRED",
            Self::Asset(error) => error.code(),
        }
    }
}

impl From<AnchorError> for CommandError {
    fn from(error: AnchorError) -> Self {
        match error {
            AnchorError::OutOfRange => Self::InvalidRange,
            AnchorError::InvalidUtf16Boundary => Self::InvalidUtf16Boundary,
        }
    }
}

pub fn semantic_hash(document: &FlowDocument) -> Result<String, SchemaError> {
    let mut semantic = document.clone();
    semantic.revision = 1;
    canonical_bytes(&semantic).map(|bytes| canonical_hash(&bytes))
}

fn validate_command_envelope(state: &EditorState, command: &Command) -> Result<(), CommandError> {
    state.history.validate()?;
    if Uuid::parse_str(command.command_id.as_str()).is_err() {
        return Err(CommandError::InvalidCommandId);
    }
    if state.history.has_seen(&command.command_id) {
        return Err(CommandError::DuplicateCommand);
    }
    if command.base_revision != state.document.revision {
        return Err(CommandError::StaleRevision);
    }
    if command.issued_at.is_empty() || command.issued_at.len() > 64 {
        return Err(CommandError::BrokenInvariant);
    }
    let operation_count = match &command.kind {
        CommandKind::Batch { mutations } => mutations.len().saturating_mul(2),
        CommandKind::Undo | CommandKind::Redo => 0,
        _ => 2,
    };
    let command_bytes = serde_json::to_vec(command)
        .map_err(|_| SchemaError::serialization())?
        .len();
    DocumentLimits::V1.check_transaction(operation_count, command_bytes)?;
    Ok(())
}

fn is_versioned_hash(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("flowpdf:blake3:v1:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_history_against_document(
    document: &FlowDocument,
    history: &HistoryState,
) -> Result<(), CommandError> {
    if history.entries.is_empty() {
        return Ok(());
    }
    let cursor = usize::try_from(history.cursor).map_err(|_| CommandError::HistoryConflict)?;
    let expected_current = if cursor == 0 {
        &history.entries[0].before_semantic_hash
    } else {
        &history.entries[cursor - 1].after_semantic_hash
    };
    if semantic_hash(document).map_err(|_| CommandError::HistoryConflict)? != *expected_current {
        return Err(CommandError::HistoryConflict);
    }

    let current = document.clone();
    let mut base = current.clone();
    for entry in history.entries[..cursor].iter().rev() {
        if semantic_hash(&base).map_err(|_| CommandError::HistoryConflict)?
            != entry.after_semantic_hash
        {
            return Err(CommandError::HistoryConflict);
        }
        apply_operations(&mut base, &entry.inverse_operations)
            .map_err(|_| CommandError::HistoryConflict)?;
        validate_document(&base).map_err(|_| CommandError::HistoryConflict)?;
        if semantic_hash(&base).map_err(|_| CommandError::HistoryConflict)?
            != entry.before_semantic_hash
        {
            return Err(CommandError::HistoryConflict);
        }
    }

    let original = base.clone();
    if cursor == 0 && original != current {
        return Err(CommandError::HistoryConflict);
    }
    let mut forward = original.clone();
    for (index, entry) in history.entries.iter().enumerate() {
        if semantic_hash(&forward).map_err(|_| CommandError::HistoryConflict)?
            != entry.before_semantic_hash
        {
            return Err(CommandError::HistoryConflict);
        }
        apply_operations(&mut forward, &entry.forward_operations)
            .map_err(|_| CommandError::HistoryConflict)?;
        validate_document(&forward).map_err(|_| CommandError::HistoryConflict)?;
        if semantic_hash(&forward).map_err(|_| CommandError::HistoryConflict)?
            != entry.after_semantic_hash
        {
            return Err(CommandError::HistoryConflict);
        }
        if index + 1 == cursor && forward != current {
            return Err(CommandError::HistoryConflict);
        }
    }

    for entry in history.entries.iter().rev() {
        apply_operations(&mut forward, &entry.inverse_operations)
            .map_err(|_| CommandError::HistoryConflict)?;
    }
    if forward != original {
        return Err(CommandError::HistoryConflict);
    }
    Ok(())
}

fn apply_mutation(state: &EditorState, command: Command) -> Result<AppliedCommand, CommandError> {
    let mutations = command_mutations(&command.kind)?;
    if mutations.is_empty() {
        return Err(CommandError::BrokenInvariant);
    }

    let mut candidate = state.document.clone();
    let mut forward = Vec::with_capacity(mutations.len());
    let mut inverse = Vec::with_capacity(mutations.len());
    let mut mapping = AnchorMapping::identity();
    let mut selection_after = None;
    let mut asset_records = Vec::new();
    for mutation in &mutations {
        let table_focus = table_focus_hint(&candidate, mutation)?;
        let removal_focus = removal_focus_hint(&candidate, mutation);
        let merge_offset = if let Mutation::MergeTextBlocks { first_node_id, .. } = mutation {
            let first =
                find_node(&candidate.content, first_node_id).ok_or(CommandError::InvalidTarget)?;
            Some(utf16_length(&first.text())?)
        } else {
            None
        };
        let redeemed_asset = match mutation {
            Mutation::InsertImage {
                session_id,
                receipt,
                ..
            }
            | Mutation::ReplaceImage {
                session_id,
                receipt,
                ..
            } => Some(asset::redeem_asset(
                receipt,
                session_id,
                &candidate.document_id,
                candidate.revision,
            )?),
            _ => None,
        };
        let (operation, inverse_operation) = derive_operation(
            &candidate,
            mutation,
            &command.command_id,
            redeemed_asset.as_ref(),
        )?;
        let replacement_selection = if let Mutation::ReplaceSelection { selection, text } = mutation
        {
            Some(selection_after_replacement(&candidate, selection, text)?)
        } else {
            None
        };
        let list_selection = match mutation {
            Mutation::ContinueListItem { selection } => Some(collapsed_selection(
                deterministic_node_id(
                    &command.command_id,
                    if find_node(&candidate.content, &selection.anchor.node_id)
                        .is_some_and(|node| node.text().is_empty())
                    {
                        "list-exit-paragraph"
                    } else {
                        "list-continuation-text"
                    },
                )?,
                0,
            )),
            Mutation::ExitListItem { .. } => Some(collapsed_selection(
                deterministic_node_id(&command.command_id, "list-exit-paragraph")?,
                0,
            )),
            _ => None,
        };
        let operation_mapping = apply_operation(&mut candidate, &operation)?;
        selection_after = match mutation {
            Mutation::ReplaceSelection { .. } => replacement_selection,
            Mutation::SplitTextBlock { new_node_id, .. } => {
                Some(collapsed_selection(new_node_id.clone(), 0))
            }
            Mutation::MergeTextBlocks { first_node_id, .. } => Some(collapsed_selection(
                first_node_id.clone(),
                merge_offset.ok_or(CommandError::BrokenInvariant)?,
            )),
            Mutation::DeleteSubtree { .. } => first_text_selection(&candidate),
            Mutation::SetInlineMarks { selection, .. }
            | Mutation::SetInlineMark { selection, .. }
            | Mutation::SetBlockAttributes { selection, .. }
            | Mutation::SetBlockStyle { selection, .. }
            | Mutation::SetListKind { selection, .. } => Some(selection.clone()),
            Mutation::ContinueListItem { .. } | Mutation::ExitListItem { .. } => list_selection,
            Mutation::IndentListItem { item_id } | Mutation::OutdentListItem { item_id } => {
                first_text_selection_for_node(&candidate.content, item_id)
            }
            Mutation::InsertPageBreak { .. } => {
                let paragraph_id =
                    deterministic_node_id(&command.command_id, "page-break-paragraph")?;
                first_text_selection_for_node(&candidate.content, &paragraph_id)
            }
            Mutation::InsertImage { .. } => removal_focus
                .as_ref()
                .and_then(|hint| selection_near_container(&candidate, hint))
                .or_else(|| first_text_selection(&candidate)),
            Mutation::ReplaceImage { .. } => removal_focus
                .as_ref()
                .and_then(|hint| selection_near_container(&candidate, hint))
                .or_else(|| first_text_selection(&candidate)),
            Mutation::RemovePageBreak { .. }
            | Mutation::RemoveTable { .. }
            | Mutation::RemoveImage { .. } => removal_focus
                .as_ref()
                .and_then(|hint| selection_near_container(&candidate, hint))
                .or_else(|| first_text_selection(&candidate)),
            Mutation::InsertTable { .. } => {
                let table_id = deterministic_node_id(&command.command_id, "table")?;
                first_text_selection_for_node(&candidate.content, &table_id)
            }
            Mutation::AddTableRow { .. } => table_focus.as_ref().and_then(|hint| {
                table_selection_at(
                    &candidate,
                    &hint.table_id,
                    hint.row_index.saturating_add(1),
                    hint.column_index,
                )
            }),
            Mutation::RemoveTableRow { .. } => table_focus.as_ref().and_then(|hint| {
                table_selection_at(
                    &candidate,
                    &hint.table_id,
                    hint.row_index
                        .min(table_row_count(&candidate, &hint.table_id).saturating_sub(1)),
                    hint.column_index,
                )
            }),
            Mutation::AddTableColumn { .. } => table_focus.as_ref().and_then(|hint| {
                table_selection_at(
                    &candidate,
                    &hint.table_id,
                    hint.row_index,
                    hint.column_index.saturating_add(1),
                )
            }),
            Mutation::RemoveTableColumn { .. } => table_focus.as_ref().and_then(|hint| {
                table_selection_at(
                    &candidate,
                    &hint.table_id,
                    hint.row_index,
                    hint.column_index
                        .min(table_column_count(&candidate, &hint.table_id).saturating_sub(1)),
                )
            }),
            Mutation::SetTableHeaderRow { table_id, .. } => {
                first_text_selection_for_node(&candidate.content, table_id)
                    .or_else(|| selection_after.clone())
            }
            _ => selection_after,
        };
        if let Some(asset) = redeemed_asset {
            asset_records.push(asset.into_record());
        }
        mapping.extend(operation_mapping);
        forward.push(operation);
        inverse.insert(0, inverse_operation);
    }

    let before_semantic_hash = semantic_hash(&state.document)?;
    let (candidate, before_hash, after_hash) = finalize_document(state, candidate)?;
    let after_semantic_hash = semantic_hash(&candidate)?;

    let mut history = state.history.clone();
    let cursor = usize::try_from(history.cursor).map_err(|_| CommandError::HistoryConflict)?;
    history.entries.truncate(cursor);
    history.entries.push(HistoryEntry {
        command_id: command.command_id.clone(),
        forward_operations: forward.clone(),
        inverse_operations: inverse.clone(),
        before_semantic_hash,
        after_semantic_hash,
    });
    history.cursor =
        u32::try_from(history.entries.len()).map_err(|_| CommandError::HistoryConflict)?;
    history.seen_command_ids.push(command.command_id.clone());
    history.validate()?;

    let transaction = build_transaction(
        &candidate,
        &command,
        before_hash,
        after_hash.clone(),
        forward,
        inverse,
        mapping,
        HistoryEffect::Commit {
            entry_command_id: command.command_id.clone(),
        },
    )?;
    Ok(AppliedCommand {
        state: EditorState {
            document: candidate,
            history,
            canonical_hash: after_hash,
        },
        transaction,
        selection: selection_after,
        asset_records,
    })
}

fn apply_undo(state: &EditorState, command: Command) -> Result<AppliedCommand, CommandError> {
    let cursor =
        usize::try_from(state.history.cursor).map_err(|_| CommandError::HistoryConflict)?;
    let entry = cursor
        .checked_sub(1)
        .and_then(|index| state.history.entries.get(index))
        .cloned()
        .ok_or(CommandError::UndoEmpty)?;
    let mut candidate = state.document.clone();
    let mapping = apply_operations(&mut candidate, &entry.inverse_operations)?;
    let current_semantic_hash = semantic_hash(&candidate)?;
    if current_semantic_hash != entry.before_semantic_hash {
        return Err(CommandError::HistoryConflict);
    }
    let (candidate, before_hash, after_hash) = finalize_document(state, candidate)?;

    let mut history = state.history.clone();
    history.cursor = history
        .cursor
        .checked_sub(1)
        .ok_or(CommandError::UndoEmpty)?;
    history.seen_command_ids.push(command.command_id.clone());
    history.validate()?;
    let transaction = build_transaction(
        &candidate,
        &command,
        before_hash,
        after_hash.clone(),
        entry.inverse_operations.clone(),
        entry.forward_operations.clone(),
        mapping,
        HistoryEffect::Undo {
            entry_command_id: entry.command_id,
        },
    )?;
    Ok(AppliedCommand {
        state: EditorState {
            document: candidate,
            history,
            canonical_hash: after_hash,
        },
        transaction,
        selection: None,
        asset_records: Vec::new(),
    })
}

fn apply_redo(state: &EditorState, command: Command) -> Result<AppliedCommand, CommandError> {
    let cursor =
        usize::try_from(state.history.cursor).map_err(|_| CommandError::HistoryConflict)?;
    let entry = state
        .history
        .entries
        .get(cursor)
        .cloned()
        .ok_or(CommandError::RedoEmpty)?;
    let mut candidate = state.document.clone();
    let mapping = apply_operations(&mut candidate, &entry.forward_operations)?;
    let current_semantic_hash = semantic_hash(&candidate)?;
    if current_semantic_hash != entry.after_semantic_hash {
        return Err(CommandError::HistoryConflict);
    }
    let (candidate, before_hash, after_hash) = finalize_document(state, candidate)?;

    let mut history = state.history.clone();
    history.cursor = history
        .cursor
        .checked_add(1)
        .ok_or(CommandError::HistoryConflict)?;
    history.seen_command_ids.push(command.command_id.clone());
    history.validate()?;
    let transaction = build_transaction(
        &candidate,
        &command,
        before_hash,
        after_hash.clone(),
        entry.forward_operations.clone(),
        entry.inverse_operations.clone(),
        mapping,
        HistoryEffect::Redo {
            entry_command_id: entry.command_id,
        },
    )?;
    Ok(AppliedCommand {
        state: EditorState {
            document: candidate,
            history,
            canonical_hash: after_hash,
        },
        transaction,
        selection: None,
        asset_records: Vec::new(),
    })
}

fn command_mutations(kind: &CommandKind) -> Result<Vec<Mutation>, CommandError> {
    match kind {
        CommandKind::Batch { mutations } => Ok(mutations.clone()),
        CommandKind::InsertText { target, text } => Ok(vec![Mutation::InsertText {
            target: target.clone(),
            text: text.clone(),
        }]),
        CommandKind::ReplaceText { range, text } => Ok(vec![Mutation::ReplaceText {
            range: range.clone(),
            text: text.clone(),
        }]),
        CommandKind::ReplaceSelection { selection, text } => Ok(vec![Mutation::ReplaceSelection {
            selection: selection.clone(),
            text: text.clone(),
        }]),
        CommandKind::DeleteText { range } => Ok(vec![Mutation::DeleteText {
            range: range.clone(),
        }]),
        CommandKind::SetNodeStyle { node_id, style_id } => Ok(vec![Mutation::SetNodeStyle {
            node_id: node_id.clone(),
            style_id: style_id.clone(),
        }]),
        CommandKind::InsertNode { index, node } => Ok(vec![Mutation::InsertNode {
            index: *index,
            node: node.clone(),
        }]),
        CommandKind::DeleteNode { node_id } => Ok(vec![Mutation::DeleteNode {
            node_id: node_id.clone(),
        }]),
        CommandKind::SplitTextBlock {
            node_id,
            utf16_offset,
            new_node_id,
        } => Ok(vec![Mutation::SplitTextBlock {
            node_id: node_id.clone(),
            utf16_offset: *utf16_offset,
            new_node_id: new_node_id.clone(),
        }]),
        CommandKind::MergeTextBlocks {
            first_node_id,
            second_node_id,
        } => Ok(vec![Mutation::MergeTextBlocks {
            first_node_id: first_node_id.clone(),
            second_node_id: second_node_id.clone(),
        }]),
        CommandKind::DeleteSubtree { node_id } => Ok(vec![Mutation::DeleteSubtree {
            node_id: node_id.clone(),
        }]),
        CommandKind::SetInlineMarks { selection, marks } => Ok(vec![Mutation::SetInlineMarks {
            selection: selection.clone(),
            marks: marks.clone(),
        }]),
        CommandKind::SetInlineMark { selection, mark } => Ok(vec![Mutation::SetInlineMark {
            selection: selection.clone(),
            mark: mark.clone(),
        }]),
        CommandKind::SetBlockAttributes {
            selection,
            attributes,
        } => Ok(vec![Mutation::SetBlockAttributes {
            selection: selection.clone(),
            attributes: attributes.clone(),
        }]),
        CommandKind::SetBlockStyle { selection, style } => Ok(vec![Mutation::SetBlockStyle {
            selection: selection.clone(),
            style: style.clone(),
        }]),
        CommandKind::SetListKind { selection, kind } => Ok(vec![Mutation::SetListKind {
            selection: selection.clone(),
            kind: *kind,
        }]),
        CommandKind::ContinueListItem { selection } => Ok(vec![Mutation::ContinueListItem {
            selection: selection.clone(),
        }]),
        CommandKind::ExitListItem { selection } => Ok(vec![Mutation::ExitListItem {
            selection: selection.clone(),
        }]),
        CommandKind::IndentListItem { item_id } => Ok(vec![Mutation::IndentListItem {
            item_id: item_id.clone(),
        }]),
        CommandKind::OutdentListItem { item_id } => Ok(vec![Mutation::OutdentListItem {
            item_id: item_id.clone(),
        }]),
        CommandKind::InsertPageBreak { placement } => Ok(vec![Mutation::InsertPageBreak {
            placement: placement.clone(),
        }]),
        CommandKind::RemovePageBreak { page_break_id } => Ok(vec![Mutation::RemovePageBreak {
            page_break_id: page_break_id.clone(),
        }]),
        CommandKind::InsertTable {
            placement,
            rows,
            columns,
            header_row,
        } => Ok(vec![Mutation::InsertTable {
            placement: placement.clone(),
            rows: *rows,
            columns: *columns,
            header_row: *header_row,
        }]),
        CommandKind::InsertImage {
            placement,
            session_id,
            receipt,
            accessibility,
        } => Ok(vec![Mutation::InsertImage {
            placement: placement.clone(),
            session_id: session_id.clone(),
            receipt: receipt.clone(),
            accessibility: accessibility.clone(),
        }]),
        CommandKind::ReplaceImage {
            image_node_id,
            session_id,
            receipt,
            accessibility,
        } => Ok(vec![Mutation::ReplaceImage {
            image_node_id: image_node_id.clone(),
            session_id: session_id.clone(),
            receipt: receipt.clone(),
            accessibility: accessibility.clone(),
        }]),
        CommandKind::SetImageAccessibility {
            image_node_id,
            accessibility,
        } => Ok(vec![Mutation::SetImageAccessibility {
            image_node_id: image_node_id.clone(),
            accessibility: accessibility.clone(),
        }]),
        CommandKind::RemoveImage {
            image_node_id,
            confirmed,
        } => Ok(vec![Mutation::RemoveImage {
            image_node_id: image_node_id.clone(),
            confirmed: *confirmed,
        }]),
        CommandKind::AddTableRow { selection } => Ok(vec![Mutation::AddTableRow {
            selection: selection.clone(),
        }]),
        CommandKind::RemoveTableRow { selection } => Ok(vec![Mutation::RemoveTableRow {
            selection: selection.clone(),
        }]),
        CommandKind::AddTableColumn { selection } => Ok(vec![Mutation::AddTableColumn {
            selection: selection.clone(),
        }]),
        CommandKind::RemoveTableColumn { selection } => Ok(vec![Mutation::RemoveTableColumn {
            selection: selection.clone(),
        }]),
        CommandKind::SetTableHeaderRow { table_id, enabled } => {
            Ok(vec![Mutation::SetTableHeaderRow {
                table_id: table_id.clone(),
                enabled: *enabled,
            }])
        }
        CommandKind::RemoveTable {
            table_id,
            confirmed,
        } => Ok(vec![Mutation::RemoveTable {
            table_id: table_id.clone(),
            confirmed: *confirmed,
        }]),
        CommandKind::SetField { field_id, field } => Ok(vec![Mutation::SetField {
            field_id: field_id.clone(),
            field: field.clone(),
        }]),
        CommandKind::InsertField { field } => Ok(vec![Mutation::InsertField {
            field: field.clone(),
        }]),
        CommandKind::RemoveField {
            field_id,
            confirmed,
        } => Ok(vec![Mutation::RemoveField {
            field_id: field_id.clone(),
            confirmed: *confirmed,
        }]),
        CommandKind::Undo | CommandKind::Redo => Err(CommandError::BrokenInvariant),
    }
}

fn contains_editable_text(node: &ContentNode) -> bool {
    // derive_operation receives an already validated, depth-bounded document.
    node.runs().is_some() || node.children().iter().any(contains_editable_text)
}

fn derive_operation(
    document: &FlowDocument,
    mutation: &Mutation,
    command_id: &CommandId,
    redeemed_asset: Option<&RedeemedAsset>,
) -> Result<(Operation, Operation), CommandError> {
    match mutation {
        Mutation::InsertText { target, text } => {
            if text.is_empty() {
                return Err(CommandError::BrokenInvariant);
            }
            derive_text_operation(document, TextRange::collapsed(target.clone()), text.clone())
        }
        Mutation::ReplaceText { range, text } => {
            derive_text_operation(document, range.clone(), text.clone())
        }
        Mutation::ReplaceSelection { selection, text } => {
            derive_selection_operation(document, selection, text.clone(), command_id)
        }
        Mutation::DeleteText { range } => {
            derive_text_operation(document, range.clone(), String::new())
        }
        Mutation::SetNodeStyle { node_id, style_id } => {
            let node = document
                .content
                .iter()
                .find(|node| node.id == *node_id)
                .ok_or(CommandError::InvalidTarget)?;
            if node.style_id == *style_id {
                return Err(CommandError::BrokenInvariant);
            }
            Ok((
                Operation::SetNodeStyle {
                    node_id: node_id.clone(),
                    expected_style_id: node.style_id.clone(),
                    style_id: style_id.clone(),
                },
                Operation::SetNodeStyle {
                    node_id: node_id.clone(),
                    expected_style_id: style_id.clone(),
                    style_id: node.style_id.clone(),
                },
            ))
        }
        Mutation::InsertNode { index, node } => {
            crate::schema::validate_new_node(document, node)?;
            let index_usize = usize::try_from(*index).map_err(|_| CommandError::InvalidRange)?;
            if index_usize > document.content.len()
                || document.content.iter().any(|current| current.id == node.id)
            {
                return Err(CommandError::InvalidRange);
            }
            Ok((
                Operation::InsertNode {
                    index: *index,
                    node: node.clone(),
                },
                Operation::DeleteNode {
                    index: *index,
                    node: node.clone(),
                },
            ))
        }
        Mutation::DeleteNode { node_id } => {
            let (index, node) = document
                .content
                .iter()
                .enumerate()
                .find(|(_, node)| node.id == *node_id)
                .ok_or(CommandError::InvalidTarget)?;
            if contains_editable_text(node)
                && !document
                    .content
                    .iter()
                    .filter(|other| other.id != *node_id)
                    .any(contains_editable_text)
            {
                // The semantic delete command will create its replacement
                // paragraph in a later plan. The legacy command must not erase
                // the last editable block in the meantime.
                return Err(CommandError::BrokenInvariant);
            }
            let index = u32::try_from(index).map_err(|_| CommandError::InvalidRange)?;
            Ok((
                Operation::DeleteNode {
                    index,
                    node: node.clone(),
                },
                Operation::InsertNode {
                    index,
                    node: node.clone(),
                },
            ))
        }
        Mutation::SplitTextBlock {
            node_id,
            utf16_offset,
            new_node_id,
        } => derive_split_operation(document, node_id, *utf16_offset, new_node_id, command_id),
        Mutation::MergeTextBlocks {
            first_node_id,
            second_node_id,
        } => derive_merge_operation(document, first_node_id, second_node_id),
        Mutation::DeleteSubtree { node_id } => {
            derive_delete_subtree_operation(document, node_id, command_id)
        }
        Mutation::SetInlineMarks { selection, marks } => {
            derive_inline_marks_operation(document, selection, marks)
        }
        Mutation::SetInlineMark { selection, mark } => {
            derive_inline_mark_operation(document, selection, mark)
        }
        Mutation::SetBlockAttributes {
            selection,
            attributes,
        } => derive_block_attributes_operation(document, selection, attributes),
        Mutation::SetBlockStyle { selection, style } => {
            derive_block_style_operation(document, selection, style)
        }
        Mutation::SetListKind { selection, kind } => {
            derive_list_kind_operation(document, selection, *kind, command_id)
        }
        Mutation::ContinueListItem { selection } => {
            derive_continue_list_item_operation(document, selection, command_id)
        }
        Mutation::ExitListItem { selection } => {
            derive_exit_list_item_operation(document, selection, command_id)
        }
        Mutation::IndentListItem { item_id } => {
            derive_indent_list_item_operation(document, item_id, command_id)
        }
        Mutation::OutdentListItem { item_id } => {
            derive_outdent_list_item_operation(document, item_id)
        }
        Mutation::InsertPageBreak { placement } => {
            derive_insert_page_break_operation(document, placement, command_id)
        }
        Mutation::RemovePageBreak { page_break_id } => {
            derive_remove_page_break_operation(document, page_break_id, command_id)
        }
        Mutation::InsertTable {
            placement,
            rows,
            columns,
            header_row,
        } => derive_insert_table_operation(
            document,
            placement,
            *rows,
            *columns,
            *header_row,
            command_id,
        ),
        Mutation::InsertImage {
            placement,
            accessibility,
            ..
        } => {
            let asset = redeemed_asset.ok_or(CommandError::BrokenInvariant)?;
            derive_insert_image_operation(document, placement, accessibility, asset, command_id)
        }
        Mutation::ReplaceImage {
            image_node_id,
            accessibility,
            ..
        } => {
            let asset = redeemed_asset.ok_or(CommandError::BrokenInvariant)?;
            derive_replace_image_operation(document, image_node_id, accessibility, asset)
        }
        Mutation::SetImageAccessibility {
            image_node_id,
            accessibility,
        } => derive_set_image_accessibility_operation(document, image_node_id, accessibility),
        Mutation::RemoveImage {
            image_node_id,
            confirmed,
        } => derive_remove_image_operation(document, image_node_id, *confirmed, command_id),
        Mutation::AddTableRow { selection } => {
            derive_add_table_row_operation(document, selection, command_id)
        }
        Mutation::RemoveTableRow { selection } => {
            derive_remove_table_row_operation(document, selection, command_id)
        }
        Mutation::AddTableColumn { selection } => {
            derive_add_table_column_operation(document, selection, command_id)
        }
        Mutation::RemoveTableColumn { selection } => {
            derive_remove_table_column_operation(document, selection, command_id)
        }
        Mutation::SetTableHeaderRow { table_id, enabled } => {
            derive_set_table_header_operation(document, table_id, *enabled)
        }
        Mutation::RemoveTable {
            table_id,
            confirmed,
        } => derive_remove_table_operation(document, table_id, *confirmed, command_id),
        Mutation::InsertField { field } => {
            if !matches!(field.anchor, FieldAnchorState::GraphemeSafe { .. })
                || document.fields.iter().any(|current| current.id == field.id)
            {
                return Err(CommandError::BrokenInvariant);
            }
            let index =
                u32::try_from(document.fields.len()).map_err(|_| CommandError::InvalidRange)?;
            Ok((
                Operation::InsertField {
                    index,
                    field: Box::new(field.clone()),
                },
                Operation::RemoveField {
                    index,
                    field: Box::new(field.clone()),
                },
            ))
        }
        Mutation::SetField { field_id, field } => {
            if !matches!(field.anchor, FieldAnchorState::GraphemeSafe { .. }) {
                return Err(CommandError::InvalidTarget);
            }
            let (index, current) = document
                .fields
                .iter()
                .enumerate()
                .find(|(_, current)| current.id == *field_id)
                .ok_or(CommandError::InvalidTarget)?;
            if current == field || field.id != *field_id {
                return Err(CommandError::BrokenInvariant);
            }
            let index = u32::try_from(index).map_err(|_| CommandError::InvalidRange)?;
            Ok((
                Operation::SetField {
                    index,
                    expected_field: Box::new(current.clone()),
                    field: Box::new(field.clone()),
                },
                Operation::SetField {
                    index,
                    expected_field: Box::new(field.clone()),
                    field: Box::new(current.clone()),
                },
            ))
        }
        Mutation::RemoveField {
            field_id,
            confirmed,
        } => {
            if !confirmed {
                return Err(CommandError::ConfirmationRequired);
            }
            let (index, field) = document
                .fields
                .iter()
                .enumerate()
                .find(|(_, current)| current.id == *field_id)
                .ok_or(CommandError::InvalidTarget)?;
            let index = u32::try_from(index).map_err(|_| CommandError::InvalidRange)?;
            Ok((
                Operation::RemoveField {
                    index,
                    field: Box::new(field.clone()),
                },
                Operation::InsertField {
                    index,
                    field: Box::new(field.clone()),
                },
            ))
        }
    }
}

fn derive_text_operation(
    document: &FlowDocument,
    range: TextRange,
    replacement: String,
) -> Result<(Operation, Operation), CommandError> {
    let (node, start, end) = resolve_range(document, &range)?;
    let expected_text =
        node.legacy_text().ok_or(CommandError::InvalidTarget)?[start.get()..end.get()].to_owned();
    if expected_text == replacement || (expected_text.is_empty() && replacement.is_empty()) {
        return Err(CommandError::BrokenInvariant);
    }
    let inserted = utf16_length(&replacement)?;
    let inverse_end = range.start.utf16_offset.checked_add(inserted)?;
    let inverse_range = TextRange {
        start: LogicalPosition {
            affinity: Affinity::Backward,
            ..range.start.clone()
        },
        end: LogicalPosition {
            node_id: range.start.node_id.clone(),
            utf16_offset: inverse_end,
            affinity: Affinity::Forward,
        },
    };
    Ok((
        Operation::ReplaceText {
            range,
            expected_text: expected_text.clone(),
            replacement: replacement.clone(),
        },
        Operation::ReplaceText {
            range: inverse_range,
            expected_text: replacement,
            replacement: expected_text,
        },
    ))
}

fn derive_selection_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    replacement: String,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    if selection.anchor.node_id != selection.focus.node_id {
        return derive_cross_block_selection_operation(
            document,
            selection,
            replacement,
            command_id,
        );
    }
    let node = find_node(&document.content, &selection.anchor.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    let Some(text) = node.legacy_text() else {
        return derive_rich_selection_operation(document, selection, replacement);
    };
    let map = NodePositionMap::new(node, document.revision)?;
    let start = map.validate(&selection.anchor, document.revision)?;
    let end = map.validate(&selection.focus, document.revision)?;
    let (start_position, end_position, start_byte, end_byte) = match (start, end) {
        (ResolvedPosition::Text(start_byte), ResolvedPosition::Text(end_byte)) => {
            if selection.anchor.utf16_offset <= selection.focus.utf16_offset {
                (
                    selection.anchor.clone(),
                    selection.focus.clone(),
                    start_byte,
                    end_byte,
                )
            } else {
                (
                    selection.focus.clone(),
                    selection.anchor.clone(),
                    end_byte,
                    start_byte,
                )
            }
        }
        _ => return Err(CommandError::InvalidTarget),
    };
    if start_byte.get() > end_byte.get() {
        return Err(CommandError::InvalidRange);
    }
    if replacement.len() > DocumentLimits::V1.text_node_bytes
        || text
            .len()
            .checked_sub(end_byte.get().saturating_sub(start_byte.get()))
            .and_then(|remaining| remaining.checked_add(replacement.len()))
            .is_none_or(|length| length > DocumentLimits::V1.text_node_bytes)
    {
        return Err(EditorPositionError::TextLimit.into());
    }
    let expected_text = text[start_byte.get()..end_byte.get()].to_owned();
    if expected_text == replacement {
        return Err(CommandError::NoOp);
    }
    let range = TextRange {
        start: start_position.clone(),
        end: end_position,
    };
    let inserted = utf16_length(&replacement)?;
    let inverse_end = start_position.utf16_offset.checked_add(inserted)?;
    let inverse_range = TextRange {
        start: LogicalPosition {
            affinity: Affinity::Backward,
            ..start_position
        },
        end: LogicalPosition {
            node_id: selection.anchor.node_id.clone(),
            utf16_offset: inverse_end,
            affinity: Affinity::Forward,
        },
    };
    Ok((
        Operation::ReplaceText {
            range,
            expected_text: expected_text.clone(),
            replacement: replacement.clone(),
        },
        Operation::ReplaceText {
            range: inverse_range,
            expected_text: replacement,
            replacement: expected_text,
        },
    ))
}

fn derive_rich_selection_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    replacement: String,
) -> Result<(Operation, Operation), CommandError> {
    let location = locate_node(&document.content, &selection.anchor.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    let node = &location.node;
    let runs = node_runs(node).ok_or(CommandError::InvalidTarget)?;
    let map = NodePositionMap::new(node, document.revision)?;
    let start = map.validate(&selection.anchor, document.revision)?;
    let end = map.validate(&selection.focus, document.revision)?;
    let (start_position, end_position, start_byte, end_byte) = match (start, end) {
        (ResolvedPosition::Text(start_byte), ResolvedPosition::Text(end_byte)) => {
            if selection.anchor.utf16_offset <= selection.focus.utf16_offset {
                (
                    selection.anchor.clone(),
                    selection.focus.clone(),
                    start_byte,
                    end_byte,
                )
            } else {
                (
                    selection.focus.clone(),
                    selection.anchor.clone(),
                    end_byte,
                    start_byte,
                )
            }
        }
        _ => return Err(CommandError::InvalidTarget),
    };
    if start_byte.get() > end_byte.get() {
        return Err(CommandError::InvalidRange);
    }
    let text = node.text();
    let expected_text = text[start_byte.get()..end_byte.get()].to_owned();
    if expected_text == replacement {
        return Err(CommandError::NoOp);
    }
    let (prefix, _) = split_runs_at(runs, start_position.utf16_offset.get())?;
    let (_, suffix) = split_runs_at(runs, end_position.utf16_offset.get())?;
    let mut result_runs = prefix;
    if !replacement.is_empty() {
        append_run(
            &mut result_runs,
            &replacement,
            &marks_at_runs(runs, start_position.utf16_offset.get()),
        );
    }
    for run in suffix {
        append_run(&mut result_runs, &run.text, &run.marks);
    }
    if result_runs.iter().map(|run| run.text.len()).sum::<usize>()
        > DocumentLimits::V1.text_node_bytes
    {
        return Err(EditorPositionError::TextLimit.into());
    }
    let mut changed = node.clone();
    set_node_runs(&mut changed, result_runs)?;
    let expected = container_children(document, location.parent_id.as_ref())?;
    let mut replacement_children = expected.clone();
    replacement_children[location.index] = changed;
    let removed_utf16_length = end_position
        .utf16_offset
        .get()
        .checked_sub(start_position.utf16_offset.get())
        .ok_or(CommandError::InvalidRange)?;
    let inserted_utf16_length = utf16_length(&replacement)?;
    let forward_mapping = mapping_with(AnchorTransformation::TextEdit {
        node_id: node.id.clone(),
        start: start_position.utf16_offset,
        removed_utf16_length,
        inserted_utf16_length,
    });
    let inverse_mapping = mapping_with(AnchorTransformation::TextEdit {
        node_id: node.id.clone(),
        start: start_position.utf16_offset,
        removed_utf16_length: inserted_utf16_length,
        inserted_utf16_length: removed_utf16_length,
    });
    structural_pair(
        document,
        location.parent_id,
        expected,
        replacement_children,
        forward_mapping,
        inverse_mapping,
        None,
    )
}

#[derive(Debug, Clone, Copy)]
struct SelectionSpan {
    index: usize,
    start: u32,
    end: u32,
}

/// Resolve a directional selection into document-local spans. The public
/// anchor/focus order is retained by the caller; only this command-local
/// representation is normalized. Every endpoint is validated through the
/// grapheme-aware `NodePositionMap`, so formatting cannot split a cluster.
fn selection_spans(
    document: &FlowDocument,
    selection: &DirectionalSelection,
) -> Result<(Option<NodeId>, Vec<SelectionSpan>), CommandError> {
    let anchor = locate_node(&document.content, &selection.anchor.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    let focus = locate_node(&document.content, &selection.focus.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    if anchor.parent_id != focus.parent_id {
        return Err(CommandError::IncompatibleStructure);
    }
    let validate_endpoint =
        |location: &NodeLocation, position: &LogicalPosition| -> Result<u32, CommandError> {
            match NodePositionMap::new(&location.node, document.revision)?
                .validate(position, document.revision)?
            {
                ResolvedPosition::Text(byte) => {
                    let text = location.node.text();
                    u32::try_from(text[..byte.get()].encode_utf16().count())
                        .map_err(|_| CommandError::InvalidRange)
                }
                _ => Err(CommandError::InvalidTarget),
            }
        };
    let anchor_offset = validate_endpoint(&anchor, &selection.anchor)?;
    let focus_offset = validate_endpoint(&focus, &selection.focus)?;

    let (first, first_offset, last, last_offset) = if anchor.index < focus.index
        || (anchor.index == focus.index && anchor_offset <= focus_offset)
    {
        (&anchor, anchor_offset, &focus, focus_offset)
    } else {
        (&focus, focus_offset, &anchor, anchor_offset)
    };
    let expected = container_children(document, first.parent_id.as_ref())?;
    if first.index >= expected.len() || last.index >= expected.len() {
        return Err(CommandError::InvalidRange);
    }
    let mut spans = Vec::new();
    if first.index == last.index {
        if first_offset > last_offset {
            return Err(CommandError::InvalidRange);
        }
        if node_runs(&expected[first.index]).is_none() {
            return Err(CommandError::InvalidTarget);
        }
        spans.push(SelectionSpan {
            index: first.index,
            start: first_offset,
            end: last_offset,
        });
    } else {
        for index in first.index..=last.index {
            let node = expected.get(index).ok_or(CommandError::InvalidRange)?;
            if node_runs(node).is_none() {
                return Err(CommandError::IncompatibleStructure);
            }
            let length = utf16_length(&node.text())?;
            spans.push(SelectionSpan {
                index,
                start: if index == first.index {
                    first_offset
                } else {
                    0
                },
                end: if index == last.index {
                    last_offset
                } else {
                    length
                },
            });
        }
    }
    Ok((first.parent_id.clone(), spans))
}

fn validate_authoring_marks(marks: &MarkSet) -> Result<(), CommandError> {
    if matches!(
        marks.font_family,
        Some(crate::model::FontFamily::LegacyUnknown { .. })
    ) || marks
        .font_size_millipoints
        .is_some_and(|size| !(6_000..=288_000).contains(&size))
    {
        return Err(CommandError::InvalidFormatting);
    }
    Ok(())
}

fn parse_authoring_color(value: &str) -> Result<[u8; 3], CommandError> {
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return Err(CommandError::InvalidFormatting);
    }
    let mut color = [0_u8; 3];
    for (index, slot) in color.iter_mut().enumerate() {
        let high = bytes[1 + index * 2];
        let low = bytes[2 + index * 2];
        if !(high.is_ascii_digit() || (b'A'..=b'F').contains(&high))
            || !(low.is_ascii_digit() || (b'A'..=b'F').contains(&low))
        {
            return Err(CommandError::InvalidFormatting);
        }
        let decode = |byte: u8| -> u8 {
            if byte.is_ascii_digit() {
                byte - b'0'
            } else {
                byte - b'A' + 10
            }
        };
        *slot = decode(high) * 16 + decode(low);
    }
    Ok(color)
}

fn update_inline_mark(marks: &mut MarkSet, mark: &InlineMark) -> Result<(), CommandError> {
    match mark {
        InlineMark::Bold { value } => marks.bold = *value,
        InlineMark::Italic { value } => marks.italic = *value,
        InlineMark::Underline { value } => marks.underline = *value,
        InlineMark::FontFamily { value } => {
            if matches!(value, Some(crate::model::FontFamily::LegacyUnknown { .. })) {
                return Err(CommandError::InvalidFormatting);
            }
            marks.font_family = value.clone();
        }
        InlineMark::FontSize { value } => {
            if value.is_some_and(|size| !(6_000..=288_000).contains(&size)) {
                return Err(CommandError::InvalidFormatting);
            }
            marks.font_size_millipoints = *value;
        }
        InlineMark::Color { value } => {
            marks.color = value.as_deref().map(parse_authoring_color).transpose()?;
        }
        InlineMark::Language { value } => marks.language = value.clone(),
    }
    Ok(())
}

fn apply_marked_range(
    node: &ContentNode,
    start: u32,
    end: u32,
    marks: Option<&MarkSet>,
    mark: Option<&InlineMark>,
) -> Result<ContentNode, CommandError> {
    if start == end {
        return Ok(node.clone());
    }
    let runs = node_runs(node).ok_or(CommandError::InvalidTarget)?;
    let (prefix, rest) = split_runs_at(runs, start)?;
    let (selected, suffix) = split_runs_at(
        &rest,
        end.checked_sub(start).ok_or(CommandError::InvalidRange)?,
    )?;
    let mut result = Vec::new();
    for run in prefix {
        append_run(&mut result, &run.text, &run.marks);
    }
    for run in selected {
        let mut selected_marks = marks.cloned().unwrap_or_else(|| run.marks.clone());
        if let Some(mark) = mark {
            update_inline_mark(&mut selected_marks, mark)?;
        }
        append_run(&mut result, &run.text, &selected_marks);
    }
    for run in suffix {
        append_run(&mut result, &run.text, &run.marks);
    }
    let mut changed = node.clone();
    set_node_runs(&mut changed, result)?;
    Ok(changed)
}

fn derive_inline_marks_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    marks: &MarkSet,
) -> Result<(Operation, Operation), CommandError> {
    validate_authoring_marks(marks)?;
    let (parent_id, spans) = selection_spans(document, selection)?;
    if spans.iter().all(|span| span.start == span.end) {
        return Err(CommandError::NoOp);
    }
    let expected = container_children(document, parent_id.as_ref())?;
    let mut replacement = expected.clone();
    let mut changed = false;
    for span in spans {
        let node = expected.get(span.index).ok_or(CommandError::InvalidRange)?;
        let updated = apply_marked_range(node, span.start, span.end, Some(marks), None)?;
        changed |= updated != *node;
        replacement[span.index] = updated;
    }
    if !changed {
        return Err(CommandError::NoOp);
    }
    structural_pair(
        document,
        parent_id,
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn derive_inline_mark_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    mark: &InlineMark,
) -> Result<(Operation, Operation), CommandError> {
    let (parent_id, spans) = selection_spans(document, selection)?;
    if spans.iter().all(|span| span.start == span.end) {
        return Err(CommandError::NoOp);
    }
    let expected = container_children(document, parent_id.as_ref())?;
    let mut replacement = expected.clone();
    let mut changed = false;
    for span in spans {
        let node = expected.get(span.index).ok_or(CommandError::InvalidRange)?;
        let updated = apply_marked_range(node, span.start, span.end, None, Some(mark))?;
        changed |= updated != *node;
        replacement[span.index] = updated;
    }
    if !changed {
        return Err(CommandError::NoOp);
    }
    structural_pair(
        document,
        parent_id,
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn update_block_attributes(
    node: &ContentNode,
    attributes: &BlockAttributes,
) -> Result<ContentNode, CommandError> {
    if attributes
        .spacing_before_millipoints
        .is_some_and(|value| value > 144_000)
        || attributes
            .spacing_after_millipoints
            .is_some_and(|value| value > 144_000)
    {
        return Err(CommandError::InvalidFormatting);
    }
    let mut changed = node.clone();
    let attrs = match &mut changed.body {
        BlockKind::Paragraph { attrs, .. } | BlockKind::Heading { attrs, .. } => attrs,
        _ => return Err(CommandError::InvalidTarget),
    };
    if let Some(alignment) = &attributes.alignment {
        attrs.alignment = alignment.clone();
    }
    if let Some(value) = attributes.spacing_before_millipoints {
        attrs.spacing_before_millipoints = value;
    }
    if let Some(value) = attributes.spacing_after_millipoints {
        attrs.spacing_after_millipoints = value;
    }
    Ok(changed)
}

fn apply_block_change(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    mut update: impl FnMut(&ContentNode) -> Result<ContentNode, CommandError>,
) -> Result<(Operation, Operation), CommandError> {
    let (parent_id, spans) = selection_spans(document, selection)?;
    let expected = container_children(document, parent_id.as_ref())?;
    let mut replacement = expected.clone();
    let mut changed = false;
    let mut visited = BTreeSet::new();
    for span in spans {
        if !visited.insert(span.index) {
            continue;
        }
        let node = expected.get(span.index).ok_or(CommandError::InvalidRange)?;
        let updated = update(node)?;
        changed |= updated != *node;
        replacement[span.index] = updated;
    }
    if !changed {
        return Err(CommandError::NoOp);
    }
    structural_pair(
        document,
        parent_id,
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn derive_block_attributes_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    attributes: &BlockAttributes,
) -> Result<(Operation, Operation), CommandError> {
    if attributes.alignment.is_none()
        && attributes.spacing_before_millipoints.is_none()
        && attributes.spacing_after_millipoints.is_none()
    {
        return Err(CommandError::NoOp);
    }
    apply_block_change(document, selection, |node| {
        update_block_attributes(node, attributes)
    })
}

fn apply_block_style(node: &ContentNode, style: &BlockStyle) -> Result<ContentNode, CommandError> {
    let mut changed = node.clone();
    match style {
        BlockStyle::Paragraph => match &node.body {
            BlockKind::Paragraph { .. } => return Ok(node.clone()),
            BlockKind::Heading { attrs, runs, .. } => {
                changed.body = BlockKind::Paragraph {
                    attrs: attrs.clone(),
                    runs: runs.clone(),
                };
            }
            _ => return Err(CommandError::InvalidTarget),
        },
        BlockStyle::Heading { level } => {
            if !(1..=6).contains(level) {
                return Err(CommandError::InvalidFormatting);
            }
            match &node.body {
                BlockKind::Paragraph { attrs, runs } => {
                    changed.body = BlockKind::Heading {
                        level: *level,
                        attrs: attrs.clone(),
                        runs: runs.clone(),
                    };
                }
                BlockKind::Heading {
                    level: current,
                    attrs,
                    runs,
                } if current != level => {
                    changed.body = BlockKind::Heading {
                        level: *level,
                        attrs: attrs.clone(),
                        runs: runs.clone(),
                    };
                }
                BlockKind::Heading { .. } => return Ok(node.clone()),
                _ => return Err(CommandError::InvalidTarget),
            }
        }
    }
    Ok(changed)
}

fn derive_block_style_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    style: &BlockStyle,
) -> Result<(Operation, Operation), CommandError> {
    apply_block_change(document, selection, |node| apply_block_style(node, style))
}

#[derive(Debug, Clone)]
struct NodeLocation {
    parent_id: Option<NodeId>,
    index: usize,
    node: ContentNode,
}

fn locate_node(nodes: &[ContentNode], target: &NodeId) -> Option<NodeLocation> {
    fn visit(
        nodes: &[ContentNode],
        target: &NodeId,
        parent_id: Option<&NodeId>,
    ) -> Option<NodeLocation> {
        for (index, node) in nodes.iter().enumerate() {
            if node.id == *target {
                return Some(NodeLocation {
                    parent_id: parent_id.cloned(),
                    index,
                    node: node.clone(),
                });
            }
            if let Some(found) = visit(node.children(), target, Some(&node.id)) {
                return Some(found);
            }
        }
        None
    }

    visit(nodes, target, None)
}

fn find_path(nodes: &[ContentNode], target: &NodeId) -> Option<Vec<usize>> {
    fn visit(nodes: &[ContentNode], target: &NodeId, prefix: &[usize]) -> Option<Vec<usize>> {
        for (index, node) in nodes.iter().enumerate() {
            let mut path = prefix.to_vec();
            path.push(index);
            if node.id == *target {
                return Some(path);
            }
            if let Some(found) = visit(node.children(), target, &path) {
                return Some(found);
            }
        }
        None
    }

    visit(nodes, target, &[])
}

fn node_at_path<'a>(nodes: &'a [ContentNode], path: &[usize]) -> Option<&'a ContentNode> {
    let index = *path.first()?;
    let node = nodes.get(index)?;
    if path.len() == 1 {
        Some(node)
    } else {
        node_at_path(node.children(), &path[1..])
    }
}

fn node_at_path_mut<'a>(
    nodes: &'a mut [ContentNode],
    path: &[usize],
) -> Option<&'a mut ContentNode> {
    let index = *path.first()?;
    let node = nodes.get_mut(index)?;
    if path.len() == 1 {
        Some(node)
    } else {
        node_at_path_mut(node.children_vec_mut()?, &path[1..])
    }
}

fn list_kind_of(node: &ContentNode) -> Option<ListKind> {
    match node.body {
        BlockKind::OrderedList { .. } => Some(ListKind::Ordered),
        BlockKind::UnorderedList { .. } => Some(ListKind::Unordered),
        _ => None,
    }
}

fn list_items(node: &ContentNode) -> Option<&[ContentNode]> {
    match &node.body {
        BlockKind::OrderedList { items } | BlockKind::UnorderedList { items } => Some(items),
        _ => None,
    }
}

fn list_items_mut(node: &mut ContentNode) -> Option<&mut Vec<ContentNode>> {
    match &mut node.body {
        BlockKind::OrderedList { items } | BlockKind::UnorderedList { items } => Some(items),
        _ => None,
    }
}

fn list_body(kind: ListKind, items: Vec<ContentNode>) -> BlockKind {
    match kind {
        ListKind::Ordered => BlockKind::OrderedList { items },
        ListKind::Unordered => BlockKind::UnorderedList { items },
        ListKind::None => unreachable!("none is not a list body"),
    }
}

fn list_context_for_path(
    nodes: &[ContentNode],
    path: &[usize],
) -> Option<(Vec<usize>, Vec<usize>)> {
    if path.len() < 2 {
        return None;
    }
    if list_kind_of(node_at_path(nodes, path)?).is_some() {
        return None;
    }
    for depth in (1..=path.len()).rev() {
        let item_path = &path[..depth];
        if !matches!(
            node_at_path(nodes, item_path)?.body,
            BlockKind::ListItem { .. }
        ) {
            continue;
        }
        let list_path = &item_path[..item_path.len() - 1];
        if list_kind_of(node_at_path(nodes, list_path)?).is_some() {
            return Some((list_path.to_vec(), item_path.to_vec()));
        }
    }
    None
}

fn validate_text_position(
    document: &FlowDocument,
    position: &LogicalPosition,
) -> Result<(), CommandError> {
    let node =
        find_node(&document.content, &position.node_id).ok_or(CommandError::InvalidTarget)?;
    match NodePositionMap::new(node, document.revision)?.validate(position, document.revision)? {
        ResolvedPosition::Text(_) => Ok(()),
        _ => Err(CommandError::InvalidTarget),
    }
}

fn list_selection_context(
    document: &FlowDocument,
    selection: &DirectionalSelection,
) -> Result<(Vec<usize>, usize, usize), CommandError> {
    validate_text_position(document, &selection.anchor)?;
    validate_text_position(document, &selection.focus)?;
    let anchor_path = find_path(&document.content, &selection.anchor.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    let focus_path = find_path(&document.content, &selection.focus.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    let (anchor_list, anchor_item) = list_context_for_path(&document.content, &anchor_path)
        .ok_or(CommandError::InvalidListStructure)?;
    let (focus_list, focus_item) = list_context_for_path(&document.content, &focus_path)
        .ok_or(CommandError::InvalidListStructure)?;
    if anchor_list != focus_list {
        return Err(CommandError::InvalidListStructure);
    }
    let anchor_index = *anchor_item
        .last()
        .ok_or(CommandError::InvalidListStructure)?;
    let focus_index = *focus_item
        .last()
        .ok_or(CommandError::InvalidListStructure)?;
    Ok((
        anchor_list,
        anchor_index.min(focus_index),
        anchor_index.max(focus_index),
    ))
}

fn list_context_for_node(
    document: &FlowDocument,
    node_id: &NodeId,
) -> Option<(Vec<usize>, Vec<usize>)> {
    let path = find_path(&document.content, node_id)?;
    list_context_for_path(&document.content, &path)
}

fn list_node_id_exists(document: &FlowDocument, id: &NodeId) -> bool {
    find_node(&document.content, id).is_some()
}

fn derive_list_kind_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    kind: ListKind,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    if let Ok((list_path, first, last)) = list_selection_context(document, selection) {
        let list =
            node_at_path(&document.content, &list_path).ok_or(CommandError::InvalidTarget)?;
        let current = list_kind_of(list).ok_or(CommandError::InvalidListStructure)?;
        let items = list_items(list).ok_or(CommandError::InvalidListStructure)?;
        if first != 0 || last + 1 != items.len() {
            return Err(CommandError::InvalidListStructure);
        }
        if current == kind {
            return Err(CommandError::NoOp);
        }
        let parent_path = &list_path[..list_path.len().saturating_sub(1)];
        let parent_id = node_at_path(&document.content, parent_path).map(|node| node.id.clone());
        let expected = container_children(document, parent_id.as_ref())?;
        let list_index = *list_path.last().ok_or(CommandError::InvalidListStructure)?;
        let mut replacement = expected.clone();
        if kind == ListKind::None {
            let mut flattened = Vec::new();
            for item in items {
                if !matches!(item.body, BlockKind::ListItem { .. }) {
                    return Err(CommandError::InvalidListStructure);
                }
                flattened.extend(item.children().iter().cloned());
            }
            replacement.splice(list_index..=list_index, flattened);
        } else {
            let mut changed_list = list.clone();
            let changed_items = items.to_vec();
            changed_list.body = list_body(kind, changed_items);
            replacement[list_index] = changed_list;
        }
        return structural_pair(
            document,
            parent_id,
            expected,
            replacement,
            AnchorMapping::identity(),
            AnchorMapping::identity(),
            None,
        );
    }

    let (parent_id, spans) = selection_spans(document, selection)?;
    if parent_id.is_some() {
        return Err(CommandError::InvalidListStructure);
    }
    if kind == ListKind::None {
        return Err(CommandError::NoOp);
    }
    let expected = container_children(document, None)?;
    let first = spans.first().ok_or(CommandError::InvalidRange)?.index;
    let last = spans.last().ok_or(CommandError::InvalidRange)?.index;
    let mut items = Vec::with_capacity(last - first + 1);
    for (offset, node) in expected[first..=last].iter().cloned().enumerate() {
        if node_runs(&node).is_none() {
            return Err(CommandError::IncompatibleStructure);
        }
        let item_id = deterministic_node_id(command_id, &format!("list-item-{offset}"))?;
        if list_node_id_exists(document, &item_id)
            || items.iter().any(|item: &ContentNode| item.id == item_id)
        {
            return Err(CommandError::InvalidRange);
        }
        items.push(ContentNode {
            id: item_id,
            style_id: None,
            body: BlockKind::ListItem {
                children: vec![node],
            },
        });
    }
    let list_id = deterministic_node_id(command_id, "list")?;
    if list_node_id_exists(document, &list_id) {
        return Err(CommandError::InvalidRange);
    }
    let list = ContentNode {
        id: list_id,
        style_id: None,
        body: list_body(kind, items),
    };
    let mut replacement = expected.clone();
    replacement.splice(first..=last, [list]);
    structural_pair(
        document,
        None,
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn derive_continue_list_item_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    if !selection.collapsed() {
        return Err(CommandError::InvalidRange);
    }
    let path = find_path(&document.content, &selection.anchor.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    let (list_path, item_path) = list_context_for_path(&document.content, &path)
        .ok_or(CommandError::InvalidListStructure)?;
    if path.len() != item_path.len() + 1 {
        return Err(CommandError::InvalidListStructure);
    }
    let item_index = *item_path.last().ok_or(CommandError::InvalidListStructure)?;
    let text_index = *path.last().ok_or(CommandError::InvalidTarget)?;
    let item = node_at_path(&document.content, &item_path).ok_or(CommandError::InvalidTarget)?;
    let text_node = item
        .children()
        .get(text_index)
        .ok_or(CommandError::InvalidTarget)?;
    let position = LogicalPosition {
        affinity: Affinity::Forward,
        ..selection.anchor.clone()
    };
    NodePositionMap::new(text_node, document.revision)?.validate(&position, document.revision)?;
    if text_node.text().is_empty() {
        return derive_exit_list_item_operation(document, selection, command_id);
    }
    let list = node_at_path(&document.content, &list_path).ok_or(CommandError::InvalidTarget)?;
    let expected = list_items(list)
        .ok_or(CommandError::InvalidListStructure)?
        .to_vec();
    let (retained, generated) = split_text_node(
        text_node,
        selection.anchor.utf16_offset.get(),
        deterministic_node_id(command_id, "list-continuation-text")?,
    )?;
    let new_item_id = deterministic_node_id(command_id, "list-continuation-item")?;
    if list_node_id_exists(document, &new_item_id) || list_node_id_exists(document, &generated.id) {
        return Err(CommandError::InvalidRange);
    }
    let mut current_children = item.children().to_vec();
    let trailing = current_children.split_off(text_index + 1);
    current_children[text_index] = retained;
    let mut new_children = vec![generated];
    new_children.extend(trailing);
    let mut current_item = item.clone();
    current_item.body = BlockKind::ListItem {
        children: current_children,
    };
    let new_item = ContentNode {
        id: new_item_id,
        style_id: item.style_id.clone(),
        body: BlockKind::ListItem {
            children: new_children,
        },
    };
    let mut replacement = expected.clone();
    replacement[item_index] = current_item;
    replacement.insert(item_index + 1, new_item);
    structural_pair(
        document,
        Some(
            node_at_path(&document.content, &list_path)
                .ok_or(CommandError::InvalidTarget)?
                .id
                .clone(),
        ),
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn derive_exit_list_item_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    if !selection.collapsed() {
        return Err(CommandError::InvalidRange);
    }
    let path = find_path(&document.content, &selection.anchor.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    let (list_path, item_path) = list_context_for_path(&document.content, &path)
        .ok_or(CommandError::InvalidListStructure)?;
    if path.len() != item_path.len() + 1 {
        return Err(CommandError::InvalidListStructure);
    }
    let item = node_at_path(&document.content, &item_path).ok_or(CommandError::InvalidTarget)?;
    let text_index = *path.last().ok_or(CommandError::InvalidTarget)?;
    let text_node = item
        .children()
        .get(text_index)
        .ok_or(CommandError::InvalidTarget)?;
    if item.children().len() != 1 || !text_node.text().is_empty() {
        return Err(CommandError::InvalidListStructure);
    }
    let list = node_at_path(&document.content, &list_path).ok_or(CommandError::InvalidTarget)?;
    let item_index = *item_path.last().ok_or(CommandError::InvalidListStructure)?;
    let list_index = *list_path.last().ok_or(CommandError::InvalidListStructure)?;
    let parent_path = &list_path[..list_path.len() - 1];
    let parent_id = node_at_path(&document.content, parent_path).map(|node| node.id.clone());
    let expected = container_children(document, parent_id.as_ref())?;
    let mut replacement = expected.clone();
    let mut remaining_items = list_items(list)
        .ok_or(CommandError::InvalidListStructure)?
        .to_vec();
    remaining_items.remove(item_index);
    let paragraph_id = deterministic_node_id(command_id, "list-exit-paragraph")?;
    if list_node_id_exists(document, &paragraph_id) {
        return Err(CommandError::InvalidRange);
    }
    let paragraph = ContentNode::paragraph(paragraph_id, text_node.style_id.clone(), String::new());
    let mut inserted = Vec::new();
    if !remaining_items.is_empty() {
        let mut changed_list = list.clone();
        changed_list.body = list_body(
            list_kind_of(list).ok_or(CommandError::InvalidListStructure)?,
            remaining_items,
        );
        inserted.push(changed_list);
    }
    inserted.push(paragraph);
    replacement.splice(list_index..=list_index, inserted);
    structural_pair(
        document,
        parent_id,
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn derive_indent_list_item_operation(
    document: &FlowDocument,
    item_id: &NodeId,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    let (list_path, item_path) =
        list_context_for_node(document, item_id).ok_or(CommandError::InvalidListStructure)?;
    let item_index = *item_path.last().ok_or(CommandError::InvalidListStructure)?;
    if item_index == 0 {
        return Err(CommandError::InvalidListStructure);
    }
    let list = node_at_path(&document.content, &list_path).ok_or(CommandError::InvalidTarget)?;
    let kind = list_kind_of(list).ok_or(CommandError::InvalidListStructure)?;
    let depth = list_depth_at_path(&document.content, &list_path);
    if depth >= crate::schema::MAX_LIST_DEPTH {
        return Err(CommandError::ListDepthExceeded);
    }
    let mut replacement = document.content.clone();
    let item = {
        let current_list =
            node_at_path_mut(&mut replacement, &list_path).ok_or(CommandError::InvalidTarget)?;
        list_items_mut(current_list)
            .ok_or(CommandError::InvalidListStructure)?
            .remove(item_index)
    };
    let previous_path = {
        let mut path = list_path.clone();
        path.push(item_index - 1);
        path
    };
    let previous =
        node_at_path_mut(&mut replacement, &previous_path).ok_or(CommandError::InvalidTarget)?;
    let nested_index = previous
        .children()
        .iter()
        .position(|child| list_kind_of(child) == Some(kind));
    if let Some(index) = nested_index {
        list_items_mut(
            previous
                .children_vec_mut()
                .ok_or(CommandError::InvalidListStructure)?
                .get_mut(index)
                .ok_or(CommandError::InvalidListStructure)?,
        )
        .ok_or(CommandError::InvalidListStructure)?
        .push(item);
    } else {
        let nested_id = deterministic_node_id(command_id, "indented-list")?;
        if list_node_id_exists(document, &nested_id) {
            return Err(CommandError::InvalidRange);
        }
        previous
            .children_vec_mut()
            .ok_or(CommandError::InvalidListStructure)?
            .push(ContentNode {
                id: nested_id,
                style_id: None,
                body: list_body(kind, vec![item]),
            });
    }
    structural_pair(
        document,
        None,
        document.content.clone(),
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn list_depth_at_path(nodes: &[ContentNode], list_path: &[usize]) -> usize {
    let mut depth = 0;
    let mut prefix = Vec::new();
    for index in list_path {
        prefix.push(*index);
        if node_at_path(nodes, &prefix).is_some_and(|node| list_kind_of(node).is_some()) {
            depth += 1;
        }
    }
    depth
}

fn derive_outdent_list_item_operation(
    document: &FlowDocument,
    item_id: &NodeId,
) -> Result<(Operation, Operation), CommandError> {
    let (list_path, item_path) =
        list_context_for_node(document, item_id).ok_or(CommandError::InvalidListStructure)?;
    if list_path.is_empty() || item_path.len() < 2 {
        return Err(CommandError::InvalidListStructure);
    }
    let parent_item_path = &list_path[..list_path.len() - 1];
    if !matches!(
        node_at_path(&document.content, parent_item_path)
            .ok_or(CommandError::InvalidTarget)?
            .body,
        BlockKind::ListItem { .. }
    ) {
        return Err(CommandError::InvalidListStructure);
    }
    let outer_list_path = &parent_item_path[..parent_item_path.len() - 1];
    let outer_list =
        node_at_path(&document.content, outer_list_path).ok_or(CommandError::InvalidTarget)?;
    if list_kind_of(outer_list).is_none() {
        return Err(CommandError::InvalidListStructure);
    }
    let inner_index = *item_path.last().ok_or(CommandError::InvalidListStructure)?;
    let parent_item_index = *parent_item_path
        .last()
        .ok_or(CommandError::InvalidListStructure)?;
    let mut replacement = document.content.clone();
    let item = {
        let inner =
            node_at_path_mut(&mut replacement, &list_path).ok_or(CommandError::InvalidTarget)?;
        list_items_mut(inner)
            .ok_or(CommandError::InvalidListStructure)?
            .remove(inner_index)
    };
    let inner_empty = node_at_path(&replacement, &list_path)
        .and_then(list_items)
        .is_some_and(<[ContentNode]>::is_empty);
    if inner_empty {
        let parent_item = node_at_path_mut(&mut replacement, parent_item_path)
            .ok_or(CommandError::InvalidTarget)?;
        let child_index = *list_path.last().ok_or(CommandError::InvalidListStructure)?;
        parent_item
            .children_vec_mut()
            .ok_or(CommandError::InvalidListStructure)?
            .remove(child_index);
    }
    let outer =
        node_at_path_mut(&mut replacement, outer_list_path).ok_or(CommandError::InvalidTarget)?;
    list_items_mut(outer)
        .ok_or(CommandError::InvalidListStructure)?
        .insert(parent_item_index + 1, item);
    structural_pair(
        document,
        None,
        document.content.clone(),
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn container_children(
    document: &FlowDocument,
    container_id: Option<&NodeId>,
) -> Result<Vec<ContentNode>, CommandError> {
    match container_id {
        None => Ok(document.content.clone()),
        Some(container_id) => find_node(&document.content, container_id)
            .and_then(|node| node.children_vec())
            .map(ToOwned::to_owned)
            .ok_or(CommandError::InvalidContainer),
    }
}

fn replace_container_children(
    document: &mut FlowDocument,
    container_id: Option<&NodeId>,
    expected: &[ContentNode],
    replacement: Vec<ContentNode>,
) -> Result<(), CommandError> {
    match container_id {
        None => {
            if document.content != expected {
                return Err(CommandError::HistoryConflict);
            }
            document.content = replacement;
            Ok(())
        }
        Some(container_id) => {
            let container = find_node_mut(&mut document.content, container_id)
                .ok_or(CommandError::InvalidContainer)?;
            let children = container
                .children_vec_mut()
                .ok_or(CommandError::InvalidContainer)?;
            if children != expected {
                return Err(CommandError::HistoryConflict);
            }
            *children = replacement;
            Ok(())
        }
    }
}

fn node_runs(node: &ContentNode) -> Option<&[InlineRun]> {
    match &node.body {
        BlockKind::Paragraph { runs, .. } | BlockKind::Heading { runs, .. } => Some(runs),
        _ => None,
    }
}

fn node_runs_mut(node: &mut ContentNode) -> Option<&mut Vec<InlineRun>> {
    match &mut node.body {
        BlockKind::Paragraph { runs, .. } | BlockKind::Heading { runs, .. } => Some(runs),
        _ => None,
    }
}

fn set_node_runs(node: &mut ContentNode, runs: Vec<InlineRun>) -> Result<(), CommandError> {
    let target = node_runs_mut(node).ok_or(CommandError::InvalidTarget)?;
    *target = runs;
    Ok(())
}

fn append_run(runs: &mut Vec<InlineRun>, text: &str, marks: &MarkSet) {
    if text.is_empty() {
        return;
    }
    if runs.last().is_some_and(|run| run.marks == *marks) {
        if let Some(last) = runs.last_mut() {
            last.text.push_str(text);
        }
        return;
    }
    runs.push(InlineRun {
        text: text.to_owned(),
        marks: marks.clone(),
    });
}

fn split_runs_at(
    runs: &[InlineRun],
    offset: u32,
) -> Result<(Vec<InlineRun>, Vec<InlineRun>), CommandError> {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut run_start = 0_u32;
    for run in runs {
        let run_length = utf16_length(&run.text)?;
        let run_end = run_start
            .checked_add(run_length)
            .ok_or(CommandError::InvalidRange)?;
        if offset >= run_end {
            append_run(&mut left, &run.text, &run.marks);
        } else if offset <= run_start {
            append_run(&mut right, &run.text, &run.marks);
        } else {
            let relative = offset
                .checked_sub(run_start)
                .ok_or(CommandError::InvalidRange)?;
            let byte = resolve_utf16_offset(&run.text, Utf16Offset::new(relative))?;
            append_run(&mut left, &run.text[..byte.get()], &run.marks);
            append_run(&mut right, &run.text[byte.get()..], &run.marks);
        }
        run_start = run_end;
    }
    if offset > run_start {
        return Err(CommandError::InvalidRange);
    }
    Ok((left, right))
}

fn marks_at_runs(runs: &[InlineRun], offset: u32) -> MarkSet {
    if runs.is_empty() {
        return MarkSet::default();
    }
    let mut start = 0_u32;
    for (index, run) in runs.iter().enumerate() {
        let end = start.saturating_add(run.text.encode_utf16().count() as u32);
        if offset < end || (offset == end && index + 1 == runs.len()) {
            return run.marks.clone();
        }
        start = end;
    }
    runs.last()
        .map_or_else(MarkSet::default, |run| run.marks.clone())
}

fn compatible_text_nodes(left: &ContentNode, right: &ContentNode) -> bool {
    if left.style_id != right.style_id {
        return false;
    }
    match (&left.body, &right.body) {
        (
            BlockKind::Paragraph {
                attrs: left_attrs, ..
            },
            BlockKind::Paragraph {
                attrs: right_attrs, ..
            },
        ) => left_attrs == right_attrs,
        (
            BlockKind::Heading {
                level: left_level,
                attrs: left_attrs,
                ..
            },
            BlockKind::Heading {
                level: right_level,
                attrs: right_attrs,
                ..
            },
        ) => left_level == right_level && left_attrs == right_attrs,
        _ => false,
    }
}

fn split_text_node(
    node: &ContentNode,
    split_utf16: u32,
    new_node_id: NodeId,
) -> Result<(ContentNode, ContentNode), CommandError> {
    let runs = node_runs(node).ok_or(CommandError::InvalidTarget)?;
    let (left, right) = split_runs_at(runs, split_utf16)?;
    let mut retained = node.clone();
    set_node_runs(&mut retained, left)?;
    let mut generated = ContentNode {
        id: new_node_id,
        style_id: node.style_id.clone(),
        body: BlockKind::Paragraph {
            attrs: ParagraphAttrs::default(),
            runs: right.clone(),
        },
    };
    match &node.body {
        BlockKind::Paragraph { attrs, .. } => {
            generated.body = BlockKind::Paragraph {
                attrs: attrs.clone(),
                runs: right,
            };
        }
        BlockKind::Heading { level, attrs, .. }
            if split_utf16 < node.text().encode_utf16().count() as u32 =>
        {
            generated.body = BlockKind::Heading {
                level: *level,
                attrs: attrs.clone(),
                runs: right,
            };
        }
        BlockKind::Heading { .. } => {}
        _ => return Err(CommandError::InvalidTarget),
    }
    Ok((retained, generated))
}

fn merge_text_nodes(
    first: &ContentNode,
    second: &ContentNode,
) -> Result<ContentNode, CommandError> {
    if !compatible_text_nodes(first, second) {
        return Err(CommandError::IncompatibleStructure);
    }
    let mut merged = first.clone();
    let mut runs = node_runs(first)
        .ok_or(CommandError::InvalidTarget)?
        .to_vec();
    for run in node_runs(second).ok_or(CommandError::InvalidTarget)? {
        append_run(&mut runs, &run.text, &run.marks);
    }
    set_node_runs(&mut merged, runs)?;
    Ok(merged)
}

fn all_node_ids(node: &ContentNode, ids: &mut Vec<NodeId>) {
    ids.push(node.id.clone());
    for child in node.children() {
        all_node_ids(child, ids);
    }
}

fn deterministic_node_id(command_id: &CommandId, purpose: &str) -> Result<NodeId, CommandError> {
    let digest = blake3::hash(format!("flowpdf:{purpose}:{}", command_id.as_str()).as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    NodeId::new(uuid::Uuid::from_bytes(bytes).hyphenated().to_string())
        .map_err(|_| CommandError::BrokenInvariant)
}

fn mapping_with(transformation: AnchorTransformation) -> AnchorMapping {
    AnchorMapping {
        transformations: vec![transformation],
    }
}

fn structural_pair(
    document: &FlowDocument,
    container_id: Option<NodeId>,
    expected_children: Vec<ContentNode>,
    replacement_children: Vec<ContentNode>,
    forward_mapping: AnchorMapping,
    inverse_mapping: AnchorMapping,
    preimage: Option<AnchorPreimage>,
) -> Result<(Operation, Operation), CommandError> {
    let field_updates = field_updates_for_mapping(document, &forward_mapping)?;
    ensure_deleted_targets_are_removed(&field_updates, &replacement_children)?;
    if let Some(preimage) = &preimage {
        validate_preimage(preimage, &field_updates)?;
    }
    let inverse_field_updates = field_updates
        .iter()
        .map(|update| FieldUpdate {
            index: update.index,
            expected: update.replacement.clone(),
            replacement: update.expected.clone(),
        })
        .collect();
    let forward = Operation::ReplaceChildren {
        container_id: container_id.clone(),
        expected_children: expected_children.clone(),
        replacement_children: replacement_children.clone(),
        anchor_mapping: forward_mapping,
        field_updates,
        preimage: preimage.clone(),
        expected_assets: Vec::new(),
        replacement_assets: Vec::new(),
    };
    let inverse = Operation::ReplaceChildren {
        container_id,
        expected_children: replacement_children,
        replacement_children: expected_children,
        anchor_mapping: inverse_mapping,
        field_updates: inverse_field_updates,
        preimage,
        expected_assets: Vec::new(),
        replacement_assets: Vec::new(),
    };
    Ok((forward, inverse))
}

#[allow(clippy::too_many_arguments)]
fn structural_pair_with_assets(
    document: &FlowDocument,
    container_id: Option<NodeId>,
    expected_children: Vec<ContentNode>,
    replacement_children: Vec<ContentNode>,
    forward_mapping: AnchorMapping,
    inverse_mapping: AnchorMapping,
    preimage: Option<AnchorPreimage>,
    expected_assets: Vec<AssetDescriptor>,
    replacement_assets: Vec<AssetDescriptor>,
) -> Result<(Operation, Operation), CommandError> {
    let (mut forward, mut inverse) = structural_pair(
        document,
        container_id,
        expected_children,
        replacement_children,
        forward_mapping,
        inverse_mapping,
        preimage,
    )?;
    let Operation::ReplaceChildren {
        expected_assets: forward_expected,
        replacement_assets: forward_replacement,
        ..
    } = &mut forward
    else {
        return Err(CommandError::BrokenInvariant);
    };
    *forward_expected = expected_assets.clone();
    *forward_replacement = replacement_assets.clone();
    let Operation::ReplaceChildren {
        expected_assets: inverse_expected,
        replacement_assets: inverse_replacement,
        ..
    } = &mut inverse
    else {
        return Err(CommandError::BrokenInvariant);
    };
    *inverse_expected = replacement_assets;
    *inverse_replacement = expected_assets;
    Ok((forward, inverse))
}

fn ensure_deleted_targets_are_removed(
    field_updates: &[FieldUpdate],
    replacement_children: &[ContentNode],
) -> Result<(), CommandError> {
    for update in field_updates {
        if !matches!(
            update.replacement.anchor,
            FieldAnchorState::TargetDeleted { .. }
        ) {
            continue;
        }
        if replacement_children.iter().any(|node| {
            find_node(
                std::slice::from_ref(node),
                &update.expected.anchor.original().node_id,
            )
            .is_some()
        }) {
            return Err(CommandError::AnchorInvalidated);
        }
    }
    Ok(())
}

fn field_updates_for_mapping(
    document: &FlowDocument,
    mapping: &AnchorMapping,
) -> Result<Vec<FieldUpdate>, CommandError> {
    let mut updates = Vec::new();
    for (index, field) in document.fields.iter().enumerate() {
        let mut replacement = field.clone();
        match mapping.map(field.anchor.original()) {
            AnchorMapResult::Mapped(position) => match &mut replacement.anchor {
                FieldAnchorState::GraphemeSafe { original }
                | FieldAnchorState::LegacyInvalid { original, .. } => {
                    *original = position;
                }
                FieldAnchorState::TargetDeleted { .. } => {}
            },
            AnchorMapResult::Deleted { tombstone } => {
                replacement.anchor = FieldAnchorState::TargetDeleted {
                    original: field.anchor.original().clone(),
                    tombstone,
                };
            }
            AnchorMapResult::Invalid(_) => return Err(CommandError::AnchorInvalidated),
        }
        if replacement != *field {
            updates.push(FieldUpdate {
                index: u32::try_from(index).map_err(|_| CommandError::InvalidRange)?,
                expected: Box::new(field.clone()),
                replacement: Box::new(replacement),
            });
        }
    }
    Ok(updates)
}

fn build_preimage(
    root_node_id: NodeId,
    affected_node_ids: Vec<NodeId>,
    command_id: &CommandId,
    field_updates: &[FieldUpdate],
) -> Result<AnchorPreimage, CommandError> {
    let tombstone = TombstoneToken {
        command_id: command_id.clone(),
        slot: 0,
    };
    let owners = field_updates
        .iter()
        .filter_map(|update| {
            let replacement = update.replacement.as_ref();
            matches!(replacement.anchor, FieldAnchorState::TargetDeleted { .. }).then(|| {
                AnchorPreimageEntry {
                    owner: AnchorOwner::Field(replacement.id.clone()),
                    position: update.expected.anchor.original().clone(),
                }
            })
        })
        .collect::<Vec<_>>();
    let preimage = AnchorPreimage {
        root_node_id,
        deleted_node_ids: affected_node_ids,
        tombstone,
        owners,
    };
    validate_preimage(&preimage, field_updates)?;
    Ok(preimage)
}

fn validate_preimage(
    preimage: &AnchorPreimage,
    field_updates: &[FieldUpdate],
) -> Result<(), CommandError> {
    if Uuid::parse_str(preimage.tombstone.command_id.as_str()).is_err()
        || preimage.deleted_node_ids.is_empty()
        || !preimage
            .deleted_node_ids
            .iter()
            .any(|node_id| node_id == &preimage.root_node_id)
    {
        return Err(CommandError::InvalidPreimage);
    }
    let mut node_ids = BTreeSet::new();
    if preimage
        .deleted_node_ids
        .iter()
        .any(|node_id| !node_ids.insert(node_id.as_str()))
    {
        return Err(CommandError::InvalidPreimage);
    }
    let mut owners = BTreeSet::new();
    for owner in &preimage.owners {
        let key = match &owner.owner {
            AnchorOwner::Field(field_id) => format!("field:{field_id}"),
            AnchorOwner::SelectionAnchor => "selection:anchor".to_owned(),
            AnchorOwner::SelectionFocus => "selection:focus".to_owned(),
        };
        if !owners.insert(key) || !node_ids.contains(owner.position.node_id.as_str()) {
            return Err(CommandError::InvalidPreimage);
        }
    }
    let encoded = serde_json::to_vec(preimage).map_err(|_| CommandError::InvalidPreimage)?;
    if preimage.owners.len() > DocumentLimits::V1.fields + 2
        || encoded.len() > DocumentLimits::V1.transaction_bytes
    {
        return Err(CommandError::PreimageLimit);
    }
    for owner in &preimage.owners {
        let AnchorOwner::Field(field_id) = &owner.owner else {
            continue;
        };
        let Some(update) = field_updates
            .iter()
            .find(|update| update.replacement.id == *field_id || update.expected.id == *field_id)
        else {
            return Err(CommandError::InvalidPreimage);
        };
        if update.expected.anchor.original() != &owner.position
            && update.replacement.anchor.original() != &owner.position
        {
            return Err(CommandError::InvalidPreimage);
        }
        let expected_deleted = matches!(
            &update.expected.anchor,
            FieldAnchorState::TargetDeleted { tombstone, .. }
                if tombstone == &preimage.tombstone
        );
        let replacement_deleted = matches!(
            &update.replacement.anchor,
            FieldAnchorState::TargetDeleted { tombstone, .. }
                if tombstone == &preimage.tombstone
        );
        if !expected_deleted && !replacement_deleted {
            return Err(CommandError::InvalidPreimage);
        }
    }
    Ok(())
}

pub(crate) fn validate_private_preimages(transaction: &Transaction) -> Result<(), CommandError> {
    for operation in transaction
        .forward_operations
        .iter()
        .chain(transaction.inverse_operations.iter())
    {
        if let Operation::ReplaceChildren {
            field_updates,
            preimage,
            ..
        } = operation
        {
            if let Some(preimage) = preimage {
                validate_preimage(preimage, field_updates)?;
            } else if field_updates.iter().any(|update| {
                matches!(
                    update.replacement.anchor,
                    FieldAnchorState::TargetDeleted { .. }
                )
            }) {
                return Err(CommandError::InvalidPreimage);
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct TableFocusHint {
    table_id: NodeId,
    row_index: usize,
    column_index: usize,
}

#[derive(Debug, Clone)]
struct RemovalFocusHint {
    parent_id: Option<NodeId>,
    index: usize,
}

fn table_focus_hint(
    document: &FlowDocument,
    mutation: &Mutation,
) -> Result<Option<TableFocusHint>, CommandError> {
    let selection = match mutation {
        Mutation::AddTableRow { selection }
        | Mutation::RemoveTableRow { selection }
        | Mutation::AddTableColumn { selection }
        | Mutation::RemoveTableColumn { selection } => selection,
        _ => return Ok(None),
    };
    if !selection.collapsed() {
        return Err(CommandError::InvalidRange);
    }
    for position in [&selection.anchor, &selection.focus] {
        let node =
            find_node(&document.content, &position.node_id).ok_or(CommandError::InvalidTarget)?;
        NodePositionMap::new(node, document.revision)?.validate(position, document.revision)?;
    }
    find_table_focus(&document.content, &selection.anchor.node_id)
        .map(Some)
        .ok_or(CommandError::InvalidTableStructure)
}

fn find_table_focus(nodes: &[ContentNode], target: &NodeId) -> Option<TableFocusHint> {
    for node in nodes {
        if let BlockKind::Table { rows, .. } = &node.body {
            for (row_index, row) in rows.iter().enumerate() {
                let BlockKind::TableRow { cells } = &row.body else {
                    continue;
                };
                for (column_index, cell) in cells.iter().enumerate() {
                    if find_node(std::slice::from_ref(cell), target).is_some() {
                        return Some(TableFocusHint {
                            table_id: node.id.clone(),
                            row_index,
                            column_index,
                        });
                    }
                }
            }
        }
        if let Some(found) = find_table_focus(node.children(), target) {
            return Some(found);
        }
    }
    None
}

fn removal_focus_hint(document: &FlowDocument, mutation: &Mutation) -> Option<RemovalFocusHint> {
    match mutation {
        Mutation::RemovePageBreak { page_break_id }
        | Mutation::RemoveTable {
            table_id: page_break_id,
            ..
        }
        | Mutation::RemoveImage {
            image_node_id: page_break_id,
            ..
        }
        | Mutation::ReplaceImage {
            image_node_id: page_break_id,
            ..
        } => locate_node(&document.content, page_break_id).map(|location| RemovalFocusHint {
            parent_id: location.parent_id,
            index: location.index,
        }),
        Mutation::InsertImage { placement, .. } => Some(RemovalFocusHint {
            parent_id: placement.parent_id.clone(),
            index: usize::try_from(placement.index).ok()?,
        }),
        _ => None,
    }
}

fn table_node<'a>(
    document: &'a FlowDocument,
    table_id: &NodeId,
) -> Result<&'a ContentNode, CommandError> {
    let table = find_node(&document.content, table_id).ok_or(CommandError::InvalidTarget)?;
    if !matches!(&table.body, BlockKind::Table { .. }) {
        return Err(CommandError::InvalidTableStructure);
    }
    Ok(table)
}

fn table_dimensions(table: &ContentNode) -> Result<(usize, usize), CommandError> {
    let BlockKind::Table { rows, .. } = &table.body else {
        return Err(CommandError::InvalidTableStructure);
    };
    if rows.is_empty() || rows.len() > MAX_TABLE_ROWS {
        return Err(CommandError::TableBoundsExceeded);
    }
    let columns = rows
        .first()
        .and_then(|row| match &row.body {
            BlockKind::TableRow { cells } => Some(cells.len()),
            _ => None,
        })
        .filter(|columns| (1..=MAX_TABLE_COLUMNS).contains(columns))
        .ok_or(CommandError::InvalidTableStructure)?;
    if rows
        .iter()
        .any(|row| !matches!(&row.body, BlockKind::TableRow { cells } if cells.len() == columns))
    {
        return Err(CommandError::InvalidTableStructure);
    }
    if rows
        .len()
        .checked_mul(columns)
        .is_none_or(|cells| cells > MAX_TABLE_CELLS)
    {
        return Err(CommandError::TableBoundsExceeded);
    }
    Ok((rows.len(), columns))
}

fn requested_table_dimensions(rows: u32, columns: u32) -> Result<(usize, usize), CommandError> {
    let rows = usize::try_from(rows).map_err(|_| CommandError::TableBoundsExceeded)?;
    let columns = usize::try_from(columns).map_err(|_| CommandError::TableBoundsExceeded)?;
    if !(1..=MAX_TABLE_ROWS).contains(&rows) || !(1..=MAX_TABLE_COLUMNS).contains(&columns) {
        return Err(CommandError::TableBoundsExceeded);
    }
    if rows
        .checked_mul(columns)
        .is_none_or(|cells| cells > MAX_TABLE_CELLS)
    {
        return Err(CommandError::TableBoundsExceeded);
    }
    Ok((rows, columns))
}

fn placement_children(
    document: &FlowDocument,
    placement: &StructuralPlacement,
) -> Result<Vec<ContentNode>, CommandError> {
    if let Some(parent_id) = &placement.parent_id {
        let parent = find_node(&document.content, parent_id).ok_or(CommandError::InvalidTarget)?;
        if !matches!(
            &parent.body,
            BlockKind::ListItem { .. } | BlockKind::TableCell { .. }
        ) {
            return Err(CommandError::InvalidContainer);
        }
    }
    let expected = container_children(document, placement.parent_id.as_ref())?;
    let index = usize::try_from(placement.index).map_err(|_| CommandError::InvalidRange)?;
    if index > expected.len() {
        return Err(CommandError::InvalidRange);
    }
    Ok(expected)
}

fn ensure_generated_ids_available(
    document: &FlowDocument,
    generated: &[NodeId],
) -> Result<(), CommandError> {
    let mut seen = BTreeSet::new();
    for id in generated {
        if find_node(&document.content, id).is_some() || !seen.insert(id.as_str()) {
            return Err(CommandError::InvalidRange);
        }
    }
    Ok(())
}

fn image_asset_for_staging(
    document: &FlowDocument,
    staged: &RedeemedAsset,
    accessibility: &ImageAccessibility,
) -> Result<(crate::model::AssetId, Vec<AssetDescriptor>), CommandError> {
    asset::validate_accessibility(accessibility)?;
    let staged_descriptor = staged.descriptor();
    if let Some(existing) = document
        .assets
        .iter()
        .find(|asset| asset.content_hash == staged_descriptor.content_hash)
    {
        if existing.byte_length != staged_descriptor.byte_length
            || existing.media_type != staged_descriptor.media_type
        {
            return Err(CommandError::BrokenInvariant);
        }
        return Ok((existing.id.clone(), document.assets.clone()));
    }
    DocumentLimits::V1.check(
        crate::schema::LimitKind::Assets,
        document.assets.len().saturating_add(1),
    )?;
    if document
        .assets
        .iter()
        .any(|asset| asset.id == staged_descriptor.id)
    {
        return Err(CommandError::BrokenInvariant);
    }
    let mut descriptor = staged_descriptor.clone();
    descriptor.alt_text = accessibility.alt_text().to_owned();
    let asset_id = descriptor.id.clone();
    let mut replacement = document.assets.clone();
    replacement.push(descriptor);
    Ok((asset_id, replacement))
}

fn derive_insert_image_operation(
    document: &FlowDocument,
    placement: &StructuralPlacement,
    accessibility: &ImageAccessibility,
    staged: &RedeemedAsset,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    let expected = placement_children(document, placement)?;
    let (asset_id, replacement_assets) = image_asset_for_staging(document, staged, accessibility)?;
    let image_id = deterministic_node_id(command_id, "image")?;
    ensure_generated_ids_available(document, std::slice::from_ref(&image_id))?;
    let image = ContentNode {
        id: image_id,
        style_id: None,
        body: BlockKind::Image {
            asset_id,
            accessibility: accessibility.clone(),
        },
    };
    let index = usize::try_from(placement.index).map_err(|_| CommandError::InvalidRange)?;
    let mut replacement = expected.clone();
    replacement.insert(index, image);
    structural_pair_with_assets(
        document,
        placement.parent_id.clone(),
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
        document.assets.clone(),
        replacement_assets,
    )
}

fn derive_replace_image_operation(
    document: &FlowDocument,
    image_node_id: &NodeId,
    accessibility: &ImageAccessibility,
    staged: &RedeemedAsset,
) -> Result<(Operation, Operation), CommandError> {
    let location =
        locate_node(&document.content, image_node_id).ok_or(CommandError::InvalidTarget)?;
    let BlockKind::Image {
        asset_id: current_asset_id,
        accessibility: current_accessibility,
    } = &location.node.body
    else {
        return Err(CommandError::InvalidTarget);
    };
    let (asset_id, replacement_assets) = image_asset_for_staging(document, staged, accessibility)?;
    if *current_asset_id == asset_id && current_accessibility == accessibility {
        return Err(CommandError::NoOp);
    }
    let expected = container_children(document, location.parent_id.as_ref())?;
    let mut replacement = expected.clone();
    let mut changed = location.node.clone();
    changed.body = BlockKind::Image {
        asset_id,
        accessibility: accessibility.clone(),
    };
    replacement[location.index] = changed;
    structural_pair_with_assets(
        document,
        location.parent_id,
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
        document.assets.clone(),
        replacement_assets,
    )
}

fn derive_set_image_accessibility_operation(
    document: &FlowDocument,
    image_node_id: &NodeId,
    accessibility: &ImageAccessibility,
) -> Result<(Operation, Operation), CommandError> {
    asset::validate_accessibility(accessibility)?;
    let location =
        locate_node(&document.content, image_node_id).ok_or(CommandError::InvalidTarget)?;
    let BlockKind::Image {
        asset_id,
        accessibility: current,
    } = &location.node.body
    else {
        return Err(CommandError::InvalidTarget);
    };
    if current == accessibility {
        return Err(CommandError::NoOp);
    }
    let expected = container_children(document, location.parent_id.as_ref())?;
    let mut replacement = expected.clone();
    let mut changed = location.node.clone();
    changed.body = BlockKind::Image {
        asset_id: asset_id.clone(),
        accessibility: accessibility.clone(),
    };
    replacement[location.index] = changed;
    structural_pair(
        document,
        location.parent_id,
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn derive_remove_image_operation(
    document: &FlowDocument,
    image_node_id: &NodeId,
    confirmed: bool,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    if !confirmed {
        return Err(CommandError::ConfirmationRequired);
    }
    let location =
        locate_node(&document.content, image_node_id).ok_or(CommandError::InvalidTarget)?;
    if !matches!(location.node.body, BlockKind::Image { .. }) {
        return Err(CommandError::InvalidTarget);
    }
    derive_remove_atomic_operation(document, image_node_id, command_id, false)
}

fn table_cell_style_id(document: &FlowDocument, table: &ContentNode) -> Option<StyleId> {
    fn first_text_style(node: &ContentNode) -> Option<StyleId> {
        if node.runs().is_some() {
            return node.style_id.clone();
        }
        node.children().iter().find_map(first_text_style)
    }
    first_text_style(table).or_else(|| document.styles.first().map(|style| style.id.clone()))
}

fn generated_table_row(
    command_id: &CommandId,
    purpose: &str,
    row_index: usize,
    columns: usize,
    style_id: Option<StyleId>,
) -> Result<(ContentNode, Vec<NodeId>), CommandError> {
    let row_id = deterministic_node_id(command_id, &format!("{purpose}-row-{row_index}"))?;
    let mut generated_ids = vec![row_id.clone()];
    let mut cells = Vec::with_capacity(columns);
    for column_index in 0..columns {
        let cell_id = deterministic_node_id(
            command_id,
            &format!("{purpose}-row-{row_index}-cell-{column_index}"),
        )?;
        let paragraph_id = deterministic_node_id(
            command_id,
            &format!("{purpose}-row-{row_index}-cell-{column_index}-paragraph"),
        )?;
        generated_ids.push(cell_id.clone());
        generated_ids.push(paragraph_id.clone());
        cells.push(ContentNode {
            id: cell_id,
            style_id: None,
            body: BlockKind::TableCell {
                children: vec![ContentNode::paragraph(
                    paragraph_id,
                    style_id.clone(),
                    String::new(),
                )],
            },
        });
    }
    Ok((
        ContentNode {
            id: row_id,
            style_id: None,
            body: BlockKind::TableRow { cells },
        },
        generated_ids,
    ))
}

fn generated_table_cell(
    command_id: &CommandId,
    purpose: &str,
    row_index: usize,
    column_index: usize,
    style_id: Option<StyleId>,
) -> Result<(ContentNode, Vec<NodeId>), CommandError> {
    let cell_id = deterministic_node_id(
        command_id,
        &format!("{purpose}-row-{row_index}-cell-{column_index}"),
    )?;
    let paragraph_id = deterministic_node_id(
        command_id,
        &format!("{purpose}-row-{row_index}-cell-{column_index}-paragraph"),
    )?;
    Ok((
        ContentNode {
            id: cell_id.clone(),
            style_id: None,
            body: BlockKind::TableCell {
                children: vec![ContentNode::paragraph(
                    paragraph_id.clone(),
                    style_id,
                    String::new(),
                )],
            },
        },
        vec![cell_id, paragraph_id],
    ))
}

fn deleted_mapping(
    nodes: &[ContentNode],
    command_id: &CommandId,
) -> Result<(AnchorMapping, Vec<NodeId>), CommandError> {
    let tombstone = TombstoneToken {
        command_id: command_id.clone(),
        slot: 0,
    };
    let mut ids = Vec::new();
    for node in nodes {
        all_node_ids(node, &mut ids);
    }
    let mapping = ids
        .iter()
        .cloned()
        .fold(AnchorMapping::identity(), |mut mapping, node_id| {
            mapping.push(AnchorTransformation::NodeDeleted {
                node_id,
                tombstone: tombstone.clone(),
            });
            mapping
        });
    Ok((mapping, ids))
}

fn table_selection_at(
    document: &FlowDocument,
    table_id: &NodeId,
    row_index: usize,
    column_index: usize,
) -> Option<DirectionalSelection> {
    let table = find_node(&document.content, table_id)?;
    let BlockKind::Table { rows, .. } = &table.body else {
        return None;
    };
    let row = rows.get(row_index)?;
    let cell = row.children().get(column_index)?;
    first_text_selection_in_node(cell)
}

fn table_row_count(document: &FlowDocument, table_id: &NodeId) -> usize {
    find_node(&document.content, table_id)
        .and_then(|table| match &table.body {
            BlockKind::Table { rows, .. } => Some(rows.len()),
            _ => None,
        })
        .unwrap_or(0)
}

fn table_column_count(document: &FlowDocument, table_id: &NodeId) -> usize {
    find_node(&document.content, table_id)
        .and_then(|table| match &table.body {
            BlockKind::Table { rows, .. } => rows.first().map(|row| row.children().len()),
            _ => None,
        })
        .unwrap_or(0)
}

fn selection_near_container(
    document: &FlowDocument,
    hint: &RemovalFocusHint,
) -> Option<DirectionalSelection> {
    let children = container_children(document, hint.parent_id.as_ref()).ok()?;
    let index = hint.index.min(children.len());
    for distance in 0..=children.len() {
        if let Some(node) = children.get(index.saturating_add(distance))
            && let Some(selection) = first_text_selection_in_node(node)
        {
            return Some(selection);
        }
        if distance > 0
            && let Some(node) = index
                .checked_sub(distance)
                .and_then(|index| children.get(index))
            && let Some(selection) = first_text_selection_in_node(node)
        {
            return Some(selection);
        }
    }
    first_text_selection(document)
}

fn derive_insert_page_break_operation(
    document: &FlowDocument,
    placement: &StructuralPlacement,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    let expected = placement_children(document, placement)?;
    let page_break_id = deterministic_node_id(command_id, "page-break")?;
    let paragraph_id = deterministic_node_id(command_id, "page-break-paragraph")?;
    ensure_generated_ids_available(document, &[page_break_id.clone(), paragraph_id.clone()])?;
    let style_id = document.styles.first().map(|style| style.id.clone());
    let page_break = ContentNode {
        id: page_break_id,
        style_id: None,
        body: BlockKind::PageBreak,
    };
    let paragraph = ContentNode::paragraph(paragraph_id, style_id, String::new());
    let index = usize::try_from(placement.index).map_err(|_| CommandError::InvalidRange)?;
    let mut replacement = expected.clone();
    replacement.splice(index..index, [page_break, paragraph]);
    structural_pair(
        document,
        placement.parent_id.clone(),
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn derive_remove_page_break_operation(
    document: &FlowDocument,
    page_break_id: &NodeId,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    let location =
        locate_node(&document.content, page_break_id).ok_or(CommandError::InvalidTarget)?;
    if !matches!(location.node.body, BlockKind::PageBreak) {
        return Err(CommandError::InvalidTarget);
    }
    derive_remove_atomic_operation(document, page_break_id, command_id, false)
}

fn derive_insert_table_operation(
    document: &FlowDocument,
    placement: &StructuralPlacement,
    rows: u32,
    columns: u32,
    header_row: bool,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    let (rows_count, columns_count) = requested_table_dimensions(rows, columns)?;
    let expected = placement_children(document, placement)?;
    let table_id = deterministic_node_id(command_id, "table")?;
    let mut generated_ids = vec![table_id.clone()];
    let style_id = document.styles.first().map(|style| style.id.clone());
    let mut table_rows = Vec::with_capacity(rows_count);
    for row_index in 0..rows_count {
        let (row, ids) = generated_table_row(
            command_id,
            "table",
            row_index,
            columns_count,
            style_id.clone(),
        )?;
        generated_ids.extend(ids);
        table_rows.push(row);
    }
    ensure_generated_ids_available(document, &generated_ids)?;
    let table = ContentNode {
        id: table_id,
        style_id: None,
        body: BlockKind::Table {
            header_rows: u8::from(header_row),
            rows: table_rows,
        },
    };
    crate::schema::validate_new_node(document, &table)?;
    let index = usize::try_from(placement.index).map_err(|_| CommandError::InvalidRange)?;
    let mut replacement = expected.clone();
    replacement.insert(index, table);
    structural_pair(
        document,
        placement.parent_id.clone(),
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn derive_add_table_row_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    let focus = table_cell_focus(document, selection)?;
    let table = table_node(document, &focus.table_id)?;
    let (row_count, column_count) = table_dimensions(table)?;
    if row_count >= MAX_TABLE_ROWS {
        return Err(CommandError::TableBoundsExceeded);
    }
    let expected = container_children(document, Some(&focus.table_id))?;
    let style_id = table_cell_style_id(document, table);
    let (row, generated_ids) =
        generated_table_row(command_id, "table-row", row_count, column_count, style_id)?;
    ensure_generated_ids_available(document, &generated_ids)?;
    let mut replacement = expected.clone();
    replacement.insert(focus.row_index + 1, row);
    structural_pair(
        document,
        Some(focus.table_id),
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn derive_remove_table_row_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    let focus = table_cell_focus(document, selection)?;
    let table = table_node(document, &focus.table_id)?;
    let (row_count, _) = table_dimensions(table)?;
    if row_count == 1 {
        return Err(CommandError::ConfirmationRequired);
    }
    let expected = container_children(document, Some(&focus.table_id))?;
    let removed = expected
        .get(focus.row_index)
        .ok_or(CommandError::InvalidTableStructure)?
        .clone();
    let mut replacement = expected.clone();
    replacement.remove(focus.row_index);
    let (mapping, affected_ids) = deleted_mapping(std::slice::from_ref(&removed), command_id)?;
    let field_updates = field_updates_for_mapping(document, &mapping)?;
    let preimage = build_preimage(removed.id.clone(), affected_ids, command_id, &field_updates)?;
    structural_pair(
        document,
        Some(focus.table_id),
        expected,
        replacement,
        mapping,
        AnchorMapping::identity(),
        Some(preimage),
    )
}

fn derive_add_table_column_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    let focus = table_cell_focus(document, selection)?;
    let table = table_node(document, &focus.table_id)?;
    let (_row_count, column_count) = table_dimensions(table)?;
    if column_count >= MAX_TABLE_COLUMNS {
        return Err(CommandError::TableBoundsExceeded);
    }
    let expected = container_children(document, Some(&focus.table_id))?;
    let style_id = table_cell_style_id(document, table);
    let mut replacement = expected.clone();
    let mut generated_ids = Vec::new();
    for (row_index, row) in expected.iter().enumerate() {
        let mut changed = row.clone();
        let cells = match &mut changed.body {
            BlockKind::TableRow { cells } => cells,
            _ => return Err(CommandError::InvalidTableStructure),
        };
        let (cell, ids) = generated_table_cell(
            command_id,
            "table-column",
            row_index,
            column_count,
            style_id.clone(),
        )?;
        generated_ids.extend(ids);
        cells.insert(focus.column_index + 1, cell);
        replacement[row_index] = changed;
    }
    ensure_generated_ids_available(document, &generated_ids)?;
    structural_pair(
        document,
        Some(focus.table_id),
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn derive_remove_table_column_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    let focus = table_cell_focus(document, selection)?;
    let table = table_node(document, &focus.table_id)?;
    let (_, column_count) = table_dimensions(table)?;
    if column_count == 1 {
        return Err(CommandError::ConfirmationRequired);
    }
    let expected = container_children(document, Some(&focus.table_id))?;
    let mut replacement = expected.clone();
    let mut removed = Vec::with_capacity(expected.len());
    for (row_index, row) in expected.iter().enumerate() {
        let mut changed = row.clone();
        let cells = match &mut changed.body {
            BlockKind::TableRow { cells } => cells,
            _ => return Err(CommandError::InvalidTableStructure),
        };
        let cell = cells
            .get(focus.column_index)
            .ok_or(CommandError::InvalidTableStructure)?
            .clone();
        removed.push(cell);
        cells.remove(focus.column_index);
        replacement[row_index] = changed;
    }
    let (mapping, affected_ids) = deleted_mapping(&removed, command_id)?;
    let root = removed
        .first()
        .map(|node| node.id.clone())
        .ok_or(CommandError::InvalidTableStructure)?;
    let field_updates = field_updates_for_mapping(document, &mapping)?;
    let preimage = build_preimage(root, affected_ids, command_id, &field_updates)?;
    structural_pair(
        document,
        Some(focus.table_id),
        expected,
        replacement,
        mapping,
        AnchorMapping::identity(),
        Some(preimage),
    )
}

fn derive_set_table_header_operation(
    document: &FlowDocument,
    table_id: &NodeId,
    enabled: bool,
) -> Result<(Operation, Operation), CommandError> {
    let location = locate_node(&document.content, table_id).ok_or(CommandError::InvalidTarget)?;
    let BlockKind::Table { header_rows, .. } = &location.node.body else {
        return Err(CommandError::InvalidTableStructure);
    };
    let next = u8::from(enabled);
    if *header_rows == next {
        return Err(CommandError::NoOp);
    }
    let expected = container_children(document, location.parent_id.as_ref())?;
    let mut changed = location.node.clone();
    if let BlockKind::Table { header_rows, .. } = &mut changed.body {
        *header_rows = next;
    }
    let mut replacement = expected.clone();
    replacement[location.index] = changed;
    structural_pair(
        document,
        location.parent_id,
        expected,
        replacement,
        AnchorMapping::identity(),
        AnchorMapping::identity(),
        None,
    )
}

fn derive_remove_table_operation(
    document: &FlowDocument,
    table_id: &NodeId,
    confirmed: bool,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    if !confirmed {
        return Err(CommandError::ConfirmationRequired);
    }
    let table = table_node(document, table_id)?;
    let _ = table_dimensions(table)?;
    derive_remove_atomic_operation(document, table_id, command_id, true)
}

fn derive_remove_atomic_operation(
    document: &FlowDocument,
    node_id: &NodeId,
    command_id: &CommandId,
    table: bool,
) -> Result<(Operation, Operation), CommandError> {
    let location = locate_node(&document.content, node_id).ok_or(CommandError::InvalidTarget)?;
    if table && !matches!(location.node.body, BlockKind::Table { .. }) {
        return Err(CommandError::InvalidTableStructure);
    }
    let expected = container_children(document, location.parent_id.as_ref())?;
    let mut replacement = expected.clone();
    replacement.remove(location.index);
    if location.parent_id.is_some() && !replacement.iter().any(contains_editable_text) {
        let fallback_id = deterministic_node_id(command_id, "structural-remove-fallback")?;
        ensure_generated_ids_available(document, std::slice::from_ref(&fallback_id))?;
        let style_id = document.styles.first().map(|style| style.id.clone());
        replacement.push(ContentNode::paragraph(fallback_id, style_id, String::new()));
    } else if location.parent_id.is_none()
        && !document_has_editable_after_removal(document, node_id)
    {
        let fallback_id = deterministic_node_id(command_id, "structural-remove-fallback")?;
        ensure_generated_ids_available(document, std::slice::from_ref(&fallback_id))?;
        let style_id = document.styles.first().map(|style| style.id.clone());
        replacement.insert(
            0,
            ContentNode::paragraph(fallback_id, style_id, String::new()),
        );
    }
    let (mapping, affected_ids) =
        deleted_mapping(std::slice::from_ref(&location.node), command_id)?;
    let field_updates = field_updates_for_mapping(document, &mapping)?;
    let preimage = build_preimage(node_id.clone(), affected_ids, command_id, &field_updates)?;
    structural_pair(
        document,
        location.parent_id,
        expected,
        replacement,
        mapping,
        AnchorMapping::identity(),
        Some(preimage),
    )
}

fn table_cell_focus(
    document: &FlowDocument,
    selection: &DirectionalSelection,
) -> Result<TableFocusHint, CommandError> {
    table_focus_hint(
        document,
        &Mutation::AddTableRow {
            selection: selection.clone(),
        },
    )?
    .ok_or(CommandError::InvalidTableStructure)
}

fn derive_split_operation(
    document: &FlowDocument,
    node_id: &NodeId,
    split_utf16: Utf16Offset,
    new_node_id: &NodeId,
    _command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    if find_node(&document.content, new_node_id).is_some() || node_id == new_node_id {
        return Err(CommandError::InvalidRange);
    }
    let location = locate_node(&document.content, node_id).ok_or(CommandError::InvalidTarget)?;
    let text = location.node.text();
    let position = LogicalPosition {
        node_id: node_id.clone(),
        utf16_offset: split_utf16,
        affinity: Affinity::Forward,
    };
    NodePositionMap::new(&location.node, document.revision)?
        .validate(&position, document.revision)?;
    let (retained, generated) =
        split_text_node(&location.node, split_utf16.get(), new_node_id.clone())?;
    let expected = container_children(document, location.parent_id.as_ref())?;
    let mut replacement = expected.clone();
    replacement.splice(location.index..=location.index, [retained, generated]);
    let forward_mapping = mapping_with(AnchorTransformation::SplitTextBlock {
        node_id: node_id.clone(),
        new_node_id: new_node_id.clone(),
        split_utf16,
    });
    let inverse_mapping = mapping_with(AnchorTransformation::MergeTextBlocks {
        first_node_id: node_id.clone(),
        second_node_id: new_node_id.clone(),
        first_utf16_length: split_utf16.get(),
    });
    let _ = text;
    structural_pair(
        document,
        location.parent_id,
        expected,
        replacement,
        forward_mapping,
        inverse_mapping,
        None,
    )
}

fn derive_merge_operation(
    document: &FlowDocument,
    first_node_id: &NodeId,
    second_node_id: &NodeId,
) -> Result<(Operation, Operation), CommandError> {
    let first = locate_node(&document.content, first_node_id).ok_or(CommandError::InvalidTarget)?;
    let second =
        locate_node(&document.content, second_node_id).ok_or(CommandError::InvalidTarget)?;
    if first.parent_id != second.parent_id || second.index != first.index + 1 {
        return Err(CommandError::IncompatibleStructure);
    }
    if !compatible_text_nodes(&first.node, &second.node) {
        return Err(CommandError::IncompatibleStructure);
    }
    let merged = merge_text_nodes(&first.node, &second.node)?;
    let first_length = utf16_length(&first.node.text())?;
    let expected = container_children(document, first.parent_id.as_ref())?;
    let mut replacement = expected.clone();
    replacement.splice(first.index..=second.index, [merged]);
    structural_pair(
        document,
        first.parent_id.clone(),
        expected,
        replacement,
        mapping_with(AnchorTransformation::MergeTextBlocks {
            first_node_id: first_node_id.clone(),
            second_node_id: second_node_id.clone(),
            first_utf16_length: first_length,
        }),
        mapping_with(AnchorTransformation::SplitTextBlock {
            node_id: first_node_id.clone(),
            new_node_id: second_node_id.clone(),
            split_utf16: first_length.into(),
        }),
        None,
    )
}

fn derive_delete_subtree_operation(
    document: &FlowDocument,
    node_id: &NodeId,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    let location = locate_node(&document.content, node_id).ok_or(CommandError::InvalidTarget)?;
    let expected = container_children(document, location.parent_id.as_ref())?;
    let mut replacement = expected.clone();
    replacement.remove(location.index);
    if replacement.is_empty() && location.parent_id.is_some() {
        return Err(CommandError::InvalidContainer);
    }
    if !document_has_editable_after_removal(document, node_id) {
        if location.parent_id.is_some() {
            return Err(CommandError::BrokenInvariant);
        }
        let fallback_id = deterministic_node_id(command_id, "editable-fallback")?;
        let style_id = document.styles.first().map(|style| style.id.clone());
        replacement.insert(
            0,
            ContentNode::paragraph(fallback_id, style_id, String::new()),
        );
    }
    let mut affected_ids = Vec::new();
    all_node_ids(&location.node, &mut affected_ids);
    let forward_mapping =
        affected_ids
            .iter()
            .cloned()
            .fold(AnchorMapping::identity(), |mut mapping, deleted_id| {
                mapping.push(AnchorTransformation::NodeDeleted {
                    node_id: deleted_id,
                    tombstone: TombstoneToken {
                        command_id: command_id.clone(),
                        slot: 0,
                    },
                });
                mapping
            });
    let updates = field_updates_for_mapping(document, &forward_mapping)?;
    let preimage = build_preimage(node_id.clone(), affected_ids, command_id, &updates)?;
    structural_pair(
        document,
        location.parent_id,
        expected,
        replacement,
        forward_mapping,
        AnchorMapping::identity(),
        Some(preimage),
    )
}

fn document_has_editable_after_removal(document: &FlowDocument, removed: &NodeId) -> bool {
    fn visit(nodes: &[ContentNode], removed: &NodeId) -> bool {
        nodes.iter().any(|node| {
            if node.id == *removed {
                return false;
            }
            node.runs().is_some() || visit(node.children(), removed)
        })
    }
    visit(&document.content, removed)
}

fn derive_cross_block_selection_operation(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    replacement: String,
    command_id: &CommandId,
) -> Result<(Operation, Operation), CommandError> {
    let anchor = locate_node(&document.content, &selection.anchor.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    let focus = locate_node(&document.content, &selection.focus.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    if anchor.parent_id != focus.parent_id {
        return Err(CommandError::IncompatibleStructure);
    }
    let (first, first_position, last, last_position) = if anchor.index < focus.index {
        (&anchor, &selection.anchor, &focus, &selection.focus)
    } else {
        (&focus, &selection.focus, &anchor, &selection.anchor)
    };
    let expected = container_children(document, first.parent_id.as_ref())?;
    if first.index >= expected.len() || last.index >= expected.len() || first.index >= last.index {
        return Err(CommandError::InvalidRange);
    }
    if !compatible_text_nodes(&first.node, &last.node)
        || expected[first.index..=last.index]
            .iter()
            .any(|node| !compatible_text_nodes(&first.node, node))
    {
        return Err(CommandError::IncompatibleStructure);
    }
    let first_map = NodePositionMap::new(&first.node, document.revision)?;
    let last_map = NodePositionMap::new(&last.node, document.revision)?;
    let first_byte = match first_map.validate(first_position, document.revision)? {
        ResolvedPosition::Text(byte) => byte,
        _ => return Err(CommandError::InvalidTarget),
    };
    let last_byte = match last_map.validate(last_position, document.revision)? {
        ResolvedPosition::Text(byte) => byte,
        _ => return Err(CommandError::InvalidTarget),
    };
    let first_length = utf16_length(&first.node.text())?;
    let last_length = utf16_length(&last.node.text())?;
    if first_byte.get() > first.node.text().len() || last_byte.get() > last.node.text().len() {
        return Err(CommandError::InvalidRange);
    }
    if replacement.len() > DocumentLimits::V1.text_node_bytes {
        return Err(EditorPositionError::TextLimit.into());
    }
    let (prefix, _) = split_runs_at(
        node_runs(&first.node).ok_or(CommandError::InvalidTarget)?,
        first_position.utf16_offset.get(),
    )?;
    let (_, suffix) = split_runs_at(
        node_runs(&last.node).ok_or(CommandError::InvalidTarget)?,
        last_position.utf16_offset.get(),
    )?;
    let mut runs = prefix;
    if !replacement.is_empty() {
        append_run(
            &mut runs,
            &replacement,
            &marks_at_runs(
                node_runs(&first.node).ok_or(CommandError::InvalidTarget)?,
                first_position.utf16_offset.get(),
            ),
        );
    }
    for run in suffix {
        append_run(&mut runs, &run.text, &run.marks);
    }
    let mut retained = first.node.clone();
    set_node_runs(&mut retained, runs)?;
    let mut replacement_children = expected.clone();
    replacement_children.splice(first.index..=last.index, [retained]);
    let removed_node_ids = expected[first.index + 1..last.index]
        .iter()
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    let mut mapped_removed_ids = removed_node_ids.clone();
    let mut affected_ids = Vec::new();
    for node in &expected[first.index..=last.index] {
        all_node_ids(node, &mut affected_ids);
    }
    let forward_mapping = mapping_with(AnchorTransformation::CrossBlockReplace {
        first_node_id: first.node.id.clone(),
        last_node_id: last.node.id.clone(),
        removed_node_ids: std::mem::take(&mut mapped_removed_ids),
        first_start: first_position.utf16_offset,
        first_end: first_length.into(),
        last_end: last_position.utf16_offset,
        inserted_utf16_length: utf16_length(&replacement)?,
        tombstone: TombstoneToken {
            command_id: command_id.clone(),
            slot: 0,
        },
    });
    let updates = field_updates_for_mapping(document, &forward_mapping)?;
    let preimage = build_preimage(first.node.id.clone(), affected_ids, command_id, &updates)?;
    let _ = last_length;
    structural_pair(
        document,
        first.parent_id.clone(),
        expected,
        replacement_children,
        forward_mapping,
        AnchorMapping::identity(),
        Some(preimage),
    )
}

fn selection_after_replacement(
    document: &FlowDocument,
    selection: &DirectionalSelection,
    replacement: &str,
) -> Result<DirectionalSelection, CommandError> {
    let start_position = ordered_selection_start(document, selection)?;
    let start = start_position.utf16_offset.get();
    let end = start
        .checked_add(utf16_length(replacement)?)
        .ok_or(AnchorError::OutOfRange)?;
    let position = LogicalPosition {
        node_id: start_position.node_id,
        utf16_offset: end.into(),
        affinity: Affinity::Forward,
    };
    Ok(DirectionalSelection {
        anchor: position.clone(),
        focus: position,
    })
}

fn collapsed_selection(node_id: NodeId, utf16_offset: u32) -> DirectionalSelection {
    let position = LogicalPosition {
        node_id,
        utf16_offset: utf16_offset.into(),
        affinity: Affinity::Forward,
    };
    DirectionalSelection {
        anchor: position.clone(),
        focus: position,
    }
}

fn first_text_selection(document: &FlowDocument) -> Option<DirectionalSelection> {
    fn visit(nodes: &[ContentNode]) -> Option<DirectionalSelection> {
        for node in nodes {
            if node.runs().is_some() {
                return Some(collapsed_selection(node.id.clone(), 0));
            }
            if let Some(selection) = visit(node.children()) {
                return Some(selection);
            }
        }
        None
    }

    visit(&document.content)
}

fn first_text_selection_for_node(
    nodes: &[ContentNode],
    target: &NodeId,
) -> Option<DirectionalSelection> {
    fn visit(nodes: &[ContentNode], target: &NodeId) -> Option<DirectionalSelection> {
        for node in nodes {
            if node.id == *target {
                return first_text_selection_in_node(node);
            }
            if let Some(selection) = visit(node.children(), target) {
                return Some(selection);
            }
        }
        None
    }

    visit(nodes, target)
}

fn first_text_selection_in_node(node: &ContentNode) -> Option<DirectionalSelection> {
    if node.runs().is_some() {
        return Some(collapsed_selection(node.id.clone(), 0));
    }
    for child in node.children() {
        if let Some(selection) = first_text_selection_in_node(child) {
            return Some(selection);
        }
    }
    None
}

fn ordered_selection_start(
    document: &FlowDocument,
    selection: &DirectionalSelection,
) -> Result<LogicalPosition, CommandError> {
    if selection.anchor.node_id == selection.focus.node_id {
        return Ok(
            if selection.anchor.utf16_offset <= selection.focus.utf16_offset {
                selection.anchor.clone()
            } else {
                selection.focus.clone()
            },
        );
    }
    let anchor = locate_node(&document.content, &selection.anchor.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    let focus = locate_node(&document.content, &selection.focus.node_id)
        .ok_or(CommandError::InvalidTarget)?;
    if anchor.parent_id != focus.parent_id {
        return Err(CommandError::IncompatibleStructure);
    }
    Ok(if anchor.index <= focus.index {
        selection.anchor.clone()
    } else {
        selection.focus.clone()
    })
}

fn resolve_range<'a>(
    document: &'a FlowDocument,
    range: &TextRange,
) -> Result<
    (
        &'a ContentNode,
        crate::anchor::NativeByteOffset,
        crate::anchor::NativeByteOffset,
    ),
    CommandError,
> {
    if range.start.node_id != range.end.node_id {
        return Err(CommandError::InvalidRange);
    }
    let node =
        find_node(&document.content, &range.start.node_id).ok_or(CommandError::InvalidTarget)?;
    let text = node.legacy_text().ok_or(CommandError::InvalidTarget)?;
    let start = resolve_utf16_offset(text, range.start.utf16_offset)?;
    let end = resolve_utf16_offset(text, range.end.utf16_offset)?;
    if start.get() > end.get() {
        return Err(CommandError::InvalidRange);
    }
    Ok((node, start, end))
}

fn find_node<'a>(nodes: &'a [ContentNode], target: &NodeId) -> Option<&'a ContentNode> {
    for node in nodes {
        if &node.id == target {
            return Some(node);
        }
        if let Some(found) = find_node(node.children(), target) {
            return Some(found);
        }
    }
    None
}

fn find_node_mut<'a>(nodes: &'a mut [ContentNode], target: &NodeId) -> Option<&'a mut ContentNode> {
    for node in nodes {
        if &node.id == target {
            return Some(node);
        }
        if let Some(found) = find_node_mut(node.children_mut(), target) {
            return Some(found);
        }
    }
    None
}

fn apply_operations(
    document: &mut FlowDocument,
    operations: &[Operation],
) -> Result<AnchorMapping, CommandError> {
    if operations.is_empty() {
        return Err(CommandError::HistoryConflict);
    }
    let mut mapping = AnchorMapping::identity();
    for operation in operations {
        mapping.extend(apply_operation(document, operation)?);
    }
    Ok(mapping)
}

pub(crate) fn replay_forward(
    document: &FlowDocument,
    transaction: &Transaction,
) -> Result<FlowDocument, CommandError> {
    if transaction.record_format_version != crate::RECORD_FORMAT_VERSION
        || transaction.transaction_id != transaction.command_id
        || transaction.document_id != document.document_id
        || transaction.schema_version != document.schema_version
        || transaction.base_revision != document.revision
        || transaction.new_revision
            != transaction
                .base_revision
                .checked_add(1)
                .ok_or(CommandError::HistoryConflict)?
        || transaction.before_hash != canonical_hash(&canonical_bytes(document)?)
    {
        return Err(CommandError::HistoryConflict);
    }

    let mut candidate = document.clone();
    let mapping = apply_operations(&mut candidate, &transaction.forward_operations)?;
    candidate.revision = transaction.new_revision;
    validate_document(&candidate).map_err(|_| CommandError::HistoryConflict)?;
    if mapping != transaction.anchor_mapping
        || canonical_hash(&canonical_bytes(&candidate)?) != transaction.after_hash
    {
        return Err(CommandError::HistoryConflict);
    }
    Ok(candidate)
}

pub(crate) fn replay_inverse(
    document: &FlowDocument,
    transaction: &Transaction,
) -> Result<FlowDocument, CommandError> {
    if transaction.record_format_version != crate::RECORD_FORMAT_VERSION
        || transaction.transaction_id != transaction.command_id
        || transaction.document_id != document.document_id
        || transaction.schema_version != document.schema_version
        || transaction.new_revision != document.revision
        || transaction.new_revision
            != transaction
                .base_revision
                .checked_add(1)
                .ok_or(CommandError::HistoryConflict)?
        || transaction.base_revision == 0
        || transaction.after_hash != canonical_hash(&canonical_bytes(document)?)
    {
        return Err(CommandError::HistoryConflict);
    }

    let mut candidate = document.clone();
    apply_operations(&mut candidate, &transaction.inverse_operations)?;
    candidate.revision = transaction.base_revision;
    validate_document(&candidate).map_err(|_| CommandError::HistoryConflict)?;
    if canonical_hash(&canonical_bytes(&candidate)?) != transaction.before_hash {
        return Err(CommandError::HistoryConflict);
    }
    Ok(candidate)
}

#[allow(clippy::too_many_arguments)]
fn apply_replace_children(
    document: &mut FlowDocument,
    container_id: &Option<NodeId>,
    expected_children: &[ContentNode],
    replacement_children: &[ContentNode],
    anchor_mapping: &AnchorMapping,
    field_updates: &[FieldUpdate],
    preimage: Option<&AnchorPreimage>,
    expected_assets: &[AssetDescriptor],
    replacement_assets: &[AssetDescriptor],
) -> Result<(), CommandError> {
    if let Some(preimage) = preimage {
        validate_preimage(preimage, field_updates)?;
    } else if field_updates.iter().any(|update| {
        matches!(
            update.replacement.anchor,
            FieldAnchorState::TargetDeleted { .. }
        )
    }) {
        return Err(CommandError::InvalidPreimage);
    }
    replace_container_children(
        document,
        container_id.as_ref(),
        expected_children,
        replacement_children.to_vec(),
    )?;
    if !expected_assets.is_empty() || !replacement_assets.is_empty() {
        if document.assets != expected_assets {
            return Err(CommandError::HistoryConflict);
        }
        document.assets = replacement_assets.to_vec();
    }

    let mut indexes = BTreeSet::new();
    for update in field_updates {
        let index = usize::try_from(update.index).map_err(|_| CommandError::HistoryConflict)?;
        if !indexes.insert(index) || document.fields.get(index) != Some(update.expected.as_ref()) {
            return Err(CommandError::HistoryConflict);
        }
    }
    for update in field_updates {
        let index = usize::try_from(update.index).map_err(|_| CommandError::HistoryConflict)?;
        document.fields[index] = update.replacement.as_ref().clone();
    }

    if let Some(preimage) = preimage {
        for owner in &preimage.owners {
            let AnchorOwner::Field(field_id) = &owner.owner else {
                continue;
            };
            let Some(update) = field_updates
                .iter()
                .find(|update| update.expected.id == *field_id)
                .or_else(|| {
                    field_updates
                        .iter()
                        .find(|update| update.replacement.id == *field_id)
                })
            else {
                return Err(CommandError::InvalidPreimage);
            };
            if update.expected.anchor.original() != &owner.position
                && update.replacement.anchor.original() != &owner.position
            {
                return Err(CommandError::InvalidPreimage);
            }
        }
    }
    let _ = anchor_mapping;
    Ok(())
}

fn apply_operation(
    document: &mut FlowDocument,
    operation: &Operation,
) -> Result<AnchorMapping, CommandError> {
    let mut mapping = AnchorMapping::identity();
    match operation {
        Operation::CreateDocument | Operation::DeleteDocument => {
            return Err(CommandError::HistoryConflict);
        }
        Operation::ReplaceText {
            range,
            expected_text,
            replacement,
        } => {
            let (_, start, end) = resolve_range(document, range)?;
            let node = find_node_mut(&mut document.content, &range.start.node_id)
                .ok_or(CommandError::InvalidTarget)?;
            let mut text = node
                .legacy_text()
                .ok_or(CommandError::InvalidTarget)?
                .to_owned();
            if text[start.get()..end.get()] != *expected_text {
                return Err(CommandError::HistoryConflict);
            }
            let removed_utf16_length = range
                .end
                .utf16_offset
                .get()
                .checked_sub(range.start.utf16_offset.get())
                .ok_or(CommandError::InvalidRange)?;
            let inserted_utf16_length = utf16_length(replacement)?;
            text.replace_range(start.get()..end.get(), replacement);
            node.set_plain_text(text)?;
            let transformation = AnchorTransformation::TextEdit {
                node_id: range.start.node_id.clone(),
                start: range.start.utf16_offset,
                removed_utf16_length,
                inserted_utf16_length,
            };
            map_field_anchors(document, &transformation)?;
            mapping.push(transformation);
        }
        Operation::SetNodeStyle {
            node_id,
            expected_style_id,
            style_id,
        } => {
            let node = document
                .content
                .iter_mut()
                .find(|node| node.id == *node_id)
                .ok_or(CommandError::InvalidTarget)?;
            if node.style_id != *expected_style_id {
                return Err(CommandError::HistoryConflict);
            }
            node.style_id.clone_from(style_id);
        }
        Operation::InsertNode { index, node } => {
            let index = usize::try_from(*index).map_err(|_| CommandError::InvalidRange)?;
            if index > document.content.len()
                || document.content.iter().any(|current| current.id == node.id)
            {
                return Err(CommandError::HistoryConflict);
            }
            document.content.insert(index, node.clone());
        }
        Operation::DeleteNode { index, node } => {
            let index = usize::try_from(*index).map_err(|_| CommandError::InvalidRange)?;
            if document.content.get(index) != Some(node) {
                return Err(CommandError::HistoryConflict);
            }
            let transformation = AnchorTransformation::NodeInvalidated {
                node_id: node.id.clone(),
            };
            map_field_anchors(document, &transformation)?;
            document.content.remove(index);
            mapping.push(transformation);
        }
        Operation::ReplaceChildren {
            container_id,
            expected_children,
            replacement_children,
            anchor_mapping,
            field_updates,
            preimage,
            expected_assets,
            replacement_assets,
        } => {
            apply_replace_children(
                document,
                container_id,
                expected_children,
                replacement_children,
                anchor_mapping,
                field_updates,
                preimage.as_ref(),
                expected_assets,
                replacement_assets,
            )?;
            mapping.extend(anchor_mapping.clone());
        }
        Operation::SetField {
            index,
            expected_field,
            field,
        } => {
            let index = usize::try_from(*index).map_err(|_| CommandError::InvalidRange)?;
            if document.fields.get(index) != Some(expected_field.as_ref())
                || expected_field.id != field.id
            {
                return Err(CommandError::HistoryConflict);
            }
            document.fields[index] = field.as_ref().clone();
        }
        Operation::InsertField { index, field } => {
            let index = usize::try_from(*index).map_err(|_| CommandError::InvalidRange)?;
            if index > document.fields.len()
                || document.fields.iter().any(|current| current.id == field.id)
            {
                return Err(CommandError::HistoryConflict);
            }
            document.fields.insert(index, field.as_ref().clone());
        }
        Operation::RemoveField { index, field } => {
            let index = usize::try_from(*index).map_err(|_| CommandError::InvalidRange)?;
            if document.fields.get(index) != Some(field.as_ref()) {
                return Err(CommandError::HistoryConflict);
            }
            document.fields.remove(index);
        }
    }
    Ok(mapping)
}

fn map_field_anchors(
    document: &mut FlowDocument,
    transformation: &AnchorTransformation,
) -> Result<(), CommandError> {
    let mapping = AnchorMapping {
        transformations: vec![transformation.clone()],
    };
    for field in &mut document.fields {
        let mapped = match mapping.map(field.anchor.original()) {
            AnchorMapResult::Mapped(position) => position,
            AnchorMapResult::Deleted { tombstone } => {
                if !matches!(field.anchor, FieldAnchorState::TargetDeleted { .. }) {
                    let original = field.anchor.original().clone();
                    field.anchor = FieldAnchorState::TargetDeleted {
                        original,
                        tombstone,
                    };
                }
                continue;
            }
            AnchorMapResult::Invalid(_) => return Err(CommandError::AnchorInvalidated),
        };
        match &mut field.anchor {
            FieldAnchorState::GraphemeSafe { original } => *original = mapped,
            FieldAnchorState::LegacyInvalid { original, .. }
            | FieldAnchorState::TargetDeleted { original, .. } => {
                if *original != mapped {
                    return Err(CommandError::AnchorInvalidated);
                }
            }
        }
    }
    Ok(())
}

fn finalize_document(
    state: &EditorState,
    mut candidate: FlowDocument,
) -> Result<(FlowDocument, String, String), CommandError> {
    candidate.revision = state
        .document
        .revision
        .checked_add(1)
        .ok_or(CommandError::BrokenInvariant)?;
    validate_document(&candidate)?;
    let bytes = canonical_bytes(&candidate)?;
    let after_hash = canonical_hash(&bytes);
    Ok((candidate, state.canonical_hash.clone(), after_hash))
}

#[allow(clippy::too_many_arguments)]
fn build_transaction(
    candidate: &FlowDocument,
    command: &Command,
    before_hash: String,
    after_hash: String,
    forward_operations: Vec<Operation>,
    inverse_operations: Vec<Operation>,
    anchor_mapping: AnchorMapping,
    history_effect: HistoryEffect,
) -> Result<Transaction, CommandError> {
    let transaction = Transaction {
        record_format_version: crate::RECORD_FORMAT_VERSION,
        transaction_id: command.command_id.clone(),
        document_id: candidate.document_id.clone(),
        schema_version: candidate.schema_version,
        command_id: command.command_id.clone(),
        base_revision: command.base_revision,
        new_revision: candidate.revision,
        command_type: command_type(&command.kind).to_owned(),
        modality: command.modality.clone(),
        issued_at: command.issued_at.clone(),
        before_hash,
        after_hash,
        forward_operations,
        inverse_operations,
        anchor_mapping,
        history_effect,
    };
    let bytes = serde_json::to_vec(&transaction).map_err(|_| SchemaError::serialization())?;
    DocumentLimits::V1.check_transaction(
        transaction.forward_operations.len() + transaction.inverse_operations.len(),
        bytes.len(),
    )?;
    Ok(transaction)
}

fn command_type(kind: &CommandKind) -> &'static str {
    match kind {
        CommandKind::InsertText { .. } => "insertText",
        CommandKind::ReplaceText { .. } => "replaceText",
        CommandKind::ReplaceSelection { .. } => "replaceSelection",
        CommandKind::DeleteText { .. } => "deleteText",
        CommandKind::SetNodeStyle { .. } => "setNodeStyle",
        CommandKind::InsertNode { .. } => "insertNode",
        CommandKind::DeleteNode { .. } => "deleteNode",
        CommandKind::SplitTextBlock { .. } => "splitTextBlock",
        CommandKind::MergeTextBlocks { .. } => "mergeTextBlocks",
        CommandKind::DeleteSubtree { .. } => "deleteSubtree",
        CommandKind::SetInlineMarks { .. } => "setInlineMarks",
        CommandKind::SetInlineMark { .. } => "setInlineMark",
        CommandKind::SetBlockAttributes { .. } => "setBlockAttributes",
        CommandKind::SetBlockStyle { .. } => "setBlockStyle",
        CommandKind::SetListKind { .. } => "setListKind",
        CommandKind::ContinueListItem { .. } => "continueListItem",
        CommandKind::ExitListItem { .. } => "exitListItem",
        CommandKind::IndentListItem { .. } => "indentListItem",
        CommandKind::OutdentListItem { .. } => "outdentListItem",
        CommandKind::InsertPageBreak { .. } => "insertPageBreak",
        CommandKind::RemovePageBreak { .. } => "removePageBreak",
        CommandKind::InsertTable { .. } => "insertTable",
        CommandKind::InsertImage { .. } => "insertImage",
        CommandKind::ReplaceImage { .. } => "replaceImage",
        CommandKind::SetImageAccessibility { .. } => "setImageAccessibility",
        CommandKind::RemoveImage { .. } => "removeImage",
        CommandKind::AddTableRow { .. } => "addTableRow",
        CommandKind::RemoveTableRow { .. } => "removeTableRow",
        CommandKind::AddTableColumn { .. } => "addTableColumn",
        CommandKind::RemoveTableColumn { .. } => "removeTableColumn",
        CommandKind::SetTableHeaderRow { .. } => "setTableHeaderRow",
        CommandKind::RemoveTable { .. } => "removeTable",
        CommandKind::InsertField { .. } => "insertField",
        CommandKind::SetField { .. } => "setField",
        CommandKind::RemoveField { .. } => "removeField",
        CommandKind::Batch { .. } => "batch",
        CommandKind::Undo => "undo",
        CommandKind::Redo => "redo",
    }
}

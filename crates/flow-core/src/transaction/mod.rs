//! Typed commands, immutable transactions, and deterministic undo/redo.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    anchor::{
        AnchorError, AnchorMapResult, AnchorMapping, AnchorTransformation, resolve_utf16_offset,
        utf16_length,
    },
    canonical::{canonical_bytes, canonical_hash},
    model::{
        Affinity, CommandId, ContentNode, ContentNodeKind, FieldDescriptor, FieldId, FlowDocument,
        LogicalPosition, NodeId, StyleId,
    },
    schema::{DocumentLimits, SchemaError, validate_document},
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
    SetField {
        field_id: FieldId,
        field: FieldDescriptor,
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
    SetField {
        field_id: FieldId,
        field: FieldDescriptor,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextRange {
    pub start: LogicalPosition,
    pub end: LogicalPosition,
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
    SetField {
        index: u32,
        expected_field: Box<FieldDescriptor>,
        field: Box<FieldDescriptor>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transaction {
    pub transaction_id: CommandId,
    pub document_id: crate::model::DocumentId,
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
        for entry in &self.entries {
            if !seen.contains(entry.command_id.as_str())
                || entry.forward_operations.is_empty()
                || entry.inverse_operations.is_empty()
            {
                return Err(CommandError::HistoryConflict);
            }
            DocumentLimits::V1.check_transaction(
                entry.forward_operations.len() + entry.inverse_operations.len(),
                serde_json::to_vec(entry)
                    .map_err(|_| SchemaError::serialization())?
                    .len(),
            )?;
        }
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
    #[error("The command would invalidate an anchor without an explicit mapping")]
    AnchorInvalidated,
    #[error("The command would violate a document or operation invariant")]
    BrokenInvariant,
    #[error("There is no committed mutation to undo")]
    UndoEmpty,
    #[error("There is no reverted mutation to redo")]
    RedoEmpty,
    #[error("The history cursor or stored operation set is inconsistent")]
    HistoryConflict,
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
            Self::AnchorInvalidated => "FLOW_ANCHOR_INVALIDATED",
            Self::BrokenInvariant => "FLOW_BROKEN_INVARIANT",
            Self::UndoEmpty => "FLOW_UNDO_EMPTY",
            Self::RedoEmpty => "FLOW_REDO_EMPTY",
            Self::HistoryConflict => "FLOW_HISTORY_CONFLICT",
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
    for mutation in &mutations {
        let (operation, inverse_operation) = derive_operation(&candidate, mutation)?;
        let operation_mapping = apply_operation(&mut candidate, &operation)?;
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
        CommandKind::SetField { field_id, field } => Ok(vec![Mutation::SetField {
            field_id: field_id.clone(),
            field: field.clone(),
        }]),
        CommandKind::Undo | CommandKind::Redo => Err(CommandError::BrokenInvariant),
    }
}

fn derive_operation(
    document: &FlowDocument,
    mutation: &Mutation,
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
        Mutation::SetField { field_id, field } => {
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
    }
}

fn derive_text_operation(
    document: &FlowDocument,
    range: TextRange,
    replacement: String,
) -> Result<(Operation, Operation), CommandError> {
    let (node, start, end) = resolve_range(document, &range)?;
    let expected_text = node.text[start.get()..end.get()].to_owned();
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
    let node = document
        .content
        .iter()
        .find(|node| node.id == range.start.node_id && node.kind == ContentNodeKind::Paragraph)
        .ok_or(CommandError::InvalidTarget)?;
    let start = resolve_utf16_offset(&node.text, range.start.utf16_offset)?;
    let end = resolve_utf16_offset(&node.text, range.end.utf16_offset)?;
    if start.get() > end.get() {
        return Err(CommandError::InvalidRange);
    }
    Ok((node, start, end))
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
            let node_index = document
                .content
                .iter()
                .position(|node| node.id == range.start.node_id)
                .ok_or(CommandError::InvalidTarget)?;
            if document.content[node_index].text[start.get()..end.get()] != *expected_text {
                return Err(CommandError::HistoryConflict);
            }
            let removed_utf16_length = range
                .end
                .utf16_offset
                .get()
                .checked_sub(range.start.utf16_offset.get())
                .ok_or(CommandError::InvalidRange)?;
            let inserted_utf16_length = utf16_length(replacement)?;
            document.content[node_index]
                .text
                .replace_range(start.get()..end.get(), replacement);
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
        field.anchor = match mapping.map(&field.anchor) {
            AnchorMapResult::Mapped(position) => position,
            AnchorMapResult::Invalid(_) => return Err(CommandError::AnchorInvalidated),
        };
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
        transaction_id: command.command_id.clone(),
        document_id: candidate.document_id.clone(),
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
        CommandKind::DeleteText { .. } => "deleteText",
        CommandKind::SetNodeStyle { .. } => "setNodeStyle",
        CommandKind::InsertNode { .. } => "insertNode",
        CommandKind::DeleteNode { .. } => "deleteNode",
        CommandKind::SetField { .. } => "setField",
        CommandKind::Batch { .. } => "batch",
        CommandKind::Undo => "undo",
        CommandKind::Redo => "redo",
    }
}

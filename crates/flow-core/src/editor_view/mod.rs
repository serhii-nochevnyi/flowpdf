//! Rust-owned, noncanonical editor selection and formatting projections.
//!
//! The editor session is deliberately separate from [`FlowDocument`]. It is a
//! short-lived view model bound to one accepted document revision; browser
//! coordinates, DOM paths, and mutable editor handles never cross this module.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::anchor::{EditorPositionError, NodePositionMap};
use crate::model::{
    Affinity, ContentNode, DocumentId, FlowDocument, FontFamily, LogicalPosition, MarkSet, NodeId,
    RunLanguage,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectionalSelection {
    pub anchor: LogicalPosition,
    pub focus: LogicalPosition,
}

impl DirectionalSelection {
    #[must_use]
    pub fn collapsed(&self) -> bool {
        self.anchor.node_id == self.focus.node_id
            && self.anchor.utf16_offset == self.focus.utf16_offset
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FormattingState {
    On,
    #[default]
    Off,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormattingProjectionDto {
    pub bold: FormattingState,
    pub italic: FormattingState,
    pub underline: FormattingState,
    pub font_family: Option<FontFamily>,
    pub font_size_millipoints: Option<u32>,
    pub color: Option<[u8; 3]>,
    pub language: Option<RunLanguage>,
}

impl Default for FormattingProjectionDto {
    fn default() -> Self {
        Self {
            bold: FormattingState::Off,
            italic: FormattingState::Off,
            underline: FormattingState::Off,
            font_family: None,
            font_size_millipoints: None,
            color: None,
            language: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EditorCapability {
    SetSelection,
    SetPendingMarks,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityDto {
    pub name: EditorCapability,
    pub enabled: bool,
    pub reason_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorSessionState {
    pub document_id: DocumentId,
    pub revision: u32,
    pub session_generation: u64,
    pub selection: DirectionalSelection,
    pub pending_marks: MarkSet,
    pub formatting: FormattingProjectionDto,
    pub capabilities: Vec<CapabilityDto>,
}

impl EditorSessionState {
    pub fn from_document(document: &FlowDocument) -> Result<Self, EditorSessionError> {
        let (selection, pending_marks) = first_position(document)?;
        let formatting = formatting_for_selection(document, &selection)?;
        Ok(Self {
            document_id: document.document_id.clone(),
            revision: document.revision,
            session_generation: 0,
            selection,
            pending_marks,
            formatting,
            capabilities: default_capabilities(),
        })
    }

    pub fn validate_against(&self, document: &FlowDocument) -> Result<(), EditorSessionError> {
        if self.document_id != document.document_id {
            return Err(EditorSessionError::DocumentMismatch);
        }
        if self.revision != document.revision {
            return Err(EditorSessionError::StaleDocumentRevision);
        }
        validate_selection(document, &self.selection)?;
        validate_pending_marks(&self.pending_marks)?;
        let expected_formatting = formatting_for_selection(document, &self.selection)?;
        if self.formatting != expected_formatting {
            return Err(EditorSessionError::ProjectionMismatch);
        }
        if self.capabilities != default_capabilities() {
            return Err(EditorSessionError::ProjectionMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub fn view(&self) -> EditorViewDto {
        EditorViewDto {
            document_id: self.document_id.clone(),
            revision: self.revision,
            session_generation: self.session_generation,
            selection: self.selection.clone(),
            pending_marks: self.pending_marks.clone(),
            formatting: self.formatting.clone(),
            capabilities: self.capabilities.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorViewDto {
    pub document_id: DocumentId,
    pub revision: u32,
    pub session_generation: u64,
    pub selection: DirectionalSelection,
    pub pending_marks: MarkSet,
    pub formatting: FormattingProjectionDto,
    pub capabilities: Vec<CapabilityDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EditorSessionAction {
    SetSelection {
        selection: DirectionalSelection,
    },
    SetPendingMarks {
        marks: MarkSet,
    },
    SetSelectionAndPendingMarks {
        selection: DirectionalSelection,
        marks: MarkSet,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorSessionRequest {
    pub canonical_json: String,
    pub session: EditorSessionState,
    pub action: EditorSessionAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorViewRequest {
    pub canonical_json: String,
    pub session: EditorSessionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorSessionResponse {
    pub session: EditorSessionState,
    pub view: EditorViewDto,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EditorSessionError {
    #[error(transparent)]
    Position(#[from] EditorPositionError),
    #[error("The editor session targets a different document")]
    DocumentMismatch,
    #[error("The editor session targets a stale document revision")]
    StaleDocumentRevision,
    #[error("The editor session has no positionable document node")]
    NoPositionableNode,
    #[error("The session projection is not the Rust-derived projection")]
    ProjectionMismatch,
    #[error("Pending typing marks require a collapsed selection")]
    PendingMarksRequireCollapsedSelection,
    #[error("The pending typing marks are not valid for new authoring")]
    InvalidPendingMarks,
    #[error("The editor session generation overflowed")]
    GenerationOverflow,
    #[error("The editor session references an unknown node")]
    UnknownNode,
}

impl EditorSessionError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Position(error) => error.code(),
            Self::DocumentMismatch => "FLOW_EDITOR_DOCUMENT_MISMATCH",
            Self::StaleDocumentRevision => "FLOW_STALE_EDITOR_SESSION",
            Self::NoPositionableNode => "FLOW_NO_EDITOR_POSITION",
            Self::ProjectionMismatch => "FLOW_EDITOR_PROJECTION_MISMATCH",
            Self::PendingMarksRequireCollapsedSelection => {
                "FLOW_PENDING_MARKS_REQUIRES_COLLAPSED_SELECTION"
            }
            Self::InvalidPendingMarks => "FLOW_INVALID_PENDING_MARKS",
            Self::GenerationOverflow => "FLOW_EDITOR_SESSION_GENERATION_OVERFLOW",
            Self::UnknownNode => "FLOW_UNKNOWN_POSITION_NODE",
        }
    }
}

fn default_capabilities() -> Vec<CapabilityDto> {
    vec![
        CapabilityDto {
            name: EditorCapability::SetSelection,
            enabled: true,
            reason_key: None,
        },
        CapabilityDto {
            name: EditorCapability::SetPendingMarks,
            enabled: true,
            reason_key: None,
        },
    ]
}

fn first_position(
    document: &FlowDocument,
) -> Result<(DirectionalSelection, MarkSet), EditorSessionError> {
    fn visit(
        nodes: &[ContentNode],
        revision: u32,
    ) -> Result<Option<(DirectionalSelection, MarkSet)>, EditorSessionError> {
        for node in nodes {
            match NodePositionMap::new(node, revision) {
                Ok(map) => {
                    let (offset, affinity, marks) = match node.runs() {
                        Some(runs) => (0, Affinity::Forward, marks_at_offset(runs, 0)),
                        None => (0, Affinity::Forward, MarkSet::default()),
                    };
                    let position = LogicalPosition {
                        node_id: node.id.clone(),
                        utf16_offset: offset.into(),
                        affinity,
                    };
                    map.validate(&position, revision)?;
                    let selection = DirectionalSelection {
                        anchor: position.clone(),
                        focus: position,
                    };
                    return Ok(Some((selection, marks)));
                }
                Err(EditorPositionError::UnsupportedNodeKind) => {
                    if let Some(found) = visit(node.children(), revision)? {
                        return Ok(Some(found));
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(None)
    }

    visit(&document.content, document.revision)?.ok_or(EditorSessionError::NoPositionableNode)
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

fn validate_selection(
    document: &FlowDocument,
    selection: &DirectionalSelection,
) -> Result<(), EditorSessionError> {
    for position in [&selection.anchor, &selection.focus] {
        let node = find_node(&document.content, &position.node_id)
            .ok_or(EditorSessionError::UnknownNode)?;
        let map = NodePositionMap::new(node, document.revision)?;
        map.validate(position, document.revision)?;
    }
    Ok(())
}

fn validate_pending_marks(marks: &MarkSet) -> Result<(), EditorSessionError> {
    if marks
        .font_size_millipoints
        .is_some_and(|size| !(6_000..=288_000).contains(&size))
        || matches!(marks.font_family, Some(FontFamily::LegacyUnknown { .. }))
    {
        return Err(EditorSessionError::InvalidPendingMarks);
    }
    Ok(())
}

fn marks_at_offset(runs: &[crate::model::InlineRun], offset: u32) -> MarkSet {
    if runs.is_empty() {
        return MarkSet::default();
    }
    let mut start = 0_u32;
    for (index, run) in runs.iter().enumerate() {
        let length = run.text.encode_utf16().count() as u32;
        let end = start.saturating_add(length);
        if offset < end || (offset == end && index + 1 == runs.len()) {
            return run.marks.clone();
        }
        start = end;
    }
    runs.last()
        .map_or_else(MarkSet::default, |run| run.marks.clone())
}

fn formatting_for_selection(
    document: &FlowDocument,
    selection: &DirectionalSelection,
) -> Result<FormattingProjectionDto, EditorSessionError> {
    validate_selection(document, selection)?;

    if selection.anchor.node_id != selection.focus.node_id {
        return Ok(FormattingProjectionDto {
            bold: FormattingState::Mixed,
            italic: FormattingState::Mixed,
            underline: FormattingState::Mixed,
            ..FormattingProjectionDto::default()
        });
    }

    let node = find_node(&document.content, &selection.anchor.node_id)
        .ok_or(EditorSessionError::UnknownNode)?;
    let Some(runs) = node.runs() else {
        return Ok(FormattingProjectionDto::default());
    };

    let start = selection
        .anchor
        .utf16_offset
        .get()
        .min(selection.focus.utf16_offset.get());
    let end = selection
        .anchor
        .utf16_offset
        .get()
        .max(selection.focus.utf16_offset.get());
    if start == end {
        return Ok(formatting_from_marks(&marks_at_offset(runs, start)));
    }

    let mut selected = Vec::new();
    let mut run_start = 0_u32;
    for run in runs {
        let run_end = run_start.saturating_add(run.text.encode_utf16().count() as u32);
        if run_start < end && run_end > start {
            selected.push(&run.marks);
        }
        run_start = run_end;
    }
    if selected.is_empty() {
        return Ok(FormattingProjectionDto::default());
    }

    Ok(FormattingProjectionDto {
        bold: bool_state(selected.iter().map(|marks| marks.bold)),
        italic: bool_state(selected.iter().map(|marks| marks.italic)),
        underline: bool_state(selected.iter().map(|marks| marks.underline)),
        font_family: uniform_value(selected.iter().map(|marks| marks.font_family.clone())),
        font_size_millipoints: uniform_value(
            selected.iter().map(|marks| marks.font_size_millipoints),
        ),
        color: uniform_value(selected.iter().map(|marks| marks.color)),
        language: uniform_value(selected.iter().map(|marks| marks.language.clone())),
    })
}

fn formatting_from_marks(marks: &MarkSet) -> FormattingProjectionDto {
    FormattingProjectionDto {
        bold: bool_state([marks.bold]),
        italic: bool_state([marks.italic]),
        underline: bool_state([marks.underline]),
        font_family: marks.font_family.clone(),
        font_size_millipoints: marks.font_size_millipoints,
        color: marks.color,
        language: marks.language.clone(),
    }
}

fn bool_state(values: impl IntoIterator<Item = bool>) -> FormattingState {
    let values = values.into_iter().collect::<Vec<_>>();
    if values.iter().all(|value| *value) {
        FormattingState::On
    } else if values.iter().all(|value| !*value) {
        FormattingState::Off
    } else {
        FormattingState::Mixed
    }
}

fn uniform_value<T: Clone + PartialEq>(values: impl IntoIterator<Item = Option<T>>) -> Option<T> {
    let mut values = values.into_iter();
    let first = values.next()?;
    if values.all(|value| value == first) {
        first
    } else {
        None
    }
}

pub(crate) fn apply_action(
    mut state: EditorSessionState,
    document: &FlowDocument,
    action: EditorSessionAction,
) -> Result<EditorSessionResponse, EditorSessionError> {
    state.validate_against(document)?;

    match action {
        EditorSessionAction::SetSelection { selection } => {
            validate_selection(document, &selection)?;
            state.pending_marks = if selection.collapsed() {
                let node = find_node(&document.content, &selection.anchor.node_id)
                    .ok_or(EditorSessionError::UnknownNode)?;
                node.runs().map_or_else(MarkSet::default, |runs| {
                    marks_at_offset(runs, selection.anchor.utf16_offset.get())
                })
            } else {
                MarkSet::default()
            };
            state.selection = selection;
        }
        EditorSessionAction::SetPendingMarks { marks } => {
            if !state.selection.collapsed() {
                return Err(EditorSessionError::PendingMarksRequireCollapsedSelection);
            }
            validate_pending_marks(&marks)?;
            state.pending_marks = marks;
        }
        EditorSessionAction::SetSelectionAndPendingMarks { selection, marks } => {
            if !selection.collapsed() {
                return Err(EditorSessionError::PendingMarksRequireCollapsedSelection);
            }
            validate_selection(document, &selection)?;
            validate_pending_marks(&marks)?;
            state.selection = selection;
            state.pending_marks = marks;
        }
    }

    state.formatting = formatting_for_selection(document, &state.selection)?;
    state.session_generation = state
        .session_generation
        .checked_add(1)
        .ok_or(EditorSessionError::GenerationOverflow)?;
    let view = state.view();
    Ok(EditorSessionResponse {
        session: state,
        view,
    })
}

pub(crate) fn project_view(
    document: &FlowDocument,
    session: &EditorSessionState,
) -> Result<EditorViewDto, EditorSessionError> {
    session.validate_against(document)?;
    Ok(session.view())
}

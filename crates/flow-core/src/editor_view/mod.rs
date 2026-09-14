//! Rust-owned, noncanonical editor selection and formatting projections.
//!
//! The editor session is deliberately separate from [`FlowDocument`]. It is a
//! short-lived view model bound to one accepted document revision; browser
//! coordinates, DOM paths, and mutable editor handles never cross this module.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::anchor::{EditorPositionError, GraphemeBoundaryMap, NodePositionMap};
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
    SplitTextBlock,
    MergeTextBlocks,
    DeleteSubtree,
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
        let capabilities = capabilities_for(document, &selection);
        Ok(Self {
            document_id: document.document_id.clone(),
            revision: document.revision,
            session_generation: 0,
            selection,
            pending_marks,
            formatting,
            capabilities,
        })
    }

    pub fn from_document_with_selection(
        document: &FlowDocument,
        selection: DirectionalSelection,
    ) -> Result<Self, EditorSessionError> {
        let mut state = Self::from_document(document)?;
        validate_selection(document, &selection)?;
        state.pending_marks = pending_marks_for_selection(document, &selection)?;
        state.selection = selection;
        state.formatting = formatting_for_selection(document, &state.selection)?;
        state.capabilities = capabilities_for(document, &state.selection);
        Ok(state)
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
        if self.capabilities != capabilities_for(document, &self.selection) {
            return Err(EditorSessionError::ProjectionMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub fn view(&self) -> EditorViewDto {
        self.view_with_document(EditorDocumentViewDto::empty(
            self.document_id.clone(),
            self.revision,
        ))
    }

    #[must_use]
    pub fn view_for_document(&self, document: &FlowDocument) -> EditorViewDto {
        self.view_with_document(EditorDocumentViewDto::from_document(document))
    }

    fn view_with_document(&self, document: EditorDocumentViewDto) -> EditorViewDto {
        EditorViewDto {
            document_id: self.document_id.clone(),
            revision: self.revision,
            session_generation: self.session_generation,
            selection: self.selection.clone(),
            pending_marks: self.pending_marks.clone(),
            formatting: self.formatting.clone(),
            capabilities: self.capabilities.clone(),
            document,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorDocumentViewDto {
    pub document_id: DocumentId,
    pub revision: u32,
    pub blocks: Vec<EditorBlockViewDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorTextSpanDto {
    pub start_utf16: u32,
    pub end_utf16: u32,
}

impl EditorDocumentViewDto {
    fn empty(document_id: DocumentId, revision: u32) -> Self {
        Self {
            document_id,
            revision,
            blocks: Vec::new(),
        }
    }

    fn from_document(document: &FlowDocument) -> Self {
        Self {
            document_id: document.document_id.clone(),
            revision: document.revision,
            blocks: document
                .content
                .iter()
                .map(EditorBlockViewDto::from_node)
                .collect(),
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
pub enum EditorBlockViewDto {
    Paragraph {
        node_id: NodeId,
        text: String,
        spans: Vec<EditorTextSpanDto>,
    },
    Heading {
        node_id: NodeId,
        level: u8,
        text: String,
        spans: Vec<EditorTextSpanDto>,
    },
    Atomic {
        node_id: NodeId,
        node_kind: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        children: Vec<EditorBlockViewDto>,
    },
}

impl EditorBlockViewDto {
    fn from_node(node: &ContentNode) -> Self {
        match &node.body {
            crate::model::BlockKind::Paragraph { .. } => Self::Paragraph {
                node_id: node.id.clone(),
                text: node.text(),
                spans: text_spans(&node.text()),
            },
            crate::model::BlockKind::Heading { level, .. } => Self::Heading {
                node_id: node.id.clone(),
                level: *level,
                text: node.text(),
                spans: text_spans(&node.text()),
            },
            body => Self::Atomic {
                node_id: node.id.clone(),
                node_kind: body_kind(body).to_owned(),
                children: node.children().iter().map(Self::from_node).collect(),
            },
        }
    }
}

fn text_spans(text: &str) -> Vec<EditorTextSpanDto> {
    let boundary_map = GraphemeBoundaryMap::new(text)
        .expect("validated editor text must fit the grapheme projection budget");
    let boundaries = boundary_map.boundaries();

    if boundaries.len() < 2 {
        return vec![EditorTextSpanDto {
            start_utf16: 0,
            end_utf16: 0,
        }];
    }

    boundaries
        .windows(2)
        .map(|window| EditorTextSpanDto {
            start_utf16: window[0].utf16_offset.get(),
            end_utf16: window[1].utf16_offset.get(),
        })
        .collect()
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
    pub document: EditorDocumentViewDto,
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

fn capabilities_for(
    document: &FlowDocument,
    selection: &DirectionalSelection,
) -> Vec<CapabilityDto> {
    let split_enabled = selection.collapsed()
        && find_node(&document.content, &selection.anchor.node_id)
            .is_some_and(|node| node.runs().is_some());
    let merge_enabled = selection.collapsed()
        && find_node(&document.content, &selection.anchor.node_id).is_some_and(|node| {
            has_compatible_text_neighbor(document, node, selection.anchor.utf16_offset.get())
        });
    let delete_enabled = find_node(&document.content, &selection.focus.node_id).is_some();
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
        CapabilityDto {
            name: EditorCapability::SplitTextBlock,
            enabled: split_enabled,
            reason_key: (!split_enabled).then_some("structuralSelectionRequired".to_owned()),
        },
        CapabilityDto {
            name: EditorCapability::MergeTextBlocks,
            enabled: merge_enabled,
            reason_key: (!merge_enabled).then_some("incompatibleStructure".to_owned()),
        },
        CapabilityDto {
            name: EditorCapability::DeleteSubtree,
            enabled: delete_enabled,
            reason_key: (!delete_enabled).then_some("unknownNode".to_owned()),
        },
    ]
}

fn has_compatible_text_neighbor(document: &FlowDocument, node: &ContentNode, offset: u32) -> bool {
    let Some(location) = locate_node(&document.content, &node.id) else {
        return false;
    };
    let siblings = match location.parent_id.as_ref() {
        None => &document.content,
        Some(parent_id) => find_node(&document.content, parent_id)
            .map(ContentNode::children)
            .unwrap_or(&[]),
    };
    let text_length = node.text().encode_utf16().count() as u32;
    let candidate_indices = if offset == 0 && offset == text_length {
        vec![location.index.checked_sub(1), Some(location.index + 1)]
    } else if offset == 0 {
        vec![location.index.checked_sub(1)]
    } else if offset == text_length {
        vec![Some(location.index + 1)]
    } else {
        Vec::new()
    };
    candidate_indices
        .into_iter()
        .flatten()
        .filter_map(|index| siblings.get(index))
        .any(|neighbor| compatible_text_nodes(node, neighbor))
}

#[derive(Debug, Clone)]
struct NodeLocation {
    parent_id: Option<NodeId>,
    index: usize,
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

fn compatible_text_nodes(left: &ContentNode, right: &ContentNode) -> bool {
    if left.style_id != right.style_id {
        return false;
    }
    match (&left.body, &right.body) {
        (
            crate::model::BlockKind::Paragraph {
                attrs: left_attrs, ..
            },
            crate::model::BlockKind::Paragraph {
                attrs: right_attrs, ..
            },
        ) => left_attrs == right_attrs,
        (
            crate::model::BlockKind::Heading {
                level: left_level,
                attrs: left_attrs,
                ..
            },
            crate::model::BlockKind::Heading {
                level: right_level,
                attrs: right_attrs,
                ..
            },
        ) => left_level == right_level && left_attrs == right_attrs,
        _ => false,
    }
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

fn body_kind(body: &crate::model::BlockKind) -> &'static str {
    match body {
        crate::model::BlockKind::Paragraph { .. } => "paragraph",
        crate::model::BlockKind::Heading { .. } => "heading",
        crate::model::BlockKind::OrderedList { .. } => "orderedList",
        crate::model::BlockKind::UnorderedList { .. } => "unorderedList",
        crate::model::BlockKind::ListItem { .. } => "listItem",
        crate::model::BlockKind::Image { .. } => "image",
        crate::model::BlockKind::Table { .. } => "table",
        crate::model::BlockKind::TableRow { .. } => "tableRow",
        crate::model::BlockKind::TableCell { .. } => "tableCell",
        crate::model::BlockKind::PageBreak => "pageBreak",
    }
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

fn pending_marks_for_selection(
    document: &FlowDocument,
    selection: &DirectionalSelection,
) -> Result<MarkSet, EditorSessionError> {
    if !selection.collapsed() {
        return Ok(MarkSet::default());
    }
    let node = find_node(&document.content, &selection.anchor.node_id)
        .ok_or(EditorSessionError::UnknownNode)?;
    Ok(node.runs().map_or_else(MarkSet::default, |runs| {
        marks_at_offset(runs, selection.anchor.utf16_offset.get())
    }))
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
            state.pending_marks = pending_marks_for_selection(document, &selection)?;
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
    state.capabilities = capabilities_for(document, &state.selection);
    state.session_generation = state
        .session_generation
        .checked_add(1)
        .ok_or(EditorSessionError::GenerationOverflow)?;
    let view = state.view_for_document(document);
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
    Ok(session.view_for_document(document))
}

//! Rust-owned, noncanonical editor selection and formatting projections.
//!
//! The editor session is deliberately separate from [`FlowDocument`]. It is a
//! short-lived view model bound to one accepted document revision; browser
//! coordinates, DOM paths, and mutable editor handles never cross this module.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

use crate::anchor::{EditorPositionError, GraphemeBoundaryMap, NodePositionMap};
use crate::asset::{
    MAX_AUTHORED_ALT_BYTES, MAX_IMAGE_DECODED_BYTES, MAX_IMAGE_DIMENSION, MAX_IMAGE_ENCODED_BYTES,
    MAX_IMAGE_PIXELS, MAX_STAGING_BYTES_PER_SESSION, MAX_STAGING_RECEIPTS_PER_SESSION,
    STAGING_RECEIPT_TTL_SECONDS,
};
use crate::model::{
    Affinity, Alignment, BlockStyle, ContentNode, DocumentId, FieldAnchorState, FieldDescriptor,
    FieldId, FieldOptionId, FieldValue, FlowDocument, FontFamily, ImageAccessibility, InlineMark,
    LegacyAnchorReason, ListKind, LogicalPosition, MarkSet, NodeId, RunLanguage, TombstoneToken,
};
use crate::schema::{MAX_TABLE_COLUMNS, MAX_TABLE_ROWS};

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
    pub block_style: Option<BlockStyle>,
    pub alignment: Option<Alignment>,
    pub spacing_before_millipoints: Option<u32>,
    pub spacing_after_millipoints: Option<u32>,
    pub list_kind: Option<ListKind>,
    pub list_item_id: Option<NodeId>,
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
            block_style: None,
            alignment: None,
            spacing_before_millipoints: None,
            spacing_after_millipoints: None,
            list_kind: None,
            list_item_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EditorCapability {
    SetSelection,
    SetPendingMarks,
    SetInlineMarks,
    SetBlockAttributes,
    SetListKind,
    ContinueListItem,
    ExitListItem,
    IndentListItem,
    OutdentListItem,
    SplitTextBlock,
    MergeTextBlocks,
    DeleteSubtree,
    InsertPageBreak,
    RemovePageBreak,
    InsertTable,
    InsertImage,
    ReplaceImage,
    SetImageAccessibility,
    RemoveImage,
    AddTableRow,
    RemoveTableRow,
    AddTableColumn,
    RemoveTableColumn,
    SetTableHeaderRow,
    RemoveTable,
}

impl EditorCapability {
    /// Returns the closed transaction command represented by this dynamic
    /// capability. Session-only capabilities intentionally return `None`.
    #[must_use]
    pub const fn command_type(&self) -> Option<&'static str> {
        match self {
            Self::SetSelection | Self::SetPendingMarks => None,
            Self::SetInlineMarks => Some("setInlineMarks"),
            Self::SetBlockAttributes => Some("setBlockAttributes"),
            Self::SetListKind => Some("setListKind"),
            Self::ContinueListItem => Some("continueListItem"),
            Self::ExitListItem => Some("exitListItem"),
            Self::IndentListItem => Some("indentListItem"),
            Self::OutdentListItem => Some("outdentListItem"),
            Self::SplitTextBlock => Some("splitTextBlock"),
            Self::MergeTextBlocks => Some("mergeTextBlocks"),
            Self::DeleteSubtree => Some("deleteSubtree"),
            Self::InsertPageBreak => Some("insertPageBreak"),
            Self::RemovePageBreak => Some("removePageBreak"),
            Self::InsertTable => Some("insertTable"),
            Self::InsertImage => Some("insertImage"),
            Self::ReplaceImage => Some("replaceImage"),
            Self::SetImageAccessibility => Some("setImageAccessibility"),
            Self::RemoveImage => Some("removeImage"),
            Self::AddTableRow => Some("addTableRow"),
            Self::RemoveTableRow => Some("removeTableRow"),
            Self::AddTableColumn => Some("addTableColumn"),
            Self::RemoveTableColumn => Some("removeTableColumn"),
            Self::SetTableHeaderRow => Some("setTableHeaderRow"),
            Self::RemoveTable => Some("removeTable"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConfirmationKindDto {
    Destructive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmationMetadataDto {
    pub kind: ConfirmationKindDto,
    pub heading_key: String,
    pub body_key: String,
    pub confirm_key: String,
    pub cancel_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StructuralPlacementDto {
    pub parent_id: Option<NodeId>,
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableLimitsDto {
    pub max_rows: u32,
    pub max_columns: u32,
    pub max_cells: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageLimitsDto {
    pub max_encoded_bytes: u32,
    pub max_dimension: u32,
    pub max_pixels: u64,
    pub max_decoded_bytes: u64,
    pub max_alt_text_bytes: u32,
    pub max_staging_receipts_per_session: u32,
    pub max_staging_bytes_per_session: u32,
    pub receipt_ttl_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageBlockViewDto {
    pub asset_id: crate::model::AssetId,
    pub content_hash: String,
    pub media_type: String,
    pub byte_length: u32,
    pub accessibility: ImageAccessibility,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableCellFocusDto {
    pub cell_id: NodeId,
    pub selection: DirectionalSelection,
    pub previous_cell_id: Option<NodeId>,
    pub next_cell_id: Option<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityDto {
    pub name: EditorCapability,
    pub enabled: bool,
    pub reason_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<ConfirmationMetadataDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<StructuralPlacementDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_limits: Option<TableLimitsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_limits: Option<ImageLimitsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_node_id: Option<NodeId>,
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
    pub fields: Vec<EditorFieldViewDto>,
    pub field_review: Vec<EditorFieldReviewDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EditorFieldValueSummaryDto {
    Empty,
    Text {
        value: String,
    },
    Checked {
        value: bool,
    },
    Selected {
        option_ids: Vec<FieldOptionId>,
        labels: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorFieldViewDto {
    pub descriptor: FieldDescriptor,
    pub value_summary: EditorFieldValueSummaryDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EditorFieldReviewStatusDto {
    LegacyInvalid { reason: LegacyAnchorReason },
    TargetDeleted { tombstone: TombstoneToken },
    GraphemeSafeTargetMissing,
    GraphemeSafePositionInvalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorFieldReviewDto {
    pub descriptor: FieldDescriptor,
    pub value_summary: EditorFieldValueSummaryDto,
    pub status: EditorFieldReviewStatusDto,
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
            fields: Vec::new(),
            field_review: Vec::new(),
        }
    }

    fn from_document(document: &FlowDocument) -> Self {
        let (fields, field_review) = field_views(document);
        Self {
            document_id: document.document_id.clone(),
            revision: document.revision,
            blocks: document
                .content
                .iter()
                .map(|node| EditorBlockViewDto::from_node(node, &document.assets))
                .collect(),
            fields,
            field_review,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FieldOrderKey {
    node_path: Vec<usize>,
    utf16_offset: u32,
    affinity: u8,
    node_id: NodeId,
    field_id: FieldId,
}

fn field_views(document: &FlowDocument) -> (Vec<EditorFieldViewDto>, Vec<EditorFieldReviewDto>) {
    let mut node_paths = BTreeMap::new();
    collect_node_paths(&document.content, &mut Vec::new(), &mut node_paths);

    let mut fields = Vec::new();
    let mut field_review = Vec::new();
    for descriptor in &document.fields {
        let original = descriptor.anchor.original();
        let key = FieldOrderKey {
            node_path: node_paths
                .get(&original.node_id)
                .cloned()
                .unwrap_or_else(|| vec![usize::MAX]),
            utf16_offset: original.utf16_offset.get(),
            affinity: affinity_order(&original.affinity),
            node_id: original.node_id.clone(),
            field_id: descriptor.id.clone(),
        };
        let value_summary = field_value_summary(descriptor);
        if valid_field_anchor(document, descriptor) {
            fields.push((
                key,
                EditorFieldViewDto {
                    descriptor: descriptor.clone(),
                    value_summary,
                },
            ));
        } else {
            field_review.push((
                key,
                EditorFieldReviewDto {
                    descriptor: descriptor.clone(),
                    value_summary,
                    status: field_review_status(document, descriptor),
                },
            ));
        }
    }

    fields.sort_by(|left, right| left.0.cmp(&right.0));
    field_review.sort_by(|left, right| left.0.cmp(&right.0));
    (
        fields.into_iter().map(|(_, field)| field).collect(),
        field_review.into_iter().map(|(_, field)| field).collect(),
    )
}

fn collect_node_paths(
    nodes: &[ContentNode],
    parent_path: &mut Vec<usize>,
    paths: &mut BTreeMap<NodeId, Vec<usize>>,
) {
    for (index, node) in nodes.iter().enumerate() {
        parent_path.push(index);
        paths.insert(node.id.clone(), parent_path.clone());
        collect_node_paths(node.children(), parent_path, paths);
        parent_path.pop();
    }
}

fn affinity_order(affinity: &Affinity) -> u8 {
    match affinity {
        Affinity::Forward => 0,
        Affinity::Backward => 1,
    }
}

fn valid_field_anchor(document: &FlowDocument, descriptor: &FieldDescriptor) -> bool {
    let FieldAnchorState::GraphemeSafe { original } = &descriptor.anchor else {
        return false;
    };
    let Some(node) = find_node(&document.content, &original.node_id) else {
        return false;
    };
    let Ok(map) = NodePositionMap::new(node, document.revision) else {
        return false;
    };
    map.validate(original, document.revision).is_ok()
}

fn field_review_status(
    document: &FlowDocument,
    descriptor: &FieldDescriptor,
) -> EditorFieldReviewStatusDto {
    match &descriptor.anchor {
        FieldAnchorState::LegacyInvalid { reason, .. } => {
            EditorFieldReviewStatusDto::LegacyInvalid {
                reason: reason.clone(),
            }
        }
        FieldAnchorState::TargetDeleted { tombstone, .. } => {
            EditorFieldReviewStatusDto::TargetDeleted {
                tombstone: tombstone.clone(),
            }
        }
        FieldAnchorState::GraphemeSafe { original } => {
            if find_node(&document.content, &original.node_id).is_none() {
                EditorFieldReviewStatusDto::GraphemeSafeTargetMissing
            } else {
                EditorFieldReviewStatusDto::GraphemeSafePositionInvalid
            }
        }
    }
}

fn field_value_summary(descriptor: &FieldDescriptor) -> EditorFieldValueSummaryDto {
    match &descriptor.default_value {
        FieldValue::Empty => EditorFieldValueSummaryDto::Empty,
        FieldValue::Text { value } => EditorFieldValueSummaryDto::Text {
            value: value.clone(),
        },
        FieldValue::Checked { value } => EditorFieldValueSummaryDto::Checked { value: *value },
        FieldValue::Selected { option_ids } => EditorFieldValueSummaryDto::Selected {
            option_ids: option_ids.clone(),
            labels: option_ids
                .iter()
                .filter_map(|option_id| {
                    descriptor
                        .options
                        .iter()
                        .find(|option| option.id == *option_id)
                        .map(|option| option.label.clone())
                })
                .collect(),
        },
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image: Option<ImageBlockViewDto>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        table_header_rows: Option<u8>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        table_cell_focus_order: Vec<TableCellFocusDto>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        children: Vec<EditorBlockViewDto>,
    },
}

impl EditorBlockViewDto {
    fn from_node(node: &ContentNode, assets: &[crate::model::AssetDescriptor]) -> Self {
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
                image: match body {
                    crate::model::BlockKind::Image {
                        asset_id,
                        accessibility,
                    } => assets
                        .iter()
                        .find(|asset| asset.id == *asset_id)
                        .map(|asset| ImageBlockViewDto {
                            asset_id: asset.id.clone(),
                            content_hash: asset.content_hash.clone(),
                            media_type: asset.media_type.clone(),
                            byte_length: asset.byte_length,
                            accessibility: accessibility.clone(),
                        }),
                    _ => None,
                },
                table_header_rows: match body {
                    crate::model::BlockKind::Table { header_rows, .. } => Some(*header_rows),
                    _ => None,
                },
                table_cell_focus_order: table_cell_focus_order(node),
                children: node
                    .children()
                    .iter()
                    .map(|child| Self::from_node(child, assets))
                    .collect(),
            },
        }
    }
}

fn table_cell_focus_order(node: &ContentNode) -> Vec<TableCellFocusDto> {
    let crate::model::BlockKind::Table { rows, .. } = &node.body else {
        return Vec::new();
    };
    let cells = rows
        .iter()
        .flat_map(|row| row.children().iter())
        .collect::<Vec<_>>();
    let selections = cells
        .iter()
        .map(|cell| first_text_selection_in_node(cell))
        .collect::<Option<Vec<_>>>();
    let Some(selections) = selections else {
        return Vec::new();
    };
    cells
        .iter()
        .enumerate()
        .map(|(index, cell)| TableCellFocusDto {
            cell_id: cell.id.clone(),
            selection: selections[index].clone(),
            previous_cell_id: index
                .checked_sub(1)
                .and_then(|previous| cells.get(previous))
                .map(|cell| cell.id.clone()),
            next_cell_id: cells.get(index + 1).map(|cell| cell.id.clone()),
        })
        .collect()
}

fn first_text_selection_in_node(node: &ContentNode) -> Option<DirectionalSelection> {
    if node.runs().is_some() {
        let position = LogicalPosition {
            node_id: node.id.clone(),
            utf16_offset: 0.into(),
            affinity: Affinity::Forward,
        };
        return Some(DirectionalSelection {
            anchor: position.clone(),
            focus: position,
        });
    }
    node.children()
        .iter()
        .find_map(first_text_selection_in_node)
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
    SetPendingMark {
        mark: InlineMark,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableContext {
    table_id: NodeId,
    row_index: Option<usize>,
    column_index: Option<usize>,
    row_count: usize,
    column_count: usize,
    header_rows: u8,
}

fn table_context_for_node(nodes: &[ContentNode], target: &NodeId) -> Option<TableContext> {
    for node in nodes {
        if let crate::model::BlockKind::Table { header_rows, rows } = &node.body {
            let row_count = rows.len();
            let column_count = rows.first().map_or(0, |row| row.children().len());
            if node.id == *target {
                return Some(TableContext {
                    table_id: node.id.clone(),
                    row_index: None,
                    column_index: None,
                    row_count,
                    column_count,
                    header_rows: *header_rows,
                });
            }
            for (row_index, row) in rows.iter().enumerate() {
                for (column_index, cell) in row.children().iter().enumerate() {
                    if contains_node_id(cell, target) {
                        return Some(TableContext {
                            table_id: node.id.clone(),
                            row_index: Some(row_index),
                            column_index: Some(column_index),
                            row_count,
                            column_count,
                            header_rows: *header_rows,
                        });
                    }
                }
            }
        }
        if let Some(found) = table_context_for_node(node.children(), target) {
            return Some(found);
        }
    }
    None
}

fn contains_node_id(node: &ContentNode, target: &NodeId) -> bool {
    node.id == *target
        || node
            .children()
            .iter()
            .any(|child| contains_node_id(child, target))
}

fn capabilities_for(
    document: &FlowDocument,
    selection: &DirectionalSelection,
) -> Vec<CapabilityDto> {
    let anchor_node = find_node(&document.content, &selection.anchor.node_id);
    let focus_node = find_node(&document.content, &selection.focus.node_id);
    let text_selection = anchor_node.is_some_and(|node| node.runs().is_some())
        && focus_node.is_some_and(|node| node.runs().is_some());
    let list_context = list_context_for_node(&document.content, &selection.anchor.node_id);
    let same_list_item = selection.collapsed()
        && list_context.is_some()
        && list_context_for_node(&document.content, &selection.focus.node_id) == list_context;
    let list_item_index = list_context.map(|(_, index, _)| index);
    let split_enabled = selection.collapsed() && text_selection;
    let merge_enabled = selection.collapsed()
        && anchor_node.is_some_and(|node| {
            has_compatible_text_neighbor(document, node, selection.anchor.utf16_offset.get())
        });
    let delete_enabled = find_node(&document.content, &selection.focus.node_id).is_some();
    let table_context = table_context_for_node(&document.content, &selection.anchor.node_id);
    let same_table_cell = selection.collapsed()
        && table_context
            .as_ref()
            .is_some_and(|context| context.row_index.is_some() && context.column_index.is_some())
        && table_context_for_node(&document.content, &selection.focus.node_id) == table_context;
    let selected_table_id = table_context
        .as_ref()
        .map(|context| context.table_id.clone());
    let page_break_selected = anchor_node.is_some_and(|node| {
        matches!(&node.body, crate::model::BlockKind::PageBreak)
            && (selection.collapsed() || whole_atomic_selection(selection))
    });
    let selected_image_id = anchor_node
        .filter(|node| {
            matches!(&node.body, crate::model::BlockKind::Image { .. })
                && selection.collapsed()
                && selection.anchor.node_id == selection.focus.node_id
        })
        .map(|node| node.id.clone());
    let image_selected = selected_image_id.is_some()
        && focus_node
            .is_some_and(|node| matches!(&node.body, crate::model::BlockKind::Image { .. }));
    let table_confirmation = table_confirmation_metadata();
    let image_confirmation = image_confirmation_metadata();
    let row_limit_reached = table_context
        .as_ref()
        .is_some_and(|context| context.row_count >= MAX_TABLE_ROWS);
    let column_limit_reached = table_context
        .as_ref()
        .is_some_and(|context| context.column_count >= MAX_TABLE_COLUMNS);
    let only_table_row = table_context
        .as_ref()
        .is_some_and(|context| context.row_count == 1 && context.row_index.is_some());
    let only_table_column = table_context
        .as_ref()
        .is_some_and(|context| context.column_count == 1 && context.column_index.is_some());
    let mut capabilities = vec![
        capability(EditorCapability::SetSelection, true, None, None),
        capability(EditorCapability::SetPendingMarks, true, None, None),
        capability(
            EditorCapability::SetBlockAttributes,
            text_selection,
            (!text_selection).then_some("textBlockRequired"),
            None,
        ),
        capability(
            EditorCapability::SetListKind,
            text_selection,
            (!text_selection).then_some("textBlockRequired"),
            None,
        ),
        capability(
            EditorCapability::ContinueListItem,
            same_list_item,
            (!same_list_item).then_some("listItemRequired"),
            None,
        ),
        capability(
            EditorCapability::ExitListItem,
            same_list_item,
            (!same_list_item).then_some("emptyListItemRequired"),
            None,
        ),
        capability(
            EditorCapability::IndentListItem,
            same_list_item && list_item_index.is_some_and(|index| index > 0),
            (!(same_list_item && list_item_index.is_some_and(|index| index > 0)))
                .then_some("listSiblingRequired"),
            None,
        ),
        capability(
            EditorCapability::OutdentListItem,
            same_list_item,
            (!same_list_item).then_some("nestedListItemRequired"),
            None,
        ),
        capability(
            EditorCapability::SetInlineMarks,
            text_selection && !selection.collapsed(),
            (!text_selection || selection.collapsed()).then_some("formattingRangeRequired"),
            None,
        ),
        capability(
            EditorCapability::SplitTextBlock,
            split_enabled,
            (!split_enabled).then_some("structuralSelectionRequired"),
            None,
        ),
        capability(
            EditorCapability::MergeTextBlocks,
            merge_enabled,
            (!merge_enabled).then_some("incompatibleStructure"),
            None,
        ),
        capability(
            EditorCapability::DeleteSubtree,
            delete_enabled,
            (!delete_enabled).then_some("unknownNode"),
            None,
        ),
        capability(
            EditorCapability::InsertPageBreak,
            selection.collapsed() && text_selection,
            (!(selection.collapsed() && text_selection)).then_some("structuralSelectionRequired"),
            None,
        ),
        capability(
            EditorCapability::RemovePageBreak,
            page_break_selected,
            (!page_break_selected).then_some("pageBreakRequired"),
            None,
        ),
        capability(
            EditorCapability::InsertTable,
            selection.collapsed() && text_selection,
            (!(selection.collapsed() && text_selection)).then_some("structuralSelectionRequired"),
            None,
        ),
        capability(
            EditorCapability::InsertImage,
            selection.collapsed() && text_selection,
            (!(selection.collapsed() && text_selection)).then_some("structuralSelectionRequired"),
            None,
        ),
        capability(
            EditorCapability::ReplaceImage,
            image_selected,
            (!image_selected).then_some("imageRequired"),
            None,
        ),
        capability(
            EditorCapability::SetImageAccessibility,
            image_selected,
            (!image_selected).then_some("imageRequired"),
            None,
        ),
        capability(
            EditorCapability::RemoveImage,
            image_selected,
            (!image_selected).then_some("imageRequired"),
            image_selected.then(|| image_confirmation.clone()),
        ),
        capability(
            EditorCapability::AddTableRow,
            same_table_cell && !row_limit_reached,
            if !same_table_cell {
                Some("tableCellRequired")
            } else if row_limit_reached {
                Some("tableRowLimit")
            } else {
                None
            },
            None,
        ),
        capability(
            EditorCapability::RemoveTableRow,
            same_table_cell && !only_table_row,
            if !same_table_cell {
                Some("tableCellRequired")
            } else if only_table_row {
                Some("tableRemovalConfirmationRequired")
            } else {
                None
            },
            only_table_row.then(|| table_confirmation.clone()),
        ),
        capability(
            EditorCapability::AddTableColumn,
            same_table_cell && !column_limit_reached,
            if !same_table_cell {
                Some("tableCellRequired")
            } else if column_limit_reached {
                Some("tableColumnLimit")
            } else {
                None
            },
            None,
        ),
        capability(
            EditorCapability::RemoveTableColumn,
            same_table_cell && !only_table_column,
            if !same_table_cell {
                Some("tableCellRequired")
            } else if only_table_column {
                Some("tableRemovalConfirmationRequired")
            } else {
                None
            },
            only_table_column.then(|| table_confirmation.clone()),
        ),
        capability(
            EditorCapability::SetTableHeaderRow,
            selected_table_id.is_some(),
            selected_table_id.is_none().then_some("tableRequired"),
            None,
        ),
        capability(
            EditorCapability::RemoveTable,
            selected_table_id.is_some(),
            selected_table_id.is_none().then_some("tableRequired"),
            selected_table_id.clone().map(|_| table_confirmation),
        ),
    ];
    let insertion_placement = if selection.collapsed() && text_selection {
        placement_for_node(&document.content, &selection.anchor.node_id)
    } else {
        None
    };
    let table_limits = table_limits();
    for capability in &mut capabilities {
        match &capability.name {
            EditorCapability::InsertPageBreak => {
                capability.placement = insertion_placement.clone();
            }
            EditorCapability::InsertTable => {
                capability.placement = insertion_placement.clone();
                capability.table_limits = Some(table_limits.clone());
            }
            EditorCapability::InsertImage => {
                capability.placement = insertion_placement.clone();
                capability.image_limits = Some(image_limits());
            }
            EditorCapability::ReplaceImage
            | EditorCapability::SetImageAccessibility
            | EditorCapability::RemoveImage => {
                capability.target_node_id = selected_image_id.clone();
                if matches!(&capability.name, EditorCapability::ReplaceImage) {
                    capability.image_limits = Some(image_limits());
                }
            }
            EditorCapability::AddTableRow
            | EditorCapability::RemoveTableRow
            | EditorCapability::AddTableColumn
            | EditorCapability::RemoveTableColumn
            | EditorCapability::SetTableHeaderRow
            | EditorCapability::RemoveTable => {
                capability.target_node_id = selected_table_id.clone();
            }
            _ => {}
        }
    }
    capabilities
}

fn capability(
    name: EditorCapability,
    enabled: bool,
    reason_key: Option<&str>,
    confirmation: Option<ConfirmationMetadataDto>,
) -> CapabilityDto {
    CapabilityDto {
        name,
        enabled,
        reason_key: reason_key.map(str::to_owned),
        confirmation,
        placement: None,
        table_limits: None,
        image_limits: None,
        target_node_id: None,
    }
}

fn table_limits() -> TableLimitsDto {
    TableLimitsDto {
        max_rows: MAX_TABLE_ROWS as u32,
        max_columns: MAX_TABLE_COLUMNS as u32,
        max_cells: crate::schema::MAX_TABLE_CELLS as u32,
    }
}

fn image_limits() -> ImageLimitsDto {
    ImageLimitsDto {
        max_encoded_bytes: u32::try_from(MAX_IMAGE_ENCODED_BYTES).unwrap_or(u32::MAX),
        max_dimension: MAX_IMAGE_DIMENSION,
        max_pixels: MAX_IMAGE_PIXELS,
        max_decoded_bytes: MAX_IMAGE_DECODED_BYTES,
        max_alt_text_bytes: u32::try_from(MAX_AUTHORED_ALT_BYTES).unwrap_or(u32::MAX),
        max_staging_receipts_per_session: u32::try_from(MAX_STAGING_RECEIPTS_PER_SESSION)
            .unwrap_or(u32::MAX),
        max_staging_bytes_per_session: u32::try_from(MAX_STAGING_BYTES_PER_SESSION)
            .unwrap_or(u32::MAX),
        receipt_ttl_seconds: u32::try_from(STAGING_RECEIPT_TTL_SECONDS).unwrap_or(u32::MAX),
    }
}

fn placement_for_node(nodes: &[ContentNode], target: &NodeId) -> Option<StructuralPlacementDto> {
    fn visit(
        nodes: &[ContentNode],
        target: &NodeId,
        parent_id: Option<&NodeId>,
    ) -> Option<StructuralPlacementDto> {
        for (index, node) in nodes.iter().enumerate() {
            if node.id == *target {
                return Some(StructuralPlacementDto {
                    parent_id: parent_id.cloned(),
                    index: u32::try_from(index).ok()?,
                });
            }
            if let Some(found) = visit(node.children(), target, Some(&node.id)) {
                return Some(found);
            }
        }
        None
    }

    visit(nodes, target, None).map(|mut placement| {
        placement.index = placement.index.saturating_add(1);
        placement
    })
}

fn table_confirmation_metadata() -> ConfirmationMetadataDto {
    ConfirmationMetadataDto {
        kind: ConfirmationKindDto::Destructive,
        heading_key: "editor.table.remove.heading".to_owned(),
        body_key: "editor.table.remove.body".to_owned(),
        confirm_key: "editor.table.remove.confirm".to_owned(),
        cancel_key: "editor.table.remove.cancel".to_owned(),
    }
}

fn image_confirmation_metadata() -> ConfirmationMetadataDto {
    ConfirmationMetadataDto {
        kind: ConfirmationKindDto::Destructive,
        heading_key: "editor.image.remove.heading".to_owned(),
        body_key: "editor.image.remove.body".to_owned(),
        confirm_key: "editor.image.remove.confirm".to_owned(),
        cancel_key: "editor.image.remove.cancel".to_owned(),
    }
}

fn whole_atomic_selection(selection: &DirectionalSelection) -> bool {
    if selection.anchor.node_id != selection.focus.node_id {
        return false;
    }
    matches!(
        (
            selection.anchor.utf16_offset.get(),
            &selection.anchor.affinity,
            selection.focus.utf16_offset.get(),
            &selection.focus.affinity,
        ),
        (
            0,
            crate::model::Affinity::Forward,
            1,
            crate::model::Affinity::Backward
        ) | (
            1,
            crate::model::Affinity::Backward,
            0,
            crate::model::Affinity::Forward
        )
    )
}

fn list_context_for_node(
    nodes: &[ContentNode],
    target: &NodeId,
) -> Option<(NodeId, usize, ListKind)> {
    fn visit(
        nodes: &[ContentNode],
        target: &NodeId,
        inherited: Option<(NodeId, usize, ListKind)>,
    ) -> Option<(NodeId, usize, ListKind)> {
        for node in nodes {
            if node.id == *target {
                return inherited;
            }
            match node.body {
                crate::model::BlockKind::OrderedList { .. }
                | crate::model::BlockKind::UnorderedList { .. } => {
                    let kind = if matches!(node.body, crate::model::BlockKind::OrderedList { .. }) {
                        ListKind::Ordered
                    } else {
                        ListKind::Unordered
                    };
                    for (index, item) in node.children().iter().enumerate() {
                        if item.id == *target {
                            return Some((node.id.clone(), index, kind));
                        }
                        if let Some(found) = visit(
                            item.children(),
                            target,
                            Some((node.id.clone(), index, kind)),
                        ) {
                            return Some(found);
                        }
                    }
                }
                _ => {
                    if let Some(found) = visit(node.children(), target, inherited.clone()) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    visit(nodes, target, None)
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
        return Ok(formatting_from_node(
            node,
            &marks_at_offset(runs, start),
            document,
        ));
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

    let mut formatting = FormattingProjectionDto {
        bold: bool_state(selected.iter().map(|marks| marks.bold)),
        italic: bool_state(selected.iter().map(|marks| marks.italic)),
        underline: bool_state(selected.iter().map(|marks| marks.underline)),
        font_family: uniform_value(selected.iter().map(|marks| marks.font_family.clone())),
        font_size_millipoints: uniform_value(
            selected.iter().map(|marks| marks.font_size_millipoints),
        ),
        color: uniform_value(selected.iter().map(|marks| marks.color)),
        language: uniform_value(selected.iter().map(|marks| marks.language.clone())),
        ..FormattingProjectionDto::default()
    };
    apply_block_projection(&mut formatting, node, document);
    Ok(formatting)
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
        ..FormattingProjectionDto::default()
    }
}

fn formatting_from_node(
    node: &ContentNode,
    marks: &MarkSet,
    document: &FlowDocument,
) -> FormattingProjectionDto {
    let mut formatting = formatting_from_marks(marks);
    apply_block_projection(&mut formatting, node, document);
    formatting
}

fn apply_block_projection(
    formatting: &mut FormattingProjectionDto,
    node: &ContentNode,
    document: &FlowDocument,
) {
    formatting.block_style = match &node.body {
        crate::model::BlockKind::Paragraph { .. } => Some(BlockStyle::Paragraph),
        crate::model::BlockKind::Heading { level, .. } => {
            Some(BlockStyle::Heading { level: *level })
        }
        _ => None,
    };
    match &node.body {
        crate::model::BlockKind::Paragraph { attrs, .. }
        | crate::model::BlockKind::Heading { attrs, .. } => {
            formatting.alignment = Some(attrs.alignment.clone());
            formatting.spacing_before_millipoints = Some(attrs.spacing_before_millipoints);
            formatting.spacing_after_millipoints = Some(attrs.spacing_after_millipoints);
        }
        _ => {}
    }
    formatting.list_kind =
        list_context_for_node(&document.content, &node.id).map(|(_, _, kind)| kind);
    formatting.list_item_id = list_item_id_for_node(&document.content, &node.id);
}

fn list_item_id_for_node(nodes: &[ContentNode], target: &NodeId) -> Option<NodeId> {
    fn visit(nodes: &[ContentNode], target: &NodeId) -> Option<NodeId> {
        for node in nodes {
            match node.body {
                crate::model::BlockKind::OrderedList { .. }
                | crate::model::BlockKind::UnorderedList { .. } => {
                    for item in node.children() {
                        if item.id == *target {
                            return Some(item.id.clone());
                        }
                        if item.children().iter().any(|child| child.id == *target) {
                            return Some(item.id.clone());
                        }
                        if let Some(found) = visit(item.children(), target) {
                            return Some(found);
                        }
                    }
                }
                _ => {
                    if let Some(found) = visit(node.children(), target) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    visit(nodes, target)
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
        EditorSessionAction::SetPendingMark { mark } => {
            if !state.selection.collapsed() {
                return Err(EditorSessionError::PendingMarksRequireCollapsedSelection);
            }
            let mut marks = state.pending_marks.clone();
            apply_pending_mark(&mut marks, &mark)?;
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

fn apply_pending_mark(marks: &mut MarkSet, mark: &InlineMark) -> Result<(), EditorSessionError> {
    match mark {
        InlineMark::Bold { value } => marks.bold = *value,
        InlineMark::Italic { value } => marks.italic = *value,
        InlineMark::Underline { value } => marks.underline = *value,
        InlineMark::FontFamily { value } => marks.font_family = value.clone(),
        InlineMark::FontSize { value } => marks.font_size_millipoints = *value,
        InlineMark::Color { value } => {
            marks.color = value.as_deref().map(parse_pending_color).transpose()?;
        }
        InlineMark::Language { value } => marks.language = value.clone(),
    }
    validate_pending_marks(marks)
}

fn parse_pending_color(value: &str) -> Result<[u8; 3], EditorSessionError> {
    if value.len() != 7
        || !value.starts_with('#')
        || !value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
        || value[1..]
            .chars()
            .any(|character| character.is_ascii_lowercase())
    {
        return Err(EditorSessionError::InvalidPendingMarks);
    }
    let red = u8::from_str_radix(&value[1..3], 16)
        .map_err(|_| EditorSessionError::InvalidPendingMarks)?;
    let green = u8::from_str_radix(&value[3..5], 16)
        .map_err(|_| EditorSessionError::InvalidPendingMarks)?;
    let blue = u8::from_str_radix(&value[5..7], 16)
        .map_err(|_| EditorSessionError::InvalidPendingMarks)?;
    Ok([red, green, blue])
}

pub(crate) fn project_view(
    document: &FlowDocument,
    session: &EditorSessionState,
) -> Result<EditorViewDto, EditorSessionError> {
    session.validate_against(document)?;
    Ok(session.view_for_document(document))
}

//! Rust-owned semantic form validation and derived widget placement.
//!
//! Form descriptors remain part of the canonical [`FlowDocument`].  This
//! module consumes an accepted PDF display list to derive page rectangles and
//! tab order; it never stores page coordinates, PDF object numbers, or browser
//! state back into the document.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    anchor::NodePositionMap,
    canonical::{canonical_bytes, canonical_hash},
    layout::{LayoutRect, LayoutUnit},
    model::{
        Affinity, ContentNode, FieldAnchorState, FieldDescriptor, FieldId, FieldKind,
        FieldOptionId, FieldValue, FlowDocument, LegacyAnchorReason, LogicalPosition, NodeId,
        TextInputHint,
    },
    pdf::{PdfDisplayList, PdfGlyphPlacement, PdfTextLine},
    schema::validate_document,
};

/// Version of the derived semantic-form projection.
pub const FORM_PROJECTION_SCHEMA_VERSION: u32 = 1;

const MAX_FORM_TEXT_BYTES: usize = 64 * 1024;
const TEXT_WIDGET_WIDTH: i64 = 144 * 64;
const SELECT_WIDGET_WIDTH: i64 = 144 * 64;
const SIGNATURE_WIDGET_WIDTH: i64 = 180 * 64;
const BUTTON_WIDGET_WIDTH: i64 = 72 * 64;
const CHECKBOX_WIDGET_SIZE: i64 = 14 * 64;

/// Stable, privacy-safe validation codes for one proposed field value.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum FormValueErrorCode {
    Required,
    TypeMismatch,
    TextSizeLimit,
    MultilineNotAllowed,
    InvalidDate,
    InvalidNumber,
    InvalidEmail,
    OptionSetInvalid,
    SelectionLimit,
    UnsupportedValue,
}

impl FormValueErrorCode {
    /// Returns the stable public error identifier.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Required => "FLOW_FORM_REQUIRED",
            Self::TypeMismatch => "FLOW_FORM_VALUE_TYPE",
            Self::TextSizeLimit => "FLOW_FORM_TEXT_SIZE_LIMIT",
            Self::MultilineNotAllowed => "FLOW_FORM_MULTILINE_NOT_ALLOWED",
            Self::InvalidDate => "FLOW_FORM_DATE_INVALID",
            Self::InvalidNumber => "FLOW_FORM_NUMBER_INVALID",
            Self::InvalidEmail => "FLOW_FORM_EMAIL_INVALID",
            Self::OptionSetInvalid => "FLOW_FORM_OPTION_SET_INVALID",
            Self::SelectionLimit => "FLOW_FORM_SELECTION_LIMIT",
            Self::UnsupportedValue => "FLOW_FORM_VALUE_UNSUPPORTED",
        }
    }
}

/// Validates one proposed semantic value without mutating its field or
/// document. The caller owns the transaction that would apply an accepted
/// value; browser code never reproduces these rules.
pub fn validate_field_value(
    field: &FieldDescriptor,
    value: &FieldValue,
) -> Result<(), FormValueErrorCode> {
    validate_field_value_inner(field, value, true)
}

fn validate_default_value(
    field: &FieldDescriptor,
    value: &FieldValue,
) -> Result<(), FormValueErrorCode> {
    validate_field_value_inner(field, value, false)
}

fn validate_field_value_inner(
    field: &FieldDescriptor,
    value: &FieldValue,
    enforce_required: bool,
) -> Result<(), FormValueErrorCode> {
    if enforce_required && field.required && value_is_empty(field, value) {
        return Err(FormValueErrorCode::Required);
    }

    match (&field.kind, value) {
        (FieldKind::Text { .. }, FieldValue::Empty) => Ok(()),
        (
            FieldKind::Text {
                multiline,
                input_hint,
            },
            FieldValue::Text { value },
        ) => {
            if value.len() > MAX_FORM_TEXT_BYTES {
                return Err(FormValueErrorCode::TextSizeLimit);
            }
            if !multiline
                && value
                    .chars()
                    .any(|character| matches!(character, '\n' | '\r'))
            {
                return Err(FormValueErrorCode::MultilineNotAllowed);
            }
            match input_hint {
                TextInputHint::Plain => Ok(()),
                TextInputHint::Date if is_iso_date(value) => Ok(()),
                TextInputHint::Date => Err(FormValueErrorCode::InvalidDate),
                TextInputHint::Number if is_decimal(value) => Ok(()),
                TextInputHint::Number => Err(FormValueErrorCode::InvalidNumber),
                TextInputHint::Email if is_email(value) => Ok(()),
                TextInputHint::Email => Err(FormValueErrorCode::InvalidEmail),
            }
        }
        (FieldKind::Checkbox, FieldValue::Checked { value }) => {
            if enforce_required && field.required && !value {
                Err(FormValueErrorCode::Required)
            } else {
                Ok(())
            }
        }
        (FieldKind::RadioGroup, FieldValue::Empty) => Ok(()),
        (FieldKind::RadioGroup, FieldValue::Selected { option_ids }) => {
            validate_options(field, option_ids, 1)
        }
        (FieldKind::Select { .. }, FieldValue::Empty) => Ok(()),
        (FieldKind::Select { multiple }, FieldValue::Selected { option_ids }) => {
            let maximum = if *multiple { usize::MAX } else { 1 };
            validate_options(field, option_ids, maximum)
        }
        (FieldKind::Checkbox, FieldValue::Empty) => Ok(()),
        (FieldKind::Signature | FieldKind::Button, FieldValue::Empty) => Ok(()),
        (FieldKind::Text { .. }, _) | (FieldKind::Checkbox, _) => {
            Err(FormValueErrorCode::TypeMismatch)
        }
        (FieldKind::RadioGroup | FieldKind::Select { .. }, _) => {
            Err(FormValueErrorCode::TypeMismatch)
        }
        (FieldKind::Signature | FieldKind::Button, _) => Err(FormValueErrorCode::UnsupportedValue),
    }
}

fn value_is_empty(field: &FieldDescriptor, value: &FieldValue) -> bool {
    match (&field.kind, value) {
        (FieldKind::Text { .. }, FieldValue::Text { value }) => value.is_empty(),
        (FieldKind::Checkbox, FieldValue::Checked { value }) => !value,
        (FieldKind::RadioGroup | FieldKind::Select { .. }, FieldValue::Selected { option_ids }) => {
            option_ids.is_empty()
        }
        (_, FieldValue::Empty) => true,
        _ => false,
    }
}

fn validate_options(
    field: &FieldDescriptor,
    option_ids: &[FieldOptionId],
    maximum: usize,
) -> Result<(), FormValueErrorCode> {
    if option_ids.len() > maximum {
        return Err(FormValueErrorCode::SelectionLimit);
    }
    let available = field
        .options
        .iter()
        .map(|option| option.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut selected = BTreeSet::new();
    for option_id in option_ids {
        if !available.contains(option_id.as_str()) || !selected.insert(option_id.as_str()) {
            return Err(FormValueErrorCode::OptionSetInvalid);
        }
    }
    Ok(())
}

fn is_decimal(value: &str) -> bool {
    if value.is_empty() || value.len() > 128 {
        return false;
    }
    let mut digits = 0;
    let mut decimal_points = 0;
    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b'0'..=b'9' => digits += 1,
            b'.' if index > 0 && decimal_points == 0 => decimal_points += 1,
            b'+' | b'-' if index == 0 => {}
            _ => return false,
        }
    }
    digits > 0
}

fn is_email(value: &str) -> bool {
    if value.is_empty() || value.len() > 320 || value.chars().any(char::is_whitespace) {
        return false;
    }
    let mut parts = value.split('@');
    let Some(local) = parts.next() else {
        return false;
    };
    let Some(domain) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
    {
        return false;
    }
    let year = decimal(&bytes[0..4]);
    let month = decimal(&bytes[5..7]);
    let day = decimal(&bytes[8..10]);
    let maximum_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => return false,
    };
    year > 0 && (1..=maximum_day).contains(&day)
}

fn decimal(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0, |value, byte| value * 10 + u32::from(byte - b'0'))
}

/// Reason a semantic field could not be projected into a page widget.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FormWidgetReviewReason {
    LegacyInvalid { reason: LegacyAnchorReason },
    TargetDeleted,
    TargetMissing,
    PositionInvalid,
    SourceMappingMissing,
}

/// A field retained for explicit review instead of being silently omitted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormWidgetReview {
    pub field_id: FieldId,
    pub source_node_id: NodeId,
    pub reason: FormWidgetReviewReason,
}

/// One derived widget placement. It carries semantic identity and value
/// metadata but no PDF object number or browser coordinate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormWidget {
    pub widget_id: String,
    pub field_id: FieldId,
    pub name: String,
    pub label: Option<String>,
    pub kind: FieldKind,
    pub required: bool,
    pub read_only: bool,
    pub default_value: FieldValue,
    pub page_index: u32,
    pub rect: LayoutRect,
    pub source_node_id: NodeId,
    pub anchor_offset_utf16: u32,
    pub tab_order: u32,
}

/// Complete derived form projection for one document/display-list identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormWidgetProjection {
    pub schema_version: u32,
    pub source_revision: u32,
    pub source_hash: String,
    pub display_list_hash: String,
    pub widgets: Vec<FormWidget>,
    pub review: Vec<FormWidgetReview>,
    pub result_hash: String,
}

/// Failure before a form projection can be published.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FormProjectionError {
    #[error("the canonical document is invalid")]
    InvalidDocument,
    #[error("the display list revision is stale")]
    StaleRevision,
    #[error("the display list source hash does not match the document")]
    SourceHashMismatch,
    #[error("field {field_id} has invalid value code {code:?}")]
    InvalidFieldValue {
        field_id: FieldId,
        code: FormValueErrorCode,
    },
    #[error("form geometry overflowed")]
    GeometryOverflow,
    #[error("the form projection could not be serialized")]
    Serialization,
}

impl FormProjectionError {
    /// Returns a stable code suitable for a boundary adapter.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidDocument => "FLOW_FORM_DOCUMENT_INVALID",
            Self::StaleRevision => "FLOW_FORM_DISPLAY_REVISION_STALE",
            Self::SourceHashMismatch => "FLOW_FORM_DISPLAY_SOURCE_HASH_MISMATCH",
            Self::InvalidFieldValue { .. } => "FLOW_FORM_VALUE_INVALID",
            Self::GeometryOverflow => "FLOW_FORM_GEOMETRY_OVERFLOW",
            Self::Serialization => "FLOW_FORM_SERIALIZATION",
        }
    }
}

/// Resolves semantic fields against one accepted PDF display list.
pub fn resolve_form_widgets(
    document: &FlowDocument,
    display_list: &PdfDisplayList,
) -> Result<FormWidgetProjection, FormProjectionError> {
    validate_document(document).map_err(|_| FormProjectionError::InvalidDocument)?;
    if display_list.source_revision != document.revision {
        return Err(FormProjectionError::StaleRevision);
    }
    let canonical = canonical_bytes(document).map_err(|_| FormProjectionError::InvalidDocument)?;
    if canonical_hash(&canonical) != display_list.source_hash {
        return Err(FormProjectionError::SourceHashMismatch);
    }

    let mut widgets = Vec::new();
    let mut review = Vec::new();
    for field in &document.fields {
        validate_default_value(field, &field.default_value).map_err(|code| {
            FormProjectionError::InvalidFieldValue {
                field_id: field.id.clone(),
                code,
            }
        })?;
        let original = match &field.anchor {
            FieldAnchorState::GraphemeSafe { original } => original.clone(),
            FieldAnchorState::LegacyInvalid { original, reason } => {
                review.push(FormWidgetReview {
                    field_id: field.id.clone(),
                    source_node_id: original.node_id.clone(),
                    reason: FormWidgetReviewReason::LegacyInvalid {
                        reason: reason.clone(),
                    },
                });
                continue;
            }
            FieldAnchorState::TargetDeleted { original, .. } => {
                review.push(FormWidgetReview {
                    field_id: field.id.clone(),
                    source_node_id: original.node_id.clone(),
                    reason: FormWidgetReviewReason::TargetDeleted,
                });
                continue;
            }
        };
        let Some((page_index, line)) = anchor_line(display_list, &original) else {
            review.push(review_for_missing_mapping(field, &original));
            continue;
        };
        if !anchor_is_currently_valid(document, &original) {
            review.push(FormWidgetReview {
                field_id: field.id.clone(),
                source_node_id: original.node_id,
                reason: FormWidgetReviewReason::PositionInvalid,
            });
            continue;
        }
        let page = display_list
            .pages
            .iter()
            .find(|page| page.page_index == page_index)
            .ok_or(FormProjectionError::GeometryOverflow)?;
        let rect = widget_rect(line, page, field)?;
        widgets.push(FormWidget {
            widget_id: format!("flow-form-widget-{}", field.id),
            field_id: field.id.clone(),
            name: field.name.clone(),
            label: field.label.clone(),
            kind: field.kind.clone(),
            required: field.required,
            read_only: field.read_only,
            default_value: field.default_value.clone(),
            page_index,
            rect,
            source_node_id: original.node_id,
            anchor_offset_utf16: original.utf16_offset.get(),
            tab_order: 0,
        });
    }

    widgets.sort_by(|left, right| {
        (
            left.page_index,
            left.rect.y.raw(),
            left.rect.x.raw(),
            &left.field_id,
        )
            .cmp(&(
                right.page_index,
                right.rect.y.raw(),
                right.rect.x.raw(),
                &right.field_id,
            ))
    });
    for (index, widget) in widgets.iter_mut().enumerate() {
        widget.tab_order =
            u32::try_from(index).map_err(|_| FormProjectionError::GeometryOverflow)?;
    }
    review.sort_by(|left, right| left.field_id.cmp(&right.field_id));

    let mut projection = FormWidgetProjection {
        schema_version: FORM_PROJECTION_SCHEMA_VERSION,
        source_revision: document.revision,
        source_hash: display_list.source_hash.clone(),
        display_list_hash: display_list.result_hash.clone(),
        widgets,
        review,
        result_hash: String::new(),
    };
    let bytes = serde_json::to_vec(&projection).map_err(|_| FormProjectionError::Serialization)?;
    projection.result_hash = blake3::hash(&bytes).to_hex().to_string();
    Ok(projection)
}

fn review_for_missing_mapping(
    field: &FieldDescriptor,
    original: &LogicalPosition,
) -> FormWidgetReview {
    let reason = match &field.anchor {
        FieldAnchorState::LegacyInvalid { reason, .. } => FormWidgetReviewReason::LegacyInvalid {
            reason: reason.clone(),
        },
        FieldAnchorState::TargetDeleted { .. } => FormWidgetReviewReason::TargetDeleted,
        FieldAnchorState::GraphemeSafe { .. } => FormWidgetReviewReason::SourceMappingMissing,
    };
    FormWidgetReview {
        field_id: field.id.clone(),
        source_node_id: original.node_id.clone(),
        reason,
    }
}

fn anchor_is_currently_valid(document: &FlowDocument, original: &LogicalPosition) -> bool {
    let Some(node) = find_node(&document.content, &original.node_id) else {
        return false;
    };
    NodePositionMap::new(node, document.revision)
        .ok()
        .is_some_and(|map| map.validate(original, document.revision).is_ok())
}

fn find_node<'a>(nodes: &'a [ContentNode], node_id: &NodeId) -> Option<&'a ContentNode> {
    for node in nodes {
        if node.id == *node_id {
            return Some(node);
        }
        if let Some(found) = find_node(node.children(), node_id) {
            return Some(found);
        }
    }
    None
}

fn anchor_line<'a>(
    display_list: &'a PdfDisplayList,
    position: &LogicalPosition,
) -> Option<(u32, &'a PdfTextLine)> {
    let mut candidates = Vec::new();
    for page in &display_list.pages {
        for item in &page.items {
            if item.source_node_id.as_ref() != Some(&position.node_id) {
                continue;
            }
            for line in &item.lines {
                if line.source.utf16_start <= position.utf16_offset.get()
                    && position.utf16_offset.get() <= line.source.utf16_end
                {
                    candidates.push((page.page_index, line));
                }
            }
        }
    }
    candidates.sort_by_key(|(page_index, line)| {
        (
            *page_index,
            line.rect.y.raw(),
            line.rect.x.raw(),
            line.source.utf16_start,
        )
    });
    let preferred = match position.affinity {
        Affinity::Forward => candidates
            .iter()
            .find(|(_, line)| line.source.utf16_start == position.utf16_offset.get()),
        Affinity::Backward => candidates
            .iter()
            .rev()
            .find(|(_, line)| line.source.utf16_end == position.utf16_offset.get()),
    };
    preferred.copied().or_else(|| candidates.into_iter().next())
}

fn widget_rect(
    line: &PdfTextLine,
    page: &crate::pdf::PdfDisplayPage,
    field: &FieldDescriptor,
) -> Result<LayoutRect, FormProjectionError> {
    let x = anchor_x(line, field.anchor.original().utf16_offset.get())?;
    let line_height = line.rect.height.raw();
    let (requested_width, height_multiplier) = match field.kind {
        FieldKind::Text {
            multiline: true, ..
        } => (TEXT_WIDGET_WIDTH, 3_i64),
        FieldKind::Text {
            multiline: false, ..
        } => (TEXT_WIDGET_WIDTH, 1_i64),
        FieldKind::Checkbox | FieldKind::RadioGroup => (CHECKBOX_WIDGET_SIZE, 1_i64),
        FieldKind::Select { .. } => (SELECT_WIDGET_WIDTH, 1_i64),
        FieldKind::Signature => (SIGNATURE_WIDGET_WIDTH, 2_i64),
        FieldKind::Button => (BUTTON_WIDGET_WIDTH, 1_i64),
    };
    let height = line_height
        .checked_mul(height_multiplier)
        .ok_or(FormProjectionError::GeometryOverflow)?;
    let content_right = page
        .bounds
        .x
        .raw()
        .checked_add(page.bounds.width.raw())
        .ok_or(FormProjectionError::GeometryOverflow)?;
    let content_bottom = page
        .bounds
        .y
        .raw()
        .checked_add(page.bounds.height.raw())
        .ok_or(FormProjectionError::GeometryOverflow)?;
    let origin_x = x.raw().max(page.bounds.x.raw());
    let origin_y = line.rect.y.raw().max(page.bounds.y.raw());
    let available_width = content_right
        .checked_sub(origin_x)
        .ok_or(FormProjectionError::GeometryOverflow)?;
    let available_height = content_bottom
        .checked_sub(origin_y)
        .ok_or(FormProjectionError::GeometryOverflow)?;
    if available_width <= 0 || available_height <= 0 {
        return Err(FormProjectionError::GeometryOverflow);
    }
    Ok(LayoutRect {
        x: LayoutUnit::from_raw(origin_x),
        y: LayoutUnit::from_raw(origin_y),
        width: LayoutUnit::from_raw(requested_width.min(available_width)),
        height: LayoutUnit::from_raw(height.min(available_height)),
    })
}

fn anchor_x(line: &PdfTextLine, offset: u32) -> Result<LayoutUnit, FormProjectionError> {
    if offset <= line.source.utf16_start {
        return Ok(line.rect.x);
    }
    if offset >= line.source.utf16_end {
        return line
            .rect
            .x
            .checked_add(line.width)
            .map_err(|_| FormProjectionError::GeometryOverflow);
    }
    let mut previous = line.rect.x;
    for glyph in &line.glyphs {
        if glyph.cluster_utf16 >= offset {
            return Ok(if glyph.cluster_utf16 == offset {
                glyph.x
            } else {
                previous
            });
        }
        previous = glyph_end(glyph)?;
    }
    Ok(previous)
}

fn glyph_end(glyph: &PdfGlyphPlacement) -> Result<LayoutUnit, FormProjectionError> {
    glyph
        .x
        .checked_add(glyph.x_advance)
        .map_err(|_| FormProjectionError::GeometryOverflow)
}

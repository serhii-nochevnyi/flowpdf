//! Bounded AcroForm field and widget emission for owned PDF exports.
//!
//! This adapter consumes a validated Rust form projection. It owns only
//! derived PDF field structure; authored defaults, current session values, and
//! page/widget geometry remain owned by their respective Rust contracts.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    canonical::{canonical_bytes, canonical_hash},
    forms::{
        FORM_PROJECTION_SCHEMA_VERSION, FormValueErrorCode, FormWidgetProjection,
        validate_field_value,
    },
    layout::{LayoutRect, LayoutUnit},
    model::{FieldDescriptor, FieldId, FieldKind, FieldOption, FieldValue, FlowDocument},
    schema::validate_document,
};

use super::{CosDocument, CosValue, PdfError, PdfName, PdfRef};

/// Version of the derived PDF form plan.
pub const PDF_FORM_SCHEMA_VERSION: u32 = 1;

const MAX_PDF_FORM_FIELDS: usize = 2_048;
const MAX_PDF_FORM_OPTIONS: usize = 4_096;
const MAX_PDF_FORM_TEXT_BYTES: usize = 64 * 1024;
const FIELD_FLAG_READ_ONLY: u32 = 1;
const FIELD_FLAG_REQUIRED: u32 = 1 << 1;
const FIELD_FLAG_MULTILINE: u32 = 1 << 12;
const FIELD_FLAG_RADIO: u32 = 1 << 15;
const FIELD_FLAG_PUSH_BUTTON: u32 = 1 << 16;
const FIELD_FLAG_COMBO: u32 = 1 << 17;
const FIELD_FLAG_MULTI_SELECT: u32 = 1 << 20;
const APPEARANCE_FONT_RESOURCE: &str = "Helv";
const APPEARANCE_FONT_SIZE: &str = "10";

/// The PDF field type emitted by the bounded form adapter.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PdfFormFieldType {
    Text,
    Button,
    Choice,
    Signature,
}

/// A bounded PDF value after semantic field conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum PdfFormValue {
    Empty,
    Text { value: String },
    Name { value: String },
    Names { values: Vec<String> },
}

/// One deterministic choice option carried by a PDF field plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfFormOption {
    pub name: String,
    pub label: String,
    pub export_value: String,
}

/// One source-bound field/widget plan ready for COS emission.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfFormField {
    pub field_id: FieldId,
    pub widget_id: String,
    pub name: String,
    pub label: Option<String>,
    pub field_type: PdfFormFieldType,
    pub flags: u32,
    pub default_value: PdfFormValue,
    pub value: PdfFormValue,
    pub tab_order: u32,
    pub options: Vec<PdfFormOption>,
    pub page_index: u32,
    pub rect: LayoutRect,
}

/// Complete derived form plan bound to one document/display-list identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfFormPlan {
    pub schema_version: u32,
    pub source_revision: u32,
    pub source_hash: String,
    pub display_list_hash: String,
    pub fields: Vec<PdfFormField>,
    pub result_hash: String,
}

/// Failure taxonomy for the bounded PDF form adapter.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PdfFormError {
    #[error("the canonical document is invalid for PDF forms")]
    InvalidDocument,
    #[error("the PDF form projection schema version is unsupported")]
    ProjectionSchema,
    #[error("the PDF form plan schema version is unsupported")]
    PlanSchema,
    #[error("the PDF form projection revision does not match the document")]
    RevisionMismatch,
    #[error("the PDF form projection source hash does not match the document")]
    SourceHashMismatch,
    #[error("the PDF form projection has no display-list identity")]
    DisplayListIdentityMissing,
    #[error("the PDF form projection contains fields requiring review")]
    ReviewRequired,
    #[error("the PDF form field count exceeds the limit")]
    FieldLimit,
    #[error("the PDF form option count exceeds the limit")]
    OptionLimit,
    #[error("the PDF form field is missing from the document")]
    FieldMissing,
    #[error("the PDF form projection does not match the document field")]
    FieldMismatch,
    #[error("the PDF form field identity is invalid")]
    InvalidIdentity,
    #[error("the PDF form field value is invalid: {0:?}")]
    InvalidValue(FormValueErrorCode),
    #[error("the PDF form field geometry is invalid")]
    InvalidGeometry,
    #[error("the PDF form field page is outside the export")]
    PageOutOfRange,
    #[error("the PDF form option is invalid")]
    InvalidOption,
    #[error("the PDF form tab order is invalid")]
    InvalidTabOrder,
    #[error("flattening requires a validated PDF form plan")]
    FlatteningRequiresPlan,
    #[error("the PDF form flattening selection is invalid")]
    InvalidFlatteningSelection,
    #[error("the PDF form plan could not be serialized")]
    Serialization,
}

impl PdfFormError {
    /// Returns a stable diagnostic code suitable for boundary adapters.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidDocument => "FLOW_PDF_FORM_DOCUMENT_INVALID",
            Self::ProjectionSchema => "FLOW_PDF_FORM_PROJECTION_SCHEMA_UNSUPPORTED",
            Self::PlanSchema => "FLOW_PDF_FORM_PLAN_SCHEMA_UNSUPPORTED",
            Self::RevisionMismatch => "FLOW_PDF_FORM_REVISION_MISMATCH",
            Self::SourceHashMismatch => "FLOW_PDF_FORM_SOURCE_HASH_MISMATCH",
            Self::DisplayListIdentityMissing => "FLOW_PDF_FORM_DISPLAY_IDENTITY_MISSING",
            Self::ReviewRequired => "FLOW_PDF_FORM_REVIEW_REQUIRED",
            Self::FieldLimit => "FLOW_PDF_FORM_FIELD_LIMIT",
            Self::OptionLimit => "FLOW_PDF_FORM_OPTION_LIMIT",
            Self::FieldMissing => "FLOW_PDF_FORM_FIELD_MISSING",
            Self::FieldMismatch => "FLOW_PDF_FORM_FIELD_MISMATCH",
            Self::InvalidIdentity => "FLOW_PDF_FORM_IDENTITY_INVALID",
            Self::InvalidValue(_) => "FLOW_PDF_FORM_VALUE_INVALID",
            Self::InvalidGeometry => "FLOW_PDF_FORM_GEOMETRY_INVALID",
            Self::PageOutOfRange => "FLOW_PDF_FORM_PAGE_OUT_OF_RANGE",
            Self::InvalidOption => "FLOW_PDF_FORM_OPTION_INVALID",
            Self::InvalidTabOrder => "FLOW_PDF_FORM_TAB_ORDER_INVALID",
            Self::FlatteningRequiresPlan => "FLOW_PDF_FORM_FLATTENING_REQUIRES_PLAN",
            Self::InvalidFlatteningSelection => "FLOW_PDF_FORM_FLATTENING_INVALID",
            Self::Serialization => "FLOW_PDF_FORM_SERIALIZATION",
        }
    }
}

/// Builds a PDF form plan from a validated document and session-aware widget
/// projection. The plan contains no indirect object references yet.
pub fn build_pdf_form_plan(
    document: &FlowDocument,
    projection: &FormWidgetProjection,
) -> Result<PdfFormPlan, PdfFormError> {
    validate_document(document).map_err(|_| PdfFormError::InvalidDocument)?;
    if projection.schema_version != FORM_PROJECTION_SCHEMA_VERSION {
        return Err(PdfFormError::ProjectionSchema);
    }
    if projection.source_revision != document.revision {
        return Err(PdfFormError::RevisionMismatch);
    }
    let canonical = canonical_bytes(document).map_err(|_| PdfFormError::InvalidDocument)?;
    if projection.source_hash != canonical_hash(&canonical) {
        return Err(PdfFormError::SourceHashMismatch);
    }
    if projection.display_list_hash.is_empty() {
        return Err(PdfFormError::DisplayListIdentityMissing);
    }
    if !projection.review.is_empty() {
        return Err(PdfFormError::ReviewRequired);
    }
    if projection.widgets.len() > MAX_PDF_FORM_FIELDS {
        return Err(PdfFormError::FieldLimit);
    }

    let mut fields = Vec::with_capacity(projection.widgets.len());
    let mut field_ids = BTreeSet::new();
    let mut widget_ids = BTreeSet::new();
    let mut option_count = 0_usize;
    for widget in &projection.widgets {
        if !field_ids.insert(widget.field_id.as_str()) || !widget_ids.insert(&widget.widget_id) {
            return Err(PdfFormError::InvalidIdentity);
        }
        let field = document
            .fields
            .iter()
            .find(|field| field.id == widget.field_id)
            .ok_or(PdfFormError::FieldMissing)?;
        if field.name != widget.name
            || field.label != widget.label
            || field.kind != widget.kind
            || field.required != widget.required
            || field.read_only != widget.read_only
            || field.default_value != widget.default_value
        {
            return Err(PdfFormError::FieldMismatch);
        }
        if widget.rect.width.raw() <= 0 || widget.rect.height.raw() <= 0 {
            return Err(PdfFormError::InvalidGeometry);
        }
        let mut validation_field = field.clone();
        validation_field.required = false;
        validate_field_value(&validation_field, &widget.default_value)
            .map_err(PdfFormError::InvalidValue)?;
        validate_field_value(&validation_field, &widget.value)
            .map_err(PdfFormError::InvalidValue)?;
        let (field_type, flags, options) = field_type_and_options(field)?;
        option_count = option_count
            .checked_add(options.len())
            .ok_or(PdfFormError::OptionLimit)?;
        if option_count > MAX_PDF_FORM_OPTIONS {
            return Err(PdfFormError::OptionLimit);
        }
        fields.push(PdfFormField {
            field_id: field.id.clone(),
            widget_id: widget.widget_id.clone(),
            name: field.name.clone(),
            label: field.label.clone(),
            field_type,
            flags: flags | flags_for_field(field),
            default_value: to_pdf_value(field, &widget.default_value)?,
            value: to_pdf_value(field, &widget.value)?,
            tab_order: widget.tab_order,
            options,
            page_index: widget.page_index,
            rect: widget.rect,
        });
    }

    let mut plan = PdfFormPlan {
        schema_version: PDF_FORM_SCHEMA_VERSION,
        source_revision: document.revision,
        source_hash: projection.source_hash.clone(),
        display_list_hash: projection.display_list_hash.clone(),
        fields,
        result_hash: String::new(),
    };
    let bytes = serde_json::to_vec(&plan).map_err(|_| PdfFormError::Serialization)?;
    plan.result_hash = blake3::hash(&bytes).to_hex().to_string();
    validate_pdf_form_plan(&plan)?;
    Ok(plan)
}

pub(crate) fn validate_pdf_form_plan(plan: &PdfFormPlan) -> Result<(), PdfFormError> {
    if plan.schema_version != PDF_FORM_SCHEMA_VERSION {
        return Err(PdfFormError::PlanSchema);
    }
    if plan.source_hash.trim().is_empty()
        || plan.source_hash.len() > super::MAX_IDENTITY_BYTES
        || !plan.source_hash.is_ascii()
        || plan.display_list_hash.trim().is_empty()
        || plan.display_list_hash.len() > super::MAX_IDENTITY_BYTES
        || !plan.display_list_hash.is_ascii()
    {
        return Err(PdfFormError::InvalidIdentity);
    }
    if plan.fields.len() > MAX_PDF_FORM_FIELDS {
        return Err(PdfFormError::FieldLimit);
    }
    let mut field_ids = BTreeSet::new();
    let mut widget_ids = BTreeSet::new();
    let mut field_names = BTreeSet::new();
    let mut tab_orders = BTreeSet::new();
    let mut option_count = 0_usize;
    for field in &plan.fields {
        if field.name.trim().is_empty()
            || field.name.len() > 512
            || !field_ids.insert(field.field_id.as_str())
            || !field_names.insert(field.name.as_str())
            || field.widget_id.trim().is_empty()
            || field.widget_id.len() > super::MAX_IDENTITY_BYTES
            || !field.widget_id.is_ascii()
            || !widget_ids.insert(field.widget_id.as_str())
        {
            return Err(PdfFormError::InvalidIdentity);
        }
        if !tab_orders.insert(field.tab_order) {
            return Err(PdfFormError::InvalidTabOrder);
        }
        option_count = option_count
            .checked_add(field.options.len())
            .ok_or(PdfFormError::OptionLimit)?;
        if option_count > MAX_PDF_FORM_OPTIONS {
            return Err(PdfFormError::OptionLimit);
        }
        let mut option_names = BTreeSet::new();
        let mut export_values = BTreeSet::new();
        for option in &field.options {
            if option.name.trim().is_empty()
                || option.name.len() > super::MAX_IDENTITY_BYTES
                || !option.name.is_ascii()
                || PdfName::new(option.name.clone()).is_err()
                || option.label.is_empty()
                || option.label.len() > MAX_PDF_FORM_TEXT_BYTES
                || option.export_value.is_empty()
                || option.export_value.len() > MAX_PDF_FORM_TEXT_BYTES
                || !option_names.insert(option.name.as_str())
                || !export_values.insert(option.export_value.as_str())
            {
                return Err(PdfFormError::InvalidOption);
            }
        }
        validate_pdf_value(field.field_type, &field.default_value)?;
        validate_pdf_value(field.field_type, &field.value)?;
        validate_admitted_value(field, &field.default_value)?;
        validate_admitted_value(field, &field.value)?;
    }
    if tab_orders.len() != plan.fields.len()
        || !(0..u32::try_from(plan.fields.len()).map_err(|_| PdfFormError::FieldLimit)?)
            .all(|tab_order| tab_orders.contains(&tab_order))
    {
        return Err(PdfFormError::InvalidTabOrder);
    }
    if plan.result_hash.trim().is_empty()
        || plan.result_hash.len() > super::MAX_IDENTITY_BYTES
        || !plan.result_hash.is_ascii()
    {
        return Err(PdfFormError::Serialization);
    }
    let mut unsigned = plan.clone();
    unsigned.result_hash.clear();
    let bytes = serde_json::to_vec(&unsigned).map_err(|_| PdfFormError::Serialization)?;
    if blake3::hash(&bytes).to_hex().to_string() != plan.result_hash {
        return Err(PdfFormError::Serialization);
    }
    Ok(())
}

fn validate_pdf_value(
    field_type: PdfFormFieldType,
    value: &PdfFormValue,
) -> Result<(), PdfFormError> {
    match (field_type, value) {
        (PdfFormFieldType::Text, PdfFormValue::Empty)
        | (PdfFormFieldType::Text, PdfFormValue::Text { .. })
        | (PdfFormFieldType::Button, PdfFormValue::Empty)
        | (PdfFormFieldType::Button, PdfFormValue::Name { .. })
        | (PdfFormFieldType::Choice, PdfFormValue::Empty)
        | (PdfFormFieldType::Choice, PdfFormValue::Name { .. })
        | (PdfFormFieldType::Choice, PdfFormValue::Names { .. })
        | (PdfFormFieldType::Signature, PdfFormValue::Empty) => {}
        _ => {
            return Err(PdfFormError::InvalidValue(FormValueErrorCode::TypeMismatch));
        }
    }
    match value {
        PdfFormValue::Empty => Ok(()),
        PdfFormValue::Text { value } => {
            if value.len() > MAX_PDF_FORM_TEXT_BYTES {
                return Err(PdfFormError::InvalidValue(
                    FormValueErrorCode::TextSizeLimit,
                ));
            }
            Ok(())
        }
        PdfFormValue::Name { value } => PdfName::new(value.clone())
            .map(|_| ())
            .map_err(|_| PdfFormError::InvalidValue(FormValueErrorCode::UnsupportedValue)),
        PdfFormValue::Names { values } => {
            if values.is_empty() {
                return Err(PdfFormError::InvalidValue(
                    FormValueErrorCode::UnsupportedValue,
                ));
            }
            for value in values {
                PdfName::new(value.clone()).map_err(|_| {
                    PdfFormError::InvalidValue(FormValueErrorCode::UnsupportedValue)
                })?;
            }
            Ok(())
        }
    }
}

fn validate_admitted_value(field: &PdfFormField, value: &PdfFormValue) -> Result<(), PdfFormError> {
    let unsupported = || PdfFormError::InvalidValue(FormValueErrorCode::OptionSetInvalid);
    match field.field_type {
        PdfFormFieldType::Button if field.flags & FIELD_FLAG_PUSH_BUTTON != 0 => {
            if !matches!(value, PdfFormValue::Empty) {
                return Err(unsupported());
            }
        }
        PdfFormFieldType::Button if field.flags & FIELD_FLAG_RADIO != 0 => {
            if let PdfFormValue::Name { value } = value
                && !field.options.iter().any(|option| option.name == *value)
            {
                return Err(unsupported());
            }
        }
        PdfFormFieldType::Button => {
            if !matches!(value, PdfFormValue::Name { value } if value == "Off" || value == "Yes") {
                return Err(unsupported());
            }
        }
        PdfFormFieldType::Choice => match value {
            PdfFormValue::Name { value } => {
                if !field.options.iter().any(|option| option.name == *value) {
                    return Err(unsupported());
                }
            }
            PdfFormValue::Names { values } => {
                if values
                    .iter()
                    .any(|value| !field.options.iter().any(|option| option.name == *value))
                {
                    return Err(unsupported());
                }
            }
            PdfFormValue::Empty | PdfFormValue::Text { .. } => {}
        },
        PdfFormFieldType::Text | PdfFormFieldType::Signature => {}
    }
    Ok(())
}

fn field_type_and_options(
    field: &FieldDescriptor,
) -> Result<(PdfFormFieldType, u32, Vec<PdfFormOption>), PdfFormError> {
    let (field_type, flags) = match field.kind {
        FieldKind::Text { .. } => (PdfFormFieldType::Text, 0),
        FieldKind::Checkbox | FieldKind::RadioGroup | FieldKind::Button => {
            (PdfFormFieldType::Button, 0)
        }
        FieldKind::Select { .. } => (PdfFormFieldType::Choice, 0),
        FieldKind::Signature => (PdfFormFieldType::Signature, 0),
    };
    let options = field
        .options
        .iter()
        .map(pdf_option)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((field_type, flags, options))
}

fn flags_for_field(field: &FieldDescriptor) -> u32 {
    let mut flags = 0;
    if field.read_only {
        flags |= FIELD_FLAG_READ_ONLY;
    }
    if field.required {
        flags |= FIELD_FLAG_REQUIRED;
    }
    match field.kind {
        FieldKind::Text { multiline, .. } if multiline => flags | FIELD_FLAG_MULTILINE,
        FieldKind::RadioGroup => flags | FIELD_FLAG_RADIO,
        FieldKind::Select { multiple: false } => flags | FIELD_FLAG_COMBO,
        FieldKind::Select { multiple: true } => flags | FIELD_FLAG_MULTI_SELECT,
        FieldKind::Button => flags | FIELD_FLAG_PUSH_BUTTON,
        _ => flags,
    }
}

fn pdf_option(option: &FieldOption) -> Result<PdfFormOption, PdfFormError> {
    if option.label.is_empty()
        || option.export_value.is_empty()
        || option.label.len() > MAX_PDF_FORM_TEXT_BYTES
        || option.export_value.len() > MAX_PDF_FORM_TEXT_BYTES
    {
        return Err(PdfFormError::InvalidOption);
    }
    Ok(PdfFormOption {
        name: option_name(&option.id),
        label: option.label.clone(),
        export_value: option.export_value.clone(),
    })
}

fn option_name(id: &crate::model::FieldOptionId) -> String {
    format!("flow-option-{}", id.as_str())
}

fn to_pdf_value(field: &FieldDescriptor, value: &FieldValue) -> Result<PdfFormValue, PdfFormError> {
    match (&field.kind, value) {
        (FieldKind::Text { .. }, FieldValue::Empty) => Ok(PdfFormValue::Empty),
        (FieldKind::Text { .. }, FieldValue::Text { value }) => {
            if value.len() > MAX_PDF_FORM_TEXT_BYTES {
                return Err(PdfFormError::InvalidValue(
                    FormValueErrorCode::TextSizeLimit,
                ));
            }
            Ok(PdfFormValue::Text {
                value: value.clone(),
            })
        }
        (FieldKind::Checkbox, FieldValue::Empty)
        | (FieldKind::Checkbox, FieldValue::Checked { value: false }) => Ok(PdfFormValue::Name {
            value: "Off".to_owned(),
        }),
        (FieldKind::Checkbox, FieldValue::Checked { value: true }) => Ok(PdfFormValue::Name {
            value: "Yes".to_owned(),
        }),
        (FieldKind::RadioGroup, FieldValue::Empty)
        | (FieldKind::Select { .. }, FieldValue::Empty) => Ok(PdfFormValue::Empty),
        (FieldKind::RadioGroup | FieldKind::Select { .. }, FieldValue::Selected { option_ids }) => {
            let available = field
                .options
                .iter()
                .map(|option| (option.id.as_str(), option_name(&option.id)))
                .collect::<std::collections::BTreeMap<_, _>>();
            let values = option_ids
                .iter()
                .map(|id| {
                    available
                        .get(id.as_str())
                        .cloned()
                        .ok_or(PdfFormError::InvalidOption)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if values.is_empty() {
                Ok(PdfFormValue::Empty)
            } else if matches!(field.kind, FieldKind::RadioGroup) {
                Ok(PdfFormValue::Name {
                    value: values[0].clone(),
                })
            } else {
                Ok(PdfFormValue::Names { values })
            }
        }
        (FieldKind::Signature | FieldKind::Button, FieldValue::Empty) => Ok(PdfFormValue::Empty),
        _ => Err(PdfFormError::InvalidValue(FormValueErrorCode::TypeMismatch)),
    }
}

pub(crate) struct PdfFormEmission {
    pub acro_form_ref: Option<PdfRef>,
    pub page_widgets: Vec<Vec<PdfRef>>,
    pub page_content: Vec<Vec<u8>>,
    pub flattened_font_ref: Option<PdfRef>,
}

pub(crate) fn validate_flattened_field_ids_shape(field_ids: &[String]) -> Result<(), PdfFormError> {
    if field_ids.len() > MAX_PDF_FORM_FIELDS {
        return Err(PdfFormError::FieldLimit);
    }
    let mut unique = BTreeSet::new();
    for field_id in field_ids {
        if field_id.trim().is_empty()
            || field_id.len() > super::MAX_IDENTITY_BYTES
            || !field_id.is_ascii()
            || !unique.insert(field_id.as_str())
        {
            return Err(PdfFormError::InvalidFlatteningSelection);
        }
    }
    Ok(())
}

pub(crate) fn normalized_flattened_field_ids(
    plan: Option<&PdfFormPlan>,
    field_ids: &[String],
) -> Result<Vec<String>, PdfError> {
    validate_flattened_field_ids_shape(field_ids)?;
    if field_ids.is_empty() {
        return Ok(Vec::new());
    }
    let plan = plan.ok_or(PdfFormError::FlatteningRequiresPlan)?;
    let selected = field_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if selected.iter().any(|field_id| {
        !plan
            .fields
            .iter()
            .any(|field| field.field_id.as_str() == *field_id)
    }) {
        return Err(PdfFormError::InvalidFlatteningSelection.into());
    }
    Ok(plan
        .fields
        .iter()
        .filter(|field| selected.contains(field.field_id.as_str()))
        .map(|field| field.field_id.as_str().to_owned())
        .collect())
}

pub(crate) fn emit_form_objects(
    document: &mut CosDocument,
    plan: &PdfFormPlan,
    pages: &[(PdfRef, PdfRef, LayoutRect)],
    flattened_field_ids: &[String],
) -> Result<PdfFormEmission, PdfError> {
    validate_pdf_form_plan(plan)?;
    let flattened_field_ids = normalized_flattened_field_ids(Some(plan), flattened_field_ids)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut page_widgets = vec![Vec::new(); pages.len()];
    let mut page_content = vec![Vec::new(); pages.len()];
    let mut ordered_fields = plan.fields.iter().collect::<Vec<_>>();
    ordered_fields.sort_by_key(|field| field.tab_order);
    for field in &ordered_fields {
        let page_index =
            usize::try_from(field.page_index).map_err(|_| PdfFormError::PageOutOfRange)?;
        let (_, _, bounds) = pages.get(page_index).ok_or(PdfFormError::PageOutOfRange)?;
        validate_rect(field.rect, *bounds)?;
    }
    let appearance_font_ref = emit_appearance_font(document)?;
    for field in &ordered_fields {
        if !flattened_field_ids.contains(field.field_id.as_str()) {
            continue;
        }
        let page_index =
            usize::try_from(field.page_index).map_err(|_| PdfFormError::PageOutOfRange)?;
        let state = if field.field_type == PdfFormFieldType::Button
            && field.flags & FIELD_FLAG_PUSH_BUTTON == 0
        {
            appearance_state(&field.value)
        } else {
            None
        };
        page_content[page_index].extend(flattened_appearance_data(field, state)?);
    }

    let retained_fields = ordered_fields
        .iter()
        .filter(|field| !flattened_field_ids.contains(field.field_id.as_str()))
        .copied()
        .collect::<Vec<_>>();
    let mut field_refs = Vec::with_capacity(retained_fields.len());
    let mut widget_refs = Vec::with_capacity(retained_fields.len());
    for _ in &retained_fields {
        field_refs.push(document.add_object(CosValue::Null)?);
        widget_refs.push(document.add_object(CosValue::Null)?);
    }

    for ((field, field_ref), widget_ref) in retained_fields
        .iter()
        .zip(field_refs.iter().copied())
        .zip(widget_refs.iter().copied())
    {
        let page_index =
            usize::try_from(field.page_index).map_err(|_| PdfFormError::PageOutOfRange)?;
        let page_ref = pages[page_index].0;
        let mut field_entries = vec![
            (
                PdfName::new("FT")?,
                CosValue::name(field_type_name(field.field_type))?,
            ),
            (
                PdfName::new("Ff")?,
                CosValue::Integer(i64::from(field.flags)),
            ),
            (
                PdfName::new("Kids")?,
                CosValue::Array(vec![CosValue::Reference(widget_ref)]),
            ),
            (PdfName::new("T")?, pdf_string(&field.name)?),
        ];
        if let Some(label) = &field.label {
            field_entries.push((PdfName::new("TU")?, pdf_string(label)?));
        }
        if !field.options.is_empty() {
            field_entries.push((
                PdfName::new("Opt")?,
                CosValue::Array(
                    field
                        .options
                        .iter()
                        .map(|option| {
                            Ok::<CosValue, PdfError>(CosValue::Array(vec![
                                pdf_string(&option.export_value)?,
                                pdf_string(&option.label)?,
                            ]))
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            ));
        }
        if !matches!(field.default_value, PdfFormValue::Empty) {
            field_entries.push((PdfName::new("DV")?, value_to_cos(&field.default_value)?));
        }
        if !matches!(field.value, PdfFormValue::Empty) {
            field_entries.push((PdfName::new("V")?, value_to_cos(&field.value)?));
        }
        document.replace_object(field_ref, CosValue::dictionary(field_entries)?)?;

        let appearance = emit_field_appearance(document, field, appearance_font_ref)?;
        let mut widget_entries = vec![
            (PdfName::new("Parent")?, CosValue::Reference(field_ref)),
            (PdfName::new("P")?, CosValue::Reference(page_ref)),
            (PdfName::new("Rect")?, rect_to_cos(field.rect)?),
            (PdfName::new("Subtype")?, CosValue::name("Widget")?),
            (PdfName::new("Type")?, CosValue::name("Annot")?),
            (PdfName::new("AP")?, appearance),
        ];
        if let Some(appearance_state) = appearance_state(&field.value) {
            widget_entries.push((PdfName::new("AS")?, CosValue::name(appearance_state)?));
        }
        document.replace_object(widget_ref, CosValue::dictionary(widget_entries)?)?;
        page_widgets[page_index].push(widget_ref);
    }

    let acro_form_ref = if field_refs.is_empty() {
        None
    } else {
        Some(document.add_object(CosValue::dictionary([
            (
                PdfName::new("DA")?,
                pdf_string(&format!(
                    "/{APPEARANCE_FONT_RESOURCE} {APPEARANCE_FONT_SIZE} Tf 0 g"
                ))?,
            ),
            (
                PdfName::new("DR")?,
                CosValue::dictionary([(
                    PdfName::new("Font")?,
                    CosValue::dictionary([(
                        PdfName::new(APPEARANCE_FONT_RESOURCE)?,
                        CosValue::Reference(appearance_font_ref),
                    )])?,
                )])?,
            ),
            (
                PdfName::new("Fields")?,
                CosValue::Array(field_refs.into_iter().map(CosValue::Reference).collect()),
            ),
            (PdfName::new("NeedAppearances")?, CosValue::Boolean(false)),
        ])?)?)
    };
    Ok(PdfFormEmission {
        acro_form_ref,
        page_widgets,
        page_content,
        flattened_font_ref: (!flattened_field_ids.is_empty()).then_some(appearance_font_ref),
    })
}

fn emit_appearance_font(document: &mut CosDocument) -> Result<PdfRef, PdfError> {
    document.add_object(CosValue::dictionary([
        (PdfName::new("BaseFont")?, CosValue::name("Helvetica")?),
        (
            PdfName::new("Encoding")?,
            CosValue::name("WinAnsiEncoding")?,
        ),
        (PdfName::new("Subtype")?, CosValue::name("Type1")?),
        (PdfName::new("Type")?, CosValue::name("Font")?),
    ])?)
}

fn emit_field_appearance(
    document: &mut CosDocument,
    field: &PdfFormField,
    font_ref: PdfRef,
) -> Result<CosValue, PdfError> {
    if field.field_type == PdfFormFieldType::Button && field.flags & FIELD_FLAG_PUSH_BUTTON == 0 {
        let mut states = vec!["Off".to_owned()];
        if field.flags & FIELD_FLAG_RADIO != 0 {
            states.extend(field.options.iter().map(|option| option.name.clone()));
            if states.len() == 1 {
                states.push("Yes".to_owned());
            }
        } else {
            states.push("Yes".to_owned());
        }
        let normal_states = states
            .iter()
            .map(|state| {
                Ok::<(PdfName, CosValue), PdfError>((
                    PdfName::new(state.clone())?,
                    CosValue::Reference(emit_normal_appearance(
                        document,
                        field,
                        font_ref,
                        Some(state),
                    )?),
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        return CosValue::dictionary([(PdfName::new("N")?, CosValue::dictionary(normal_states)?)]);
    }

    let normal_ref = emit_normal_appearance(document, field, font_ref, None)?;
    CosValue::dictionary([(PdfName::new("N")?, CosValue::Reference(normal_ref))])
}

fn emit_normal_appearance(
    document: &mut CosDocument,
    field: &PdfFormField,
    font_ref: PdfRef,
    state: Option<&str>,
) -> Result<PdfRef, PdfError> {
    let data = appearance_stream_data(field, state)?;
    document.add_object(CosValue::stream(
        [
            (PdfName::new("BBox")?, appearance_bbox(field.rect)?),
            (PdfName::new("FormType")?, CosValue::Integer(1)),
            (PdfName::new("Resources")?, appearance_resources(font_ref)?),
            (PdfName::new("Subtype")?, CosValue::name("Form")?),
        ],
        data,
    )?)
}

fn appearance_bbox(rect: LayoutRect) -> Result<CosValue, PdfError> {
    if rect.width.raw() <= 0 || rect.height.raw() <= 0 {
        return Err(PdfError::InvalidPageGeometry);
    }
    Ok(CosValue::Array(vec![
        CosValue::Real(LayoutUnit::from_raw(0)),
        CosValue::Real(LayoutUnit::from_raw(0)),
        CosValue::Real(rect.width),
        CosValue::Real(rect.height),
    ]))
}

fn appearance_resources(font_ref: PdfRef) -> Result<CosValue, PdfError> {
    CosValue::dictionary([(
        PdfName::new("Font")?,
        CosValue::dictionary([(
            PdfName::new(APPEARANCE_FONT_RESOURCE)?,
            CosValue::Reference(font_ref),
        )])?,
    )])
}

fn appearance_stream_data(field: &PdfFormField, state: Option<&str>) -> Result<Vec<u8>, PdfError> {
    let width = super::format_layout_unit(field.rect.width);
    let height = super::format_layout_unit(field.rect.height);
    let mut data = Vec::new();
    super::append_ascii(
        &mut data,
        &format!(
            "q\n0.95 0.95 0.95 rg\n0 0 {width} {height} re\nf\n0 0 0 RG\n1 w\n0 0 {width} {height} re\nS\nQ\n"
        ),
    )?;

    if let Some(state) = state {
        if state != "Off" {
            if field.flags & FIELD_FLAG_RADIO != 0 {
                super::append_ascii(
                    &mut data,
                    &format!(
                        "q\n0 0 0 rg\n{} {} {} {} re\nf\nQ\n",
                        super::format_layout_unit(LayoutUnit::from_raw(4 * 64)),
                        super::format_layout_unit(LayoutUnit::from_raw(4 * 64)),
                        super::format_layout_unit(LayoutUnit::from_raw(
                            field.rect.width.raw().saturating_sub(8 * 64).max(1),
                        )),
                        super::format_layout_unit(LayoutUnit::from_raw(
                            field.rect.height.raw().saturating_sub(8 * 64).max(1),
                        )),
                    ),
                )?;
            } else {
                super::append_ascii(
                    &mut data,
                    &format!("0 0 m\n{width} {height} l\n0 {height} m\n{width} 0 l\nS\n"),
                )?;
            }
        }
    } else if let Some(value) = appearance_text(field) {
        let encoded = appearance_text_hex(&value)?;
        let baseline = field
            .rect
            .height
            .raw()
            .checked_sub(14 * 64)
            .unwrap_or(4 * 64)
            .max(4 * 64);
        super::append_ascii(
            &mut data,
            &format!(
                "BT\n/{APPEARANCE_FONT_RESOURCE} {APPEARANCE_FONT_SIZE} Tf\n0 0 0 rg\n2 {} Td\n<{encoded}> Tj\nET\n",
                super::format_layout_unit(LayoutUnit::from_raw(baseline)),
            ),
        )?;
    }

    Ok(data)
}

fn flattened_appearance_data(
    field: &PdfFormField,
    state: Option<&str>,
) -> Result<Vec<u8>, PdfError> {
    let mut data = Vec::new();
    super::append_ascii(
        &mut data,
        &format!(
            "q\n1 0 0 1 {} {} cm\n",
            super::format_layout_unit(field.rect.x),
            super::format_layout_unit(field.rect.y),
        ),
    )?;
    data.extend_from_slice(&appearance_stream_data(field, state)?);
    super::append_ascii(&mut data, "Q\n")?;
    Ok(data)
}

fn appearance_text(field: &PdfFormField) -> Option<String> {
    match &field.value {
        PdfFormValue::Text { value } | PdfFormValue::Name { value } => Some(value.clone()),
        PdfFormValue::Names { values } => Some(values.join(", ")),
        PdfFormValue::Empty => None,
    }
}

fn appearance_text_hex(value: &str) -> Result<String, PdfError> {
    if value.len() > MAX_PDF_FORM_TEXT_BYTES {
        return Err(PdfError::StringSizeLimit);
    }
    let mut bytes = Vec::with_capacity(value.len().saturating_mul(2).saturating_add(2));
    bytes.extend_from_slice(&[0xFE, 0xFF]);
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Ok(super::hex_bytes(&bytes))
}

fn field_type_name(field_type: PdfFormFieldType) -> &'static str {
    match field_type {
        PdfFormFieldType::Text => "Tx",
        PdfFormFieldType::Button => "Btn",
        PdfFormFieldType::Choice => "Ch",
        PdfFormFieldType::Signature => "Sig",
    }
}

fn pdf_string(value: &str) -> Result<CosValue, PdfError> {
    CosValue::string(value.as_bytes().to_vec())
}

fn value_to_cos(value: &PdfFormValue) -> Result<CosValue, PdfError> {
    match value {
        PdfFormValue::Empty => Ok(CosValue::Null),
        PdfFormValue::Text { value } => pdf_string(value),
        PdfFormValue::Name { value } => CosValue::name(value.clone()),
        PdfFormValue::Names { values } => Ok(CosValue::Array(
            values
                .iter()
                .map(|value| CosValue::name(value.clone()))
                .collect::<Result<Vec<_>, _>>()?,
        )),
    }
}

fn appearance_state(value: &PdfFormValue) -> Option<&str> {
    match value {
        PdfFormValue::Name { value } => Some(value.as_str()),
        PdfFormValue::Names { values } => values.first().map(String::as_str),
        PdfFormValue::Empty | PdfFormValue::Text { .. } => None,
    }
}

fn rect_to_cos(rect: LayoutRect) -> Result<CosValue, PdfError> {
    let right = rect
        .x
        .raw()
        .checked_add(rect.width.raw())
        .ok_or(PdfError::NumericLimit)?;
    let bottom = rect
        .y
        .raw()
        .checked_add(rect.height.raw())
        .ok_or(PdfError::NumericLimit)?;
    Ok(CosValue::Array(vec![
        CosValue::Real(rect.x),
        CosValue::Real(rect.y),
        CosValue::Real(LayoutUnit::from_raw(right)),
        CosValue::Real(LayoutUnit::from_raw(bottom)),
    ]))
}

fn validate_rect(rect: LayoutRect, bounds: LayoutRect) -> Result<(), PdfFormError> {
    if rect.width.raw() <= 0 || rect.height.raw() <= 0 {
        return Err(PdfFormError::InvalidGeometry);
    }
    let right = rect
        .x
        .raw()
        .checked_add(rect.width.raw())
        .ok_or(PdfFormError::InvalidGeometry)?;
    let bottom = rect
        .y
        .raw()
        .checked_add(rect.height.raw())
        .ok_or(PdfFormError::InvalidGeometry)?;
    let bounds_right = bounds
        .x
        .raw()
        .checked_add(bounds.width.raw())
        .ok_or(PdfFormError::InvalidGeometry)?;
    let bounds_bottom = bounds
        .y
        .raw()
        .checked_add(bounds.height.raw())
        .ok_or(PdfFormError::InvalidGeometry)?;
    if rect.x.raw() < bounds.x.raw()
        || rect.y.raw() < bounds.y.raw()
        || right > bounds_right
        || bottom > bounds_bottom
    {
        return Err(PdfFormError::InvalidGeometry);
    }
    Ok(())
}

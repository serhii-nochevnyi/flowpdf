//! Bounded, lazy reader for the controlled PDF syntax subset.
//!
//! This module deliberately indexes a classic xref table at open time and
//! parses indirect objects only when a page/scene request needs them.  It is a
//! reader boundary, not a general-purpose PDF repair engine: unsupported
//! syntax is classified explicitly and active content is never interpreted.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    io::Cursor as IoCursor,
};

use image::{ColorType, ImageDecoder, Limits, codecs::jpeg::JpegDecoder};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::layout::{LayoutRect, LayoutUnit};

pub const PDF_READER_SCHEMA_VERSION: u32 = 1;

const MAX_PDF_VERSION: &[u8] = b"1.7";
const MAX_PDF_IDENTITY_BYTES: usize = 128;

const fn default_scene_bytes() -> usize {
    16 * 1024 * 1024
}

/// Limits applied to one request-local PDF reader.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfReaderLimits {
    pub max_input_bytes: usize,
    pub max_objects: usize,
    pub max_pages: usize,
    pub max_nesting: usize,
    pub max_string_bytes: usize,
    pub max_array_items: usize,
    pub max_dictionary_entries: usize,
    pub max_stream_bytes: usize,
    pub max_decoded_stream_bytes: usize,
    pub max_content_tokens: usize,
    pub max_scene_elements: usize,
    #[serde(default = "default_scene_bytes")]
    pub max_scene_bytes: usize,
    pub max_image_pixels: u64,
    pub max_form_depth: usize,
}

impl Default for PdfReaderLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 64 * 1024 * 1024,
            max_objects: 16_384,
            max_pages: 2_048,
            max_nesting: 64,
            max_string_bytes: 64 * 1024,
            max_array_items: 16_384,
            max_dictionary_entries: 4_096,
            max_stream_bytes: 8 * 1024 * 1024,
            max_decoded_stream_bytes: 16 * 1024 * 1024,
            max_content_tokens: 1_000_000,
            max_scene_elements: 250_000,
            max_scene_bytes: default_scene_bytes(),
            max_image_pixels: 64 * 1024 * 1024,
            max_form_depth: 16,
        }
    }
}

impl PdfReaderLimits {
    fn validate(&self) -> Result<(), PdfReadError> {
        if self.max_input_bytes == 0
            || self.max_objects == 0
            || self.max_pages == 0
            || self.max_nesting == 0
            || self.max_string_bytes == 0
            || self.max_array_items == 0
            || self.max_dictionary_entries == 0
            || self.max_stream_bytes == 0
            || self.max_decoded_stream_bytes == 0
            || self.max_content_tokens == 0
            || self.max_scene_elements == 0
            || !(1..=default_scene_bytes()).contains(&self.max_scene_bytes)
            || self.max_image_pixels == 0
            || self.max_form_depth == 0
        {
            return Err(PdfReadError::InvalidLimits);
        }
        Ok(())
    }
}

/// Severity of a non-fatal reader diagnostic.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PdfReadSeverity {
    Warning,
    Blocked,
}

/// Closed diagnostic vocabulary shared by the core, WASM, and browser panel.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PdfReadDiagnosticCode {
    UnsupportedFilter,
    UnsupportedFont,
    MissingUnicodeMapping,
    UnsupportedOperator,
    UnsupportedAnnotation,
    UnsupportedXref,
    UncertainReadingOrder,
    ActiveContent,
    ExternalResource,
    Encryption,
    ObjectStream,
    ImageLimit,
    SceneLimit,
    ContentLimit,
    FormDepthLimit,
    MalformedAnnotation,
    PartialPage,
}

impl PdfReadDiagnosticCode {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedFilter => "FLOW_PDF_READER_FILTER_UNSUPPORTED",
            Self::UnsupportedFont => "FLOW_PDF_READER_FONT_UNSUPPORTED",
            Self::MissingUnicodeMapping => "FLOW_PDF_READER_UNICODE_MAPPING_MISSING",
            Self::UnsupportedOperator => "FLOW_PDF_READER_OPERATOR_UNSUPPORTED",
            Self::UnsupportedAnnotation => "FLOW_PDF_READER_ANNOTATION_UNSUPPORTED",
            Self::UnsupportedXref => "FLOW_PDF_READER_XREF_UNSUPPORTED",
            Self::UncertainReadingOrder => "FLOW_PDF_READER_READING_ORDER_UNCERTAIN",
            Self::ActiveContent => "FLOW_PDF_READER_ACTIVE_CONTENT_BLOCKED",
            Self::ExternalResource => "FLOW_PDF_READER_EXTERNAL_RESOURCE_BLOCKED",
            Self::Encryption => "FLOW_PDF_READER_ENCRYPTION_UNSUPPORTED",
            Self::ObjectStream => "FLOW_PDF_READER_OBJECT_STREAM_UNSUPPORTED",
            Self::ImageLimit => "FLOW_PDF_READER_IMAGE_LIMIT",
            Self::SceneLimit => "FLOW_PDF_READER_SCENE_LIMIT",
            Self::ContentLimit => "FLOW_PDF_READER_CONTENT_LIMIT",
            Self::FormDepthLimit => "FLOW_PDF_READER_FORM_DEPTH_LIMIT",
            Self::MalformedAnnotation => "FLOW_PDF_READER_ANNOTATION_MALFORMED",
            Self::PartialPage => "FLOW_PDF_READER_PAGE_PARTIAL",
        }
    }
}

/// A privacy-safe unsupported/blocked condition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfReadDiagnostic {
    pub code: PdfReadDiagnosticCode,
    pub severity: PdfReadSeverity,
    pub page_index: Option<u32>,
    pub object_number: Option<u32>,
    pub operator: Option<String>,
}

impl PdfReadDiagnostic {
    #[must_use]
    pub const fn blocked(
        code: PdfReadDiagnosticCode,
        page_index: Option<u32>,
        object_number: Option<u32>,
    ) -> Self {
        Self {
            code,
            severity: PdfReadSeverity::Blocked,
            page_index,
            object_number,
            operator: None,
        }
    }

    #[must_use]
    pub const fn warning(
        code: PdfReadDiagnosticCode,
        page_index: Option<u32>,
        object_number: Option<u32>,
    ) -> Self {
        Self {
            code,
            severity: PdfReadSeverity::Warning,
            page_index,
            object_number,
            operator: None,
        }
    }

    #[must_use]
    pub fn with_operator(mut self, operator: &str) -> Self {
        self.operator = Some(operator.to_owned());
        self
    }
}

/// Report attached to every scene result.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfReadReport {
    pub diagnostics: Vec<PdfReadDiagnostic>,
    pub partial: bool,
}

impl PdfReadReport {
    pub(crate) fn push(&mut self, diagnostic: PdfReadDiagnostic) {
        if diagnostic.severity == PdfReadSeverity::Blocked {
            self.partial = true;
        }
        if !self.diagnostics.contains(&diagnostic) {
            self.diagnostics.push(diagnostic);
        }
    }

    pub(crate) fn extend(&mut self, other: &Self) {
        for diagnostic in &other.diagnostics {
            self.push(diagnostic.clone());
        }
        self.partial |= other.partial;
    }
}

/// Stable failure taxonomy for structural or budget failures.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PdfReadError {
    #[error("the PDF input exceeds the reader byte limit")]
    InputSizeLimit,
    #[error("the PDF reader limits are invalid")]
    InvalidLimits,
    #[error("the PDF header is missing or invalid")]
    HeaderInvalid,
    #[error("the PDF version is outside the supported subset")]
    VersionUnsupported,
    #[error("the PDF startxref marker is missing or invalid")]
    StartxrefInvalid,
    #[error("the PDF classic xref table is invalid")]
    XrefInvalid,
    #[error("the PDF xref/object stream form is unsupported")]
    XrefUnsupported,
    #[error("the PDF declares unsupported encryption")]
    EncryptionUnsupported,
    #[error("the PDF root catalog is missing")]
    RootMissing,
    #[error("the PDF object is missing from the xref table")]
    ObjectMissing,
    #[error("the PDF object number limit was exceeded")]
    ObjectLimit,
    #[error("the PDF nesting limit was exceeded")]
    NestingLimit,
    #[error("the PDF token limit was exceeded")]
    TokenLimit,
    #[error("the PDF stream limit was exceeded")]
    StreamLimit,
    #[error("the PDF decoded stream limit was exceeded")]
    DecodedStreamLimit,
    #[error("the PDF syntax is malformed")]
    Syntax,
    #[error("the PDF reference is invalid")]
    ReferenceInvalid,
    #[error("the PDF page tree is malformed or cyclic")]
    PageTreeInvalid,
    #[error("the PDF page count limit was exceeded")]
    PageCountLimit,
    #[error("the PDF number is outside the fixed-point range")]
    NumericLimit,
    #[error("the PDF stream filter is outside the supported subset")]
    FilterUnsupported,
    #[error("the PDF image limit was exceeded")]
    ImageLimit,
}

impl From<crate::layout::LayoutError> for PdfReadError {
    fn from(_: crate::layout::LayoutError) -> Self {
        Self::NumericLimit
    }
}

impl PdfReadError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InputSizeLimit => "FLOW_PDF_READER_INPUT_LIMIT",
            Self::InvalidLimits => "FLOW_PDF_READER_LIMITS_INVALID",
            Self::HeaderInvalid => "FLOW_PDF_READER_HEADER_INVALID",
            Self::VersionUnsupported => "FLOW_PDF_READER_VERSION_UNSUPPORTED",
            Self::StartxrefInvalid => "FLOW_PDF_READER_STARTXREF_INVALID",
            Self::XrefInvalid => "FLOW_PDF_READER_XREF_INVALID",
            Self::XrefUnsupported => "FLOW_PDF_READER_XREF_UNSUPPORTED",
            Self::EncryptionUnsupported => "FLOW_PDF_READER_ENCRYPTION_UNSUPPORTED",
            Self::RootMissing => "FLOW_PDF_READER_ROOT_MISSING",
            Self::ObjectMissing => "FLOW_PDF_READER_OBJECT_MISSING",
            Self::ObjectLimit => "FLOW_PDF_READER_OBJECT_LIMIT",
            Self::NestingLimit => "FLOW_PDF_READER_NESTING_LIMIT",
            Self::TokenLimit => "FLOW_PDF_READER_TOKEN_LIMIT",
            Self::StreamLimit => "FLOW_PDF_READER_STREAM_LIMIT",
            Self::DecodedStreamLimit => "FLOW_PDF_READER_DECODED_STREAM_LIMIT",
            Self::Syntax => "FLOW_PDF_READER_SYNTAX_INVALID",
            Self::ReferenceInvalid => "FLOW_PDF_READER_REFERENCE_INVALID",
            Self::PageTreeInvalid => "FLOW_PDF_READER_PAGE_TREE_INVALID",
            Self::PageCountLimit => "FLOW_PDF_READER_PAGE_COUNT_LIMIT",
            Self::NumericLimit => "FLOW_PDF_READER_NUMERIC_LIMIT",
            Self::FilterUnsupported => "FLOW_PDF_READER_FILTER_UNSUPPORTED",
            Self::ImageLimit => "FLOW_PDF_READER_IMAGE_LIMIT",
        }
    }
}

/// An indirect PDF object identity retained for scene provenance.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfObjectRef {
    pub object_number: u32,
    pub generation: u16,
}

/// The cheap summary available after page-tree discovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfDocumentSummary {
    pub schema_version: u32,
    pub pdf_version: String,
    pub source_hash: String,
    pub page_count: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct PdfStream {
    pub dict: BTreeMap<String, PdfValue>,
    pub data: Vec<u8>,
    // @invariant: Preservation edits reuse the original encoded bytes.
    pub encoded: Vec<u8>,
    pub source_offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum PdfValue {
    Null,
    Boolean(bool),
    Integer(i64),
    Real((LayoutUnit, f64)),
    // @invariant: The exact decimal token is retained independently of layout rounding.
    ExactReal(String),
    Name(String),
    String(Vec<u8>),
    Array(Vec<PdfValue>),
    Dictionary(BTreeMap<String, PdfValue>),
    Stream(PdfStream),
    Reference(PdfObjectRef),
}

impl PdfValue {
    pub(crate) fn as_dictionary(&self) -> Option<&BTreeMap<String, PdfValue>> {
        match self {
            Self::Dictionary(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_name(&self) -> Option<&str> {
        match self {
            Self::Name(value) => scene_name_label(value),
            _ => None,
        }
    }

    pub(crate) fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_number(&self) -> Option<LayoutUnit> {
        match self {
            Self::Integer(value) => value
                .checked_mul(LayoutUnit::UNITS_PER_POINT)
                .map(LayoutUnit::from_raw),
            Self::Real((value, _)) => Some(*value),
            Self::ExactReal(token) => parse_decimal(token.as_bytes()).ok(),
            _ => None,
        }
    }

    pub(super) fn as_precise_number(&self) -> Option<f64> {
        let value = match self {
            Self::Integer(value) => *value as f64,
            Self::Real((_, precise)) => *precise,
            Self::ExactReal(token) => token.parse().ok()?,
            _ => return None,
        };
        value.is_finite().then_some(value)
    }

    pub(crate) fn as_reference(&self) -> Option<PdfObjectRef> {
        match self {
            Self::Reference(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[PdfValue]> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }
}

pub(super) fn decode_pdf_text_string(
    bytes: &[u8],
    limit: usize,
    remaining: &mut usize,
) -> Result<Option<String>, PdfReadError> {
    const ACCENTS: [char; 8] = ['˘', 'ˇ', 'ˆ', '˙', '˝', '˛', '˚', '˜'];
    const SPECIAL: [char; 31] = [
        '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚',
        '™', 'ﬁ', 'ﬂ', 'Ł', 'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž',
    ];
    let utf16 = bytes.strip_prefix(&[0xfe, 0xff]);
    let units = if let Some(bytes) = utf16 {
        if bytes.len() % 2 != 0 {
            return Ok(None);
        }
        bytes.len() / 2
    } else {
        bytes.len()
    };
    if units > limit || (utf16.is_none() && bytes.iter().any(|&b| matches!(b, 0x7f | 0x9f | 0xad)))
    {
        return Ok(None);
    }
    let capacity = units.checked_mul(3).ok_or(PdfReadError::StreamLimit)?;
    let available = remaining
        .checked_sub(capacity)
        .ok_or(PdfReadError::StreamLimit)?;
    let mut text = String::new();
    text.try_reserve_exact(capacity)
        .map_err(|_| PdfReadError::StreamLimit)?;
    if let Some(bytes) = utf16 {
        let units = bytes
            .chunks_exact(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]));
        for scalar in char::decode_utf16(units) {
            let Ok(scalar) = scalar else {
                return Ok(None);
            };
            text.push(scalar);
        }
    } else {
        text.extend(bytes.iter().map(|&b| match b {
            0x18..=0x1f => ACCENTS[usize::from(b - 0x18)],
            0x80..=0x9e => SPECIAL[usize::from(b - 0x80)],
            0xa0 => '€',
            _ => char::from(b),
        }));
    }
    if text.len() > limit {
        return Ok(None);
    }
    *remaining = available;
    Ok(Some(text))
}

pub(super) fn resolve_annotation_value<'value, 'object: 'value>(
    value: &'value PdfValue,
    max_nesting: usize,
    work: &mut usize,
    object: &mut impl FnMut(PdfObjectRef) -> Result<Option<Cow<'object, PdfValue>>, PdfReadError>,
) -> Result<Option<Cow<'value, PdfValue>>, PdfReadError> {
    Ok(
        follow_annotation_references(value, max_nesting, work, object)?
            .filter(|value| value.as_reference().is_none()),
    )
}

fn follow_annotation_references<'value, 'object: 'value>(
    value: &'value PdfValue,
    max_nesting: usize,
    work: &mut usize,
    object: &mut impl FnMut(PdfObjectRef) -> Result<Option<Cow<'object, PdfValue>>, PdfReadError>,
) -> Result<Option<Cow<'value, PdfValue>>, PdfReadError> {
    let mut value = Cow::Borrowed(value);
    for _ in 0..max_nesting {
        let Some(reference) = value.as_reference() else {
            return Ok(Some(value));
        };
        *work = work.checked_sub(1).ok_or(PdfReadError::TokenLimit)?;
        let Some(resolved) = object(reference)? else {
            return Ok(None);
        };
        value = resolved;
    }
    Ok(Some(value))
}

// @invariant: Allow for a new B-tree node and its slots before insertion.
pub(super) const WIDGET_LABEL_ENTRY_BYTES: usize = 128 + 12 * size_of::<PdfObjectRef>();

pub(super) struct WidgetLabelLimits {
    pub max_nesting: usize,
    pub scratch_bytes: usize,
}

pub(super) fn map_widget_label<'a, T>(
    widget: PdfObjectRef,
    dictionary: &'a BTreeMap<String, PdfValue>,
    limits: WidgetLabelLimits,
    work: &mut usize,
    mut object: impl FnMut(PdfObjectRef) -> Result<Option<Cow<'a, PdfValue>>, PdfReadError>,
    mut root_count: impl FnMut(PdfObjectRef, &mut usize) -> Result<usize, PdfReadError>,
    read: impl FnOnce(&[u8]) -> Result<T, PdfReadError>,
) -> Result<Option<T>, PdfReadError> {
    if let Some(label) = dictionary.get("T") {
        let Some(label) = resolve_annotation_value(label, limits.max_nesting, work, &mut object)?
        else {
            return Ok(None);
        };
        if !matches!(*label, PdfValue::Null) {
            return label.as_bytes().map(read).transpose();
        }
    }
    let Some(parent) = dictionary
        .get("Parent")
        .and_then(PdfValue::as_reference)
        .filter(|&parent| parent != widget)
    else {
        return Ok(None);
    };
    if root_count(widget, work)? != 0 {
        return Ok(None);
    }
    *work = work.checked_sub(1).ok_or(PdfReadError::TokenLimit)?;
    let field = object(parent)?;
    let Some(field) = field.as_deref().and_then(PdfValue::as_dictionary) else {
        return Ok(None);
    };
    if !has_registered_field_ancestry(
        widget,
        parent,
        field,
        limits.max_nesting,
        work,
        &mut object,
        &mut root_count,
    )? {
        return Ok(None);
    }
    let Some(kids) = field.get("Kids") else {
        return Ok(None);
    };
    let children = resolve_annotation_value(kids, limits.max_nesting, work, &mut object)?;
    let Some(children) = children.as_deref().and_then(PdfValue::as_array) else {
        return Ok(None);
    };
    *work = work
        .checked_sub(children.len())
        .ok_or(PdfReadError::TokenLimit)?;
    let mut scratch = limits.scratch_bytes;
    let mut seen = BTreeSet::new();
    for child in children {
        let Some(child) = child.as_reference() else {
            return Ok(None);
        };
        if child == parent || seen.contains(&child) {
            return Ok(None);
        }
        scratch = scratch
            .checked_sub(WIDGET_LABEL_ENTRY_BYTES)
            .ok_or(PdfReadError::StreamLimit)?;
        seen.insert(child);
        let entry = object(child)?;
        let Some(entry) = entry.as_deref().and_then(PdfValue::as_dictionary) else {
            return Ok(None);
        };
        if entry.get("Subtype").and_then(PdfValue::as_name) != Some("Widget")
            || entry.get("Parent").and_then(PdfValue::as_reference) != Some(parent)
            || ["FT", "V", "Kids"]
                .iter()
                .any(|key| entry.contains_key(*key))
        {
            return Ok(None);
        }
        if child != widget
            && let Some(label) = entry.get("T")
            && !matches!(
                resolve_annotation_value(label, limits.max_nesting, work, &mut object)?.as_deref(),
                Some(PdfValue::Null)
            )
        {
            return Ok(None);
        }
    }
    if !seen.contains(&widget) {
        return Ok(None);
    }
    drop(seen);
    let Some(label) = field.get("T") else {
        return Ok(None);
    };
    resolve_annotation_value(label, limits.max_nesting, work, &mut object)?
        .as_deref()
        .and_then(PdfValue::as_bytes)
        .map(read)
        .transpose()
}

fn has_registered_field_ancestry<'a>(
    widget: PdfObjectRef,
    mut child: PdfObjectRef,
    field: &BTreeMap<String, PdfValue>,
    max_nesting: usize,
    work: &mut usize,
    object: &mut impl FnMut(PdfObjectRef) -> Result<Option<Cow<'a, PdfValue>>, PdfReadError>,
    root_count: &mut impl FnMut(PdfObjectRef, &mut usize) -> Result<usize, PdfReadError>,
) -> Result<bool, PdfReadError> {
    let parent_ref =
        |field: &BTreeMap<String, PdfValue>| field.get("Parent").map(PdfValue::as_reference);
    let mut parent = parent_ref(field);
    for depth in 1..=max_nesting {
        let registrations = root_count(child, work)?;
        let Some(next) = parent else {
            return Ok(registrations == 1);
        };
        let Some(next) = next else {
            return Ok(false);
        };
        // @invariant: A nested field has one reciprocal parent link and no root registration.
        if registrations != 0 || next == widget || next == child || depth == max_nesting {
            return Ok(false);
        }
        *work = work.checked_sub(1).ok_or(PdfReadError::TokenLimit)?;
        let ancestor = object(next)?;
        let Some(ancestor) = ancestor.as_deref().and_then(PdfValue::as_dictionary) else {
            return Ok(false);
        };
        let Some(kids) = ancestor.get("Kids") else {
            return Ok(false);
        };
        let children = resolve_annotation_value(kids, max_nesting, work, object)?;
        let Some(children) = children.as_deref().and_then(PdfValue::as_array) else {
            return Ok(false);
        };
        *work = work
            .checked_sub(children.len())
            .ok_or(PdfReadError::TokenLimit)?;
        let mut matches = 0;
        for entry in children {
            let Some(reference) = entry.as_reference() else {
                return Ok(false);
            };
            matches += usize::from(reference == child);
        }
        if matches != 1 {
            return Ok(false);
        }
        child = next;
        parent = parent_ref(ancestor);
    }
    Ok(false)
}

#[derive(Debug, Clone, Copy)]
struct PdfXrefEntry {
    offset: usize,
    generation: u16,
}

#[derive(Debug, Clone, Copy)]
struct PageBox {
    layout: LayoutRect,
    edges: [f64; 4],
}

#[derive(Debug, Clone)]
pub(crate) struct PdfPageRecord {
    pub object_ref: PdfObjectRef,
    pub media_box: LayoutRect,
    pub crop_box: Option<LayoutRect>,
    pub(super) media_edges: [f64; 4],
    pub(super) crop_edges: Option<[f64; 4]>,
    pub resources: Option<PdfValue>,
    pub contents: Vec<PdfObjectRef>,
    pub annotations: Vec<PdfObjectRef>,
}

/// Request-local owned PDF reader. The object graph is intentionally absent;
/// only xref offsets and the trailer are retained after opening.
#[derive(Debug, Clone)]
pub struct PdfReader {
    bytes: Vec<u8>,
    version: String,
    source_hash: String,
    xref: BTreeMap<u32, PdfXrefEntry>,
    retained_starts: Vec<usize>,
    trailer: BTreeMap<String, PdfValue>,
    root: PdfObjectRef,
    pub(crate) retained_cost: usize,
    pub(crate) open_work: usize,
    limits: PdfReaderLimits,
}

impl PdfReader {
    #[must_use]
    pub fn source_hash(&self) -> &str {
        &self.source_hash
    }

    #[must_use]
    pub fn pdf_version(&self) -> &str {
        &self.version
    }

    pub(crate) fn root_for_scene(&self) -> PdfObjectRef {
        self.root
    }

    #[must_use]
    pub fn limits(&self) -> &PdfReaderLimits {
        &self.limits
    }

    /// Returns the cheap reportable findings in the catalog/trailer without
    /// walking page content.
    #[must_use]
    pub fn preflight_report(&self) -> PdfReadReport {
        let mut report = PdfReadReport::default();
        report_active_content(&self.trailer, None, None, &mut report);
        report
    }

    /// Discovers the page tree. Page content streams and resource objects are
    /// not parsed here, so this remains cheaper than building a scene.
    pub fn summary(&self) -> Result<PdfDocumentSummary, PdfReadError> {
        let pages = self.page_records()?;
        Ok(PdfDocumentSummary {
            schema_version: PDF_READER_SCHEMA_VERSION,
            pdf_version: self.version.clone(),
            source_hash: self.source_hash.clone(),
            page_count: u32::try_from(pages.len()).map_err(|_| PdfReadError::PageCountLimit)?,
        })
    }

    pub(crate) fn object(&self, reference: PdfObjectRef) -> Result<PdfValue, PdfReadError> {
        self.object_with_depth(reference, 0)
    }

    pub(super) fn non_stream_object(
        &self,
        reference: PdfObjectRef,
    ) -> Result<PdfValue, PdfReadError> {
        let (mut cursor, _) = self.object_cursor(reference, 0, None)?;
        let value = cursor.parse_value(0)?;
        if !cursor.consume_keyword(b"endobj")? {
            return Err(PdfReadError::Syntax);
        }
        Ok(value)
    }

    pub(super) fn label_object(
        &self,
        reference: PdfObjectRef,
    ) -> Result<Option<PdfValue>, PdfReadError> {
        match self.non_stream_object(reference) {
            Ok(value) => Ok(Some(value)),
            Err(
                error @ (PdfReadError::TokenLimit
                | PdfReadError::NestingLimit
                | PdfReadError::StreamLimit),
            ) => Err(error),
            Err(_) => Ok(None),
        }
    }

    pub(super) fn link_destination_value<'a>(
        &self,
        value: &'a PdfValue,
        work: &mut usize,
    ) -> Result<Option<Cow<'a, PdfValue>>, PdfReadError> {
        let destination =
            follow_annotation_references(value, self.limits.max_nesting, work, &mut |reference| {
                match self.non_stream_object(reference) {
                    Ok(value) => Ok(Some(Cow::Owned(value))),
                    Err(
                        PdfReadError::ObjectMissing
                        | PdfReadError::ReferenceInvalid
                        | PdfReadError::Syntax
                        | PdfReadError::XrefInvalid,
                    ) => Ok(None),
                    Err(error) => Err(error),
                }
            })?;
        if destination
            .as_deref()
            .and_then(PdfValue::as_reference)
            .is_some()
        {
            return Err(PdfReadError::NestingLimit);
        }
        Ok(destination)
    }

    pub(crate) fn object_offset(&self, reference: PdfObjectRef) -> Option<usize> {
        self.xref
            .get(&reference.object_number)
            .map(|entry| entry.offset)
    }

    pub(crate) fn resolve(&self, value: &PdfValue, depth: usize) -> Result<PdfValue, PdfReadError> {
        if depth > self.limits.max_nesting {
            return Err(PdfReadError::NestingLimit);
        }
        match value {
            PdfValue::Reference(reference) => self.object_with_depth(
                *reference,
                depth.checked_add(1).ok_or(PdfReadError::NestingLimit)?,
            ),
            _ => Ok(value.clone()),
        }
    }

    pub(crate) fn page_records(&self) -> Result<Vec<PdfPageRecord>, PdfReadError> {
        let pages_value = self
            .trailer
            .get("Root")
            .ok_or(PdfReadError::RootMissing)
            .and_then(|_| self.object(self.root))?;
        let catalog = pages_value
            .as_dictionary()
            .ok_or(PdfReadError::PageTreeInvalid)?;
        let pages_ref = catalog
            .get("Pages")
            .and_then(PdfValue::as_reference)
            .ok_or(PdfReadError::PageTreeInvalid)?;
        let mut pages = Vec::new();
        let mut active = BTreeSet::new();
        self.collect_pages(pages_ref, (None, None), None, 0, &mut active, &mut pages)?;
        Ok(pages)
    }

    pub(crate) fn into_retained_parts(self) -> (Vec<u8>, BTreeMap<String, PdfValue>) {
        (self.bytes, self.trailer)
    }

    pub(crate) fn retained_trailer(&self) -> &BTreeMap<String, PdfValue> {
        &self.trailer
    }

    // @contract: Retained syntax preserves exact decimals alongside scene-facing quantization.
    pub(crate) fn retained_object(
        &self,
        reference: PdfObjectRef,
        retained: usize,
        decoded: usize,
        work: usize,
    ) -> Result<(PdfValue, usize, usize), PdfReadError> {
        self.read_object(reference, 0, Some((retained, decoded, work)))
    }

    fn object_with_depth(
        &self,
        reference: PdfObjectRef,
        depth: usize,
    ) -> Result<PdfValue, PdfReadError> {
        self.read_object(reference, depth, None)
            .map(|result| result.0)
    }

    fn object_cursor(
        &self,
        reference: PdfObjectRef,
        depth: usize,
        retention: Option<(usize, usize, usize)>,
    ) -> Result<(Cursor<'_>, usize), PdfReadError> {
        if depth > self.limits.max_nesting {
            return Err(PdfReadError::NestingLimit);
        }
        let entry = self
            .xref
            .get(&reference.object_number)
            .ok_or(PdfReadError::ObjectMissing)?;
        if entry.generation != reference.generation {
            return Err(PdfReadError::ReferenceInvalid);
        }
        let bytes = if retention.is_some() {
            let index = self
                .retained_starts
                .binary_search(&entry.offset)
                .map_err(|_| PdfReadError::XrefInvalid)?;
            let end = self
                .retained_starts
                .get(index + 1)
                .ok_or(PdfReadError::XrefInvalid)?;
            &self.bytes[..*end]
        } else {
            &self.bytes
        };
        let mut cursor = Cursor::new(bytes, entry.offset, self.limits.clone());
        if let Some((retained, decoded, work)) = retention {
            cursor.exact = true;
            cursor.remaining = Some(retained);
            cursor.limits.max_decoded_stream_bytes =
                decoded.min(self.limits.max_decoded_stream_bytes);
            cursor.limits.max_content_tokens = work;
        }
        let object_number = cursor
            .parse_u64_token()?
            .ok_or_else(|| cursor.object_syntax_error())?;
        let generation = cursor
            .parse_u64_token()?
            .ok_or_else(|| cursor.object_syntax_error())?;
        let object_keyword = cursor
            .token()?
            .ok_or_else(|| cursor.object_syntax_error())?;
        if object_number != u64::from(reference.object_number)
            || generation != u64::from(reference.generation)
            || object_keyword != b"obj"
            || (cursor.exact && cursor.position == cursor.bytes.len())
        {
            return Err(PdfReadError::XrefInvalid);
        }
        Ok((cursor, entry.offset))
    }

    fn read_object(
        &self,
        reference: PdfObjectRef,
        depth: usize,
        retention: Option<(usize, usize, usize)>,
    ) -> Result<(PdfValue, usize, usize), PdfReadError> {
        let (mut cursor, source_offset) = self.object_cursor(reference, depth, retention)?;
        let mut value = cursor.object_value()?;
        cursor.skip_space();
        if cursor.consume_keyword(b"stream")? {
            let dictionary = match value {
                PdfValue::Dictionary(dict) => dict,
                _ => return Err(PdfReadError::Syntax),
            };
            if !matches!(cursor.bytes.get(cursor.position()), Some(b'\r' | b'\n')) {
                return Err(PdfReadError::Syntax);
            }
            cursor.consume_stream_eol();
            let length_value = dictionary.get("Length").ok_or(PdfReadError::Syntax)?;
            let length = if retention.is_some() {
                self.retained_stream_length(length_value, &mut cursor)?
            } else {
                self.stream_length(length_value, depth + 1)?
            };
            if length > self.limits.max_stream_bytes {
                return Err(PdfReadError::StreamLimit);
            }
            let payload = cursor.stream_payload(length)?;
            if retention.is_some() {
                cursor.require_object_keyword(b"endstream")?;
                cursor.require_object_keyword(b"endobj")?;
            }
            cursor.charge(length.checked_mul(2).ok_or(PdfReadError::InputSizeLimit)?)?;
            let raw = payload.to_vec();
            if retention.is_none() {
                cursor.require_object_keyword(b"endstream")?;
            }
            let encoded = raw.clone();
            let data = decode_stream(&dictionary, raw, &cursor.limits, cursor.remaining)?;
            if matches!(
                dictionary.get("Filter").and_then(single_filter),
                Some("FlateDecode" | "Fl" | "DCTDecode" | "DCT")
            ) && let Some(remaining) = &mut cursor.remaining
            {
                *remaining += length;
                cursor.charge(data.capacity())?;
            }
            value = PdfValue::Stream(PdfStream {
                dict: dictionary,
                data,
                encoded,
                source_offset,
            });
        }
        if retention.is_none() || !matches!(value, PdfValue::Stream(_)) {
            cursor.require_object_keyword(b"endobj")?;
        }
        let retained =
            retention.map_or(0, |(initial, _, _)| initial - cursor.remaining.unwrap_or(0));
        Ok((value, retained, cursor.tokens))
    }

    fn retained_stream_length(
        &self,
        value: &PdfValue,
        owner: &mut Cursor<'_>,
    ) -> Result<usize, PdfReadError> {
        let length = if let PdfValue::Reference(reference) = value {
            let work = owner
                .limits
                .max_content_tokens
                .checked_sub(owner.tokens)
                .ok_or(PdfReadError::TokenLimit)?;
            // @security: Resolve Length as a scalar integer to avoid recursive stream decoding.
            let (mut cursor, _) = self.object_cursor(
                *reference,
                0,
                Some((
                    owner.remaining.ok_or(PdfReadError::InputSizeLimit)?,
                    owner.limits.max_decoded_stream_bytes,
                    work,
                )),
            )?;
            let number = cursor
                .object_value()?
                .as_integer()
                .ok_or(PdfReadError::Syntax)?;
            cursor.require_object_keyword(b"endobj")?;
            owner.tokens = owner
                .tokens
                .checked_add(cursor.tokens)
                .ok_or(PdfReadError::TokenLimit)?;
            owner.remaining = cursor.remaining;
            number
        } else {
            value.as_integer().ok_or(PdfReadError::Syntax)?
        };
        usize::try_from(length).map_err(|_| PdfReadError::StreamLimit)
    }

    fn stream_length(&self, value: &PdfValue, depth: usize) -> Result<usize, PdfReadError> {
        let value = self.resolve(value, depth)?;
        let length = value.as_integer().ok_or(PdfReadError::Syntax)?;
        if length < 0 {
            return Err(PdfReadError::NumericLimit);
        }
        usize::try_from(length).map_err(|_| PdfReadError::StreamLimit)
    }

    fn collect_pages(
        &self,
        reference: PdfObjectRef,
        inherited_boxes: (Option<PageBox>, Option<PageBox>),
        inherited_resources: Option<PdfValue>,
        depth: usize,
        active: &mut BTreeSet<PdfObjectRef>,
        pages: &mut Vec<PdfPageRecord>,
    ) -> Result<(), PdfReadError> {
        if depth > self.limits.max_nesting {
            return Err(PdfReadError::NestingLimit);
        }
        if !active.insert(reference) {
            return Err(PdfReadError::PageTreeInvalid);
        }
        let value = self.object(reference)?;
        let dictionary = value.as_dictionary().ok_or(PdfReadError::PageTreeInvalid)?;
        let type_name = dictionary
            .get("Type")
            .and_then(PdfValue::as_name)
            .unwrap_or("");
        let (inherited_media_box, inherited_crop_box) = inherited_boxes;
        let media_box = dictionary
            .get("MediaBox")
            .map(|value| self.resolve(value, depth + 1))
            .transpose()?
            .map(|value| parse_page_box(&value, false))
            .transpose()?;
        let crop_box = dictionary
            .get("CropBox")
            .map(|value| self.resolve(value, depth + 1))
            .transpose()?
            .filter(|value| !matches!(value, PdfValue::Null))
            .map(|value| parse_page_box(&value, true))
            .transpose()?;
        let resources = dictionary
            .get("Resources")
            .map(|value| self.resolve(value, depth + 1))
            .transpose()?;
        let media_box = media_box.or(inherited_media_box);
        let crop_box = crop_box.or(inherited_crop_box);
        let resources = resources.or(inherited_resources);
        match type_name {
            "Pages" => {
                let kids = dictionary
                    .get("Kids")
                    .ok_or(PdfReadError::PageTreeInvalid)
                    .and_then(|value| self.resolve(value, depth + 1))?
                    .as_array()
                    .ok_or(PdfReadError::PageTreeInvalid)?
                    .to_vec();
                if kids.len() > self.limits.max_pages {
                    return Err(PdfReadError::PageCountLimit);
                }
                for kid in kids {
                    let kid = kid.as_reference().ok_or(PdfReadError::PageTreeInvalid)?;
                    self.collect_pages(
                        kid,
                        (media_box, crop_box),
                        resources.clone(),
                        depth + 1,
                        active,
                        pages,
                    )?;
                    if pages.len() > self.limits.max_pages {
                        return Err(PdfReadError::PageCountLimit);
                    }
                }
            }
            "Page" => {
                let media_box = media_box.ok_or(PdfReadError::PageTreeInvalid)?;
                let contents = match dictionary.get("Contents") {
                    None | Some(PdfValue::Null) => Vec::new(),
                    Some(PdfValue::Reference(reference)) => {
                        let (mut cursor, _) = self.object_cursor(*reference, 0, None)?;
                        cursor.skip_space();
                        if cursor.bytes.get(cursor.position) == Some(&b'[')
                            && depth + 2 > self.limits.max_nesting
                        {
                            return Err(PdfReadError::NestingLimit);
                        }
                        match cursor.parse_value(0)? {
                            array @ PdfValue::Array(_) => {
                                if !cursor.consume_keyword(b"endobj")? {
                                    return Err(PdfReadError::Syntax);
                                }
                                refs_from_array(&array)?
                            }
                            PdfValue::Null => {
                                if !cursor.consume_keyword(b"endobj")? {
                                    return Err(PdfReadError::Syntax);
                                }
                                Vec::new()
                            }
                            PdfValue::Dictionary(_) if cursor.consume_keyword(b"stream")? => {
                                vec![*reference]
                            }
                            _ => return Err(PdfReadError::ReferenceInvalid),
                        }
                    }
                    Some(value) => refs_from_array(value)?,
                };
                let annotations = match dictionary.get("Annots") {
                    None => Vec::new(),
                    Some(value) => refs_from_array(&self.resolve(value, depth + 1)?)?,
                };
                pages.push(PdfPageRecord {
                    object_ref: reference,
                    media_box: media_box.layout,
                    crop_box: crop_box.map(|bounds| bounds.layout),
                    media_edges: media_box.edges,
                    crop_edges: crop_box.map(|bounds| bounds.edges),
                    resources,
                    contents,
                    annotations,
                });
            }
            _ => return Err(PdfReadError::PageTreeInvalid),
        }
        active.remove(&reference);
        Ok(())
    }
}

/// Opens an owned bounded reader. No indirect object other than the trailer is
/// parsed until a caller requests a summary or scene page.
pub fn open_pdf(bytes: &[u8], limits: PdfReaderLimits) -> Result<PdfReader, PdfReadError> {
    open_pdf_mode(bytes, limits, None)
}

pub(crate) fn open_pdf_retained(
    bytes: &[u8],
    limits: PdfReaderLimits,
    retained_limit: usize,
) -> Result<PdfReader, PdfReadError> {
    open_pdf_mode(bytes, limits, Some(retained_limit))
}

fn open_pdf_mode(
    bytes: &[u8],
    limits: PdfReaderLimits,
    retained_limit: Option<usize>,
) -> Result<PdfReader, PdfReadError> {
    limits.validate()?;
    if bytes.len() > limits.max_input_bytes {
        return Err(PdfReadError::InputSizeLimit);
    }
    let version = parse_header(bytes)?;
    let startxref = find_startxref(bytes)?;
    let retention = retained_limit
        .map(|limit| {
            limit
                .checked_sub(bytes.len())
                .ok_or(PdfReadError::InputSizeLimit)
        })
        .transpose()?;
    let (xref, trailer, mut retained_cost, mut open_work) =
        parse_xref(bytes, startxref, &limits, retention)?;
    let retained_starts = if let Some(available) = retention {
        let index_work = xref
            .len()
            .checked_mul(xref.len().ilog2() as usize + 1)
            .ok_or(PdfReadError::TokenLimit)?;
        open_work = open_work
            .checked_add(index_work)
            .filter(|work| *work <= limits.max_content_tokens)
            .ok_or(PdfReadError::TokenLimit)?;
        let (starts, cost) =
            retained_object_starts(&xref, &bytes[..startxref], available - retained_cost)?;
        retained_cost += cost;
        starts
    } else {
        Vec::new()
    };
    let root = trailer
        .get("Root")
        .and_then(PdfValue::as_reference)
        .ok_or(PdfReadError::RootMissing)?;
    if trailer.contains_key("Encrypt") {
        return Err(PdfReadError::EncryptionUnsupported);
    }
    if trailer.contains_key("XRefStm") {
        return Err(PdfReadError::XrefUnsupported);
    }
    Ok(PdfReader {
        bytes: bytes.to_vec(),
        version,
        source_hash: blake3::hash(bytes).to_hex().to_string(),
        xref,
        retained_starts,
        trailer,
        root,
        retained_cost,
        open_work,
        limits,
    })
}

fn retained_object_starts(
    xref: &BTreeMap<u32, PdfXrefEntry>,
    before_xref: &[u8],
    available: usize,
) -> Result<(Vec<usize>, usize), PdfReadError> {
    let count = xref
        .len()
        .checked_add(1)
        .ok_or(PdfReadError::InputSizeLimit)?;
    let cost = count
        .checked_mul(std::mem::size_of::<usize>())
        .filter(|cost| *cost <= available)
        .ok_or(PdfReadError::InputSizeLimit)?;
    let mut starts = Vec::new();
    starts
        .try_reserve_exact(count)
        .map_err(|_| PdfReadError::InputSizeLimit)?;
    for entry in xref.values() {
        if entry.offset < 8
            || !before_xref
                .get(entry.offset)
                .is_some_and(u8::is_ascii_digit)
        {
            return Err(PdfReadError::XrefInvalid);
        }
        starts.push(entry.offset);
    }
    starts.sort_unstable();
    if starts.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(PdfReadError::XrefInvalid);
    }
    starts.push(before_xref.len());
    Ok((starts, cost))
}

fn parse_header(bytes: &[u8]) -> Result<String, PdfReadError> {
    if !bytes.starts_with(b"%PDF-") || bytes.len() < 8 {
        return Err(PdfReadError::HeaderInvalid);
    }
    let version = &bytes[5..8];
    if version[0] != b'1'
        || version[1] != b'.'
        || !(b'0'..=MAX_PDF_VERSION[2]).contains(&version[2])
    {
        return Err(PdfReadError::VersionUnsupported);
    }
    std::str::from_utf8(version)
        .map(str::to_owned)
        .map_err(|_| PdfReadError::HeaderInvalid)
}

fn find_startxref(bytes: &[u8]) -> Result<usize, PdfReadError> {
    let marker = b"startxref";
    let start = bytes
        .windows(marker.len())
        .rposition(|window| window == marker)
        .ok_or(PdfReadError::StartxrefInvalid)?;
    let mut cursor = Cursor::new(bytes, start + marker.len(), PdfReaderLimits::default());
    let offset = cursor
        .parse_u64_token()?
        .ok_or(PdfReadError::StartxrefInvalid)?;
    usize::try_from(offset).map_err(|_| PdfReadError::StartxrefInvalid)
}

type ParsedXref = (
    BTreeMap<u32, PdfXrefEntry>,
    BTreeMap<String, PdfValue>,
    usize,
    usize,
);

fn parse_xref(
    bytes: &[u8],
    offset: usize,
    limits: &PdfReaderLimits,
    retention: Option<usize>,
) -> Result<ParsedXref, PdfReadError> {
    let exact = retention.is_some();
    if offset >= bytes.len() {
        return Err(PdfReadError::XrefInvalid);
    }
    if exact && (is_pdf_space(bytes[offset]) || bytes[offset] == b'%') {
        return Err(PdfReadError::XrefInvalid);
    }
    if bytes[offset..].starts_with(b"obj") {
        return Err(PdfReadError::XrefUnsupported);
    }
    let mut cursor = Cursor::new(bytes, offset, limits.clone());
    cursor.exact = exact;
    cursor.remaining = retention;
    let mut seen = BTreeSet::new();
    if cursor.token()?.as_deref() != Some(b"xref".as_slice()) {
        return Err(PdfReadError::XrefUnsupported);
    }
    let mut xref = BTreeMap::new();
    loop {
        let token = cursor.token()?.ok_or(PdfReadError::XrefInvalid)?;
        if token == b"trailer" {
            break;
        }
        let first = parse_u32(&token)?;
        let count = parse_usize(&cursor.token()?.ok_or(PdfReadError::XrefInvalid)?)?;
        if count > limits.max_objects.saturating_add(1)
            || (!exact && first as usize > limits.max_objects)
        {
            return Err(PdfReadError::ObjectLimit);
        }
        for index in 0..count {
            let offset = parse_usize(&cursor.token()?.ok_or(PdfReadError::XrefInvalid)?)?;
            let generation = parse_u16(&cursor.token()?.ok_or(PdfReadError::XrefInvalid)?)?;
            let flag = cursor.token()?.ok_or(PdfReadError::XrefInvalid)?;
            let object_number = first
                .checked_add(u32::try_from(index).map_err(|_| PdfReadError::ObjectLimit)?)
                .ok_or(PdfReadError::ObjectLimit)?;
            if exact {
                cursor.charge(128)?;
            }
            if !seen.insert(object_number) {
                return Err(PdfReadError::XrefInvalid);
            }
            if seen.len() > limits.max_objects.saturating_add(1) {
                return Err(PdfReadError::ObjectLimit);
            }
            if flag == b"n" {
                if object_number == 0
                    || (!exact
                        && usize::try_from(object_number).unwrap_or(usize::MAX)
                            > limits.max_objects)
                {
                    return Err(PdfReadError::ObjectLimit);
                }
                if offset >= bytes.len()
                    || xref
                        .insert(object_number, PdfXrefEntry { offset, generation })
                        .is_some()
                {
                    return Err(PdfReadError::XrefInvalid);
                }
            } else if flag != b"f" {
                return Err(PdfReadError::XrefInvalid);
            }
        }
    }
    let trailer = match cursor.parse_value(0)? {
        PdfValue::Dictionary(dict) => dict,
        _ => return Err(PdfReadError::XrefInvalid),
    };
    if xref.is_empty() || trailer.contains_key("Encrypt") {
        if trailer.contains_key("Encrypt") {
            return Err(PdfReadError::EncryptionUnsupported);
        }
        return Err(PdfReadError::XrefInvalid);
    }
    if let Some(size) = trailer.get("Size").and_then(PdfValue::as_integer)
        && (size < 1
            || (!exact
                && usize::try_from(size).unwrap_or(usize::MAX)
                    > limits.max_objects.saturating_add(1)))
    {
        return Err(PdfReadError::ObjectLimit);
    }
    if exact {
        let size = trailer
            .get("Size")
            .and_then(PdfValue::as_integer)
            .ok_or(PdfReadError::XrefInvalid)?;
        if size < 1 || size > i64::from(u32::MAX) + 1 || seen.iter().any(|n| i64::from(*n) >= size)
        {
            return Err(PdfReadError::XrefInvalid);
        }
        if !cursor.consume_keyword(b"startxref")?
            || cursor.parse_u64_token()? != Some(offset as u64)
        {
            return Err(PdfReadError::StartxrefInvalid);
        }
        let tail = &bytes[cursor.position()..];
        let tail = tail.trim_ascii();
        if tail != b"%%EOF" {
            return Err(PdfReadError::XrefInvalid);
        }
    }
    let cost = if exact {
        retention.unwrap_or(0) - cursor.remaining.unwrap_or(0)
    } else {
        0
    };
    Ok((xref, trailer, cost, cursor.tokens))
}

fn take_pdf_name<'a>(bytes: &'a [u8], position: &mut usize) -> Result<&'a [u8], PdfReadError> {
    let start = *position;
    while *position < bytes.len() && !is_delimiter(bytes[*position]) {
        if *position - start == MAX_PDF_IDENTITY_BYTES * 3 {
            return Err(PdfReadError::Syntax);
        }
        *position += 1;
    }
    Ok(&bytes[start..*position])
}

fn decode_pdf_name_bytes(bytes: &[u8]) -> Result<Vec<u8>, PdfReadError> {
    let capacity = bytes.len().min(MAX_PDF_IDENTITY_BYTES);
    let mut output = Vec::with_capacity(capacity);
    let mut index = 0;
    while index < bytes.len() {
        if output.len() == MAX_PDF_IDENTITY_BYTES {
            return Err(PdfReadError::Syntax);
        }
        if bytes[index] == b'#' {
            if index + 2 >= bytes.len() {
                return Err(PdfReadError::Syntax);
            }
            let high = hex_nibble(bytes[index + 1]).ok_or(PdfReadError::Syntax)?;
            let low = hex_nibble(bytes[index + 2]).ok_or(PdfReadError::Syntax)?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    if output.contains(&0) {
        return Err(PdfReadError::Syntax);
    }
    Ok(output)
}

pub(super) fn read_scene_name(bytes: &[u8], position: &mut usize) -> Result<String, PdfReadError> {
    let bytes = take_pdf_name(bytes, position)?;
    Ok(scene_name_identity(decode_pdf_name_bytes(bytes)?))
}

fn scene_name_identity(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|invalid| {
        let mut name = String::with_capacity(1 + invalid.as_bytes().len() * 2);
        name.push('\0');
        name.extend(invalid.into_bytes().into_iter().map(char::from));
        name
    })
}

pub(super) fn scene_name_label(name: &str) -> Option<&str> {
    (!name.starts_with('\0')).then_some(name)
}

fn refs_from_array(value: &PdfValue) -> Result<Vec<PdfObjectRef>, PdfReadError> {
    value
        .as_array()
        .ok_or(PdfReadError::ReferenceInvalid)?
        .iter()
        .map(|value| value.as_reference().ok_or(PdfReadError::ReferenceInvalid))
        .collect()
}

fn parse_page_box(value: &PdfValue, allow_precise_only: bool) -> Result<PageBox, PdfReadError> {
    let values = value.as_array().ok_or(PdfReadError::PageTreeInvalid)?;
    if values.len() != 4 {
        return Err(PdfReadError::PageTreeInvalid);
    }
    let x0 = values[0].as_number().ok_or(PdfReadError::NumericLimit)?;
    let y0 = values[1].as_number().ok_or(PdfReadError::NumericLimit)?;
    let x1 = values[2].as_number().ok_or(PdfReadError::NumericLimit)?;
    let y1 = values[3].as_number().ok_or(PdfReadError::NumericLimit)?;
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (y0, y1) = (y0.min(y1), y0.max(y1));
    let width = x1.checked_sub(x0).map_err(|_| PdfReadError::NumericLimit)?;
    let height = y1.checked_sub(y0).map_err(|_| PdfReadError::NumericLimit)?;
    let precise = |index: usize| {
        values[index]
            .as_precise_number()
            .ok_or(PdfReadError::NumericLimit)
    };
    let (left, top, right, bottom) = (precise(0)?, precise(1)?, precise(2)?, precise(3)?);
    let edges = [
        left.min(right),
        top.min(bottom),
        left.max(right),
        top.max(bottom),
    ];
    let [x_min, y_min, x_max, y_max] = edges;
    let has_layout_area = width.raw() > 0 && height.raw() > 0;
    let has_precise_area = x_min < x_max && y_min < y_max;
    if !(has_layout_area || allow_precise_only && has_precise_area) {
        return Err(PdfReadError::PageTreeInvalid);
    }
    Ok(PageBox {
        layout: LayoutRect {
            x: x0,
            y: y0,
            width,
            height,
        },
        edges,
    })
}

pub(super) fn single_filter(value: &PdfValue) -> Option<&str> {
    match value {
        PdfValue::Array(filters) if filters.len() == 1 => filters[0].as_name(),
        value => value.as_name(),
    }
}

fn decode_stream(
    dictionary: &BTreeMap<String, PdfValue>,
    raw: Vec<u8>,
    limits: &PdfReaderLimits,
    workspace: Option<usize>,
) -> Result<Vec<u8>, PdfReadError> {
    let Some(filter) = dictionary.get("Filter") else {
        if raw.len() > limits.max_decoded_stream_bytes {
            return Err(PdfReadError::DecodedStreamLimit);
        }
        return Ok(raw);
    };
    let filter = single_filter(filter).ok_or(PdfReadError::FilterUnsupported)?;
    if let Some(available) = workspace
        && matches!(filter, "DCTDecode" | "DCT")
    {
        return decode_retained_jpeg(dictionary, &raw, limits, available);
    }
    match filter {
        "Identity" => {
            if raw.len() > limits.max_decoded_stream_bytes {
                return Err(PdfReadError::DecodedStreamLimit);
            }
            Ok(raw)
        }
        "DCTDecode" | "DCT" => {
            if raw.len() > limits.max_decoded_stream_bytes {
                return Err(PdfReadError::DecodedStreamLimit);
            }
            Ok(raw)
        }
        "FlateDecode" | "Fl" => {
            let mut output_limit = limits.max_decoded_stream_bytes;
            if let Some(available) = workspace {
                let scratch = std::mem::size_of::<miniz_oxide::inflate::core::DecompressorOxide>();
                let buffers = available
                    .checked_sub(scratch + 8)
                    .ok_or(PdfReadError::DecodedStreamLimit)?;
                // @security: Vec growth may briefly hold both the old and new output buffers.
                output_limit = output_limit.min(buffers / 3);
                if output_limit == 0 {
                    return Err(PdfReadError::DecodedStreamLimit);
                }
            }
            let decoded =
                miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&raw, output_limit)
                    .map_err(|error| {
                        if error.status == miniz_oxide::inflate::TINFLStatus::HasMoreOutput {
                            PdfReadError::DecodedStreamLimit
                        } else {
                            PdfReadError::Syntax
                        }
                    })?;
            if decoded.len() > limits.max_decoded_stream_bytes {
                return Err(PdfReadError::DecodedStreamLimit);
            }
            Ok(decoded)
        }
        "ASCIIHexDecode" | "AHx" => {
            let decoded = decode_ascii_hex(raw, limits.max_decoded_stream_bytes)?;
            Ok(decoded)
        }
        _ => Err(PdfReadError::FilterUnsupported),
    }
}

pub(super) fn jpeg_frame(bytes: &[u8]) -> Option<(u32, u32, ColorType, &[u8])> {
    let mut segments = bytes
        .strip_prefix(&[0xff, 0xd8])?
        .strip_suffix(&[0xff, 0xd9])?;
    loop {
        segments = segments.strip_prefix(&[0xff])?;
        while let Some(rest) = segments.strip_prefix(&[0xff]) {
            segments = rest;
        }
        let (&marker, rest) = segments.split_first()?;
        if matches!(marker, 0x00 | 0x01 | 0xd0..=0xda) {
            return None;
        }
        let length = usize::from(u16::from_be_bytes(rest.get(..2)?.try_into().ok()?));
        let segment = rest.get(2..length)?;
        if matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            let &[8, h1, h2, w1, w2, components, ..] = segment else {
                return None;
            };
            if !matches!(marker, 0xc0..=0xc2) || segment.len() != 6 + 3 * usize::from(components) {
                return None;
            }
            let color = match components {
                1 => ColorType::L8,
                3 => ColorType::Rgb8,
                _ => return None,
            };
            return Some((
                u16::from_be_bytes([w1, w2]).into(),
                u16::from_be_bytes([h1, h2]).into(),
                color,
                &segment[6..],
            ));
        }
        segments = rest.get(length..)?;
    }
}

fn decode_retained_jpeg(
    dictionary: &BTreeMap<String, PdfValue>,
    raw: &[u8],
    limits: &PdfReaderLimits,
    available: usize,
) -> Result<Vec<u8>, PdfReadError> {
    use super::native::{NATIVE_MAX_IMAGE_DIMENSION, NATIVE_MAX_IMAGE_PIXELS};

    let name = |key| dictionary.get(key).and_then(PdfValue::as_name);
    let integer = |key| dictionary.get(key).and_then(PdfValue::as_integer);
    if (dictionary.contains_key("Type") && name("Type") != Some("XObject"))
        || dictionary
            .get("Interpolate")
            .is_some_and(|value| !matches!(value, PdfValue::Boolean(_)))
        || name("Subtype") != Some("Image")
        || integer("BitsPerComponent") != Some(8)
        || dictionary.keys().any(|key| {
            !matches!(
                key.as_str(),
                "Type"
                    | "Subtype"
                    | "Length"
                    | "Width"
                    | "Height"
                    | "BitsPerComponent"
                    | "ColorSpace"
                    | "Filter"
                    | "Interpolate"
            )
        })
    {
        return Err(PdfReadError::FilterUnsupported);
    }
    let (width, height, color, components) = jpeg_frame(raw).ok_or(PdfReadError::Syntax)?;
    let expected_color = match name("ColorSpace") {
        Some("DeviceGray") => ColorType::L8,
        Some("DeviceRGB") => ColorType::Rgb8,
        _ => return Err(PdfReadError::FilterUnsupported),
    };
    if integer("Width") != Some(width.into())
        || integer("Height") != Some(height.into())
        || color != expected_color
    {
        return Err(PdfReadError::Syntax);
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0
        || pixels > limits.max_image_pixels.min(NATIVE_MAX_IMAGE_PIXELS)
        || width.max(height) > NATIVE_MAX_IMAGE_DIMENSION
    {
        return Err(PdfReadError::ImageLimit);
    }
    let samples = (pixels * u64::from(color.bytes_per_pixel())) as usize;
    if samples > limits.max_decoded_stream_bytes {
        return Err(PdfReadError::DecodedStreamLimit);
    }
    let peak = jpeg_decode_peak(
        raw.len(),
        width as usize,
        height as usize,
        components,
        samples,
    )
    .ok_or(PdfReadError::DecodedStreamLimit)?;
    if peak > available {
        return Err(PdfReadError::DecodedStreamLimit);
    }
    let mut decoder = JpegDecoder::new(IoCursor::new(raw)).map_err(|_| PdfReadError::Syntax)?;
    if decoder.dimensions() != (width, height) || decoder.color_type() != color {
        return Err(PdfReadError::Syntax);
    }
    let mut image_limits = Limits::default();
    image_limits.max_image_width = Some(width);
    image_limits.max_image_height = Some(height);
    image_limits.max_alloc = Some(limits.max_decoded_stream_bytes as u64);
    decoder
        .set_limits(image_limits)
        .map_err(|_| PdfReadError::ImageLimit)?;
    let mut data = vec![0; samples];
    decoder
        .read_image(&mut data)
        .map_err(|_| PdfReadError::Syntax)?;
    Ok(data)
}

// @security: Bound the pinned JPEG decoder's progressive and multi-scan allocations.
pub(super) fn jpeg_decode_peak(
    encoded: usize,
    width: usize,
    height: usize,
    components: &[u8],
    samples: usize,
) -> Option<usize> {
    let mut horizontal = 1;
    let mut vertical = 1;
    for component in components.chunks_exact(3) {
        horizontal = horizontal.max(usize::from(component[1] >> 4));
        vertical = vertical.max(usize::from(component[1] & 15));
    }
    let mcu_width = horizontal * 8;
    let mcu_height = vertical * 8;
    let padded_width = width.div_ceil(mcu_width).checked_mul(mcu_width)?;
    let padded_height = height.div_ceil(mcu_height).checked_mul(mcu_height)?;
    let count = components.len() / 3;
    let coefficients = padded_width
        .checked_mul(padded_height)?
        .checked_mul(count)?
        .checked_mul(std::mem::size_of::<i16>())?;
    // @security: Include coefficient rows, upsampling destinations, and a shared scratch row.
    let rows = padded_width
        .checked_mul(mcu_height)?
        .checked_mul(8 * count + 1)?;
    // @security: Allow for the wrapper input Vec, copied APP metadata, and decoder tables.
    let input_and_headers = encoded.checked_mul(8)?.checked_add(64 * 1024)?;
    samples
        .checked_add(coefficients)?
        .checked_add(rows)?
        .checked_add(input_and_headers)
}

fn decode_ascii_hex(mut bytes: Vec<u8>, limit: usize) -> Result<Vec<u8>, PdfReadError> {
    let mut decoded = 0;
    let mut high = None;
    for index in 0..bytes.len() {
        let byte = bytes[index];
        if is_pdf_space(byte) {
            continue;
        }
        if byte == b'>' {
            break;
        }
        let nibble = hex_nibble(byte).ok_or(PdfReadError::Syntax)?;
        if let Some(high) = high.take() {
            if decoded == limit {
                return Err(PdfReadError::DecodedStreamLimit);
            }
            bytes[decoded] = (high << 4) | nibble;
            decoded += 1;
        } else {
            high = Some(nibble);
        }
    }
    if let Some(high) = high {
        if decoded == limit {
            return Err(PdfReadError::DecodedStreamLimit);
        }
        bytes[decoded] = high << 4;
        decoded += 1;
    }
    bytes.truncate(decoded);
    Ok(bytes)
}

pub(crate) fn report_active_content(
    dictionary: &BTreeMap<String, PdfValue>,
    page_index: Option<u32>,
    object_number: Option<u32>,
    report: &mut PdfReadReport,
) {
    for key in dictionary.keys() {
        let diagnostic = match key.as_str() {
            "JS" | "JavaScript" | "OpenAction" | "AA" | "Launch" | "SubmitForm" | "GoToR"
            | "URI" | "RichMedia" | "Movie" | "Sound" => Some(PdfReadDiagnostic::blocked(
                PdfReadDiagnosticCode::ActiveContent,
                page_index,
                object_number,
            )),
            "F" | "FS" | "EmbeddedFiles" | "EF" | "External" => Some(PdfReadDiagnostic::blocked(
                PdfReadDiagnosticCode::ExternalResource,
                page_index,
                object_number,
            )),
            _ => None,
        };
        if let Some(diagnostic) = diagnostic {
            report.push(diagnostic);
        }
    }
}

fn validate_decimal_token(bytes: &[u8]) -> Result<(), PdfReadError> {
    if bytes.len() > 128 {
        return Err(PdfReadError::NumericLimit);
    }
    let start = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let mut dots = 0;
    let mut digits = 0;
    for byte in &bytes[start..] {
        if *byte == b'.' {
            dots += 1;
        } else if byte.is_ascii_digit() {
            digits += 1;
        } else {
            return Err(PdfReadError::NumericLimit);
        }
    }
    if dots != 1 || digits == 0 {
        return Err(PdfReadError::NumericLimit);
    }
    Ok(())
}

fn parse_decimal(bytes: &[u8]) -> Result<LayoutUnit, PdfReadError> {
    if bytes.is_empty() || bytes.len() > 32 {
        return Err(PdfReadError::NumericLimit);
    }
    let mut index = 0;
    let negative = match bytes[0] {
        b'-' => {
            index = 1;
            true
        }
        b'+' => {
            index = 1;
            false
        }
        _ => false,
    };
    let mut whole: i128 = 0;
    let mut fraction: i128 = 0;
    let mut denominator: i128 = 1;
    let mut after_decimal = false;
    let mut digit_count = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'.' && !after_decimal {
            after_decimal = true;
            index += 1;
            continue;
        }
        if !byte.is_ascii_digit() {
            return Err(PdfReadError::NumericLimit);
        }
        digit_count += 1;
        if digit_count > 18 {
            return Err(PdfReadError::NumericLimit);
        }
        let digit = i128::from(byte - b'0');
        if after_decimal {
            fraction = fraction
                .checked_mul(10)
                .and_then(|v| v.checked_add(digit))
                .ok_or(PdfReadError::NumericLimit)?;
            denominator = denominator
                .checked_mul(10)
                .ok_or(PdfReadError::NumericLimit)?;
        } else {
            whole = whole
                .checked_mul(10)
                .and_then(|v| v.checked_add(digit))
                .ok_or(PdfReadError::NumericLimit)?;
        }
        index += 1;
    }
    if digit_count == 0 {
        return Err(PdfReadError::NumericLimit);
    }
    let numerator = whole
        .checked_mul(denominator)
        .and_then(|v| v.checked_add(fraction))
        .and_then(|v| v.checked_mul(i128::from(LayoutUnit::UNITS_PER_POINT)))
        .ok_or(PdfReadError::NumericLimit)?;
    let rounded = (numerator.unsigned_abs() + (denominator as u128 / 2)) / denominator as u128;
    let rounded = i128::try_from(rounded).map_err(|_| PdfReadError::NumericLimit)?;
    let signed = if negative { -rounded } else { rounded };
    i64::try_from(signed)
        .map(LayoutUnit::from_raw)
        .map_err(|_| PdfReadError::NumericLimit)
}

fn parse_u32(bytes: &[u8]) -> Result<u32, PdfReadError> {
    let value = parse_usize(bytes)?;
    u32::try_from(value).map_err(|_| PdfReadError::NumericLimit)
}

fn parse_u16(bytes: &[u8]) -> Result<u16, PdfReadError> {
    let value = parse_usize(bytes)?;
    u16::try_from(value).map_err(|_| PdfReadError::NumericLimit)
}

fn parse_usize(bytes: &[u8]) -> Result<usize, PdfReadError> {
    if bytes.is_empty() || bytes.iter().any(|byte| !byte.is_ascii_digit()) {
        return Err(PdfReadError::Syntax);
    }
    let mut value = 0_usize;
    for byte in bytes {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(usize::from(byte - b'0')))
            .ok_or(PdfReadError::NumericLimit)?;
    }
    Ok(value)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    limits: PdfReaderLimits,
    tokens: usize,
    exact: bool,
    remaining: Option<usize>,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], position: usize, limits: PdfReaderLimits) -> Self {
        Self {
            bytes,
            position,
            limits,
            tokens: 0,
            exact: false,
            remaining: None,
        }
    }

    fn charge(&mut self, bytes: usize) -> Result<(), PdfReadError> {
        if let Some(remaining) = &mut self.remaining {
            *remaining = remaining
                .checked_sub(bytes)
                .ok_or(PdfReadError::InputSizeLimit)?;
        }
        Ok(())
    }

    fn position(&self) -> usize {
        self.position
    }

    fn skip_space(&mut self) {
        loop {
            while self
                .bytes
                .get(self.position)
                .is_some_and(|byte| is_pdf_space(*byte))
            {
                self.position += 1;
            }
            if self.bytes.get(self.position) != Some(&b'%') {
                break;
            }
            while self.position < self.bytes.len()
                && !matches!(self.bytes[self.position], b'\r' | b'\n')
            {
                self.position += 1;
            }
        }
    }

    fn token(&mut self) -> Result<Option<Vec<u8>>, PdfReadError> {
        self.skip_space();
        if self.position >= self.bytes.len() {
            return Ok(None);
        }
        self.tokens = self.tokens.checked_add(1).ok_or(PdfReadError::TokenLimit)?;
        if self.tokens > self.limits.max_content_tokens {
            return Err(PdfReadError::TokenLimit);
        }
        let start = self.position;
        if self.bytes[start] == b'<' && self.bytes.get(start + 1) == Some(&b'<') {
            self.position += 2;
            return Ok(Some(b"<<".to_vec()));
        }
        if self.bytes[start] == b'>' && self.bytes.get(start + 1) == Some(&b'>') {
            self.position += 2;
            return Ok(Some(b">>".to_vec()));
        }
        while self.position < self.bytes.len() && !is_delimiter(self.bytes[self.position]) {
            self.position += 1;
        }
        if self.position == start {
            self.position += 1;
        }
        self.charge(self.position - start)?;
        Ok(Some(self.bytes[start..self.position].to_vec()))
    }

    fn parse_u64_token(&mut self) -> Result<Option<u64>, PdfReadError> {
        let Some(token) = self.token()? else {
            return Ok(None);
        };
        if token.is_empty() || token.iter().any(|byte| !byte.is_ascii_digit()) {
            return Err(PdfReadError::Syntax);
        }
        let mut value = 0_u64;
        for byte in token {
            value = value
                .checked_mul(10)
                .and_then(|value| value.checked_add(u64::from(byte - b'0')))
                .ok_or(PdfReadError::NumericLimit)?;
        }
        Ok(Some(value))
    }

    fn consume_keyword(&mut self, keyword: &[u8]) -> Result<bool, PdfReadError> {
        let checkpoint = self.position;
        if let Some(token) = self.token()?
            && token == keyword
        {
            return Ok(true);
        }
        self.position = checkpoint;
        Ok(false)
    }

    fn object_syntax_error(&self) -> PdfReadError {
        if self.exact && self.position >= self.bytes.len() {
            PdfReadError::XrefInvalid
        } else {
            PdfReadError::Syntax
        }
    }

    fn object_value(&mut self) -> Result<PdfValue, PdfReadError> {
        self.parse_value(0).map_err(|error| match error {
            PdfReadError::Syntax => self.object_syntax_error(),
            error => error,
        })
    }

    fn stream_payload(&mut self, length: usize) -> Result<&'a [u8], PdfReadError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(PdfReadError::StreamLimit)?;
        let refusal = if self.exact {
            PdfReadError::XrefInvalid
        } else {
            PdfReadError::StreamLimit
        };
        let bytes = self.bytes.get(self.position..end).ok_or(refusal)?;
        self.position = end;
        Ok(bytes)
    }

    fn require_object_keyword(&mut self, keyword: &[u8]) -> Result<(), PdfReadError> {
        let token = self.token()?;
        if (self.exact && self.position == self.bytes.len()) || token.as_deref() != Some(keyword) {
            return Err(self.object_syntax_error());
        }
        Ok(())
    }

    fn consume_stream_eol(&mut self) {
        if self.bytes.get(self.position) == Some(&b'\r') {
            self.position += 1;
            if self.bytes.get(self.position) == Some(&b'\n') {
                self.position += 1;
            }
        } else if self.bytes.get(self.position) == Some(&b'\n') {
            self.position += 1;
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<PdfValue, PdfReadError> {
        if depth > self.limits.max_nesting {
            return Err(PdfReadError::NestingLimit);
        }
        // @security: Charge retained visits before allocating nested values.
        if self.exact {
            self.tokens = self.tokens.checked_add(1).ok_or(PdfReadError::TokenLimit)?;
            if self.tokens > self.limits.max_content_tokens {
                return Err(PdfReadError::TokenLimit);
            }
        }
        self.charge(std::mem::size_of::<PdfValue>() * 2)?;
        self.skip_space();
        match self.bytes.get(self.position).copied() {
            Some(b'(') => self.parse_literal_string().map(PdfValue::String),
            Some(b'<') if self.bytes.get(self.position + 1) != Some(&b'<') => {
                self.parse_hex_string().map(PdfValue::String)
            }
            Some(b'[') => self.parse_array(depth),
            Some(b'<') => self.parse_dictionary(depth).map(PdfValue::Dictionary),
            Some(b'/') => self.parse_name().map(PdfValue::Name),
            Some(_) => {
                let token = self.token()?.ok_or(PdfReadError::Syntax)?;
                match token.as_slice() {
                    b"null" => Ok(PdfValue::Null),
                    b"true" => Ok(PdfValue::Boolean(true)),
                    b"false" => Ok(PdfValue::Boolean(false)),
                    _ => {
                        let integer = token
                            .iter()
                            .all(|byte| byte.is_ascii_digit() || *byte == b'+' || *byte == b'-');
                        let number = token.iter().all(|byte| {
                            byte.is_ascii_digit() || matches!(*byte, b'+' | b'-' | b'.')
                        });
                        if !number {
                            return Err(PdfReadError::Syntax);
                        }
                        let first = if integer {
                            let parsed = parse_signed_i64(&token)?;
                            PdfValue::Integer(parsed)
                        } else {
                            if self.exact {
                                validate_decimal_token(&token)?;
                                self.charge(token.len())?;
                                PdfValue::ExactReal(
                                    String::from_utf8(token.clone())
                                        .map_err(|_| PdfReadError::Syntax)?,
                                )
                            } else {
                                let rounded = parse_decimal(&token)?;
                                let precise = std::str::from_utf8(&token)
                                    .map_err(|_| PdfReadError::Syntax)?
                                    .parse::<f64>()
                                    .map_err(|_| PdfReadError::NumericLimit)?;
                                PdfValue::Real((rounded, precise))
                            }
                        };
                        if let PdfValue::Integer(object_number) = first {
                            let checkpoint = self.position;
                            self.skip_space();
                            if let Some(second) = self.token()?
                                && let Ok(generation) = parse_u16(&second)
                                && self.consume_keyword(b"R")?
                                && object_number >= 0
                            {
                                return Ok(PdfValue::Reference(PdfObjectRef {
                                    object_number: u32::try_from(object_number)
                                        .map_err(|_| PdfReadError::ReferenceInvalid)?,
                                    generation,
                                }));
                            }
                            self.position = checkpoint;
                            Ok(PdfValue::Integer(object_number))
                        } else {
                            Ok(first)
                        }
                    }
                }
            }
            None => Err(PdfReadError::Syntax),
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<PdfValue, PdfReadError> {
        self.position += 1;
        let mut values = Vec::new();
        loop {
            self.skip_space();
            if self.bytes.get(self.position) == Some(&b']') {
                self.position += 1;
                return Ok(PdfValue::Array(values));
            }
            if values.len() >= self.limits.max_array_items {
                return Err(PdfReadError::ObjectLimit);
            }
            values.push(self.parse_value(depth + 1)?);
        }
    }

    fn parse_dictionary(
        &mut self,
        depth: usize,
    ) -> Result<BTreeMap<String, PdfValue>, PdfReadError> {
        if self.bytes.get(self.position) != Some(&b'<')
            || self.bytes.get(self.position + 1) != Some(&b'<')
        {
            return Err(PdfReadError::Syntax);
        }
        self.position += 2;
        let mut dictionary = BTreeMap::new();
        loop {
            self.skip_space();
            if self.bytes.get(self.position) == Some(&b'>')
                && self.bytes.get(self.position + 1) == Some(&b'>')
            {
                self.position += 2;
                return Ok(dictionary);
            }
            if dictionary.len() >= self.limits.max_dictionary_entries {
                return Err(PdfReadError::ObjectLimit);
            }
            let key = self.parse_name()?;
            let value = self.parse_value(depth + 1)?;
            if dictionary.insert(key, value).is_some() {
                return Err(PdfReadError::Syntax);
            }
        }
    }

    fn parse_name(&mut self) -> Result<String, PdfReadError> {
        if self.bytes.get(self.position) != Some(&b'/') {
            return Err(PdfReadError::Syntax);
        }
        self.position += 1;
        let bytes = take_pdf_name(self.bytes, &mut self.position)?;
        self.charge(bytes.len().min(MAX_PDF_IDENTITY_BYTES))?;
        let output = decode_pdf_name_bytes(bytes)?;
        if !self.exact {
            return Ok(scene_name_identity(output));
        }
        let utf8_len = output.len() + output.iter().filter(|byte| !byte.is_ascii()).count();
        self.charge(utf8_len)?;
        let mut name = String::with_capacity(utf8_len);
        name.extend(output.into_iter().map(char::from));
        Ok(name)
    }

    fn parse_literal_string(&mut self) -> Result<Vec<u8>, PdfReadError> {
        self.position += 1;
        let mut output = Vec::new();
        let mut nesting = 1_usize;
        while self.position < self.bytes.len() {
            self.charge(2)?;
            let byte = self.bytes[self.position];
            self.position += 1;
            match byte {
                b'(' => {
                    nesting = nesting.checked_add(1).ok_or(PdfReadError::NestingLimit)?;
                    if nesting > self.limits.max_nesting {
                        return Err(PdfReadError::NestingLimit);
                    }
                    output.push(byte);
                }
                b')' => {
                    nesting = nesting.checked_sub(1).ok_or(PdfReadError::Syntax)?;
                    if nesting == 0 {
                        return Ok(output);
                    }
                    output.push(byte);
                }
                b'\\' => {
                    let escaped = self
                        .bytes
                        .get(self.position)
                        .copied()
                        .ok_or(PdfReadError::Syntax)?;
                    self.position += 1;
                    match escaped {
                        b'n' => output.push(b'\n'),
                        b'r' => output.push(b'\r'),
                        b't' => output.push(b'\t'),
                        b'b' => output.push(8),
                        b'f' => output.push(12),
                        b'\r' => {
                            if self.bytes.get(self.position) == Some(&b'\n') {
                                self.position += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut value = escaped - b'0';
                            for _ in 0..2 {
                                let Some(next) = self.bytes.get(self.position).copied() else {
                                    break;
                                };
                                if !(b'0'..=b'7').contains(&next) {
                                    break;
                                }
                                self.position += 1;
                                value = value.wrapping_mul(8).wrapping_add(next - b'0');
                            }
                            output.push(value);
                        }
                        other => output.push(other),
                    }
                }
                b'\r' => {
                    if self.bytes.get(self.position) == Some(&b'\n') {
                        self.position += 1;
                    }
                    output.push(b'\n');
                }
                other => output.push(other),
            }
            if output.len() > self.limits.max_string_bytes {
                return Err(PdfReadError::StreamLimit);
            }
        }
        Err(PdfReadError::Syntax)
    }

    fn parse_hex_string(&mut self) -> Result<Vec<u8>, PdfReadError> {
        self.position += 1;
        let mut output = Vec::new();
        let mut high: Option<u8> = None;
        while self.position < self.bytes.len() {
            self.charge(2)?;
            let byte = self.bytes[self.position];
            self.position += 1;
            if is_pdf_space(byte) {
                continue;
            }
            if byte == b'>' {
                if let Some(high) = high {
                    output.push(high << 4);
                }
                if output.len() > self.limits.max_string_bytes {
                    return Err(PdfReadError::StreamLimit);
                }
                return Ok(output);
            }
            let nibble = hex_nibble(byte).ok_or(PdfReadError::Syntax)?;
            if let Some(high) = high.take() {
                output.push((high << 4) | nibble);
            } else {
                high = Some(nibble);
            }
            if output.len() > self.limits.max_string_bytes {
                return Err(PdfReadError::StreamLimit);
            }
        }
        Err(PdfReadError::Syntax)
    }
}

fn is_pdf_space(byte: u8) -> bool {
    matches!(byte, 0 | 9 | 10 | 12 | 13 | 32)
}

fn is_delimiter(byte: u8) -> bool {
    is_pdf_space(byte)
        || matches!(
            byte,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_signed_i64(bytes: &[u8]) -> Result<i64, PdfReadError> {
    if bytes.is_empty() {
        return Err(PdfReadError::NumericLimit);
    }
    let negative = bytes[0] == b'-';
    let start = usize::from(bytes[0] == b'+' || negative);
    if start == bytes.len() || bytes[start..].iter().any(|byte| !byte.is_ascii_digit()) {
        return Err(PdfReadError::NumericLimit);
    }
    let mut value = 0_i128;
    for byte in &bytes[start..] {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(i128::from(byte - b'0')))
            .ok_or(PdfReadError::NumericLimit)?;
    }
    if negative {
        value = -value;
    }
    i64::try_from(value).map_err(|_| PdfReadError::NumericLimit)
}

#[cfg(test)]
mod retained_reader_tests {
    use super::*;

    #[test]
    fn retained_starts_require_budget_and_distinct_offsets_before_xref() {
        let mut xref = BTreeMap::from([
            (
                2,
                PdfXrefEntry {
                    offset: 30,
                    generation: 0,
                },
            ),
            (
                8,
                PdfXrefEntry {
                    offset: 10,
                    generation: 0,
                },
            ),
        ]);
        let mut bytes = [b'1'; 50];
        let cost = 3 * std::mem::size_of::<usize>();
        assert_eq!(
            retained_object_starts(&xref, &bytes, cost - 1),
            Err(PdfReadError::InputSizeLimit)
        );
        assert_eq!(
            retained_object_starts(&xref, &bytes, cost),
            Ok((vec![10, 30, 50], cost))
        );
        assert_eq!(
            retained_object_starts(&xref, &bytes[..30], cost),
            Err(PdfReadError::XrefInvalid)
        );
        bytes[10] = b' ';
        assert_eq!(
            retained_object_starts(&xref, &bytes, cost),
            Err(PdfReadError::XrefInvalid)
        );
        bytes[10] = b'1';
        xref.get_mut(&8).unwrap().offset = 30;
        assert_eq!(
            retained_object_starts(&xref, &bytes, cost),
            Err(PdfReadError::XrefInvalid)
        );
    }

    #[test]
    fn stream_payload_checks_extent_before_advancing() {
        let paint = b"q Q";
        let mut cursor = Cursor::new(paint, 0, PdfReaderLimits::default());
        assert_eq!(cursor.stream_payload(4), Err(PdfReadError::StreamLimit));
        cursor.exact = true;
        assert_eq!(cursor.stream_payload(4), Err(PdfReadError::XrefInvalid));
        assert_eq!(cursor.position(), 0);
        assert_eq!(cursor.stream_payload(3).unwrap(), paint);
        assert_eq!(cursor.position(), 3);
    }

    #[test]
    fn retained_scalar_requires_a_complete_terminator_within_its_extent() {
        let scalar = b"42 endobj\n";
        for end in [5, 9, scalar.len()] {
            let mut cursor = Cursor::new(&scalar[..end], 0, PdfReaderLimits::default());
            cursor.exact = true;
            assert!(matches!(cursor.parse_value(0), Ok(PdfValue::Integer(42))));
            assert_eq!(
                cursor.require_object_keyword(b"endobj"),
                if end == scalar.len() {
                    Ok(())
                } else {
                    Err(PdfReadError::XrefInvalid)
                },
            );
        }
    }

    #[test]
    fn retained_jpeg_checks_decoded_budget_and_dictionary_before_returning_samples() {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode(&[30, 60, 90, 120], 2, 2, image::ExtendedColorType::L8)
            .unwrap();
        let mut dictionary = BTreeMap::from([
            ("Subtype".into(), PdfValue::Name("Image".into())),
            ("Filter".into(), PdfValue::Name("DCTDecode".into())),
            ("ColorSpace".into(), PdfValue::Name("DeviceGray".into())),
            ("BitsPerComponent".into(), PdfValue::Integer(8)),
            ("Width".into(), PdfValue::Integer(2)),
            ("Height".into(), PdfValue::Integer(2)),
        ]);
        let mut limits = PdfReaderLimits {
            max_decoded_stream_bytes: 3,
            ..Default::default()
        };
        assert_eq!(
            decode_stream(&dictionary, bytes.clone(), &limits, Some(usize::MAX)),
            Err(PdfReadError::DecodedStreamLimit)
        );
        limits.max_decoded_stream_bytes = 4;
        let (_, _, _, components) = jpeg_frame(&bytes).unwrap();
        let peak = jpeg_decode_peak(bytes.len(), 2, 2, components, 4).unwrap();
        assert_eq!(
            decode_stream(&dictionary, bytes.clone(), &limits, Some(peak - 1)),
            Err(PdfReadError::DecodedStreamLimit)
        );
        assert_eq!(
            decode_stream(&dictionary, bytes.clone(), &limits, Some(peak))
                .unwrap()
                .len(),
            4
        );
        dictionary.insert("Width".into(), PdfValue::Integer(1));
        assert_eq!(
            decode_stream(&dictionary, bytes, &limits, Some(usize::MAX)),
            Err(PdfReadError::Syntax)
        );
    }

    #[test]
    fn retained_lookahead_preserves_byte_and_token_exhaustion() {
        let mut cursor = Cursor::new(b"endobj", 0, PdfReaderLimits::default());
        cursor.exact = true;
        cursor.remaining = Some(5);
        assert_eq!(
            cursor.consume_keyword(b"stream"),
            Err(PdfReadError::InputSizeLimit)
        );

        for work in [2, 3] {
            let mut cursor = Cursor::new(b"1 0 R", 0, PdfReaderLimits::default());
            cursor.exact = true;
            cursor.remaining = Some(1024);
            cursor.limits.max_content_tokens = work;
            assert_eq!(cursor.parse_value(0).unwrap_err(), PdfReadError::TokenLimit);
        }

        let mut cursor = Cursor::new(b"endobj", 0, PdfReaderLimits::default());
        cursor.exact = true;
        cursor.remaining = Some(12);
        assert_eq!(cursor.consume_keyword(b"stream"), Ok(false));
        assert_eq!(cursor.consume_keyword(b"endobj"), Ok(true));
    }

    #[test]
    fn scene_names_decode_utf8_while_retained_names_keep_encoded_bytes() {
        let mut cursor = Cursor::new(b"/#D0#90", 0, PdfReaderLimits::default());
        assert_eq!(cursor.parse_name().unwrap(), "А");
        cursor.position = 0;
        cursor.exact = true;
        assert_eq!(cursor.parse_name().unwrap(), "\u{d0}\u{90}");
    }

    #[test]
    fn scene_name_identity_is_lossless_and_bounded() {
        let mut cursor = Cursor::new(
            b"<< /Logo#E9 1 /Logo#C3#A9 2 >>",
            0,
            PdfReaderLimits::default(),
        );
        let dictionary = cursor.parse_dictionary(0).unwrap();
        let byte_name = read_scene_name(b"#4Cogo#e9", &mut 0).unwrap();
        let utf8_name = read_scene_name("Logoé".as_bytes(), &mut 0).unwrap();
        assert_eq!(dictionary[&byte_name].as_integer(), Some(1));
        assert_eq!(dictionary[&utf8_name].as_integer(), Some(2));
        assert_eq!(scene_name_label(&byte_name), None);
        assert_eq!(scene_name_label(&utf8_name), Some("Logoé"));

        for invalid in [b"Logo#".as_slice(), b"Logo#00", b"Logo#XZ"] {
            assert_eq!(read_scene_name(invalid, &mut 0), Err(PdfReadError::Syntax));
        }
        let maximum = b"#E9".repeat(MAX_PDF_IDENTITY_BYTES);
        assert!(read_scene_name(&maximum, &mut 0).is_ok());
        let too_long = [maximum.as_slice(), b"X"].concat();
        assert_eq!(
            read_scene_name(&too_long, &mut 0),
            Err(PdfReadError::Syntax)
        );
        assert_eq!(
            read_scene_name(&vec![b'X'; MAX_PDF_IDENTITY_BYTES + 1], &mut 0),
            Err(PdfReadError::Syntax)
        );
    }

    #[test]
    fn ascii_hex_checks_limits_for_complete_and_padded_bytes() {
        assert_eq!(decode_ascii_hex(b"41 2>".to_vec(), 2).unwrap(), b"A ");
        assert!(matches!(
            decode_ascii_hex(b"41>".to_vec(), 0),
            Err(PdfReadError::DecodedStreamLimit)
        ));
        assert!(matches!(
            decode_ascii_hex(b"41 2>".to_vec(), 1),
            Err(PdfReadError::DecodedStreamLimit)
        ));
    }
}

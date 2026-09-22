//! Bounded, lazy reader for the controlled PDF syntax subset.
//!
//! This module deliberately indexes a classic xref table at open time and
//! parses indirect objects only when a page/scene request needs them.  It is a
//! reader boundary, not a general-purpose PDF repair engine: unsupported
//! syntax is classified explicitly and active content is never interpreted.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::layout::{LayoutRect, LayoutUnit};

pub const PDF_READER_SCHEMA_VERSION: u32 = 1;

const MAX_PDF_VERSION: &[u8] = b"1.7";
const MAX_PDF_IDENTITY_BYTES: usize = 128;

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
    pub source_offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum PdfValue {
    Null,
    Boolean,
    Integer(i64),
    Real(LayoutUnit),
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
            Self::Name(value) => Some(value),
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
            Self::Real(value) => Some(*value),
            _ => None,
        }
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

#[derive(Debug, Clone, Copy)]
struct PdfXrefEntry {
    offset: usize,
    generation: u16,
}

#[derive(Debug, Clone)]
pub(crate) struct PdfPageRecord {
    pub object_ref: PdfObjectRef,
    pub media_box: LayoutRect,
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
    trailer: BTreeMap<String, PdfValue>,
    root: PdfObjectRef,
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
        self.collect_pages(pages_ref, None, None, 0, &mut active, &mut pages)?;
        Ok(pages)
    }

    fn object_with_depth(
        &self,
        reference: PdfObjectRef,
        depth: usize,
    ) -> Result<PdfValue, PdfReadError> {
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
        let mut cursor = Cursor::new(&self.bytes, entry.offset, self.limits.clone());
        let object_number = cursor.parse_u64_token()?.ok_or(PdfReadError::Syntax)?;
        let generation = cursor.parse_u64_token()?.ok_or(PdfReadError::Syntax)?;
        let object_keyword = cursor.token()?.ok_or(PdfReadError::Syntax)?;
        if object_number != u64::from(reference.object_number)
            || generation != u64::from(reference.generation)
            || object_keyword != b"obj"
        {
            return Err(PdfReadError::XrefInvalid);
        }
        let mut value = cursor.parse_value(0)?;
        cursor.skip_space();
        if cursor.consume_keyword(b"stream") {
            let dictionary = value.as_dictionary().cloned().ok_or(PdfReadError::Syntax)?;
            cursor.consume_stream_eol();
            let length = dictionary
                .get("Length")
                .ok_or(PdfReadError::Syntax)
                .and_then(|length| self.stream_length(length, depth + 1))?;
            if length > self.limits.max_stream_bytes {
                return Err(PdfReadError::StreamLimit);
            }
            let end = cursor
                .position()
                .checked_add(length)
                .ok_or(PdfReadError::StreamLimit)?;
            if end > self.bytes.len() {
                return Err(PdfReadError::StreamLimit);
            }
            let raw = self.bytes[cursor.position()..end].to_vec();
            cursor.set_position(end);
            cursor.skip_space();
            if !cursor.consume_keyword(b"endstream") {
                return Err(PdfReadError::Syntax);
            }
            let data = decode_stream(&dictionary, raw, &self.limits)?;
            value = PdfValue::Stream(PdfStream {
                dict: dictionary,
                data,
                source_offset: entry.offset,
            });
        }
        Ok(value)
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
        inherited_media_box: Option<LayoutRect>,
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
        let media_box = dictionary
            .get("MediaBox")
            .map(|value| self.resolve(value, depth + 1))
            .transpose()?
            .map(|value| parse_media_box(&value))
            .transpose()?;
        let resources = dictionary
            .get("Resources")
            .map(|value| self.resolve(value, depth + 1))
            .transpose()?;
        let media_box = media_box.or(inherited_media_box);
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
                        media_box,
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
                    None => Vec::new(),
                    Some(PdfValue::Reference(reference)) => vec![*reference],
                    Some(value) => refs_from_value(&self.resolve(value, depth + 1)?)?,
                };
                let annotations = match dictionary.get("Annots") {
                    None => Vec::new(),
                    Some(PdfValue::Reference(reference)) => vec![*reference],
                    Some(value) => refs_from_value(&self.resolve(value, depth + 1)?)?,
                };
                pages.push(PdfPageRecord {
                    object_ref: reference,
                    media_box,
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
    limits.validate()?;
    if bytes.len() > limits.max_input_bytes {
        return Err(PdfReadError::InputSizeLimit);
    }
    let version = parse_header(bytes)?;
    let startxref = find_startxref(bytes)?;
    let (xref, trailer) = parse_xref(bytes, startxref, &limits)?;
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
        trailer,
        root,
        limits,
    })
}

fn parse_header(bytes: &[u8]) -> Result<String, PdfReadError> {
    if !bytes.starts_with(b"%PDF-") || bytes.len() < 8 {
        return Err(PdfReadError::HeaderInvalid);
    }
    let version = &bytes[5..8];
    if version[0] != b'1' || version[1] != b'.' || version[2] > MAX_PDF_VERSION[2] {
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

type ParsedXref = (BTreeMap<u32, PdfXrefEntry>, BTreeMap<String, PdfValue>);

fn parse_xref(
    bytes: &[u8],
    offset: usize,
    limits: &PdfReaderLimits,
) -> Result<ParsedXref, PdfReadError> {
    if offset >= bytes.len() {
        return Err(PdfReadError::XrefInvalid);
    }
    if bytes[offset..].starts_with(b"obj") {
        return Err(PdfReadError::XrefUnsupported);
    }
    let mut cursor = Cursor::new(bytes, offset, limits.clone());
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
        if count > limits.max_objects || first as usize > limits.max_objects {
            return Err(PdfReadError::ObjectLimit);
        }
        for index in 0..count {
            let offset = parse_usize(&cursor.token()?.ok_or(PdfReadError::XrefInvalid)?)?;
            let generation = parse_u16(&cursor.token()?.ok_or(PdfReadError::XrefInvalid)?)?;
            let flag = cursor.token()?.ok_or(PdfReadError::XrefInvalid)?;
            let object_number = first
                .checked_add(u32::try_from(index).map_err(|_| PdfReadError::ObjectLimit)?)
                .ok_or(PdfReadError::ObjectLimit)?;
            if flag == b"n" {
                if object_number == 0
                    || usize::try_from(object_number).unwrap_or(usize::MAX) > limits.max_objects
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
    let trailer = cursor
        .parse_value(0)?
        .as_dictionary()
        .cloned()
        .ok_or(PdfReadError::XrefInvalid)?;
    if xref.is_empty() || trailer.contains_key("Encrypt") {
        if trailer.contains_key("Encrypt") {
            return Err(PdfReadError::EncryptionUnsupported);
        }
        return Err(PdfReadError::XrefInvalid);
    }
    if let Some(size) = trailer.get("Size").and_then(PdfValue::as_integer)
        && (size < 1 || usize::try_from(size).unwrap_or(usize::MAX) > limits.max_objects + 1)
    {
        return Err(PdfReadError::ObjectLimit);
    }
    Ok((xref, trailer))
}

fn refs_from_value(value: &PdfValue) -> Result<Vec<PdfObjectRef>, PdfReadError> {
    match value {
        PdfValue::Reference(reference) => Ok(vec![*reference]),
        PdfValue::Array(values) => values
            .iter()
            .map(|value| value.as_reference().ok_or(PdfReadError::ReferenceInvalid))
            .collect(),
        _ => Err(PdfReadError::ReferenceInvalid),
    }
}

fn parse_media_box(value: &PdfValue) -> Result<LayoutRect, PdfReadError> {
    let values = value.as_array().ok_or(PdfReadError::PageTreeInvalid)?;
    if values.len() != 4 {
        return Err(PdfReadError::PageTreeInvalid);
    }
    let x0 = values[0].as_number().ok_or(PdfReadError::NumericLimit)?;
    let y0 = values[1].as_number().ok_or(PdfReadError::NumericLimit)?;
    let x1 = values[2].as_number().ok_or(PdfReadError::NumericLimit)?;
    let y1 = values[3].as_number().ok_or(PdfReadError::NumericLimit)?;
    let width = x1.checked_sub(x0).map_err(|_| PdfReadError::NumericLimit)?;
    let height = y1.checked_sub(y0).map_err(|_| PdfReadError::NumericLimit)?;
    if width.raw() <= 0 || height.raw() <= 0 {
        return Err(PdfReadError::PageTreeInvalid);
    }
    Ok(LayoutRect {
        x: x0,
        y: y0,
        width,
        height,
    })
}

fn decode_stream(
    dictionary: &BTreeMap<String, PdfValue>,
    raw: Vec<u8>,
    limits: &PdfReaderLimits,
) -> Result<Vec<u8>, PdfReadError> {
    let Some(filter) = dictionary.get("Filter") else {
        if raw.len() > limits.max_decoded_stream_bytes {
            return Err(PdfReadError::DecodedStreamLimit);
        }
        return Ok(raw);
    };
    let filter = match filter {
        PdfValue::Name(name) => name.as_str(),
        PdfValue::Array(filters) if filters.len() == 1 => filters[0]
            .as_name()
            .ok_or(PdfReadError::FilterUnsupported)?,
        _ => return Err(PdfReadError::FilterUnsupported),
    };
    match filter {
        "Identity" => Ok(raw),
        "DCTDecode" | "DCT" => {
            if raw.len() > limits.max_decoded_stream_bytes {
                return Err(PdfReadError::DecodedStreamLimit);
            }
            Ok(raw)
        }
        "FlateDecode" | "Fl" => {
            let decoded = miniz_oxide::inflate::decompress_to_vec_zlib(&raw)
                .map_err(|_| PdfReadError::Syntax)?;
            if decoded.len() > limits.max_decoded_stream_bytes {
                return Err(PdfReadError::DecodedStreamLimit);
            }
            Ok(decoded)
        }
        "ASCIIHexDecode" | "AHx" => {
            let decoded = decode_ascii_hex(&raw, limits.max_decoded_stream_bytes)?;
            Ok(decoded)
        }
        _ => Err(PdfReadError::FilterUnsupported),
    }
}

fn decode_ascii_hex(bytes: &[u8], limit: usize) -> Result<Vec<u8>, PdfReadError> {
    let mut output = Vec::new();
    let mut high: Option<u8> = None;
    for byte in bytes {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if *byte == b'>' {
            break;
        }
        let nibble = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return Err(PdfReadError::Syntax),
        };
        if let Some(high) = high.take() {
            output.push((high << 4) | nibble);
        } else {
            high = Some(nibble);
        }
        if output.len() > limit {
            return Err(PdfReadError::DecodedStreamLimit);
        }
    }
    if let Some(high) = high {
        output.push(high << 4);
    }
    if output.len() > limit {
        return Err(PdfReadError::DecodedStreamLimit);
    }
    Ok(output)
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
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], position: usize, limits: PdfReaderLimits) -> Self {
        Self {
            bytes,
            position,
            limits,
            tokens: 0,
        }
    }

    fn position(&self) -> usize {
        self.position
    }

    fn set_position(&mut self, position: usize) {
        self.position = position;
    }

    fn skip_space(&mut self) {
        loop {
            while self
                .bytes
                .get(self.position)
                .is_some_and(|byte| byte.is_ascii_whitespace())
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

    fn consume_keyword(&mut self, keyword: &[u8]) -> bool {
        let checkpoint = self.position;
        if let Ok(Some(token)) = self.token()
            && token == keyword
        {
            return true;
        }
        self.position = checkpoint;
        false
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
                    b"true" => Ok(PdfValue::Boolean),
                    b"false" => Ok(PdfValue::Boolean),
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
                            PdfValue::Real(parse_decimal(&token)?)
                        };
                        if let PdfValue::Integer(object_number) = first {
                            let checkpoint = self.position;
                            self.skip_space();
                            if let Ok(Some(second)) = self.token()
                                && let Ok(generation) = parse_u16(&second)
                                && self.consume_keyword(b"R")
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
        let start = self.position;
        while self.position < self.bytes.len() && !is_delimiter(self.bytes[self.position]) {
            self.position += 1;
        }
        if self.position == start || self.position - start > MAX_PDF_IDENTITY_BYTES {
            return Err(PdfReadError::Syntax);
        }
        let mut output = Vec::with_capacity(self.position - start);
        let bytes = &self.bytes[start..self.position];
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'#' && index + 2 < bytes.len() {
                let high = hex_nibble(bytes[index + 1]).ok_or(PdfReadError::Syntax)?;
                let low = hex_nibble(bytes[index + 2]).ok_or(PdfReadError::Syntax)?;
                output.push((high << 4) | low);
                index += 3;
            } else {
                output.push(bytes[index]);
                index += 1;
            }
        }
        String::from_utf8(output).map_err(|_| PdfReadError::Syntax)
    }

    fn parse_literal_string(&mut self) -> Result<Vec<u8>, PdfReadError> {
        self.position += 1;
        let mut output = Vec::new();
        let mut nesting = 1_usize;
        while self.position < self.bytes.len() {
            let byte = self.bytes[self.position];
            self.position += 1;
            match byte {
                b'(' => {
                    nesting += 1;
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
                                value = value * 8 + next - b'0';
                            }
                            output.push(value);
                        }
                        other => output.push(other),
                    }
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
            let byte = self.bytes[self.position];
            self.position += 1;
            if byte.is_ascii_whitespace() {
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

fn is_delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace()
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

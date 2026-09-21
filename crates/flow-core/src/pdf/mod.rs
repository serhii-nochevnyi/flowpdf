//! Deterministic, owned PDF syntax and export primitives.
//!
//! This module starts the Phase 4 writer with a deliberately small COS model.
//! It is not a PDF reader and it does not promise selectable text or external
//! PDF compatibility yet. Later adapters feed typed display-list resources
//! into the same bounded writer.

use std::{collections::BTreeMap, fmt};

use thiserror::Error;

use crate::layout::{LayoutRect, LayoutUnit};

mod assets;
mod display_list;
mod font;
mod metadata;
mod provenance;
mod recovery;

pub use assets::{PdfImageEncoding, PdfImageError, PdfImageResource, build_image_resources};
pub use display_list::{
    PDF_DISPLAY_LIST_SCHEMA_VERSION, PdfDisplayDiagnostic, PdfDisplayDiagnosticCode,
    PdfDisplayList, PdfDisplayListError, PdfDisplayPage, PdfGlyphPlacement, PdfImageItem,
    PdfTextItem, PdfTextLine, build_display_list,
};
pub use font::{PdfFontError, PdfFontResource, PdfSubsetGlyph, build_font_resources};
pub use metadata::{
    MAX_PDF_LINK_ENTRIES, MAX_PDF_METADATA_FIELD_BYTES, MAX_PDF_OUTLINE_ENTRIES, PdfInternalLink,
    PdfMetadataError, PdfMetadataOptions, PdfOutlineEntry, PdfSupportReport, PdfSupportedFeature,
    PdfUnsupportedFeature,
};
pub use provenance::{
    MAX_PDF_MANIFEST_BYTES, MAX_PDF_SOURCE_PAYLOAD_BYTES, MAX_PDF_SOURCE_STREAM_BYTES,
    PDF_PROVENANCE_SCHEMA_VERSION, PdfExportManifest, PdfFontManifestIdentity, PdfManifestOptions,
    PdfProvenanceError, PdfReproducibilityInputs,
};
pub use recovery::{
    PdfRecoveredSource, PdfRecoveryError, PdfRecoveryExpectation, recover_owned_source,
};

/// Version of the owned PDF export envelope.
pub const PDF_EXPORT_SCHEMA_VERSION: u32 = 1;
const MAX_PDF_OBJECTS: usize = 4_096;
const MAX_PDF_NESTING: usize = 64;
const MAX_PDF_NAME_BYTES: usize = 127;
const MAX_PDF_STRING_BYTES: usize = 64 * 1024;
const MAX_PDF_STREAM_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PDF_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PDF_PAGES: usize = 2_048;
const MAX_IDENTITY_BYTES: usize = 128;

/// A stable indirect object reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PdfRef {
    object_number: u32,
    generation: u16,
}

impl PdfRef {
    /// Creates a reference for a one-based object number.
    pub const fn new(object_number: u32, generation: u16) -> Result<Self, PdfError> {
        if object_number == 0 {
            return Err(PdfError::InvalidReference);
        }
        Ok(Self {
            object_number,
            generation,
        })
    }

    /// Returns the one-based object number.
    #[must_use]
    pub const fn object_number(self) -> u32 {
        self.object_number
    }
}

/// An ASCII PDF name. Names are intentionally closed and bounded at the
/// writer boundary; callers must use strings for arbitrary Unicode metadata.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PdfName(String);

impl PdfName {
    /// Validates one canonical PDF name token without its leading slash.
    pub fn new(value: impl Into<String>) -> Result<Self, PdfError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_PDF_NAME_BYTES || !value.is_ascii() {
            return Err(PdfError::InvalidName);
        }
        if value.bytes().any(|byte| {
            byte.is_ascii_whitespace()
                || matches!(
                    byte,
                    b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
                )
        }) {
            return Err(PdfError::InvalidName);
        }
        Ok(Self(value))
    }

    fn write_to(&self, output: &mut Vec<u8>) {
        output.push(b'/');
        output.extend_from_slice(self.0.as_bytes());
    }
}

/// A bounded PDF COS value. Dictionaries are ordered by `BTreeMap` key, so
/// serialization does not depend on host hash iteration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CosValue {
    Null,
    Boolean(bool),
    Integer(i64),
    Real(LayoutUnit),
    Name(PdfName),
    String(Vec<u8>),
    Array(Vec<Self>),
    Dictionary(BTreeMap<PdfName, Self>),
    Stream {
        dictionary: BTreeMap<PdfName, Self>,
        data: Vec<u8>,
    },
    Reference(PdfRef),
}

impl CosValue {
    /// Creates a PDF name value.
    pub fn name(value: impl Into<String>) -> Result<Self, PdfError> {
        PdfName::new(value).map(Self::Name)
    }

    /// Creates a PDF string value from bytes. UTF-8 metadata should be encoded
    /// by the caller; the writer treats strings as bytes and escapes them.
    pub fn string(value: impl Into<Vec<u8>>) -> Result<Self, PdfError> {
        let value = value.into();
        if value.len() > MAX_PDF_STRING_BYTES {
            return Err(PdfError::StringSizeLimit);
        }
        Ok(Self::String(value))
    }

    /// Creates an ordered dictionary and rejects the reserved stream length
    /// key, which the stream serializer owns.
    pub fn dictionary(
        entries: impl IntoIterator<Item = (PdfName, Self)>,
    ) -> Result<Self, PdfError> {
        let mut dictionary = BTreeMap::new();
        for (name, value) in entries {
            if dictionary.insert(name, value).is_some() {
                return Err(PdfError::DuplicateDictionaryKey);
            }
        }
        let entries = dictionary;
        if entries.keys().any(|key| key.0 == "Length") {
            return Err(PdfError::ReservedDictionaryKey);
        }
        Ok(Self::Dictionary(entries))
    }

    /// Creates a stream with an automatically generated `/Length` entry.
    pub fn stream(
        entries: impl IntoIterator<Item = (PdfName, Self)>,
        data: Vec<u8>,
    ) -> Result<Self, PdfError> {
        if data.len() > MAX_PDF_STREAM_BYTES {
            return Err(PdfError::StreamSizeLimit);
        }
        let mut dictionary = BTreeMap::new();
        for (name, value) in entries {
            if dictionary.insert(name, value).is_some() {
                return Err(PdfError::DuplicateDictionaryKey);
            }
        }
        if dictionary.keys().any(|key| key.0 == "Length") {
            return Err(PdfError::ReservedDictionaryKey);
        }
        Ok(Self::Stream { dictionary, data })
    }
}

#[derive(Debug, Clone)]
struct PdfObject {
    value: CosValue,
}

/// An owned indirect-object graph. Object numbers are assigned once, in
/// insertion order, and references cannot point to an absent object.
#[derive(Debug, Clone, Default)]
pub struct CosDocument {
    objects: Vec<PdfObject>,
    root: Option<PdfRef>,
    info: Option<PdfRef>,
}

impl CosDocument {
    /// Creates an empty object graph.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            objects: Vec::new(),
            root: None,
            info: None,
        }
    }

    /// Adds one indirect object and returns its stable reference.
    pub fn add_object(&mut self, value: CosValue) -> Result<PdfRef, PdfError> {
        if self.objects.len() >= MAX_PDF_OBJECTS {
            return Err(PdfError::ObjectCountLimit);
        }
        let number =
            u32::try_from(self.objects.len() + 1).map_err(|_| PdfError::ObjectCountLimit)?;
        self.objects.push(PdfObject { value });
        PdfRef::new(number, 0)
    }

    /// Replaces an object without changing its number. This is used to build
    /// parent/page references deterministically after reserving IDs.
    pub fn replace_object(&mut self, reference: PdfRef, value: CosValue) -> Result<(), PdfError> {
        let index = self.object_index(reference)?;
        self.objects[index].value = value;
        Ok(())
    }

    /// Sets the catalog/root object.
    pub fn set_root(&mut self, reference: PdfRef) -> Result<(), PdfError> {
        self.object_index(reference)?;
        self.root = Some(reference);
        Ok(())
    }

    /// Sets the optional document information dictionary.
    pub fn set_info(&mut self, reference: PdfRef) -> Result<(), PdfError> {
        self.object_index(reference)?;
        self.info = Some(reference);
        Ok(())
    }

    /// Validates and writes the complete PDF byte stream.
    pub fn write(&self, trailer_id: &str) -> Result<Vec<u8>, PdfError> {
        validate_identity(trailer_id)?;
        let root = self.root.ok_or(PdfError::MissingRoot)?;
        self.validate_graph(root)?;

        let mut output = Vec::with_capacity(8 * 1024);
        output.extend_from_slice(b"%PDF-1.7\n%\xFF\xFF\xFF\xFF\n");
        let mut offsets = Vec::with_capacity(self.objects.len() + 1);
        offsets.push(0_u64);
        for (index, object) in self.objects.iter().enumerate() {
            let number = index + 1;
            offsets.push(u64::try_from(output.len()).map_err(|_| PdfError::OutputSizeLimit)?);
            append_ascii(&mut output, &format!("{number} 0 obj\n"))?;
            serialize_value(&object.value, &mut output, 0)?;
            output.extend_from_slice(b"\nendobj\n");
            ensure_output_size(output.len())?;
        }

        let xref_offset = u64::try_from(output.len()).map_err(|_| PdfError::OutputSizeLimit)?;
        append_ascii(
            &mut output,
            &format!("xref\n0 {}\n", self.objects.len() + 1),
        )?;
        output.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            append_ascii(&mut output, &format!("{offset:010} 00000 n \n"))?;
        }
        output.extend_from_slice(b"trailer\n<< ");
        PdfName::new("Size")?.write_to(&mut output);
        append_ascii(&mut output, &format!(" {} ", self.objects.len() + 1))?;
        PdfName::new("Root")?.write_to(&mut output);
        append_ascii(&mut output, &format!(" {} 0 R ", root.object_number))?;
        if let Some(info) = self.info {
            self.object_index(info)?;
            PdfName::new("Info")?.write_to(&mut output);
            append_ascii(&mut output, &format!(" {} 0 R ", info.object_number))?;
        }
        PdfName::new("ID")?.write_to(&mut output);
        let id_hex = hex_bytes(trailer_id.as_bytes());
        append_ascii(&mut output, &format!("[<{id_hex}> <{id_hex}>] >>\n"))?;
        append_ascii(&mut output, &format!("startxref\n{xref_offset}\n%%EOF\n"))?;
        ensure_output_size(output.len())?;
        Ok(output)
    }

    fn object_index(&self, reference: PdfRef) -> Result<usize, PdfError> {
        let index = usize::try_from(reference.object_number)
            .ok()
            .and_then(|number| number.checked_sub(1))
            .ok_or(PdfError::InvalidReference)?;
        if reference.generation != 0 || index >= self.objects.len() {
            return Err(PdfError::DanglingReference);
        }
        Ok(index)
    }

    fn validate_graph(&self, root: PdfRef) -> Result<(), PdfError> {
        let mut visited = vec![false; self.objects.len()];
        let mut active = vec![false; self.objects.len()];
        self.validate_object(root, 0, &mut visited, &mut active)?;
        for object_number in 1..=self.objects.len() {
            if !visited[object_number - 1] {
                let reference = PdfRef::new(
                    u32::try_from(object_number).map_err(|_| PdfError::ObjectCountLimit)?,
                    0,
                )?;
                self.validate_object(reference, 0, &mut visited, &mut active)?;
            }
        }
        Ok(())
    }

    fn validate_object(
        &self,
        reference: PdfRef,
        depth: usize,
        visited: &mut [bool],
        active: &mut [bool],
    ) -> Result<(), PdfError> {
        if depth > MAX_PDF_NESTING {
            return Err(PdfError::NestingLimit);
        }
        let index = self.object_index(reference)?;
        if active[index] {
            return Err(PdfError::IndirectCycle);
        }
        if visited[index] {
            return Ok(());
        }
        active[index] = true;
        validate_value_references(
            &self.objects[index].value,
            depth,
            false,
            &mut |child, child_depth, allow_parent_cycle| {
                let child_index = self.object_index(child)?;
                if active[child_index] && allow_parent_cycle {
                    return Ok(());
                }
                self.validate_object(child, child_depth, visited, active)
            },
        )?;
        active[index] = false;
        visited[index] = true;
        Ok(())
    }
}

/// A page plan consumed by the first PDF envelope. Text and images are added
/// by later display-list plans; the empty content variant still proves page
/// geometry and writer determinism without claiming selectable output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfPagePlan {
    pub bounds: LayoutRect,
    pub content: PdfPageContent,
}

impl PdfPagePlan {
    /// Creates an empty page with positive fixed-point dimensions.
    pub fn empty(width: LayoutUnit, height: LayoutUnit) -> Result<Self, PdfError> {
        if width.raw() <= 0 || height.raw() <= 0 {
            return Err(PdfError::InvalidPageGeometry);
        }
        Ok(Self {
            bounds: LayoutRect {
                x: LayoutUnit::from_raw(0),
                y: LayoutUnit::from_raw(0),
                width,
                height,
            },
            content: PdfPageContent::Empty,
        })
    }
}

/// Content supported by the minimal page envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfPageContent {
    Empty,
}

/// Options that affect the deterministic export fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfExportOptions {
    pub producer: String,
    pub metadata: PdfMetadataOptions,
    pub outlines: Vec<PdfOutlineEntry>,
    pub internal_links: Vec<PdfInternalLink>,
}

impl Default for PdfExportOptions {
    fn default() -> Self {
        Self {
            producer: "FlowPDF".to_owned(),
            metadata: PdfMetadataOptions::default(),
            outlines: Vec::new(),
            internal_links: Vec::new(),
        }
    }
}

/// Immutable identity-bound request for the PDF envelope.
#[derive(Clone, PartialEq, Eq)]
pub struct PdfExportRequest {
    pub source_revision: u32,
    pub source_hash: String,
    pub layout_settings_fingerprint: String,
    pub pages: Vec<PdfPagePlan>,
    pub options: PdfExportOptions,
    pub reproducibility: PdfReproducibilityInputs,
    source_payload: Option<Vec<u8>>,
}

impl fmt::Debug for PdfExportRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PdfExportRequest")
            .field("source_revision", &self.source_revision)
            .field("source_hash", &self.source_hash)
            .field(
                "layout_settings_fingerprint",
                &self.layout_settings_fingerprint,
            )
            .field("pages", &self.pages)
            .field("options", &self.options)
            .field("reproducibility", &self.reproducibility)
            .field("source_payload_present", &self.source_payload.is_some())
            .finish()
    }
}

impl PdfExportRequest {
    /// Validates the identities and page geometry used by the writer.
    pub fn new(
        source_revision: u32,
        source_hash: impl Into<String>,
        layout_settings_fingerprint: impl Into<String>,
        pages: Vec<PdfPagePlan>,
        options: PdfExportOptions,
    ) -> Result<Self, PdfError> {
        let source_hash = source_hash.into();
        let layout_settings_fingerprint = layout_settings_fingerprint.into();
        validate_identity(&source_hash)?;
        validate_identity(&layout_settings_fingerprint)?;
        validate_identity(&options.producer)?;
        if pages.is_empty() {
            return Err(PdfError::EmptyPagePlan);
        }
        if pages.len() > MAX_PDF_PAGES {
            return Err(PdfError::PageCountLimit);
        }
        if pages
            .iter()
            .any(|page| page.bounds.width.raw() <= 0 || page.bounds.height.raw() <= 0)
        {
            return Err(PdfError::InvalidPageGeometry);
        }
        metadata::validate_export_metadata(
            &options.metadata,
            &options.outlines,
            &options.internal_links,
            &pages,
        )?;
        Ok(Self {
            source_revision,
            source_hash,
            layout_settings_fingerprint,
            pages,
            options,
            reproducibility: PdfReproducibilityInputs::default(),
            source_payload: None,
        })
    }

    /// Adds the exact derived identities that make an export reproducible.
    pub fn with_reproducibility_inputs(
        mut self,
        inputs: PdfReproducibilityInputs,
    ) -> Result<Self, PdfError> {
        provenance::validate_reproducibility_inputs(&inputs)?;
        self.reproducibility = inputs;
        Ok(self)
    }

    /// Binds one canonical FlowDocument JSON payload to this export request.
    /// The payload must already match the request's source hash and revision.
    pub fn with_source_payload(mut self, payload: Vec<u8>) -> Result<Self, PdfError> {
        if payload.len() > MAX_PDF_SOURCE_PAYLOAD_BYTES {
            return Err(PdfProvenanceError::SourcePayloadLimit.into());
        }
        let document = crate::canonical::decode_canonical(&payload)
            .map_err(|_| PdfProvenanceError::InvalidCanonicalPayload)?;
        if crate::canonical::canonical_hash(&payload) != self.source_hash {
            return Err(PdfProvenanceError::SourceHashMismatch.into());
        }
        if document.revision != self.source_revision {
            return Err(PdfProvenanceError::SourceRevisionMismatch.into());
        }
        self.source_payload = Some(payload);
        Ok(self)
    }

    /// Canonicalizes and binds a typed FlowDocument to this export request.
    pub fn with_source_document(
        self,
        document: &crate::model::FlowDocument,
    ) -> Result<Self, PdfError> {
        let payload = crate::canonical::canonical_bytes(document)
            .map_err(|_| PdfProvenanceError::InvalidCanonicalPayload)?;
        self.with_source_payload(payload)
    }

    fn fingerprint(&self) -> Result<String, PdfError> {
        let mut bytes = Vec::new();
        append_ascii(
            &mut bytes,
            &format!(
                "{}\n{}\n{}\n",
                self.source_revision, self.source_hash, self.layout_settings_fingerprint
            ),
        )?;
        append_fingerprint_text(&mut bytes, &self.options.producer);
        metadata::append_fingerprint(
            &mut bytes,
            &self.options.metadata,
            &self.options.outlines,
            &self.options.internal_links,
        );
        provenance::append_inputs_fingerprint(&mut bytes, &self.reproducibility);
        for page in &self.pages {
            append_ascii(
                &mut bytes,
                &format!(
                    "{}\n{}\n",
                    page.bounds.width.raw(),
                    page.bounds.height.raw()
                ),
            )?;
        }
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
}

/// Immutable PDF bytes and the identity used to produce them.
#[derive(Clone, PartialEq, Eq)]
pub struct PdfExportResult {
    pub schema_version: u32,
    pub source_revision: u32,
    pub source_hash: String,
    pub layout_settings_fingerprint: String,
    pub export_fingerprint: String,
    pub byte_hash: String,
    pub support_report: PdfSupportReport,
    pub manifest: PdfExportManifest,
    pub private_source_stream: Vec<u8>,
    pub bytes: Vec<u8>,
}

impl fmt::Debug for PdfExportResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PdfExportResult")
            .field("schema_version", &self.schema_version)
            .field("source_revision", &self.source_revision)
            .field("source_hash", &self.source_hash)
            .field(
                "layout_settings_fingerprint",
                &self.layout_settings_fingerprint,
            )
            .field("export_fingerprint", &self.export_fingerprint)
            .field("byte_hash", &self.byte_hash)
            .field("support_report", &self.support_report)
            .field("manifest", &self.manifest)
            .field(
                "private_source_stream_bytes",
                &self.private_source_stream.len(),
            )
            .field("pdf_bytes", &self.bytes.len())
            .finish()
    }
}

/// Stable failure taxonomy for the bounded owned writer.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PdfError {
    #[error("PDF identity is empty or oversized")]
    InvalidIdentity,
    #[error("PDF name is invalid")]
    InvalidName,
    #[error("PDF string exceeds the limit")]
    StringSizeLimit,
    #[error("PDF stream exceeds the limit")]
    StreamSizeLimit,
    #[error("PDF output exceeds the limit")]
    OutputSizeLimit,
    #[error("PDF object count exceeds the limit")]
    ObjectCountLimit,
    #[error("PDF nesting exceeds the limit")]
    NestingLimit,
    #[error("PDF root object is missing")]
    MissingRoot,
    #[error("PDF reference is invalid")]
    InvalidReference,
    #[error("PDF reference points to no object")]
    DanglingReference,
    #[error("PDF object graph contains an indirect cycle")]
    IndirectCycle,
    #[error("PDF stream dictionary owns a reserved key")]
    ReservedDictionaryKey,
    #[error("PDF dictionary contains a duplicate key")]
    DuplicateDictionaryKey,
    #[error("PDF page geometry is invalid")]
    InvalidPageGeometry,
    #[error("PDF page plan is empty")]
    EmptyPagePlan,
    #[error("PDF page count exceeds the limit")]
    PageCountLimit,
    #[error("PDF serialization exceeded a numeric limit")]
    NumericLimit,
    #[error("PDF metadata or document-structure options are invalid: {0}")]
    Metadata(#[from] PdfMetadataError),
    #[error("PDF provenance or source-payload validation failed: {0}")]
    Provenance(#[from] PdfProvenanceError),
}

impl PdfError {
    /// Stable diagnostic code suitable for JSON/WASM callers.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidIdentity => "FLOW_PDF_IDENTITY_INVALID",
            Self::InvalidName => "FLOW_PDF_NAME_INVALID",
            Self::StringSizeLimit => "FLOW_PDF_STRING_LIMIT",
            Self::StreamSizeLimit => "FLOW_PDF_STREAM_LIMIT",
            Self::OutputSizeLimit => "FLOW_PDF_OUTPUT_LIMIT",
            Self::ObjectCountLimit => "FLOW_PDF_OBJECT_LIMIT",
            Self::NestingLimit => "FLOW_PDF_NESTING_LIMIT",
            Self::MissingRoot => "FLOW_PDF_ROOT_MISSING",
            Self::InvalidReference => "FLOW_PDF_REFERENCE_INVALID",
            Self::DanglingReference => "FLOW_PDF_REFERENCE_DANGLING",
            Self::IndirectCycle => "FLOW_PDF_REFERENCE_CYCLE",
            Self::ReservedDictionaryKey => "FLOW_PDF_DICTIONARY_KEY_RESERVED",
            Self::DuplicateDictionaryKey => "FLOW_PDF_DICTIONARY_KEY_DUPLICATE",
            Self::InvalidPageGeometry => "FLOW_PDF_PAGE_GEOMETRY_INVALID",
            Self::EmptyPagePlan => "FLOW_PDF_PAGE_PLAN_EMPTY",
            Self::PageCountLimit => "FLOW_PDF_PAGE_COUNT_LIMIT",
            Self::NumericLimit => "FLOW_PDF_NUMERIC_LIMIT",
            Self::Metadata(error) => error.code(),
            Self::Provenance(error) => error.code(),
        }
    }
}

/// Writes a deterministic minimal owned PDF envelope.
pub fn export_pdf(request: &PdfExportRequest) -> Result<PdfExportResult, PdfError> {
    let export_fingerprint = request.fingerprint()?;
    let manifest = provenance::build_manifest(request, &export_fingerprint)?;
    let private_source_stream =
        provenance::encode_source_envelope(&manifest, request.source_payload.as_deref())?;
    let support_report = PdfSupportReport::current();
    let mut document = CosDocument::new();
    let pages_ref = document.add_object(CosValue::Null)?;
    let mut page_refs = Vec::with_capacity(request.pages.len());

    for page in &request.pages {
        let content_ref = document.add_object(CosValue::stream([], Vec::new())?)?;
        let page_ref = document.add_object(CosValue::Null)?;
        page_refs.push((page_ref, content_ref, page.bounds));
    }

    let mut page_annotations = vec![Vec::new(); request.pages.len()];
    for link in metadata::ordered_links(&request.options.internal_links) {
        let page_index = usize::try_from(link.page_index).map_err(|_| PdfError::NumericLimit)?;
        let destination_page_index =
            usize::try_from(link.destination_page_index).map_err(|_| PdfError::NumericLimit)?;
        let destination = page_refs[destination_page_index].0;
        let right = link
            .rect
            .x
            .raw()
            .checked_add(link.rect.width.raw())
            .ok_or(PdfError::NumericLimit)?;
        let bottom = link
            .rect
            .y
            .raw()
            .checked_add(link.rect.height.raw())
            .ok_or(PdfError::NumericLimit)?;
        let link_ref = document.add_object(CosValue::dictionary([
            (
                PdfName::new("Border")?,
                CosValue::Array(vec![
                    CosValue::Integer(0),
                    CosValue::Integer(0),
                    CosValue::Integer(0),
                ]),
            ),
            (
                PdfName::new("Dest")?,
                CosValue::Array(vec![
                    CosValue::Reference(destination),
                    CosValue::name("Fit")?,
                ]),
            ),
            (
                PdfName::new("Rect")?,
                CosValue::Array(vec![
                    CosValue::Real(link.rect.x),
                    CosValue::Real(link.rect.y),
                    CosValue::Real(LayoutUnit::from_raw(right)),
                    CosValue::Real(LayoutUnit::from_raw(bottom)),
                ]),
            ),
            (PdfName::new("Subtype")?, CosValue::name("Link")?),
            (PdfName::new("Type")?, CosValue::name("Annot")?),
        ])?)?;
        page_annotations[page_index].push(link_ref);
    }

    let outline_ref = if request.options.outlines.is_empty() {
        None
    } else {
        let outline_root = document.add_object(CosValue::Null)?;
        let ordered = metadata::ordered_outlines(&request.options.outlines);
        let mut item_refs = Vec::with_capacity(ordered.len());
        for _ in &ordered {
            item_refs.push(document.add_object(CosValue::Null)?);
        }
        for (index, (item_ref, outline)) in item_refs.iter().zip(ordered.iter()).enumerate() {
            let page_index =
                usize::try_from(outline.page_index).map_err(|_| PdfError::NumericLimit)?;
            let mut entries = vec![
                (
                    PdfName::new("Dest")?,
                    CosValue::Array(vec![
                        CosValue::Reference(page_refs[page_index].0),
                        CosValue::name("Fit")?,
                    ]),
                ),
                (PdfName::new("Parent")?, CosValue::Reference(outline_root)),
                (
                    PdfName::new("Title")?,
                    CosValue::string(metadata::encode_pdf_text(&outline.title))?,
                ),
            ];
            if let Some(next) = item_refs.get(index + 1) {
                entries.push((PdfName::new("Next")?, CosValue::Reference(*next)));
            }
            document.replace_object(*item_ref, CosValue::dictionary(entries)?)?;
        }
        document.replace_object(
            outline_root,
            CosValue::dictionary([
                (
                    PdfName::new("Count")?,
                    CosValue::Integer(
                        i64::try_from(item_refs.len()).map_err(|_| PdfError::NumericLimit)?,
                    ),
                ),
                (PdfName::new("First")?, CosValue::Reference(item_refs[0])),
                (
                    PdfName::new("Last")?,
                    CosValue::Reference(*item_refs.last().ok_or(PdfError::NumericLimit)?),
                ),
                (PdfName::new("Type")?, CosValue::name("Outlines")?),
            ])?,
        )?;
        Some(outline_root)
    };

    let kids = page_refs
        .iter()
        .map(|(page_ref, _, _)| CosValue::Reference(*page_ref))
        .collect::<Vec<_>>();
    document.replace_object(
        pages_ref,
        CosValue::dictionary([
            (
                PdfName::new("Count")?,
                CosValue::Integer(i64::try_from(kids.len()).map_err(|_| PdfError::NumericLimit)?),
            ),
            (PdfName::new("Kids")?, CosValue::Array(kids)),
            (PdfName::new("Type")?, CosValue::name("Pages")?),
        ])?,
    )?;

    for (index, (page_ref, content_ref, bounds)) in page_refs.iter().enumerate() {
        let mut entries = vec![
            (PdfName::new("Contents")?, CosValue::Reference(*content_ref)),
            (
                PdfName::new("MediaBox")?,
                CosValue::Array(vec![
                    CosValue::Real(LayoutUnit::from_raw(0)),
                    CosValue::Real(LayoutUnit::from_raw(0)),
                    CosValue::Real(bounds.width),
                    CosValue::Real(bounds.height),
                ]),
            ),
            (PdfName::new("Parent")?, CosValue::Reference(pages_ref)),
            (PdfName::new("Resources")?, CosValue::dictionary([])?),
            (PdfName::new("Type")?, CosValue::name("Page")?),
        ];
        if !page_annotations[index].is_empty() {
            entries.push((
                PdfName::new("Annots")?,
                CosValue::Array(
                    page_annotations[index]
                        .iter()
                        .copied()
                        .map(CosValue::Reference)
                        .collect(),
                ),
            ));
        }
        document.replace_object(*page_ref, CosValue::dictionary(entries)?)?;
    }

    let info_ref = if request.options.metadata.is_empty()
        && request.options.producer == PdfExportOptions::default().producer
    {
        None
    } else {
        let mut entries = vec![(
            PdfName::new("Producer")?,
            CosValue::string(metadata::encode_pdf_text(&request.options.producer))?,
        )];
        if let Some(title) = &request.options.metadata.title {
            entries.push((
                PdfName::new("Title")?,
                CosValue::string(metadata::encode_pdf_text(title))?,
            ));
        }
        if let Some(author) = &request.options.metadata.author {
            entries.push((
                PdfName::new("Author")?,
                CosValue::string(metadata::encode_pdf_text(author))?,
            ));
        }
        if let Some(subject) = &request.options.metadata.subject {
            entries.push((
                PdfName::new("Subject")?,
                CosValue::string(metadata::encode_pdf_text(subject))?,
            ));
        }
        Some(document.add_object(CosValue::dictionary(entries)?)?)
    };
    if let Some(info_ref) = info_ref {
        document.set_info(info_ref)?;
    }

    let private_source_ref = document.add_object(CosValue::stream(
        [
            (PdfName::new("Subtype")?, CosValue::name("FlowPDF.Source")?),
            (PdfName::new("Type")?, CosValue::name("Metadata")?),
        ],
        private_source_stream.clone(),
    )?)?;

    let mut catalog_entries = vec![
        (PdfName::new("Pages")?, CosValue::Reference(pages_ref)),
        (PdfName::new("Type")?, CosValue::name("Catalog")?),
        (
            PdfName::new("FlowPDFSource")?,
            CosValue::Reference(private_source_ref),
        ),
    ];
    if let Some(language) = &request.options.metadata.language {
        catalog_entries.push((
            PdfName::new("Lang")?,
            CosValue::string(language.as_bytes().to_vec())?,
        ));
    }
    if let Some(outline_ref) = outline_ref {
        catalog_entries.push((PdfName::new("Outlines")?, CosValue::Reference(outline_ref)));
    }
    let catalog_ref = document.add_object(CosValue::dictionary(catalog_entries)?)?;
    document.set_root(catalog_ref)?;
    let bytes = document.write(&export_fingerprint)?;
    let byte_hash = blake3::hash(&bytes).to_hex().to_string();
    Ok(PdfExportResult {
        schema_version: PDF_EXPORT_SCHEMA_VERSION,
        source_revision: request.source_revision,
        source_hash: request.source_hash.clone(),
        layout_settings_fingerprint: request.layout_settings_fingerprint.clone(),
        export_fingerprint,
        byte_hash,
        support_report,
        manifest,
        private_source_stream,
        bytes,
    })
}

fn validate_identity(value: &str) -> Result<(), PdfError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTITY_BYTES || !value.is_ascii() {
        return Err(PdfError::InvalidIdentity);
    }
    Ok(())
}

fn validate_value_references(
    value: &CosValue,
    depth: usize,
    allow_parent_cycle: bool,
    visit: &mut dyn FnMut(PdfRef, usize, bool) -> Result<(), PdfError>,
) -> Result<(), PdfError> {
    if depth > MAX_PDF_NESTING {
        return Err(PdfError::NestingLimit);
    }
    match value {
        CosValue::Array(values) => {
            for value in values {
                validate_value_references(value, depth + 1, allow_parent_cycle, visit)?;
            }
        }
        CosValue::Dictionary(entries) => {
            for (name, value) in entries {
                let allow_structural_cycle =
                    allow_parent_cycle || matches!(name.0.as_str(), "Parent" | "Kids" | "Dest");
                validate_value_references(value, depth + 1, allow_structural_cycle, visit)?;
            }
        }
        CosValue::Stream { dictionary, data } => {
            if data.len() > MAX_PDF_STREAM_BYTES {
                return Err(PdfError::StreamSizeLimit);
            }
            for value in dictionary.values() {
                validate_value_references(value, depth + 1, false, visit)?;
            }
        }
        CosValue::Reference(reference) => visit(*reference, depth + 1, allow_parent_cycle)?,
        CosValue::String(value) if value.len() > MAX_PDF_STRING_BYTES => {
            return Err(PdfError::StringSizeLimit);
        }
        CosValue::Null
        | CosValue::Boolean(_)
        | CosValue::Integer(_)
        | CosValue::Real(_)
        | CosValue::Name(_)
        | CosValue::String(_) => {}
    }
    Ok(())
}

fn serialize_value(value: &CosValue, output: &mut Vec<u8>, depth: usize) -> Result<(), PdfError> {
    if depth > MAX_PDF_NESTING {
        return Err(PdfError::NestingLimit);
    }
    match value {
        CosValue::Null => output.extend_from_slice(b"null"),
        CosValue::Boolean(true) => output.extend_from_slice(b"true"),
        CosValue::Boolean(false) => output.extend_from_slice(b"false"),
        CosValue::Integer(value) => append_ascii(output, &value.to_string())?,
        CosValue::Real(value) => append_ascii(output, &format_layout_unit(*value))?,
        CosValue::Name(value) => value.write_to(output),
        CosValue::String(value) => write_string(value, output),
        CosValue::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(b' ');
                }
                serialize_value(value, output, depth + 1)?;
            }
            output.push(b']');
        }
        CosValue::Dictionary(entries) => write_dictionary(entries, output, depth)?,
        CosValue::Stream { dictionary, data } => {
            let mut entries = dictionary.clone();
            entries.insert(
                PdfName::new("Length")?,
                CosValue::Integer(i64::try_from(data.len()).map_err(|_| PdfError::NumericLimit)?),
            );
            write_dictionary(&entries, output, depth)?;
            output.extend_from_slice(b"\nstream\n");
            output.extend_from_slice(data);
            output.extend_from_slice(b"\nendstream");
        }
        CosValue::Reference(reference) => append_ascii(
            output,
            &format!("{} {} R", reference.object_number, reference.generation),
        )?,
    }
    ensure_output_size(output.len())
}

fn write_dictionary(
    entries: &BTreeMap<PdfName, CosValue>,
    output: &mut Vec<u8>,
    depth: usize,
) -> Result<(), PdfError> {
    output.extend_from_slice(b"<<");
    for (name, value) in entries {
        output.push(b' ');
        name.write_to(output);
        output.push(b' ');
        serialize_value(value, output, depth + 1)?;
    }
    output.extend_from_slice(b" >>");
    Ok(())
}

fn write_string(value: &[u8], output: &mut Vec<u8>) {
    output.push(b'(');
    for byte in value {
        match byte {
            b'(' | b')' | b'\\' => {
                output.push(b'\\');
                output.push(*byte);
            }
            b'\n' => output.extend_from_slice(b"\\n"),
            b'\r' => output.extend_from_slice(b"\\r"),
            b'\t' => output.extend_from_slice(b"\\t"),
            0x00..=0x1f | 0x7f..=0xff => {
                output.extend_from_slice(format!("\\{:03o}", byte).as_bytes());
            }
            _ => output.push(*byte),
        }
    }
    output.push(b')');
}

fn format_layout_unit(value: LayoutUnit) -> String {
    let raw = value.raw();
    let sign = if raw < 0 { "-" } else { "" };
    let absolute = raw.unsigned_abs();
    let whole = absolute / 64;
    let remainder = absolute % 64;
    if remainder == 0 {
        return format!("{sign}{whole}");
    }
    let fraction = remainder * 15_625;
    let fraction = format!("{fraction:06}").trim_end_matches('0').to_owned();
    format!("{sign}{whole}.{fraction}")
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02X}"));
    }
    output
}

fn append_ascii(output: &mut Vec<u8>, value: &str) -> Result<(), PdfError> {
    if !value.is_ascii() {
        return Err(PdfError::InvalidIdentity);
    }
    output.extend_from_slice(value.as_bytes());
    ensure_output_size(output.len())
}

fn append_fingerprint_text(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(&u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    output.extend_from_slice(value.as_bytes());
}

fn ensure_output_size(size: usize) -> Result<(), PdfError> {
    if size > MAX_PDF_OUTPUT_BYTES {
        Err(PdfError::OutputSizeLimit)
    } else {
        Ok(())
    }
}

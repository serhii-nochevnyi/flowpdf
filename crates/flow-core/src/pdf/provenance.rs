//! Deterministic owned-export manifests and the private source envelope.
//!
//! The manifest records identities and options only.  A canonical FlowDocument
//! JSON payload is carried separately, behind a bounded namespaced envelope;
//! physical font and image bytes never enter either structure.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    canonical::{canonical_hash, decode_canonical},
    model::{DocumentId, FlowDocument},
};

use super::{
    PdfExportOptions, PdfExportRequest, PdfInternalLink, PdfMetadataOptions, PdfOutlineEntry,
    metadata,
};

pub const PDF_PROVENANCE_SCHEMA_VERSION: u32 = 1;
pub const MAX_PDF_MANIFEST_BYTES: usize = 256 * 1024;
pub const MAX_PDF_SOURCE_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PDF_SOURCE_STREAM_BYTES: usize = 8 * 1024 * 1024;
const SOURCE_MAGIC: &[u8] = b"FlowPDF\0Source\0v1\0";
const MAX_FONT_FACES: usize = 1_024;

/// Input identities that must be retained when a caller has a derived layout
/// result and explicit font/hyphenation providers available.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfReproducibilityInputs {
    pub layout_result_hash: Option<String>,
    pub font_catalog_identity: Option<String>,
    pub font_faces: Vec<PdfFontManifestIdentity>,
    pub hyphenation_identity: Option<String>,
}

/// Identity and content hash for one admitted font face.  Font bytes are
/// deliberately absent from this record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfFontManifestIdentity {
    pub face_id: String,
    pub content_hash: String,
}

/// Normalized options retained in the reproducibility manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfManifestOptions {
    pub producer: String,
    pub metadata: PdfMetadataOptions,
    pub outlines: Vec<PdfOutlineEntry>,
    pub internal_links: Vec<PdfInternalLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flattened_field_ids: Vec<String>,
}

/// Versioned identity record for one owned export.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfExportManifest {
    pub manifest_version: u32,
    pub export_schema_version: u32,
    pub source_document_id: Option<DocumentId>,
    pub source_revision: u32,
    pub source_schema_version: Option<u32>,
    pub source_hash: String,
    pub source_payload_hash: Option<String>,
    pub layout_settings_fingerprint: String,
    pub layout_result_hash: Option<String>,
    pub engine: String,
    pub engine_version: String,
    pub font_catalog_identity: Option<String>,
    pub font_faces: Vec<PdfFontManifestIdentity>,
    pub hyphenation_identity: Option<String>,
    pub options: PdfManifestOptions,
    pub export_fingerprint: String,
}

/// Stable failure taxonomy for manifest and private-source construction.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PdfProvenanceError {
    #[error("the PDF provenance identity is empty, oversized, or non-ASCII")]
    InvalidIdentity,
    #[error("the PDF provenance font-face list exceeds the limit")]
    FontFaceLimit,
    #[error("the PDF provenance font-face identity is duplicated")]
    DuplicateFontFace,
    #[error("the PDF source payload exceeds the bounded limit")]
    SourcePayloadLimit,
    #[error("the PDF provenance manifest exceeds the bounded limit")]
    ManifestLimit,
    #[error("the canonical source payload is invalid")]
    InvalidCanonicalPayload,
    #[error("the canonical source payload hash does not match the export source hash")]
    SourceHashMismatch,
    #[error("the canonical source payload revision does not match the export revision")]
    SourceRevisionMismatch,
    #[error("the private PDF source envelope is truncated")]
    EnvelopeTruncated,
    #[error("the private PDF source envelope length is invalid")]
    EnvelopeLength,
    #[error("the private PDF source envelope magic is invalid")]
    EnvelopeMagic,
    #[error("the private PDF source manifest is invalid")]
    InvalidManifest,
    #[error("the private PDF source manifest is not canonical JSON")]
    NonCanonicalManifest,
    #[error("the private PDF source payload hash does not match its manifest")]
    PayloadHashMismatch,
    #[error("the private PDF source envelope exceeds the bounded limit")]
    SourceStreamLimit,
}

impl PdfProvenanceError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidIdentity => "FLOW_PDF_PROVENANCE_IDENTITY_INVALID",
            Self::FontFaceLimit => "FLOW_PDF_PROVENANCE_FONT_LIMIT",
            Self::DuplicateFontFace => "FLOW_PDF_PROVENANCE_FONT_DUPLICATE",
            Self::SourcePayloadLimit => "FLOW_PDF_SOURCE_PAYLOAD_LIMIT",
            Self::ManifestLimit => "FLOW_PDF_MANIFEST_LIMIT",
            Self::InvalidCanonicalPayload => "FLOW_PDF_SOURCE_CANONICAL_INVALID",
            Self::SourceHashMismatch => "FLOW_PDF_SOURCE_HASH_MISMATCH",
            Self::SourceRevisionMismatch => "FLOW_PDF_SOURCE_REVISION_MISMATCH",
            Self::EnvelopeTruncated => "FLOW_PDF_SOURCE_ENVELOPE_TRUNCATED",
            Self::EnvelopeLength => "FLOW_PDF_SOURCE_ENVELOPE_LENGTH_INVALID",
            Self::EnvelopeMagic => "FLOW_PDF_SOURCE_ENVELOPE_MAGIC_INVALID",
            Self::InvalidManifest => "FLOW_PDF_SOURCE_MANIFEST_INVALID",
            Self::NonCanonicalManifest => "FLOW_PDF_SOURCE_MANIFEST_NON_CANONICAL",
            Self::PayloadHashMismatch => "FLOW_PDF_SOURCE_PAYLOAD_HASH_MISMATCH",
            Self::SourceStreamLimit => "FLOW_PDF_SOURCE_STREAM_LIMIT",
        }
    }
}

pub(crate) fn validate_reproducibility_inputs(
    inputs: &PdfReproducibilityInputs,
) -> Result<(), PdfProvenanceError> {
    if let Some(value) = &inputs.layout_result_hash {
        validate_identity(value)?;
    }
    if let Some(value) = &inputs.font_catalog_identity {
        validate_identity(value)?;
    }
    if let Some(value) = &inputs.hyphenation_identity {
        validate_identity(value)?;
    }
    if inputs.font_faces.len() > MAX_FONT_FACES {
        return Err(PdfProvenanceError::FontFaceLimit);
    }
    let mut seen = std::collections::BTreeSet::new();
    for face in &inputs.font_faces {
        validate_identity(&face.face_id)?;
        validate_identity(&face.content_hash)?;
        if !seen.insert(&face.face_id) {
            return Err(PdfProvenanceError::DuplicateFontFace);
        }
    }
    Ok(())
}

pub(crate) fn build_manifest(
    request: &PdfExportRequest,
    export_fingerprint: &str,
) -> Result<PdfExportManifest, PdfProvenanceError> {
    validate_identity(&request.source_hash)?;
    validate_identity(&request.layout_settings_fingerprint)?;
    validate_identity(export_fingerprint)?;
    validate_reproducibility_inputs(&request.reproducibility)?;
    let source = request
        .source_payload
        .as_deref()
        .map(decode_source_document)
        .transpose()?;
    let (source_document_id, source_schema_version, source_payload_hash) = match source {
        Some((document, bytes)) => {
            let payload_hash = canonical_hash(&bytes);
            if payload_hash != request.source_hash {
                return Err(PdfProvenanceError::SourceHashMismatch);
            }
            if document.revision != request.source_revision {
                return Err(PdfProvenanceError::SourceRevisionMismatch);
            }
            (
                Some(document.document_id.clone()),
                Some(document.schema_version),
                Some(payload_hash),
            )
        }
        None => (None, None, None),
    };
    let mut font_faces = request.reproducibility.font_faces.clone();
    font_faces.sort_by(|left, right| left.face_id.cmp(&right.face_id));
    let manifest = PdfExportManifest {
        manifest_version: PDF_PROVENANCE_SCHEMA_VERSION,
        export_schema_version: super::PDF_EXPORT_SCHEMA_VERSION,
        source_document_id,
        source_revision: request.source_revision,
        source_schema_version,
        source_hash: request.source_hash.clone(),
        source_payload_hash,
        layout_settings_fingerprint: request.layout_settings_fingerprint.clone(),
        layout_result_hash: request.reproducibility.layout_result_hash.clone(),
        engine: "flow-core".to_owned(),
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        font_catalog_identity: request.reproducibility.font_catalog_identity.clone(),
        font_faces,
        hyphenation_identity: request.reproducibility.hyphenation_identity.clone(),
        options: normalized_options(&request.options),
        export_fingerprint: export_fingerprint.to_owned(),
    };
    serialized_manifest(&manifest)?;
    Ok(manifest)
}

pub(crate) fn normalized_options(options: &PdfExportOptions) -> PdfManifestOptions {
    let mut flattened_field_ids = options.flattened_field_ids.clone();
    flattened_field_ids.sort();
    PdfManifestOptions {
        producer: options.producer.clone(),
        metadata: options.metadata.clone(),
        outlines: metadata::ordered_outlines(&options.outlines),
        internal_links: metadata::ordered_links(&options.internal_links),
        flattened_field_ids,
    }
}

pub(crate) fn append_inputs_fingerprint(output: &mut Vec<u8>, inputs: &PdfReproducibilityInputs) {
    output.extend_from_slice(b"pdf-reproducibility-v1");
    append_optional_text(output, inputs.layout_result_hash.as_deref());
    append_optional_text(output, inputs.font_catalog_identity.as_deref());
    append_optional_text(output, inputs.hyphenation_identity.as_deref());
    let mut faces = inputs.font_faces.clone();
    faces.sort_by(|left, right| left.face_id.cmp(&right.face_id));
    output.extend_from_slice(&u64::try_from(faces.len()).unwrap_or(u64::MAX).to_be_bytes());
    for face in faces {
        append_text(output, &face.face_id);
        append_text(output, &face.content_hash);
    }
}

pub(crate) fn encode_source_envelope(
    manifest: &PdfExportManifest,
    source_payload: Option<&[u8]>,
) -> Result<Vec<u8>, PdfProvenanceError> {
    let manifest_bytes = serialized_manifest(manifest)?;
    let source_payload = source_payload.unwrap_or_default();
    if source_payload.len() > MAX_PDF_SOURCE_PAYLOAD_BYTES {
        return Err(PdfProvenanceError::SourcePayloadLimit);
    }
    if manifest.source_payload_hash.is_some() != !source_payload.is_empty() {
        return Err(PdfProvenanceError::PayloadHashMismatch);
    }
    if let Some(expected_hash) = &manifest.source_payload_hash
        && canonical_hash(source_payload) != *expected_hash
    {
        return Err(PdfProvenanceError::PayloadHashMismatch);
    }
    let manifest_length =
        u32::try_from(manifest_bytes.len()).map_err(|_| PdfProvenanceError::ManifestLimit)?;
    let payload_length =
        u32::try_from(source_payload.len()).map_err(|_| PdfProvenanceError::SourcePayloadLimit)?;
    let total = SOURCE_MAGIC
        .len()
        .checked_add(8)
        .and_then(|length| length.checked_add(manifest_bytes.len()))
        .and_then(|length| length.checked_add(source_payload.len()))
        .ok_or(PdfProvenanceError::SourceStreamLimit)?;
    if total > MAX_PDF_SOURCE_STREAM_BYTES {
        return Err(PdfProvenanceError::SourceStreamLimit);
    }
    let mut output = Vec::with_capacity(total);
    output.extend_from_slice(SOURCE_MAGIC);
    output.extend_from_slice(&manifest_length.to_be_bytes());
    output.extend_from_slice(&payload_length.to_be_bytes());
    output.extend_from_slice(&manifest_bytes);
    output.extend_from_slice(source_payload);
    Ok(output)
}

pub(crate) fn decode_source_envelope(
    stream: &[u8],
) -> Result<(PdfExportManifest, Vec<u8>), PdfProvenanceError> {
    if stream.len() > MAX_PDF_SOURCE_STREAM_BYTES {
        return Err(PdfProvenanceError::SourceStreamLimit);
    }
    if stream.len() < SOURCE_MAGIC.len() {
        return Err(PdfProvenanceError::EnvelopeTruncated);
    }
    if !stream.starts_with(SOURCE_MAGIC) {
        return Err(PdfProvenanceError::EnvelopeMagic);
    }
    let lengths_start = SOURCE_MAGIC.len();
    let lengths_end = lengths_start
        .checked_add(8)
        .ok_or(PdfProvenanceError::EnvelopeLength)?;
    if stream.len() < lengths_end {
        return Err(PdfProvenanceError::EnvelopeTruncated);
    }
    let manifest_length = u32::from_be_bytes(
        stream[lengths_start..lengths_start + 4]
            .try_into()
            .map_err(|_| PdfProvenanceError::EnvelopeLength)?,
    ) as usize;
    let payload_length = u32::from_be_bytes(
        stream[lengths_start + 4..lengths_end]
            .try_into()
            .map_err(|_| PdfProvenanceError::EnvelopeLength)?,
    ) as usize;
    if manifest_length > MAX_PDF_MANIFEST_BYTES {
        return Err(PdfProvenanceError::ManifestLimit);
    }
    if payload_length > MAX_PDF_SOURCE_PAYLOAD_BYTES {
        return Err(PdfProvenanceError::SourcePayloadLimit);
    }
    let manifest_start = lengths_end;
    let payload_start = manifest_start
        .checked_add(manifest_length)
        .ok_or(PdfProvenanceError::EnvelopeLength)?;
    let end = payload_start
        .checked_add(payload_length)
        .ok_or(PdfProvenanceError::EnvelopeLength)?;
    if end > stream.len() {
        return Err(PdfProvenanceError::EnvelopeTruncated);
    }
    if end < stream.len() {
        return Err(PdfProvenanceError::EnvelopeLength);
    }
    let manifest_bytes = &stream[manifest_start..payload_start];
    let manifest: PdfExportManifest =
        serde_json::from_slice(manifest_bytes).map_err(|_| PdfProvenanceError::InvalidManifest)?;
    if serialized_manifest(&manifest)? != manifest_bytes {
        return Err(PdfProvenanceError::NonCanonicalManifest);
    }
    let payload = stream[payload_start..end].to_vec();
    if manifest.source_payload_hash.is_some() != !payload.is_empty() {
        return Err(PdfProvenanceError::PayloadHashMismatch);
    }
    if let Some(expected_hash) = &manifest.source_payload_hash
        && canonical_hash(&payload) != *expected_hash
    {
        return Err(PdfProvenanceError::PayloadHashMismatch);
    }
    validate_identity(&manifest.source_hash)?;
    validate_identity(&manifest.layout_settings_fingerprint)?;
    validate_identity(&manifest.export_fingerprint)?;
    Ok((manifest, payload))
}

pub(crate) fn serialized_manifest(
    manifest: &PdfExportManifest,
) -> Result<Vec<u8>, PdfProvenanceError> {
    let bytes = serde_json::to_vec(manifest).map_err(|_| PdfProvenanceError::InvalidManifest)?;
    if bytes.len() > MAX_PDF_MANIFEST_BYTES {
        return Err(PdfProvenanceError::ManifestLimit);
    }
    Ok(bytes)
}

fn decode_source_document(bytes: &[u8]) -> Result<(FlowDocument, Vec<u8>), PdfProvenanceError> {
    if bytes.len() > MAX_PDF_SOURCE_PAYLOAD_BYTES {
        return Err(PdfProvenanceError::SourcePayloadLimit);
    }
    let document =
        decode_canonical(bytes).map_err(|_| PdfProvenanceError::InvalidCanonicalPayload)?;
    Ok((document, bytes.to_vec()))
}

fn validate_identity(value: &str) -> Result<(), PdfProvenanceError> {
    if value.trim().is_empty() || value.len() > 128 || !value.is_ascii() {
        return Err(PdfProvenanceError::InvalidIdentity);
    }
    Ok(())
}

fn append_optional_text(output: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            output.push(1);
            append_text(output, value);
        }
        None => output.push(0),
    }
}

fn append_text(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(&u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    output.extend_from_slice(value.as_bytes());
}

//! Bounded exact recovery for the private owned-source envelope.
//!
//! This module deliberately does not parse ordinary PDF syntax.  A caller
//! must provide the explicitly extracted FlowPDF private stream; everything
//! else is classified as unavailable external reconstruction.

use thiserror::Error;

use crate::{
    canonical::{canonical_bytes, canonical_hash, decode_canonical},
    model::{DocumentId, FlowDocument},
};

use super::{PdfExportManifest, PdfProvenanceError, provenance::decode_source_envelope};

/// Optional identity expectations supplied by the caller requesting recovery.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PdfRecoveryExpectation {
    pub document_id: Option<DocumentId>,
    pub revision: Option<u32>,
    pub canonical_hash: Option<String>,
    pub export_fingerprint: Option<String>,
}

/// A source that passed every owned-payload and canonical identity check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfRecoveredSource {
    pub exact: bool,
    pub document: FlowDocument,
    pub canonical_json: String,
    pub canonical_hash: String,
    pub manifest: PdfExportManifest,
}

/// Stable recovery failure taxonomy.  No error variant returns a best-effort
/// document, so callers cannot mistake unavailable reconstruction for exact
/// owned recovery.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PdfRecoveryError {
    #[error("the owned PDF source payload is missing")]
    MissingPayload,
    #[error("the supplied PDF is external or does not expose the owned source stream")]
    UnsupportedExternalPdf,
    #[error("the private PDF source envelope is truncated")]
    EnvelopeTruncated,
    #[error("the private PDF source envelope length is invalid")]
    EnvelopeLength,
    #[error("the private PDF source manifest is invalid")]
    InvalidManifest,
    #[error("the private PDF source payload exceeds the recovery budget")]
    BudgetExceeded,
    #[error("the private PDF source payload hash does not match the manifest")]
    HashMismatch,
    #[error("the private PDF source payload is not valid canonical FlowDocument JSON")]
    InvalidCanonicalPayload,
    #[error("the recovered document ID does not match the manifest or expectation")]
    DocumentIdMismatch,
    #[error("the recovered revision does not match the manifest or expectation")]
    RevisionMismatch,
    #[error("the recovered manifest identity does not match the expectation")]
    ManifestMismatch,
}

impl PdfRecoveryError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MissingPayload => "FLOW_PDF_RECOVERY_PAYLOAD_MISSING",
            Self::UnsupportedExternalPdf => "FLOW_PDF_RECOVERY_EXTERNAL_UNSUPPORTED",
            Self::EnvelopeTruncated => "FLOW_PDF_RECOVERY_ENVELOPE_TRUNCATED",
            Self::EnvelopeLength => "FLOW_PDF_RECOVERY_ENVELOPE_LENGTH_INVALID",
            Self::InvalidManifest => "FLOW_PDF_RECOVERY_MANIFEST_INVALID",
            Self::BudgetExceeded => "FLOW_PDF_RECOVERY_BUDGET_EXCEEDED",
            Self::HashMismatch => "FLOW_PDF_RECOVERY_HASH_MISMATCH",
            Self::InvalidCanonicalPayload => "FLOW_PDF_RECOVERY_CANONICAL_INVALID",
            Self::DocumentIdMismatch => "FLOW_PDF_RECOVERY_DOCUMENT_ID_MISMATCH",
            Self::RevisionMismatch => "FLOW_PDF_RECOVERY_REVISION_MISMATCH",
            Self::ManifestMismatch => "FLOW_PDF_RECOVERY_MANIFEST_MISMATCH",
        }
    }
}

/// Recovers an exact owned source from a previously extracted private stream.
/// `None` represents an owned export with no embedded payload; arbitrary PDF
/// bytes are never treated as a source envelope.
pub fn recover_owned_source(
    private_source_stream: Option<&[u8]>,
    expectation: &PdfRecoveryExpectation,
) -> Result<PdfRecoveredSource, PdfRecoveryError> {
    let Some(private_source_stream) = private_source_stream else {
        return Err(PdfRecoveryError::MissingPayload);
    };
    if private_source_stream.is_empty() {
        return Err(PdfRecoveryError::MissingPayload);
    }
    if private_source_stream.starts_with(b"%PDF-") {
        return Err(PdfRecoveryError::UnsupportedExternalPdf);
    }
    let (manifest, payload) =
        decode_source_envelope(private_source_stream).map_err(map_provenance_error)?;
    if payload.is_empty() {
        return Err(PdfRecoveryError::MissingPayload);
    }
    let document =
        decode_canonical(&payload).map_err(|_| PdfRecoveryError::InvalidCanonicalPayload)?;
    let canonical =
        canonical_bytes(&document).map_err(|_| PdfRecoveryError::InvalidCanonicalPayload)?;
    let hash = canonical_hash(&canonical);
    if manifest.source_payload_hash.as_deref() != Some(hash.as_str())
        || manifest.source_hash != hash
    {
        return Err(PdfRecoveryError::HashMismatch);
    }
    if manifest.source_document_id.as_ref() != Some(&document.document_id) {
        return Err(PdfRecoveryError::DocumentIdMismatch);
    }
    if manifest.source_revision != document.revision {
        return Err(PdfRecoveryError::RevisionMismatch);
    }
    if manifest.source_schema_version != Some(document.schema_version) {
        return Err(PdfRecoveryError::ManifestMismatch);
    }
    if let Some(document_id) = &expectation.document_id
        && document_id != &document.document_id
    {
        return Err(PdfRecoveryError::DocumentIdMismatch);
    }
    if let Some(revision) = expectation.revision
        && revision != document.revision
    {
        return Err(PdfRecoveryError::RevisionMismatch);
    }
    if let Some(expected_hash) = &expectation.canonical_hash
        && expected_hash != &hash
    {
        return Err(PdfRecoveryError::HashMismatch);
    }
    if let Some(expected_fingerprint) = &expectation.export_fingerprint
        && expected_fingerprint != &manifest.export_fingerprint
    {
        return Err(PdfRecoveryError::ManifestMismatch);
    }
    let canonical_json =
        String::from_utf8(canonical).map_err(|_| PdfRecoveryError::InvalidCanonicalPayload)?;
    Ok(PdfRecoveredSource {
        exact: true,
        document,
        canonical_json,
        canonical_hash: hash,
        manifest,
    })
}

fn map_provenance_error(error: PdfProvenanceError) -> PdfRecoveryError {
    match error {
        PdfProvenanceError::EnvelopeTruncated => PdfRecoveryError::EnvelopeTruncated,
        PdfProvenanceError::EnvelopeLength => PdfRecoveryError::EnvelopeLength,
        PdfProvenanceError::EnvelopeMagic => PdfRecoveryError::UnsupportedExternalPdf,
        PdfProvenanceError::ManifestLimit
        | PdfProvenanceError::SourcePayloadLimit
        | PdfProvenanceError::SourceStreamLimit
        | PdfProvenanceError::FontFaceLimit => PdfRecoveryError::BudgetExceeded,
        PdfProvenanceError::PayloadHashMismatch | PdfProvenanceError::SourceHashMismatch => {
            PdfRecoveryError::HashMismatch
        }
        PdfProvenanceError::InvalidManifest
        | PdfProvenanceError::NonCanonicalManifest
        | PdfProvenanceError::InvalidIdentity
        | PdfProvenanceError::DuplicateFontFace => PdfRecoveryError::InvalidManifest,
        PdfProvenanceError::InvalidCanonicalPayload => PdfRecoveryError::InvalidCanonicalPayload,
        PdfProvenanceError::SourceRevisionMismatch => PdfRecoveryError::RevisionMismatch,
    }
}

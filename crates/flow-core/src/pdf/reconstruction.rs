//! Bounded, best-effort reconstruction of a verified PDF scene.
//!
//! The reconstruction lane is deliberately separate from the reader and the
//! canonical editor transaction path. It consumes a verified fixed-point scene,
//! emits a candidate FlowDocument with source mappings/confidence, and keeps
//! unsupported visual elements in a source-bound opaque sidecar.

use std::cmp::{max, min};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    canonical::{canonical_bytes, canonical_hash},
    layout::{LayoutRect, LayoutUnit},
    model::{
        BlockKind, ContentNode, DocumentId, FlowDocument, FontFamily, NodeId, PageMargins,
        PageOrientation, PageSettings, PageSize, Provenance, SectionSettings, StyleDefinition,
        StyleId,
    },
};

use super::{
    PdfObjectRef, PdfReadReport, PdfScene, PdfSceneElement, PdfTextElement, PdfTextMapping,
};

pub const PDF_RECONSTRUCTION_SCHEMA_VERSION: u32 = 1;
pub const PDF_EXTERNAL_SOURCE_HASH_PREFIX: &str = "pdf:blake3:v1:";
const MAX_RECONSTRUCTION_PAGES: usize = 2_048;
const MAX_RECONSTRUCTION_ELEMENTS: usize = 100_000;
const MAX_RECONSTRUCTION_BLOCKS: usize = 16_384;
const MAX_RECONSTRUCTION_TEXT_BYTES: usize = 512 * 1024;
const MAX_OCR_CANDIDATES: usize = 16_384;
const MAX_OCR_TEXT_BYTES: usize = 512 * 1024;
const REVIEW_CONFIDENCE_BASIS_POINTS: u16 = 7_500;
const LINE_GAP_UNITS: i64 = 64;
const BLOCK_GAP_MULTIPLIER: i64 = 2;

/// A bounded OCR adapter result. The adapter owns recognition; Rust owns
/// identity, geometry, confidence, and review policy after this boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfOcrCandidate {
    pub page_index: u32,
    pub rect: LayoutRect,
    pub text: String,
    pub confidence_basis_points: u16,
    pub provider: String,
}

/// Input to deterministic reconstruction. `source_hash` is the raw 64-digit
/// PDF byte hash returned by the Phase 7 reader; the candidate provenance
/// stores the explicit `pdf:blake3:v1:` envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfReconstructionRequest {
    pub source_hash: String,
    pub scene: PdfScene,
    pub locale: String,
    #[serde(default)]
    pub ocr_candidates: Vec<PdfOcrCandidate>,
}

/// The source location of one candidate fragment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfSourceMapping {
    pub page_index: u32,
    pub rect: LayoutRect,
    pub object_ref: Option<PdfObjectRef>,
    pub operator: Option<String>,
    pub origin: PdfMappingOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PdfMappingOrigin {
    PdfText { mapping: PdfTextMapping },
    Ocr { provider: String },
}

/// One semantic block in the candidate FlowDocument.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfReconstructedBlock {
    pub node_id: NodeId,
    pub text: String,
    pub rect: LayoutRect,
    pub confidence_basis_points: u16,
    pub review_required: bool,
    pub mappings: Vec<PdfSourceMapping>,
}

/// An unsupported visual element preserved by identity and geometry rather
/// than being guessed into editable semantic text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfOpaqueIsland {
    pub page_index: u32,
    pub element_index: u32,
    pub rect: LayoutRect,
    pub kind: PdfOpaqueIslandKind,
    pub object_ref: Option<PdfObjectRef>,
    pub operator: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PdfOpaqueIslandKind {
    Path,
    Clip,
    Image,
    Form,
    Link,
    Annotation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PdfReconstructionDiagnosticCode {
    ReadingOrderUncertain,
    MissingUnicodeMapping,
    UnsupportedVisual,
    OcrReviewRequired,
    OcrInputUnmatched,
    PageHasNoText,
    ScenePartial,
}

impl PdfReconstructionDiagnosticCode {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ReadingOrderUncertain => "FLOW_PDF_RECONSTRUCTION_READING_ORDER_UNCERTAIN",
            Self::MissingUnicodeMapping => "FLOW_PDF_RECONSTRUCTION_UNICODE_MAPPING_MISSING",
            Self::UnsupportedVisual => "FLOW_PDF_RECONSTRUCTION_OPAQUE_ISLAND",
            Self::OcrReviewRequired => "FLOW_PDF_RECONSTRUCTION_OCR_REVIEW_REQUIRED",
            Self::OcrInputUnmatched => "FLOW_PDF_RECONSTRUCTION_OCR_INPUT_UNMATCHED",
            Self::PageHasNoText => "FLOW_PDF_RECONSTRUCTION_PAGE_NO_TEXT",
            Self::ScenePartial => "FLOW_PDF_RECONSTRUCTION_SCENE_PARTIAL",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfReconstructionDiagnostic {
    pub code: PdfReconstructionDiagnosticCode,
    pub page_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfReconstructionReport {
    pub reader_report: PdfReadReport,
    pub diagnostics: Vec<PdfReconstructionDiagnostic>,
    pub review_required_count: u32,
    pub partial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfDocumentCandidate {
    pub document: FlowDocument,
    pub canonical_json: String,
    pub canonical_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfReconstructionResult {
    pub schema_version: u32,
    pub source_hash: String,
    pub scene_result_hash: String,
    pub candidate: PdfDocumentCandidate,
    pub blocks: Vec<PdfReconstructedBlock>,
    pub opaque_islands: Vec<PdfOpaqueIsland>,
    pub report: PdfReconstructionReport,
    pub result_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfReviewDecision {
    pub node_id: NodeId,
    pub action: PdfReviewAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PdfReviewAction {
    Keep,
    Replace { text: String },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PdfReconstructionError {
    #[error("The reconstruction source identity is invalid")]
    SourceIdentity,
    #[error("The reconstruction scene identity is invalid")]
    SceneIdentity,
    #[error("The reconstruction request exceeds a bounded limit")]
    Limit,
    #[error("The reconstruction locale is not supported")]
    Locale,
    #[error("The reconstruction candidate is not schema-valid")]
    Candidate,
    #[error("A required reconstruction review decision is missing or duplicated")]
    ReviewRequired,
    #[error("The reconstruction review decision is invalid")]
    ReviewInvalid,
    #[error("The reconstruction geometry overflowed")]
    NumericLimit,
}

impl PdfReconstructionError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SourceIdentity => "FLOW_PDF_RECONSTRUCTION_SOURCE_INVALID",
            Self::SceneIdentity => "FLOW_PDF_RECONSTRUCTION_SCENE_INVALID",
            Self::Limit => "FLOW_PDF_RECONSTRUCTION_LIMIT",
            Self::Locale => "FLOW_PDF_RECONSTRUCTION_LOCALE_UNSUPPORTED",
            Self::Candidate => "FLOW_PDF_RECONSTRUCTION_CANDIDATE_INVALID",
            Self::ReviewRequired => "FLOW_PDF_RECONSTRUCTION_REVIEW_REQUIRED",
            Self::ReviewInvalid => "FLOW_PDF_RECONSTRUCTION_REVIEW_INVALID",
            Self::NumericLimit => "FLOW_PDF_RECONSTRUCTION_NUMERIC_LIMIT",
        }
    }
}

#[derive(Debug, Clone)]
struct TextPart {
    rect: LayoutRect,
    text: String,
    confidence_basis_points: u16,
    review_required: bool,
    mapping: PdfSourceMapping,
}

#[derive(Debug, Clone)]
struct TextLine {
    parts: Vec<TextPart>,
    rect: LayoutRect,
    confidence_basis_points: u16,
    review_required: bool,
}

/// Reconstructs one verified reader scene into a bounded semantic candidate.
pub fn reconstruct_pdf_scene(
    request: &PdfReconstructionRequest,
) -> Result<PdfReconstructionResult, PdfReconstructionError> {
    validate_request(request)?;
    verify_scene_hash(&request.scene)?;

    let mut blocks = Vec::new();
    let mut opaque_islands = Vec::new();
    let mut diagnostics = Vec::new();
    let mut review_required_count = 0_u32;
    let mut block_index = 0_usize;
    let mut total_text_bytes = 0_usize;

    for page in &request.scene.pages {
        let mut parts = Vec::new();
        let mut ocr_for_page = Vec::new();

        for candidate in request
            .ocr_candidates
            .iter()
            .filter(|candidate| candidate.page_index == page.page_index)
        {
            ocr_for_page.push(candidate);
        }

        for (element_index, element) in page.elements.iter().enumerate() {
            match element {
                PdfSceneElement::Text { value } => {
                    let part = text_part_from_pdf(page.page_index, value, total_text_bytes)?;
                    total_text_bytes = total_text_bytes
                        .checked_add(part.text.len())
                        .ok_or(PdfReconstructionError::Limit)?;
                    parts.push(part);
                }
                other => {
                    opaque_islands.push(opaque_island_from_scene(
                        page.page_index,
                        u32::try_from(element_index).map_err(|_| PdfReconstructionError::Limit)?,
                        other,
                    )?);
                    diagnostics.push(PdfReconstructionDiagnostic {
                        code: PdfReconstructionDiagnosticCode::UnsupportedVisual,
                        page_index: Some(page.page_index),
                    });
                }
            }
        }

        if parts.is_empty() && !ocr_for_page.is_empty() {
            for candidate in ocr_for_page {
                total_text_bytes = total_text_bytes
                    .checked_add(candidate.text.len())
                    .ok_or(PdfReconstructionError::Limit)?;
                parts.push(text_part_from_ocr(candidate)?);
            }
        } else if !parts.is_empty() && !ocr_for_page.is_empty() {
            diagnostics.push(PdfReconstructionDiagnostic {
                code: PdfReconstructionDiagnosticCode::OcrInputUnmatched,
                page_index: Some(page.page_index),
            });
        }

        if parts.is_empty() {
            diagnostics.push(PdfReconstructionDiagnostic {
                code: PdfReconstructionDiagnosticCode::PageHasNoText,
                page_index: Some(page.page_index),
            });
            continue;
        }

        parts.sort_by(|left, right| {
            right
                .rect
                .y
                .raw()
                .cmp(&left.rect.y.raw())
                .then_with(|| left.rect.x.raw().cmp(&right.rect.x.raw()))
                .then_with(|| left.mapping.object_ref.cmp(&right.mapping.object_ref))
                .then_with(|| left.mapping.operator.cmp(&right.mapping.operator))
        });

        let ambiguous_columns = has_ambiguous_columns(&parts, page.bounds.width.raw());
        if ambiguous_columns {
            diagnostics.push(PdfReconstructionDiagnostic {
                code: PdfReconstructionDiagnosticCode::ReadingOrderUncertain,
                page_index: Some(page.page_index),
            });
            for part in &mut parts {
                part.confidence_basis_points = min(part.confidence_basis_points, 5_000);
                part.review_required = true;
            }
        }

        let lines = make_lines(parts)?;
        let page_blocks = make_blocks(lines)?;
        for block in page_blocks {
            if blocks.len() >= MAX_RECONSTRUCTION_BLOCKS {
                return Err(PdfReconstructionError::Limit);
            }
            let node_id = NodeId::new(stable_uuid(&request.source_hash, "node", block_index))
                .map_err(|_| PdfReconstructionError::Candidate)?;
            block_index = block_index
                .checked_add(1)
                .ok_or(PdfReconstructionError::Limit)?;
            let confidence_basis_points =
                block
                    .confidence_basis_points
                    .min(if ambiguous_columns { 5_000 } else { 10_000 });
            let review_required =
                block.review_required || confidence_basis_points < REVIEW_CONFIDENCE_BASIS_POINTS;
            if review_required {
                review_required_count = review_required_count
                    .checked_add(1)
                    .ok_or(PdfReconstructionError::Limit)?;
            }
            let text = block
                .parts
                .iter()
                .map(|part| part.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            blocks.push(PdfReconstructedBlock {
                node_id,
                text,
                rect: block.rect,
                confidence_basis_points,
                review_required,
                mappings: block.parts.into_iter().map(|part| part.mapping).collect(),
            });
        }
    }

    if request.scene.pages.iter().any(|page| page.partial) {
        diagnostics.push(PdfReconstructionDiagnostic {
            code: PdfReconstructionDiagnosticCode::ScenePartial,
            page_index: None,
        });
    }
    if blocks.iter().any(|block| {
        block
            .mappings
            .iter()
            .any(|mapping| matches!(mapping.origin, PdfMappingOrigin::Ocr { .. }))
            && block.review_required
    }) {
        diagnostics.push(PdfReconstructionDiagnostic {
            code: PdfReconstructionDiagnosticCode::OcrReviewRequired,
            page_index: None,
        });
    }
    if blocks.iter().any(|block| {
        block.mappings.iter().any(|mapping| {
            matches!(
                mapping.origin,
                PdfMappingOrigin::PdfText {
                    mapping: PdfTextMapping::Missing
                }
            )
        })
    }) {
        diagnostics.push(PdfReconstructionDiagnostic {
            code: PdfReconstructionDiagnosticCode::MissingUnicodeMapping,
            page_index: None,
        });
    }

    let document = build_candidate_document(request, &blocks)?;
    let canonical = canonical_bytes(&document).map_err(|_| PdfReconstructionError::Candidate)?;
    let canonical_json =
        String::from_utf8(canonical.clone()).map_err(|_| PdfReconstructionError::Candidate)?;
    let candidate = PdfDocumentCandidate {
        document,
        canonical_json,
        canonical_hash: canonical_hash(&canonical),
    };
    let mut result = PdfReconstructionResult {
        schema_version: PDF_RECONSTRUCTION_SCHEMA_VERSION,
        source_hash: request.source_hash.clone(),
        scene_result_hash: request.scene.result_hash.clone(),
        candidate,
        blocks,
        opaque_islands,
        report: PdfReconstructionReport {
            reader_report: request.scene.report.clone(),
            diagnostics,
            review_required_count,
            partial: request.scene.pages.iter().any(|page| page.partial),
        },
        result_hash: String::new(),
    };
    result.result_hash = result_hash(&result)?;
    Ok(result)
}

/// Accepts a candidate only after every low-confidence block has an explicit
/// decision. The returned candidate remains externally reconstructed and keeps
/// its source-bound provenance.
pub fn accept_pdf_reconstruction(
    result: &PdfReconstructionResult,
    decisions: &[PdfReviewDecision],
) -> Result<PdfDocumentCandidate, PdfReconstructionError> {
    let required = result
        .blocks
        .iter()
        .filter(|block| block.review_required)
        .map(|block| block.node_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut seen = std::collections::BTreeSet::new();
    for decision in decisions {
        if !required.contains(decision.node_id.as_str()) || !seen.insert(decision.node_id.as_str())
        {
            return Err(PdfReconstructionError::ReviewInvalid);
        }
        if let PdfReviewAction::Replace { text } = &decision.action
            && (text.len() > MAX_RECONSTRUCTION_TEXT_BYTES || text.contains('\0'))
        {
            return Err(PdfReconstructionError::Limit);
        }
    }
    if seen.len() != required.len() {
        return Err(PdfReconstructionError::ReviewRequired);
    }

    let mut document = result.candidate.document.clone();
    for decision in decisions {
        let Some(node) = document
            .content
            .iter_mut()
            .find(|node| node.id == decision.node_id)
        else {
            return Err(PdfReconstructionError::ReviewInvalid);
        };
        match &decision.action {
            PdfReviewAction::Keep => {}
            PdfReviewAction::Replace { text } => node
                .set_plain_text(text.clone())
                .map_err(|_| PdfReconstructionError::ReviewInvalid)?,
        }
    }
    let canonical = canonical_bytes(&document).map_err(|_| PdfReconstructionError::Candidate)?;
    Ok(PdfDocumentCandidate {
        document,
        canonical_json: String::from_utf8(canonical.clone())
            .map_err(|_| PdfReconstructionError::Candidate)?,
        canonical_hash: canonical_hash(&canonical),
    })
}

/// Verifies the closed reconstruction result before it crosses a worker or
/// browser boundary. The scene itself is not repeated in the result, so the
/// caller must also compare `source_hash` and `scene_result_hash` with the
/// request it scheduled.
#[must_use]
pub fn verify_pdf_reconstruction_result(result: &PdfReconstructionResult) -> bool {
    if result.schema_version != PDF_RECONSTRUCTION_SCHEMA_VERSION
        || !valid_raw_source_hash(&result.source_hash)
        || result.scene_result_hash.trim().is_empty()
        || result.result_hash.trim().is_empty()
        || result.blocks.len() > MAX_RECONSTRUCTION_BLOCKS
        || result.opaque_islands.len() > MAX_RECONSTRUCTION_ELEMENTS
    {
        return false;
    }

    let expected_provenance = Provenance::ExternalReconstruction {
        source_hash: normalized_source_hash(&result.source_hash),
        reconstruction_schema_version: PDF_RECONSTRUCTION_SCHEMA_VERSION,
    };
    if result.candidate.document.provenance != expected_provenance {
        return false;
    }

    let Ok(canonical) = canonical_bytes(&result.candidate.document) else {
        return false;
    };
    if result.candidate.canonical_json.as_bytes() != canonical.as_slice()
        || result.candidate.canonical_hash != canonical_hash(&canonical)
    {
        return false;
    }

    let mut node_ids = std::collections::BTreeSet::new();
    for block in &result.blocks {
        if !node_ids.insert(block.node_id.as_str())
            || block.text.len() > MAX_RECONSTRUCTION_TEXT_BYTES
            || block.confidence_basis_points > 10_000
            || block.mappings.len() > MAX_RECONSTRUCTION_ELEMENTS
        {
            return false;
        }
        if !result
            .candidate
            .document
            .content
            .iter()
            .any(|node| node.id == block.node_id && node.text() == block.text)
        {
            return false;
        }
    }
    if usize::try_from(result.report.review_required_count).ok()
        != Some(
            result
                .blocks
                .iter()
                .filter(|block| block.review_required)
                .count(),
        )
    {
        return false;
    }

    let mut unhashed = result.clone();
    unhashed.result_hash.clear();
    let Ok(bytes) = serde_json::to_vec(&unhashed) else {
        return false;
    };
    blake3::hash(&bytes).to_hex().as_str() == result.result_hash
}

/// Verifies a candidate returned after explicit review decisions.
#[must_use]
pub fn verify_pdf_document_candidate(candidate: &PdfDocumentCandidate) -> bool {
    let Provenance::ExternalReconstruction {
        source_hash,
        reconstruction_schema_version,
    } = &candidate.document.provenance
    else {
        return false;
    };
    let Some(raw_source_hash) = source_hash.strip_prefix(PDF_EXTERNAL_SOURCE_HASH_PREFIX) else {
        return false;
    };
    if *reconstruction_schema_version != PDF_RECONSTRUCTION_SCHEMA_VERSION
        || !valid_raw_source_hash(raw_source_hash)
    {
        return false;
    }
    let Ok(canonical) = canonical_bytes(&candidate.document) else {
        return false;
    };
    candidate.canonical_json.as_bytes() == canonical.as_slice()
        && candidate.canonical_hash == canonical_hash(&canonical)
}

fn validate_request(request: &PdfReconstructionRequest) -> Result<(), PdfReconstructionError> {
    if !valid_raw_source_hash(&request.source_hash)
        || request.source_hash != request.scene.source_hash
    {
        return Err(PdfReconstructionError::SourceIdentity);
    }
    if request.scene.pages.len() > MAX_RECONSTRUCTION_PAGES
        || request
            .scene
            .pages
            .iter()
            .map(|page| page.elements.len())
            .sum::<usize>()
            > MAX_RECONSTRUCTION_ELEMENTS
        || request.ocr_candidates.len() > MAX_OCR_CANDIDATES
    {
        return Err(PdfReconstructionError::Limit);
    }
    if request.locale != "uk-UA" && request.locale != "en-US" {
        return Err(PdfReconstructionError::Locale);
    }
    let mut ocr_bytes = 0_usize;
    for candidate in &request.ocr_candidates {
        if candidate.page_index >= request.scene.page_count
            || candidate.text.len() > MAX_RECONSTRUCTION_TEXT_BYTES
            || candidate.provider.is_empty()
            || candidate.provider.len() > 128
            || candidate.confidence_basis_points > 10_000
        {
            return Err(PdfReconstructionError::Limit);
        }
        ocr_bytes = ocr_bytes
            .checked_add(candidate.text.len())
            .ok_or(PdfReconstructionError::Limit)?;
        if ocr_bytes > MAX_OCR_TEXT_BYTES {
            return Err(PdfReconstructionError::Limit);
        }
    }
    Ok(())
}

fn verify_scene_hash(scene: &PdfScene) -> Result<(), PdfReconstructionError> {
    if scene.result_hash.is_empty() {
        return Err(PdfReconstructionError::SceneIdentity);
    }
    let mut unhashed = scene.clone();
    unhashed.result_hash.clear();
    let bytes = serde_json::to_vec(&unhashed).map_err(|_| PdfReconstructionError::SceneIdentity)?;
    if blake3::hash(&bytes).to_hex().as_str() != scene.result_hash {
        return Err(PdfReconstructionError::SceneIdentity);
    }
    Ok(())
}

fn text_part_from_pdf(
    page_index: u32,
    value: &PdfTextElement,
    current_text_bytes: usize,
) -> Result<TextPart, PdfReconstructionError> {
    if current_text_bytes
        .checked_add(value.text.len())
        .is_none_or(|bytes| bytes > MAX_RECONSTRUCTION_TEXT_BYTES)
    {
        return Err(PdfReconstructionError::Limit);
    }
    let (confidence_basis_points, review_required) = match value.mapping {
        PdfTextMapping::ToUnicode => (10_000, false),
        PdfTextMapping::SimpleEncoding => (8_500, false),
        PdfTextMapping::Missing => (3_000, true),
    };
    Ok(TextPart {
        rect: value.rect,
        text: value.text.clone(),
        confidence_basis_points,
        review_required,
        mapping: PdfSourceMapping {
            page_index,
            rect: value.rect,
            object_ref: Some(value.provenance.object_ref),
            operator: value.provenance.operator.clone(),
            origin: PdfMappingOrigin::PdfText {
                mapping: value.mapping,
            },
        },
    })
}

fn text_part_from_ocr(candidate: &PdfOcrCandidate) -> Result<TextPart, PdfReconstructionError> {
    if candidate.text.is_empty() || candidate.text.len() > MAX_RECONSTRUCTION_TEXT_BYTES {
        return Err(PdfReconstructionError::Limit);
    }
    Ok(TextPart {
        rect: candidate.rect,
        text: candidate.text.clone(),
        confidence_basis_points: candidate.confidence_basis_points,
        review_required: candidate.confidence_basis_points < REVIEW_CONFIDENCE_BASIS_POINTS,
        mapping: PdfSourceMapping {
            page_index: candidate.page_index,
            rect: candidate.rect,
            object_ref: None,
            operator: None,
            origin: PdfMappingOrigin::Ocr {
                provider: candidate.provider.clone(),
            },
        },
    })
}

fn make_lines(mut parts: Vec<TextPart>) -> Result<Vec<TextLine>, PdfReconstructionError> {
    let mut lines = Vec::new();
    for part in parts.drain(..) {
        let should_append = lines.last().is_some_and(|line: &TextLine| {
            vertical_overlap(line.rect, part.rect) > 0
                || vertical_distance(line.rect, part.rect) <= LINE_GAP_UNITS
        });
        if should_append {
            let line = lines.last_mut().ok_or(PdfReconstructionError::Candidate)?;
            line.rect = union_rect(line.rect, part.rect)?;
            line.confidence_basis_points =
                min(line.confidence_basis_points, part.confidence_basis_points);
            line.review_required |= part.review_required;
            line.parts.push(part);
        } else {
            lines.push(TextLine {
                rect: part.rect,
                confidence_basis_points: part.confidence_basis_points,
                review_required: part.review_required,
                parts: vec![part],
            });
        }
    }
    for line in &mut lines {
        line.parts.sort_by(|left, right| {
            left.rect
                .x
                .raw()
                .cmp(&right.rect.x.raw())
                .then_with(|| left.mapping.object_ref.cmp(&right.mapping.object_ref))
        });
    }
    Ok(lines)
}

fn make_blocks(lines: Vec<TextLine>) -> Result<Vec<TextLine>, PdfReconstructionError> {
    let mut blocks: Vec<TextLine> = Vec::new();
    for line in lines {
        let should_append = blocks.last().is_some_and(|block| {
            let gap = block.rect.y.raw() - (line.rect.y.raw() + line.rect.height.raw());
            gap <= max(block.rect.height.raw(), line.rect.height.raw()) * BLOCK_GAP_MULTIPLIER
                && (block.rect.x.raw() - line.rect.x.raw()).abs()
                    <= max(block.rect.height.raw(), line.rect.height.raw()) * 4
        });
        if should_append {
            let block = blocks.last_mut().ok_or(PdfReconstructionError::Candidate)?;
            block.rect = union_rect(block.rect, line.rect)?;
            block.confidence_basis_points =
                min(block.confidence_basis_points, line.confidence_basis_points);
            block.review_required |= line.review_required;
            block.parts.extend(line.parts);
        } else {
            blocks.push(line);
        }
    }
    Ok(blocks)
}

fn has_ambiguous_columns(parts: &[TextPart], page_width: i64) -> bool {
    if parts.len() < 4 || page_width <= 0 {
        return false;
    }
    let threshold = page_width / 4;
    parts.iter().enumerate().any(|(index, left)| {
        parts.iter().skip(index + 1).any(|right| {
            let overlap = vertical_overlap(left.rect, right.rect);
            let left_end = left.rect.x.raw().saturating_add(left.rect.width.raw());
            let right_end = right.rect.x.raw().saturating_add(right.rect.width.raw());
            let gap = if right.rect.x.raw() >= left_end {
                right.rect.x.raw() - left_end
            } else if left.rect.x.raw() >= right_end {
                left.rect.x.raw() - right_end
            } else {
                0
            };
            overlap > 0 && gap > threshold
        })
    })
}

fn build_candidate_document(
    request: &PdfReconstructionRequest,
    blocks: &[PdfReconstructedBlock],
) -> Result<FlowDocument, PdfReconstructionError> {
    let body_style_id = StyleId::new(stable_uuid(&request.source_hash, "style-body", 0))
        .map_err(|_| PdfReconstructionError::Candidate)?;
    let document_id = DocumentId::new(stable_uuid(&request.source_hash, "document", 0))
        .map_err(|_| PdfReconstructionError::Candidate)?;
    let page_settings = PageSettings {
        page_size: PageSize::A4,
        orientation: PageOrientation::Portrait,
        margins_millimetres: PageMargins {
            top: 20,
            right: 20,
            bottom: 20,
            left: 20,
        },
    };
    let section = SectionSettings::default_for(&request.locale, page_settings.clone())
        .map_err(|_| PdfReconstructionError::Candidate)?;
    let content = blocks
        .iter()
        .map(|block| ContentNode {
            id: block.node_id.clone(),
            style_id: Some(body_style_id.clone()),
            body: BlockKind::Paragraph {
                attrs: Default::default(),
                runs: if block.text.is_empty() {
                    Vec::new()
                } else {
                    vec![crate::model::InlineRun {
                        text: block.text.clone(),
                        marks: Default::default(),
                    }]
                },
            },
        })
        .collect();
    Ok(FlowDocument {
        schema_version: crate::model::SCHEMA_VERSION,
        document_id,
        revision: 1,
        locale: request.locale.clone(),
        page_settings,
        sections: vec![section],
        styles: vec![StyleDefinition {
            id: body_style_id,
            name: "Imported body".to_owned(),
            font_family: FontFamily::noto_sans(),
            font_size_millipoints: 12_000,
        }],
        content,
        assets: Vec::new(),
        fields: Vec::new(),
        provenance: Provenance::ExternalReconstruction {
            source_hash: normalized_source_hash(&request.source_hash),
            reconstruction_schema_version: PDF_RECONSTRUCTION_SCHEMA_VERSION,
        },
    })
}

fn opaque_island_from_scene(
    page_index: u32,
    element_index: u32,
    element: &PdfSceneElement,
) -> Result<PdfOpaqueIsland, PdfReconstructionError> {
    let (rect, object_ref, operator, kind) = match element {
        PdfSceneElement::Text { .. } => return Err(PdfReconstructionError::Candidate),
        PdfSceneElement::Path { value } => (
            value.rect,
            Some(value.provenance.object_ref),
            value.provenance.operator.clone(),
            PdfOpaqueIslandKind::Path,
        ),
        PdfSceneElement::Clip { value } => (
            value.rect,
            Some(value.provenance.object_ref),
            value.provenance.operator.clone(),
            PdfOpaqueIslandKind::Clip,
        ),
        PdfSceneElement::Image { value } => (
            value.rect,
            Some(value.provenance.object_ref),
            value.provenance.operator.clone(),
            PdfOpaqueIslandKind::Image,
        ),
        PdfSceneElement::Form { value } => (
            value.rect,
            Some(value.provenance.object_ref),
            value.provenance.operator.clone(),
            PdfOpaqueIslandKind::Form,
        ),
        PdfSceneElement::Link { value } => (
            value.rect,
            Some(value.provenance.object_ref),
            value.provenance.operator.clone(),
            PdfOpaqueIslandKind::Link,
        ),
        PdfSceneElement::Annotation { value } => (
            value.rect,
            Some(value.provenance.object_ref),
            value.provenance.operator.clone(),
            PdfOpaqueIslandKind::Annotation,
        ),
    };
    Ok(PdfOpaqueIsland {
        page_index,
        element_index,
        rect,
        kind,
        object_ref,
        operator,
    })
}

fn union_rect(left: LayoutRect, right: LayoutRect) -> Result<LayoutRect, PdfReconstructionError> {
    let left_max_x = left
        .x
        .raw()
        .checked_add(left.width.raw())
        .ok_or(PdfReconstructionError::NumericLimit)?;
    let right_max_x = right
        .x
        .raw()
        .checked_add(right.width.raw())
        .ok_or(PdfReconstructionError::NumericLimit)?;
    let left_max_y = left
        .y
        .raw()
        .checked_add(left.height.raw())
        .ok_or(PdfReconstructionError::NumericLimit)?;
    let right_max_y = right
        .y
        .raw()
        .checked_add(right.height.raw())
        .ok_or(PdfReconstructionError::NumericLimit)?;
    let x = min(left.x.raw(), right.x.raw());
    let y = min(left.y.raw(), right.y.raw());
    let max_x = max(left_max_x, right_max_x);
    let max_y = max(left_max_y, right_max_y);
    Ok(LayoutRect {
        x: LayoutUnit::from_raw(x),
        y: LayoutUnit::from_raw(y),
        width: LayoutUnit::from_raw(
            max_x
                .checked_sub(x)
                .ok_or(PdfReconstructionError::NumericLimit)?,
        ),
        height: LayoutUnit::from_raw(
            max_y
                .checked_sub(y)
                .ok_or(PdfReconstructionError::NumericLimit)?,
        ),
    })
}

fn vertical_overlap(left: LayoutRect, right: LayoutRect) -> i64 {
    let left_max = left.y.raw().saturating_add(left.height.raw());
    let right_max = right.y.raw().saturating_add(right.height.raw());
    max(
        0,
        min(left_max, right_max) - max(left.y.raw(), right.y.raw()),
    )
}

fn vertical_distance(left: LayoutRect, right: LayoutRect) -> i64 {
    if vertical_overlap(left, right) > 0 {
        0
    } else if left.y.raw() > right.y.raw() {
        left.y.raw() - right.y.raw().saturating_add(right.height.raw())
    } else {
        right.y.raw() - left.y.raw().saturating_add(left.height.raw())
    }
}

fn result_hash(result: &PdfReconstructionResult) -> Result<String, PdfReconstructionError> {
    let mut unhashed = result.clone();
    unhashed.result_hash.clear();
    let bytes = serde_json::to_vec(&unhashed).map_err(|_| PdfReconstructionError::Candidate)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn valid_raw_source_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn normalized_source_hash(value: &str) -> String {
    format!("{PDF_EXTERNAL_SOURCE_HASH_PREFIX}{value}")
}

fn stable_uuid(source_hash: &str, kind: &str, index: usize) -> String {
    let mut input = Vec::with_capacity(source_hash.len() + kind.len() + 24);
    input.extend_from_slice(source_hash.as_bytes());
    input.push(0);
    input.extend_from_slice(kind.as_bytes());
    input.push(0);
    input.extend_from_slice(&index.to_le_bytes());
    let digest = blake3::hash(&input);
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).hyphenated().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_uuid_is_a_valid_deterministic_v4_identity() {
        let first = stable_uuid("a".repeat(64).as_str(), "node", 1);
        assert_eq!(first, stable_uuid("a".repeat(64).as_str(), "node", 1));
        assert_ne!(first, stable_uuid("a".repeat(64).as_str(), "node", 2));
        assert!(uuid::Uuid::parse_str(&first).is_ok());
    }
}

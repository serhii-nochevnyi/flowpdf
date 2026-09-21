//! Revision-bound display-list projection shared by PDF export and preview.
//!
//! The projection resolves canonical source text and Phase 3 fragments into
//! fixed-point glyph placements. It is deliberately derived data: it carries
//! source ranges for selection/extraction, but it never becomes FlowDocument
//! state.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    canonical::{canonical_bytes, canonical_hash},
    layout::{
        FontCatalog, FragmentKind, LayoutError, LayoutFragment, LayoutPage, LayoutRect, LayoutUnit,
        PaginationResult, SourceRange, TextDirection, TextGlyphRun, TextLanguage,
        TextLayoutRequest, layout_text,
    },
    model::{
        AssetId, BlockKind, ContentNode, FlowDocument, HeaderFooterSettings, NodeId, RunLanguage,
    },
    schema::validate_document,
};

const TABLE_CELL_PADDING_MILLIPOINTS: u32 = 2_000;
const DEFAULT_FONT_SIZE_MILLIPOINTS: u32 = 12_000;

/// Version of the derived display-list projection.
pub const PDF_DISPLAY_LIST_SCHEMA_VERSION: u32 = 1;

/// One glyph placement with source cluster and fixed-point geometry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfGlyphPlacement {
    pub glyph_id: u32,
    pub face_id: String,
    pub direction: TextDirection,
    pub cluster_utf8: u32,
    pub cluster_utf16: u32,
    pub x: LayoutUnit,
    pub y: LayoutUnit,
    pub x_advance: LayoutUnit,
    pub x_offset: LayoutUnit,
    pub y_offset: LayoutUnit,
    pub unsafe_to_break: bool,
}

/// One line of selectable text in the derived display list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfTextLine {
    pub source: SourceRange,
    pub rect: LayoutRect,
    pub width: LayoutUnit,
    pub text: String,
    pub glyphs: Vec<PdfGlyphPlacement>,
}

/// A source-backed text item. `text` is derived and is never accepted as a
/// replacement for canonical document bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfTextItem {
    pub fragment_id: String,
    pub source_node_id: Option<NodeId>,
    pub source: SourceRange,
    pub rect: LayoutRect,
    pub text: String,
    pub font_size_millipoints: u32,
    pub derived: bool,
    pub repeat_index: u16,
    pub lines: Vec<PdfTextLine>,
}

/// A source-backed image placement. The physical bytes are resolved later by
/// the bounded asset adapter; this item carries only semantic identity and
/// fixed-point geometry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfImageItem {
    pub fragment_id: String,
    pub source_node_id: Option<NodeId>,
    pub asset_id: AssetId,
    pub rect: LayoutRect,
    pub derived: bool,
    pub repeat_index: u16,
}

/// A visual page containing only derived display-list items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfDisplayPage {
    pub page_index: u32,
    pub bounds: LayoutRect,
    pub items: Vec<PdfTextItem>,
    pub images: Vec<PdfImageItem>,
}

/// Privacy-safe display-list diagnostic.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PdfDisplayDiagnosticCode {
    UnsupportedFragment,
    SourceMappingMissing,
    GlyphMappingMissing,
    UnsupportedGlyph,
    AssetMappingMissing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfDisplayDiagnostic {
    pub code: PdfDisplayDiagnosticCode,
    pub page_index: u32,
    pub fragment_id: String,
    pub source_node_id: Option<NodeId>,
}

/// A complete display list bound to one accepted pagination result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfDisplayList {
    pub schema_version: u32,
    pub source_revision: u32,
    pub source_hash: String,
    pub layout_settings_fingerprint: String,
    pub pagination_result_hash: String,
    pub font_catalog_identity: String,
    pub hyphenation_data_identity: Option<String>,
    pub pages: Vec<PdfDisplayPage>,
    pub diagnostics: Vec<PdfDisplayDiagnostic>,
    pub result_hash: String,
}

impl PdfDisplayList {
    /// Returns the deterministic hash of this derived display list.
    #[must_use]
    pub fn result_hash(&self) -> &str {
        &self.result_hash
    }
}

/// Failure taxonomy for display-list construction.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PdfDisplayListError {
    #[error("the canonical document is invalid")]
    InvalidDocument,
    #[error("the pagination result revision is stale")]
    StaleRevision,
    #[error("the pagination source hash does not match the document")]
    SourceHashMismatch,
    #[error("the pagination font catalog does not match the admitted catalog")]
    FontCatalogMismatch,
    #[error("the pagination hyphenation identity does not match the admitted data")]
    HyphenationMismatch,
    #[error("a source range is not a UTF-8/UTF-16 boundary in the source text")]
    SourceRangeInvalid,
    #[error("a pagination line has no matching shaped text line")]
    SourceMappingMissing,
    #[error("a shaped glyph has no source cluster mapping")]
    GlyphMappingMissing,
    #[error("the layout adapter failed")]
    Layout(#[from] LayoutError),
    #[error("the display list could not be serialized")]
    Serialization,
}

impl PdfDisplayListError {
    /// Stable diagnostic code for boundary callers.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidDocument => "FLOW_PDF_DISPLAY_DOCUMENT_INVALID",
            Self::StaleRevision => "FLOW_PDF_DISPLAY_REVISION_STALE",
            Self::SourceHashMismatch => "FLOW_PDF_DISPLAY_SOURCE_HASH_MISMATCH",
            Self::FontCatalogMismatch => "FLOW_PDF_DISPLAY_FONT_CATALOG_MISMATCH",
            Self::HyphenationMismatch => "FLOW_PDF_DISPLAY_HYPHENATION_MISMATCH",
            Self::SourceRangeInvalid => "FLOW_PDF_DISPLAY_SOURCE_RANGE_INVALID",
            Self::SourceMappingMissing => "FLOW_PDF_DISPLAY_SOURCE_MAPPING_MISSING",
            Self::GlyphMappingMissing => "FLOW_PDF_DISPLAY_GLYPH_MAPPING_MISSING",
            Self::Layout(error) => error.code(),
            Self::Serialization => "FLOW_PDF_DISPLAY_SERIALIZATION",
        }
    }
}

#[derive(Debug, Clone)]
struct TextContext {
    text: String,
    font_size_millipoints: u32,
    language: TextLanguage,
}

/// Builds a PDF/preview display list from one validated document and one
/// accepted Phase 3 pagination result.
pub fn build_display_list(
    document: &FlowDocument,
    pagination: &PaginationResult,
    catalog: &FontCatalog,
    ukrainian_hyphenation: Option<&crate::layout::UkrainianHyphenation>,
) -> Result<PdfDisplayList, PdfDisplayListError> {
    validate_document(document).map_err(|_| PdfDisplayListError::InvalidDocument)?;
    if pagination.source_revision != document.revision {
        return Err(PdfDisplayListError::StaleRevision);
    }
    let canonical = canonical_bytes(document).map_err(|_| PdfDisplayListError::InvalidDocument)?;
    if canonical_hash(&canonical) != pagination.source_hash {
        return Err(PdfDisplayListError::SourceHashMismatch);
    }
    if pagination.font_catalog_identity != catalog.identity() {
        return Err(PdfDisplayListError::FontCatalogMismatch);
    }
    let supplied_hyphenation_identity = ukrainian_hyphenation.map(|data| data.identity());
    if pagination.hyphenation_data_identity.as_deref() != supplied_hyphenation_identity {
        return Err(PdfDisplayListError::HyphenationMismatch);
    }

    let mut nodes = BTreeMap::new();
    index_nodes(&document.content, &mut nodes);
    let mut pages = Vec::with_capacity(pagination.pages.len());
    let mut diagnostics = Vec::new();
    for page in &pagination.pages {
        let mut items = Vec::new();
        let mut images = Vec::new();
        if let Some(header) = &page.header {
            append_fragment(
                document,
                page,
                header,
                pagination.source_revision,
                catalog,
                ukrainian_hyphenation,
                &nodes,
                &mut items,
                &mut images,
                &mut diagnostics,
            )?;
        }
        if let Some(footer) = &page.footer {
            append_fragment(
                document,
                page,
                footer,
                pagination.source_revision,
                catalog,
                ukrainian_hyphenation,
                &nodes,
                &mut items,
                &mut images,
                &mut diagnostics,
            )?;
        }
        for fragment in &page.fragments {
            append_fragment(
                document,
                page,
                fragment,
                pagination.source_revision,
                catalog,
                ukrainian_hyphenation,
                &nodes,
                &mut items,
                &mut images,
                &mut diagnostics,
            )?;
        }
        pages.push(PdfDisplayPage {
            page_index: page.page_index,
            bounds: page.bounds,
            items,
            images,
        });
    }

    let mut display_list = PdfDisplayList {
        schema_version: PDF_DISPLAY_LIST_SCHEMA_VERSION,
        source_revision: pagination.source_revision,
        source_hash: pagination.source_hash.clone(),
        layout_settings_fingerprint: pagination.layout_settings_fingerprint.clone(),
        pagination_result_hash: pagination.result_hash.clone(),
        font_catalog_identity: pagination.font_catalog_identity.clone(),
        hyphenation_data_identity: pagination.hyphenation_data_identity.clone(),
        pages,
        diagnostics,
        result_hash: String::new(),
    };
    let bytes =
        serde_json::to_vec(&display_list).map_err(|_| PdfDisplayListError::Serialization)?;
    display_list.result_hash = blake3::hash(&bytes).to_hex().to_string();
    Ok(display_list)
}

#[allow(clippy::too_many_arguments)]
fn append_fragment(
    document: &FlowDocument,
    page: &LayoutPage,
    fragment: &LayoutFragment,
    source_revision: u32,
    catalog: &FontCatalog,
    ukrainian_hyphenation: Option<&crate::layout::UkrainianHyphenation>,
    nodes: &BTreeMap<NodeId, &ContentNode>,
    items: &mut Vec<PdfTextItem>,
    images: &mut Vec<PdfImageItem>,
    diagnostics: &mut Vec<PdfDisplayDiagnostic>,
) -> Result<(), PdfDisplayListError> {
    if fragment.kind == FragmentKind::Image {
        append_image_item(page, fragment, nodes, images, diagnostics);
    } else if let Some(context) = text_context(document, page, fragment, nodes)? {
        if !fragment.children.is_empty() {
            match build_text_item(
                fragment,
                context,
                source_revision,
                catalog,
                ukrainian_hyphenation,
            ) {
                Ok(item) => items.push(item),
                Err(PdfDisplayListError::SourceMappingMissing) => {
                    diagnostics.push(PdfDisplayDiagnostic {
                        code: PdfDisplayDiagnosticCode::SourceMappingMissing,
                        page_index: page.page_index,
                        fragment_id: fragment.id.clone(),
                        source_node_id: fragment.source_node_id.clone(),
                    });
                }
                Err(PdfDisplayListError::GlyphMappingMissing) => {
                    diagnostics.push(PdfDisplayDiagnostic {
                        code: PdfDisplayDiagnosticCode::GlyphMappingMissing,
                        page_index: page.page_index,
                        fragment_id: fragment.id.clone(),
                        source_node_id: fragment.source_node_id.clone(),
                    });
                }
                Err(PdfDisplayListError::Layout(LayoutError::UnsupportedGlyph)) => {
                    diagnostics.push(PdfDisplayDiagnostic {
                        code: PdfDisplayDiagnosticCode::UnsupportedGlyph,
                        page_index: page.page_index,
                        fragment_id: fragment.id.clone(),
                        source_node_id: fragment.source_node_id.clone(),
                    });
                }
                Err(error) => return Err(error),
            }
        }
    } else if fragment.kind == FragmentKind::Unsupported {
        diagnostics.push(PdfDisplayDiagnostic {
            code: PdfDisplayDiagnosticCode::UnsupportedFragment,
            page_index: page.page_index,
            fragment_id: fragment.id.clone(),
            source_node_id: fragment.source_node_id.clone(),
        });
    }

    for child in &fragment.children {
        append_fragment(
            document,
            page,
            child,
            source_revision,
            catalog,
            ukrainian_hyphenation,
            nodes,
            items,
            images,
            diagnostics,
        )?;
    }
    Ok(())
}

fn append_image_item(
    page: &LayoutPage,
    fragment: &LayoutFragment,
    nodes: &BTreeMap<NodeId, &ContentNode>,
    images: &mut Vec<PdfImageItem>,
    diagnostics: &mut Vec<PdfDisplayDiagnostic>,
) {
    let Some(source_node_id) = fragment.source_node_id.as_ref() else {
        diagnostics.push(PdfDisplayDiagnostic {
            code: PdfDisplayDiagnosticCode::AssetMappingMissing,
            page_index: page.page_index,
            fragment_id: fragment.id.clone(),
            source_node_id: None,
        });
        return;
    };
    let Some(node) = nodes.get(source_node_id) else {
        diagnostics.push(PdfDisplayDiagnostic {
            code: PdfDisplayDiagnosticCode::AssetMappingMissing,
            page_index: page.page_index,
            fragment_id: fragment.id.clone(),
            source_node_id: Some(source_node_id.clone()),
        });
        return;
    };
    let BlockKind::Image { asset_id, .. } = &node.body else {
        diagnostics.push(PdfDisplayDiagnostic {
            code: PdfDisplayDiagnosticCode::AssetMappingMissing,
            page_index: page.page_index,
            fragment_id: fragment.id.clone(),
            source_node_id: Some(source_node_id.clone()),
        });
        return;
    };
    images.push(PdfImageItem {
        fragment_id: fragment.id.clone(),
        source_node_id: Some(source_node_id.clone()),
        asset_id: asset_id.clone(),
        rect: fragment.rect,
        derived: fragment.derived,
        repeat_index: fragment.repeat_index,
    });
}

fn build_text_item(
    fragment: &LayoutFragment,
    context: TextContext,
    source_revision: u32,
    catalog: &FontCatalog,
    ukrainian_hyphenation: Option<&crate::layout::UkrainianHyphenation>,
) -> Result<PdfTextItem, PdfDisplayListError> {
    let hyphenation_identity = if context.language == TextLanguage::Ukrainian {
        Some(
            ukrainian_hyphenation
                .ok_or(LayoutError::MissingHyphenationData)?
                .identity()
                .to_owned(),
        )
    } else {
        None
    };
    let width = if fragment.kind == FragmentKind::TableCell {
        let padding = LayoutUnit::from_millipoints(TABLE_CELL_PADDING_MILLIPOINTS)?;
        fragment
            .rect
            .width
            .checked_sub(padding.checked_add(padding)?)?
    } else {
        fragment.rect.width
    };
    if width.raw() <= 0 {
        return Err(LayoutError::InvalidPaginationGeometry.into());
    }
    let request = TextLayoutRequest::new(
        source_revision,
        blake3::hash(context.text.as_bytes()).to_hex().to_string(),
        width,
        context.font_size_millipoints,
        TextDirection::Auto,
        context.language,
        catalog,
        hyphenation_identity,
    )?;
    let shaped = layout_text(
        &context.text,
        &request,
        catalog,
        if context.language == TextLanguage::Ukrainian {
            ukrainian_hyphenation
        } else {
            None
        },
    )?;
    let source = fragment.source.unwrap_or(full_source_range(&context.text)?);
    let mut lines = Vec::with_capacity(fragment.children.len());
    for line_fragment in fragment
        .children
        .iter()
        .filter(|child| child.kind == FragmentKind::Line)
    {
        let line_source = line_fragment
            .source
            .ok_or(PdfDisplayListError::SourceMappingMissing)?;
        let line = shaped
            .lines
            .iter()
            .find(|candidate| candidate.source == line_source)
            .ok_or(PdfDisplayListError::SourceMappingMissing)?;
        let text = source_slice(&context.text, line_source)?;
        let glyphs = glyphs_for_line(&shaped.glyph_runs, line_source, line_fragment.rect)?;
        if !text.is_empty() && glyphs.is_empty() {
            return Err(PdfDisplayListError::GlyphMappingMissing);
        }
        lines.push(PdfTextLine {
            source: line_source,
            rect: line_fragment.rect,
            width: line.width,
            text,
            glyphs,
        });
    }
    if !context.text.is_empty() && lines.is_empty() {
        return Err(PdfDisplayListError::SourceMappingMissing);
    }
    Ok(PdfTextItem {
        fragment_id: fragment.id.clone(),
        source_node_id: fragment.source_node_id.clone(),
        source,
        rect: fragment.rect,
        text: context.text,
        font_size_millipoints: context.font_size_millipoints,
        derived: fragment.derived,
        repeat_index: fragment.repeat_index,
        lines,
    })
}

fn glyphs_for_line(
    runs: &[TextGlyphRun],
    source: SourceRange,
    rect: LayoutRect,
) -> Result<Vec<PdfGlyphPlacement>, PdfDisplayListError> {
    let mut cursor = rect.x;
    let mut glyphs = Vec::new();
    for run in runs {
        for glyph in &run.glyphs {
            if glyph.cluster_utf8 < source.utf8_start || glyph.cluster_utf8 >= source.utf8_end {
                continue;
            }
            let x = cursor.checked_add(glyph.x_offset)?;
            let y = rect.y.checked_add(glyph.y_offset)?;
            glyphs.push(PdfGlyphPlacement {
                glyph_id: glyph.glyph_id,
                face_id: run.face_id.clone(),
                direction: run.direction,
                cluster_utf8: glyph.cluster_utf8,
                cluster_utf16: glyph.cluster_utf16,
                x,
                y,
                x_advance: glyph.x_advance,
                x_offset: glyph.x_offset,
                y_offset: glyph.y_offset,
                unsafe_to_break: glyph.unsafe_to_break,
            });
            cursor = cursor.checked_add(glyph.x_advance)?;
        }
    }
    Ok(glyphs)
}

fn text_context(
    document: &FlowDocument,
    page: &LayoutPage,
    fragment: &LayoutFragment,
    nodes: &BTreeMap<NodeId, &ContentNode>,
) -> Result<Option<TextContext>, PdfDisplayListError> {
    match fragment.kind {
        FragmentKind::Paragraph | FragmentKind::Heading | FragmentKind::TableCell => {
            let node = fragment
                .source_node_id
                .as_ref()
                .and_then(|id| nodes.get(id))
                .ok_or(PdfDisplayListError::SourceMappingMissing)?;
            let text = node_text_recursive(node);
            Ok(Some(TextContext {
                font_size_millipoints: font_size_for_node(document, node),
                language: language_for_node(document, node),
                text,
            }))
        }
        FragmentKind::Header | FragmentKind::Footer => {
            let section = document
                .sections
                .iter()
                .find(|section| section.id == page.section_id)
                .ok_or(PdfDisplayListError::InvalidDocument)?;
            let settings = if fragment.kind == FragmentKind::Header {
                &section.header
            } else {
                &section.footer
            };
            Ok(header_footer_context(document, settings))
        }
        _ => Ok(None),
    }
}

fn header_footer_context(
    document: &FlowDocument,
    settings: &HeaderFooterSettings,
) -> Option<TextContext> {
    if !settings.enabled {
        return None;
    }
    let text = settings
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    Some(TextContext {
        text,
        font_size_millipoints: settings
            .runs
            .iter()
            .map(|run| run.style.font_size_millipoints)
            .max()
            .unwrap_or(DEFAULT_FONT_SIZE_MILLIPOINTS),
        language: language_for_locale(&settings.locale, document.locale.as_str()),
    })
}

fn index_nodes<'a>(nodes: &'a [ContentNode], index: &mut BTreeMap<NodeId, &'a ContentNode>) {
    for node in nodes {
        index.insert(node.id.clone(), node);
        index_nodes(node.children(), index);
    }
}

fn node_text_recursive(node: &ContentNode) -> String {
    if let Some(runs) = node.runs() {
        return runs.iter().map(|run| run.text.as_str()).collect();
    }
    node.children()
        .iter()
        .map(node_text_recursive)
        .collect::<Vec<_>>()
        .join(" ")
}

fn font_size_for_node(document: &FlowDocument, node: &ContentNode) -> u32 {
    let style_size = node
        .style_id
        .as_ref()
        .and_then(|style_id| document.styles.iter().find(|style| &style.id == style_id))
        .map_or(DEFAULT_FONT_SIZE_MILLIPOINTS, |style| {
            style.font_size_millipoints
        });
    node.runs()
        .into_iter()
        .flatten()
        .filter_map(|run| run.marks.font_size_millipoints)
        .fold(style_size, u32::max)
}

fn language_for_node(document: &FlowDocument, node: &ContentNode) -> TextLanguage {
    let explicit = node
        .runs()
        .into_iter()
        .flatten()
        .filter_map(|run| run.marks.language.as_ref())
        .next();
    match explicit {
        Some(RunLanguage::English) => TextLanguage::English,
        Some(RunLanguage::Ukrainian) => TextLanguage::Ukrainian,
        None => language_for_locale(document.locale.as_str(), document.locale.as_str()),
    }
}

fn language_for_locale(locale: &str, fallback: &str) -> TextLanguage {
    let locale = if locale.trim().is_empty() {
        fallback
    } else {
        locale
    };
    if locale.to_ascii_lowercase().starts_with("uk") {
        TextLanguage::Ukrainian
    } else {
        TextLanguage::English
    }
}

fn full_source_range(text: &str) -> Result<SourceRange, PdfDisplayListError> {
    Ok(SourceRange {
        utf8_start: 0,
        utf8_end: u32::try_from(text.len()).map_err(|_| PdfDisplayListError::SourceRangeInvalid)?,
        utf16_start: 0,
        utf16_end: u32::try_from(text.encode_utf16().count())
            .map_err(|_| PdfDisplayListError::SourceRangeInvalid)?,
    })
}

fn source_slice(text: &str, source: SourceRange) -> Result<String, PdfDisplayListError> {
    let start =
        usize::try_from(source.utf8_start).map_err(|_| PdfDisplayListError::SourceRangeInvalid)?;
    let end =
        usize::try_from(source.utf8_end).map_err(|_| PdfDisplayListError::SourceRangeInvalid)?;
    let slice = text
        .get(start..end)
        .ok_or(PdfDisplayListError::SourceRangeInvalid)?;
    let utf16_start = u32::try_from(text[..start].encode_utf16().count())
        .map_err(|_| PdfDisplayListError::SourceRangeInvalid)?;
    let utf16_end = u32::try_from(text[..end].encode_utf16().count())
        .map_err(|_| PdfDisplayListError::SourceRangeInvalid)?;
    if utf16_start != source.utf16_start || utf16_end != source.utf16_end {
        return Err(PdfDisplayListError::SourceRangeInvalid);
    }
    Ok(slice.to_owned())
}

//! Bounded PDF document-structure options.
//!
//! The public types in this module are intentionally narrower than arbitrary
//! PDF annotations and actions.  They describe only metadata, internal page
//! destinations, and outline entries that the owned writer can validate
//! without following external references or importing viewer behavior.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::layout::LayoutRect;

use super::PdfPagePlan;

pub const MAX_PDF_METADATA_FIELD_BYTES: usize = 4 * 1024;
pub const MAX_PDF_OUTLINE_ENTRIES: usize = 2_048;
pub const MAX_PDF_LINK_ENTRIES: usize = 4_096;

/// Optional bounded document metadata emitted in the PDF Info/catalog layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfMetadataOptions {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub language: Option<String>,
}

impl PdfMetadataOptions {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.author.is_none()
            && self.subject.is_none()
            && self.language.is_none()
    }
}

/// One deterministic bookmark destination within the exported page list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfOutlineEntry {
    pub id: String,
    pub title: String,
    pub page_index: u32,
}

/// One bounded internal link.  External URI/file/action targets are not part
/// of this type and therefore cannot be smuggled into the owned writer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfInternalLink {
    pub id: String,
    pub page_index: u32,
    pub rect: LayoutRect,
    pub destination_page_index: u32,
}

/// Features that the current owned writer can represent intentionally.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PdfSupportedFeature {
    Text,
    Images,
    Metadata,
    Outlines,
    InternalLinks,
}

/// Source/PDF constructs that remain explicit non-goals for this export
/// subset.  They are reported instead of being inferred or silently dropped.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PdfUnsupportedFeature {
    FormFields,
    Actions,
    JavaScript,
    LaunchActions,
    ExternalLinks,
    ExternalFiles,
    Annotations,
    Paths,
    Encryption,
}

impl PdfUnsupportedFeature {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::FormFields => "FLOW_PDF_UNSUPPORTED_FORM_FIELDS",
            Self::Actions => "FLOW_PDF_UNSUPPORTED_ACTIONS",
            Self::JavaScript => "FLOW_PDF_UNSUPPORTED_JAVASCRIPT",
            Self::LaunchActions => "FLOW_PDF_UNSUPPORTED_LAUNCH_ACTIONS",
            Self::ExternalLinks => "FLOW_PDF_UNSUPPORTED_EXTERNAL_LINKS",
            Self::ExternalFiles => "FLOW_PDF_UNSUPPORTED_EXTERNAL_FILES",
            Self::Annotations => "FLOW_PDF_UNSUPPORTED_ANNOTATIONS",
            Self::Paths => "FLOW_PDF_UNSUPPORTED_PATHS",
            Self::Encryption => "FLOW_PDF_UNSUPPORTED_ENCRYPTION",
        }
    }
}

/// Stable support matrix carried by every export result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfSupportReport {
    pub supported: Vec<PdfSupportedFeature>,
    pub unsupported: Vec<PdfUnsupportedFeature>,
}

impl PdfSupportReport {
    #[must_use]
    pub fn current() -> Self {
        Self {
            supported: vec![
                PdfSupportedFeature::Text,
                PdfSupportedFeature::Images,
                PdfSupportedFeature::Metadata,
                PdfSupportedFeature::Outlines,
                PdfSupportedFeature::InternalLinks,
            ],
            unsupported: vec![
                PdfUnsupportedFeature::FormFields,
                PdfUnsupportedFeature::Actions,
                PdfUnsupportedFeature::JavaScript,
                PdfUnsupportedFeature::LaunchActions,
                PdfUnsupportedFeature::ExternalLinks,
                PdfUnsupportedFeature::ExternalFiles,
                PdfUnsupportedFeature::Annotations,
                PdfUnsupportedFeature::Paths,
                PdfUnsupportedFeature::Encryption,
            ],
        }
    }
}

/// Stable validation taxonomy for export document-structure options.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PdfMetadataError {
    #[error("a PDF metadata text field is empty or contains a forbidden NUL")]
    EmptyText,
    #[error("a PDF metadata text field exceeds the bounded limit")]
    TextLimit,
    #[error("the PDF language tag is invalid")]
    InvalidLanguage,
    #[error("a PDF outline or link identity is invalid")]
    InvalidIdentity,
    #[error("a PDF outline or link identity is duplicated")]
    DuplicateIdentity,
    #[error("the PDF outline/link count exceeds the bounded limit")]
    CountLimit,
    #[error("a PDF outline destination page is invalid")]
    InvalidOutlinePage,
    #[error("a PDF link page or destination is invalid")]
    InvalidLinkPage,
    #[error("a PDF link rectangle is invalid")]
    InvalidLinkRect,
}

impl PdfMetadataError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyText => "FLOW_PDF_METADATA_EMPTY",
            Self::TextLimit => "FLOW_PDF_METADATA_LIMIT",
            Self::InvalidLanguage => "FLOW_PDF_METADATA_LANGUAGE_INVALID",
            Self::InvalidIdentity => "FLOW_PDF_STRUCTURE_ID_INVALID",
            Self::DuplicateIdentity => "FLOW_PDF_STRUCTURE_ID_DUPLICATE",
            Self::CountLimit => "FLOW_PDF_STRUCTURE_COUNT_LIMIT",
            Self::InvalidOutlinePage => "FLOW_PDF_OUTLINE_PAGE_INVALID",
            Self::InvalidLinkPage => "FLOW_PDF_LINK_PAGE_INVALID",
            Self::InvalidLinkRect => "FLOW_PDF_LINK_RECT_INVALID",
        }
    }
}

pub(crate) fn validate_export_metadata(
    metadata: &PdfMetadataOptions,
    outlines: &[PdfOutlineEntry],
    links: &[PdfInternalLink],
    pages: &[PdfPagePlan],
) -> Result<(), PdfMetadataError> {
    for value in [&metadata.title, &metadata.author, &metadata.subject]
        .into_iter()
        .flatten()
    {
        validate_text(value)?;
    }
    if let Some(language) = &metadata.language
        && (language.is_empty()
            || language.len() > 35
            || !language.is_ascii()
            || language
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || byte == b'-')))
    {
        return Err(PdfMetadataError::InvalidLanguage);
    }
    if outlines.len() > MAX_PDF_OUTLINE_ENTRIES || links.len() > MAX_PDF_LINK_ENTRIES {
        return Err(PdfMetadataError::CountLimit);
    }

    let mut identities = BTreeSet::new();
    for outline in outlines {
        validate_identity(&outline.id)?;
        if !identities.insert(outline.id.as_str()) {
            return Err(PdfMetadataError::DuplicateIdentity);
        }
        validate_text(&outline.title)?;
        if !valid_page_index(outline.page_index, pages.len()) {
            return Err(PdfMetadataError::InvalidOutlinePage);
        }
    }
    for link in links {
        validate_identity(&link.id)?;
        if !identities.insert(link.id.as_str()) {
            return Err(PdfMetadataError::DuplicateIdentity);
        }
        if !valid_page_index(link.page_index, pages.len())
            || !valid_page_index(link.destination_page_index, pages.len())
        {
            return Err(PdfMetadataError::InvalidLinkPage);
        }
        let page = &pages[usize::try_from(link.page_index).expect("validated page index")];
        if !valid_rect(link.rect, page.bounds) {
            return Err(PdfMetadataError::InvalidLinkRect);
        }
    }
    Ok(())
}

pub(crate) fn ordered_outlines(outlines: &[PdfOutlineEntry]) -> Vec<PdfOutlineEntry> {
    let mut ordered = outlines.to_vec();
    ordered.sort_by(|left, right| {
        left.page_index
            .cmp(&right.page_index)
            .then_with(|| left.id.cmp(&right.id))
    });
    ordered
}

pub(crate) fn ordered_links(links: &[PdfInternalLink]) -> Vec<PdfInternalLink> {
    let mut ordered = links.to_vec();
    ordered.sort_by(|left, right| {
        left.page_index
            .cmp(&right.page_index)
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| {
                left.destination_page_index
                    .cmp(&right.destination_page_index)
            })
    });
    ordered
}

/// Appends a length-delimited, order-normalized option representation to an
/// export fingerprint.  The representation contains metadata and geometry,
/// never physical asset/font bytes.
pub(crate) fn append_fingerprint(
    output: &mut Vec<u8>,
    metadata: &PdfMetadataOptions,
    outlines: &[PdfOutlineEntry],
    links: &[PdfInternalLink],
) {
    output.extend_from_slice(b"pdf-structure-v1");
    append_optional(output, metadata.title.as_deref());
    append_optional(output, metadata.author.as_deref());
    append_optional(output, metadata.subject.as_deref());
    append_optional(output, metadata.language.as_deref());
    let outlines = ordered_outlines(outlines);
    append_count(output, outlines.len());
    for outline in outlines {
        append_text(output, &outline.id);
        append_text(output, &outline.title);
        output.extend_from_slice(&outline.page_index.to_be_bytes());
    }
    let links = ordered_links(links);
    append_count(output, links.len());
    for link in links {
        append_text(output, &link.id);
        output.extend_from_slice(&link.page_index.to_be_bytes());
        for coordinate in [
            link.rect.x.raw(),
            link.rect.y.raw(),
            link.rect.width.raw(),
            link.rect.height.raw(),
        ] {
            output.extend_from_slice(&coordinate.to_be_bytes());
        }
        output.extend_from_slice(&link.destination_page_index.to_be_bytes());
    }
}

pub(crate) fn encode_pdf_text(value: &str) -> Vec<u8> {
    if value.is_ascii() {
        return value.as_bytes().to_vec();
    }
    let mut output = Vec::with_capacity(2 + value.len() * 2);
    output.extend_from_slice(&[0xfe, 0xff]);
    for code_unit in value.encode_utf16() {
        output.extend_from_slice(&code_unit.to_be_bytes());
    }
    output
}

fn validate_text(value: &str) -> Result<(), PdfMetadataError> {
    if value.is_empty() || value.bytes().any(|byte| byte == 0) {
        return Err(PdfMetadataError::EmptyText);
    }
    if value.len() > MAX_PDF_METADATA_FIELD_BYTES {
        return Err(PdfMetadataError::TextLimit);
    }
    Ok(())
}

fn validate_identity(value: &str) -> Result<(), PdfMetadataError> {
    if value.is_empty()
        || value.len() > 128
        || !value.is_ascii()
        || value.trim() != value
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(PdfMetadataError::InvalidIdentity);
    }
    Ok(())
}

fn valid_page_index(index: u32, page_count: usize) -> bool {
    usize::try_from(index).is_ok_and(|index| index < page_count)
}

fn valid_rect(rect: LayoutRect, bounds: LayoutRect) -> bool {
    if rect.x.raw() < 0 || rect.y.raw() < 0 || rect.width.raw() <= 0 || rect.height.raw() <= 0 {
        return false;
    }
    let Some(right) = rect.x.raw().checked_add(rect.width.raw()) else {
        return false;
    };
    let Some(bottom) = rect.y.raw().checked_add(rect.height.raw()) else {
        return false;
    };
    right <= bounds.width.raw() && bottom <= bounds.height.raw()
}

fn append_optional(output: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            output.push(1);
            append_text(output, value);
        }
        None => output.push(0),
    }
}

fn append_text(output: &mut Vec<u8>, value: &str) {
    append_count(output, value.len());
    output.extend_from_slice(value.as_bytes());
}

fn append_count(output: &mut Vec<u8>, value: usize) {
    output.extend_from_slice(&u64::try_from(value).unwrap_or(u64::MAX).to_be_bytes());
}

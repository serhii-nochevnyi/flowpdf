//! Deterministic, Rust-owned text layout primitives.
//!
//! This module is deliberately derived-data only. It accepts a validated text
//! request and an explicit byte-backed font/data catalog, then returns shaped
//! glyphs and line opportunities without changing the canonical FlowDocument.
//! Page fragmentation is added by the following Phase 3 plans.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    str::FromStr,
};

use hyphenation::{Hyphenator, Language, Load, Standard};
use icu_segmenter::{GraphemeClusterSegmenter, LineSegmenter, options::LineBreakOptions};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_bidi::{BidiInfo, LTR_LEVEL, RTL_LEVEL};

const ENGINE_VERSION: &str = "flowpdf-layout-text-0.1";
const LAYOUT_UNITS_PER_POINT: i64 = 64;
const MILLIPOINTS_PER_POINT: i64 = 1_000;
const MAX_FONT_FACES: usize = 16;
const MAX_FONT_FACE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 512 * 1024;
const MAX_REQUEST_HASH_BYTES: usize = 128;

/// The fixed-point unit used by derived layout geometry: 1/64 of a typographic
/// point. It is serialized as a signed integer, never as a floating-point
/// value.
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(transparent)]
pub struct LayoutUnit(i64);

impl LayoutUnit {
    /// The number of fixed-point units in one typographic point.
    pub const UNITS_PER_POINT: i64 = LAYOUT_UNITS_PER_POINT;

    /// Creates a unit value from its raw fixed-point representation.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// Returns the raw fixed-point representation.
    #[must_use]
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// Converts an existing semantic millipoint value with deterministic
    /// nearest-unit rounding.
    pub fn from_millipoints(value: u32) -> Result<Self, LayoutError> {
        let numerator = i128::from(value)
            .checked_mul(i128::from(LAYOUT_UNITS_PER_POINT))
            .ok_or(LayoutError::NumericOverflow)?;
        Self::from_ratio(numerator, i128::from(MILLIPOINTS_PER_POINT))
    }

    /// Converts a signed font-unit advance at a concrete font size. This is
    /// the only conversion boundary from RustyBuzz/TrueType font units into
    /// FlowPDF geometry.
    pub fn from_font_units(
        font_units: i32,
        units_per_em: u16,
        font_size: Self,
    ) -> Result<Self, LayoutError> {
        if units_per_em == 0 {
            return Err(LayoutError::InvalidFont);
        }
        let numerator = i128::from(font_units)
            .checked_mul(i128::from(font_size.0))
            .ok_or(LayoutError::NumericOverflow)?;
        Self::from_ratio(numerator, i128::from(units_per_em))
    }

    /// Adds two fixed-point values with overflow detection.
    pub fn checked_add(self, other: Self) -> Result<Self, LayoutError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(LayoutError::NumericOverflow)
    }

    fn from_ratio(numerator: i128, denominator: i128) -> Result<Self, LayoutError> {
        if denominator <= 0 {
            return Err(LayoutError::NumericOverflow);
        }
        let absolute = numerator.unsigned_abs();
        let rounded = absolute
            .checked_add((denominator as u128) / 2)
            .ok_or(LayoutError::NumericOverflow)?
            / (denominator as u128);
        let signed = if numerator.is_negative() {
            -(i128::try_from(rounded).map_err(|_| LayoutError::NumericOverflow)?)
        } else {
            i128::try_from(rounded).map_err(|_| LayoutError::NumericOverflow)?
        };
        i64::try_from(signed)
            .map(Self)
            .map_err(|_| LayoutError::NumericOverflow)
    }
}

/// Direction policy for one paragraph.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum TextDirection {
    Auto,
    LeftToRight,
    RightToLeft,
}

/// Language data policy for one paragraph.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum TextLanguage {
    Ukrainian,
    English,
}

impl TextLanguage {
    fn language_tag(self) -> &'static str {
        match self {
            Self::Ukrainian => "uk",
            Self::English => "en",
        }
    }
}

/// Stable identity for an admitted font face. Font bytes are intentionally
/// kept out of serializable layout requests and results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FontFaceIdentity {
    pub id: String,
    pub family: String,
    pub face_index: u32,
    pub content_hash: String,
}

/// A validated font face owned by a request-local catalog.
#[derive(Debug, Clone)]
pub struct FontFace {
    identity: FontFaceIdentity,
    bytes: Vec<u8>,
}

impl FontFace {
    /// Validates and admits one bounded byte-backed TrueType/OpenType face.
    pub fn new(
        id: impl Into<String>,
        family: impl Into<String>,
        face_index: u32,
        bytes: Vec<u8>,
    ) -> Result<Self, LayoutError> {
        let id = id.into();
        let family = family.into();
        if id.trim().is_empty() || family.trim().is_empty() {
            return Err(LayoutError::InvalidFontIdentity);
        }
        if bytes.is_empty() || bytes.len() > MAX_FONT_FACE_BYTES {
            return Err(LayoutError::FontSizeLimit);
        }
        ttf_parser::Face::parse(&bytes, face_index).map_err(|_| LayoutError::InvalidFont)?;
        let content_hash = blake3::hash(&bytes).to_hex().to_string();
        Ok(Self {
            identity: FontFaceIdentity {
                id,
                family,
                face_index,
                content_hash,
            },
            bytes,
        })
    }

    /// Returns the stable identity without exposing font bytes.
    #[must_use]
    pub fn identity(&self) -> &FontFaceIdentity {
        &self.identity
    }

    fn supports_text(&self, text: &str) -> bool {
        let Ok(face) = ttf_parser::Face::parse(&self.bytes, self.identity.face_index) else {
            return false;
        };
        text.chars().all(|character| {
            face.glyph_index(character)
                .is_some_and(|glyph| glyph.0 != 0)
        })
    }

    fn shape_face(&self) -> Result<rustybuzz::Face<'_>, LayoutError> {
        rustybuzz::Face::from_slice(&self.bytes, self.identity.face_index)
            .ok_or(LayoutError::InvalidFont)
    }
}

/// Ordered, bounded fallback catalog. Earlier faces win when more than one
/// face covers a grapheme cluster.
#[derive(Debug, Clone)]
pub struct FontCatalog {
    faces: Vec<FontFace>,
    identity: String,
}

impl FontCatalog {
    /// Creates a deterministic catalog and rejects duplicate face identities.
    pub fn new(faces: Vec<FontFace>) -> Result<Self, LayoutError> {
        if faces.is_empty() {
            return Err(LayoutError::EmptyFontCatalog);
        }
        if faces.len() > MAX_FONT_FACES {
            return Err(LayoutError::FontCatalogLimit);
        }
        let mut seen = BTreeSet::new();
        for face in &faces {
            if !seen.insert(face.identity.id.clone()) {
                return Err(LayoutError::DuplicateFontIdentity);
            }
        }
        let mut hasher = blake3::Hasher::new();
        for face in &faces {
            hasher.update(face.identity.id.as_bytes());
            hasher.update(face.identity.family.as_bytes());
            hasher.update(&face.identity.face_index.to_le_bytes());
            hasher.update(face.identity.content_hash.as_bytes());
        }
        Ok(Self {
            faces,
            identity: hasher.finalize().to_hex().to_string(),
        })
    }

    /// Returns the ordered admitted faces.
    #[must_use]
    pub fn faces(&self) -> &[FontFace] {
        &self.faces
    }

    /// Returns the stable catalog identity.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    fn find_face(&self, text: &str) -> Option<&FontFace> {
        self.faces.iter().find(|face| face.supports_text(text))
    }
}

/// A revision-bound text layout request. It contains identities, not raw font
/// or hyphenation bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextLayoutRequest {
    pub source_revision: u32,
    pub source_hash: String,
    pub width: LayoutUnit,
    pub font_size_millipoints: u32,
    pub direction: TextDirection,
    pub language: TextLanguage,
    pub font_catalog_identity: String,
    pub hyphenation_data_identity: Option<String>,
    pub engine_version: String,
}

impl TextLayoutRequest {
    /// Builds a request bound to the supplied catalog and optional Ukrainian
    /// dictionary identity.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_revision: u32,
        source_hash: impl Into<String>,
        width: LayoutUnit,
        font_size_millipoints: u32,
        direction: TextDirection,
        language: TextLanguage,
        catalog: &FontCatalog,
        hyphenation_data_identity: Option<String>,
    ) -> Result<Self, LayoutError> {
        let source_hash = source_hash.into();
        if source_hash.is_empty() || source_hash.len() > MAX_REQUEST_HASH_BYTES {
            return Err(LayoutError::InvalidRequest);
        }
        if width.raw() <= 0 || font_size_millipoints == 0 {
            return Err(LayoutError::InvalidRequest);
        }
        Ok(Self {
            source_revision,
            source_hash,
            width,
            font_size_millipoints,
            direction,
            language,
            font_catalog_identity: catalog.identity().to_owned(),
            hyphenation_data_identity,
            engine_version: ENGINE_VERSION.to_owned(),
        })
    }

    fn fingerprint(&self) -> Result<String, LayoutError> {
        serde_json::to_vec(self)
            .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
            .map_err(|_| LayoutError::Serialization)
    }
}

/// Versioned Ukrainian Knuth-Liang data loaded from one explicitly admitted
/// bincode artifact. No all-language embedding is enabled.
pub struct UkrainianHyphenation {
    identity: String,
    dictionary: Standard,
}

impl UkrainianHyphenation {
    /// Loads one Ukrainian dictionary and binds it to a stable data identity.
    pub fn from_bincode(identity: impl Into<String>, bytes: &[u8]) -> Result<Self, LayoutError> {
        let identity = identity.into();
        if identity.trim().is_empty() || bytes.is_empty() {
            return Err(LayoutError::InvalidHyphenationData);
        }
        let mut reader = Cursor::new(bytes);
        let dictionary = Standard::from_reader(Language::Ukrainian, &mut reader)
            .map_err(|_| LayoutError::InvalidHyphenationData)?;
        Ok(Self {
            identity,
            dictionary,
        })
    }

    /// Returns the data identity recorded in a layout request.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    fn opportunities(&self, word: &str) -> Vec<usize> {
        self.dictionary.hyphenate(word).breaks
    }
}

/// Inclusive/exclusive source ranges in both UTF-8 bytes and UTF-16 code
/// units. The source text itself remains outside derived layout DTOs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRange {
    pub utf8_start: u32,
    pub utf8_end: u32,
    pub utf16_start: u32,
    pub utf16_end: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum BreakKind {
    Unicode,
    UkrainianHyphenation,
    Mandatory,
    Overflow,
}

/// A legal or rejected source boundary discovered by the text layout stages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BreakOpportunity {
    pub offset_utf8: u32,
    pub offset_utf16: u32,
    pub kind: BreakKind,
    pub legal: bool,
}

/// One shaped glyph with source cluster provenance and fixed-point placement
/// values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextGlyph {
    pub glyph_id: u32,
    pub cluster_utf8: u32,
    pub cluster_utf16: u32,
    pub x_advance: LayoutUnit,
    pub x_offset: LayoutUnit,
    pub y_offset: LayoutUnit,
    pub unsafe_to_break: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextGlyphRun {
    pub source: SourceRange,
    pub face_id: String,
    pub direction: TextDirection,
    pub glyphs: Vec<TextGlyph>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextLine {
    pub source: SourceRange,
    pub width: LayoutUnit,
    pub break_kind: BreakKind,
    pub discretionary_hyphen: bool,
}

/// Deterministic output of the Phase 3 text tracer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextLayoutResult {
    pub request_fingerprint: String,
    pub source_text_hash: String,
    pub glyph_runs: Vec<TextGlyphRun>,
    pub break_opportunities: Vec<BreakOpportunity>,
    pub lines: Vec<TextLine>,
    pub result_hash: String,
}

impl TextLayoutResult {
    /// Returns the stable hash of this result's derived fields.
    #[must_use]
    pub fn result_hash(&self) -> &str {
        &self.result_hash
    }
}

/// Stable, non-content-bearing layout failures.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum LayoutError {
    #[error("The font catalog is empty")]
    EmptyFontCatalog,
    #[error("The font catalog exceeds its face limit")]
    FontCatalogLimit,
    #[error("A font face exceeds the byte limit")]
    FontSizeLimit,
    #[error("The font identity is invalid")]
    InvalidFontIdentity,
    #[error("The font bytes are invalid")]
    InvalidFont,
    #[error("The font catalog contains a duplicate identity")]
    DuplicateFontIdentity,
    #[error("The layout request is invalid")]
    InvalidRequest,
    #[error("The text exceeds the layout input limit")]
    TextLimit,
    #[error("Ukrainian hyphenation data is missing")]
    MissingHyphenationData,
    #[error("Ukrainian hyphenation data is invalid")]
    InvalidHyphenationData,
    #[error("A layout numeric conversion overflowed")]
    NumericOverflow,
    #[error("A text cluster has no admitted font coverage")]
    UnsupportedGlyph,
    #[error("A serialized layout value could not be encoded")]
    Serialization,
}

impl LayoutError {
    /// Stable diagnostic code suitable for worker/UI allowlists.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::EmptyFontCatalog => "FLOW_LAYOUT_FONT_CATALOG_EMPTY",
            Self::FontCatalogLimit => "FLOW_LAYOUT_FONT_CATALOG_LIMIT",
            Self::FontSizeLimit => "FLOW_LAYOUT_FONT_SIZE_LIMIT",
            Self::InvalidFontIdentity => "FLOW_LAYOUT_FONT_IDENTITY_INVALID",
            Self::InvalidFont => "FLOW_LAYOUT_FONT_INVALID",
            Self::DuplicateFontIdentity => "FLOW_LAYOUT_FONT_DUPLICATE",
            Self::InvalidRequest => "FLOW_LAYOUT_REQUEST_INVALID",
            Self::TextLimit => "FLOW_LAYOUT_TEXT_LIMIT",
            Self::MissingHyphenationData => "FLOW_LAYOUT_HYPHENATION_MISSING",
            Self::InvalidHyphenationData => "FLOW_LAYOUT_HYPHENATION_INVALID",
            Self::NumericOverflow => "FLOW_LAYOUT_NUMERIC_OVERFLOW",
            Self::UnsupportedGlyph => "FLOW_LAYOUT_GLYPH_UNSUPPORTED",
            Self::Serialization => "FLOW_LAYOUT_SERIALIZATION",
        }
    }
}

/// Shapes one paragraph and computes deterministic Unicode/hyphenation break
/// opportunities. The function does not modify `text`.
pub fn layout_text(
    text: &str,
    request: &TextLayoutRequest,
    catalog: &FontCatalog,
    ukrainian_hyphenation: Option<&UkrainianHyphenation>,
) -> Result<TextLayoutResult, LayoutError> {
    if text.len() > MAX_TEXT_BYTES || text.contains(['\n', '\r']) {
        return Err(LayoutError::TextLimit);
    }
    if request.font_catalog_identity != catalog.identity() {
        return Err(LayoutError::InvalidRequest);
    }
    let font_size = LayoutUnit::from_millipoints(request.font_size_millipoints)?;
    let hyphenator = match request.language {
        TextLanguage::Ukrainian => {
            let hyphenator = ukrainian_hyphenation.ok_or(LayoutError::MissingHyphenationData)?;
            if request.hyphenation_data_identity.as_deref() != Some(hyphenator.identity()) {
                return Err(LayoutError::InvalidRequest);
            }
            Some(hyphenator)
        }
        TextLanguage::English => None,
    };

    let request_fingerprint = request.fingerprint()?;
    let source_text_hash = blake3::hash(text.as_bytes()).to_hex().to_string();
    let grapheme_boundaries = grapheme_boundaries(text);
    let directional_runs = directional_runs(text, request.direction)?;
    let mut glyph_runs = Vec::new();
    let mut unsafe_clusters = BTreeSet::new();

    for directional_run in directional_runs {
        let mut pending: Option<(usize, usize, String)> = None;
        for window in grapheme_boundaries.windows(2) {
            let start = window[0];
            let end = window[1];
            if start < directional_run.start || end > directional_run.end {
                continue;
            }
            let cluster = &text[start..end];
            let face = catalog
                .find_face(cluster)
                .ok_or(LayoutError::UnsupportedGlyph)?;
            let face_id = face.identity.id.clone();
            match pending.take() {
                Some((pending_start, pending_end, pending_face))
                    if pending_end == start && pending_face == face_id =>
                {
                    pending = Some((pending_start, end, face_id));
                }
                Some((pending_start, pending_end, pending_face)) => {
                    let shaped = shape_run(
                        text,
                        pending_start,
                        pending_end,
                        directional_run.direction,
                        &pending_face,
                        catalog,
                        font_size,
                        request.language,
                    )?;
                    unsafe_clusters.extend(
                        shaped
                            .glyphs
                            .iter()
                            .filter(|glyph| glyph.unsafe_to_break)
                            .map(|glyph| glyph.cluster_utf8),
                    );
                    glyph_runs.push(shaped);
                    pending = Some((start, end, face_id));
                }
                None => pending = Some((start, end, face_id)),
            }
        }
        if let Some((start, end, face_id)) = pending {
            let shaped = shape_run(
                text,
                start,
                end,
                directional_run.direction,
                &face_id,
                catalog,
                font_size,
                request.language,
            )?;
            unsafe_clusters.extend(
                shaped
                    .glyphs
                    .iter()
                    .filter(|glyph| glyph.unsafe_to_break)
                    .map(|glyph| glyph.cluster_utf8),
            );
            glyph_runs.push(shaped);
        }
    }

    let break_opportunities =
        collect_break_opportunities(text, &grapheme_boundaries, &unsafe_clusters, hyphenator)?;
    let lines = break_lines(
        text,
        request.width,
        &grapheme_boundaries,
        &break_opportunities,
        &glyph_runs,
    )?;
    let mut result = TextLayoutResult {
        request_fingerprint,
        source_text_hash,
        glyph_runs,
        break_opportunities,
        lines,
        result_hash: String::new(),
    };
    result.result_hash = result_hash(&result)?;
    Ok(result)
}

#[derive(Debug, Clone, Copy)]
struct DirectionalRun {
    start: usize,
    end: usize,
    direction: TextDirection,
}

fn directional_runs(
    text: &str,
    requested: TextDirection,
) -> Result<Vec<DirectionalRun>, LayoutError> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let default_level = match requested {
        TextDirection::Auto => None,
        TextDirection::LeftToRight => Some(LTR_LEVEL),
        TextDirection::RightToLeft => Some(RTL_LEVEL),
    };
    let bidi = BidiInfo::new(text, default_level);
    let paragraph = bidi.paragraphs.first().ok_or(LayoutError::InvalidRequest)?;
    let levels = bidi.reordered_levels_per_char(paragraph, paragraph.range.clone());
    let chars: Vec<(usize, usize)> = text
        .char_indices()
        .map(|(start, character)| (start, start + character.len_utf8()))
        .collect();
    if chars.len() != levels.len() {
        return Err(LayoutError::InvalidRequest);
    }
    let forced_uniform_direction = match requested {
        TextDirection::LeftToRight if levels.iter().all(|level| level.is_rtl()) => Some(false),
        TextDirection::RightToLeft if levels.iter().all(|level| !level.is_rtl()) => Some(true),
        _ => None,
    };
    let mut runs = Vec::new();
    let mut start_index = 0;
    let mut current_rtl = forced_uniform_direction.unwrap_or(levels[0].is_rtl());
    for (index, level) in levels.iter().enumerate().skip(1) {
        let level_rtl = forced_uniform_direction.unwrap_or(level.is_rtl());
        if level_rtl != current_rtl {
            runs.push(DirectionalRun {
                start: chars[start_index].0,
                end: chars[index - 1].1,
                direction: if current_rtl {
                    TextDirection::RightToLeft
                } else {
                    TextDirection::LeftToRight
                },
            });
            start_index = index;
            current_rtl = level_rtl;
        }
    }
    runs.push(DirectionalRun {
        start: chars[start_index].0,
        end: chars.last().ok_or(LayoutError::InvalidRequest)?.1,
        direction: if current_rtl {
            TextDirection::RightToLeft
        } else {
            TextDirection::LeftToRight
        },
    });
    Ok(runs)
}

fn grapheme_boundaries(text: &str) -> Vec<usize> {
    GraphemeClusterSegmenter::new().segment_str(text).collect()
}

#[allow(clippy::too_many_arguments)]
fn shape_run(
    text: &str,
    start: usize,
    end: usize,
    direction: TextDirection,
    face_id: &str,
    catalog: &FontCatalog,
    font_size: LayoutUnit,
    language: TextLanguage,
) -> Result<TextGlyphRun, LayoutError> {
    let face_record = catalog
        .faces
        .iter()
        .find(|face| face.identity.id == face_id)
        .ok_or(LayoutError::InvalidFont)?;
    let face = face_record.shape_face()?;
    let shape_direction = match direction {
        TextDirection::LeftToRight | TextDirection::Auto => rustybuzz::Direction::LeftToRight,
        TextDirection::RightToLeft => rustybuzz::Direction::RightToLeft,
    };
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.set_cluster_level(rustybuzz::BufferClusterLevel::MonotoneGraphemes);
    buffer.set_flags(rustybuzz::BufferFlags::PRODUCE_UNSAFE_TO_CONCAT);
    for (offset, character) in text[start..end].char_indices() {
        buffer.add(
            character,
            u32::try_from(start + offset).map_err(|_| LayoutError::TextLimit)?,
        );
    }
    buffer.guess_segment_properties();
    buffer.set_direction(shape_direction);
    buffer.set_language(
        rustybuzz::Language::from_str(language.language_tag())
            .map_err(|_| LayoutError::InvalidRequest)?,
    );
    let shaped = rustybuzz::shape(&face, &[], buffer);
    let units_per_em = u16::try_from(face.units_per_em()).map_err(|_| LayoutError::InvalidFont)?;
    let glyphs = shaped
        .glyph_infos()
        .iter()
        .zip(shaped.glyph_positions())
        .map(|(info, position)| {
            let cluster = usize::try_from(info.cluster).map_err(|_| LayoutError::TextLimit)?;
            let cluster_utf16 = utf16_offset(text, cluster)?;
            Ok(TextGlyph {
                glyph_id: info.glyph_id,
                cluster_utf8: info.cluster,
                cluster_utf16,
                x_advance: LayoutUnit::from_font_units(
                    position.x_advance,
                    units_per_em,
                    font_size,
                )?,
                x_offset: LayoutUnit::from_font_units(position.x_offset, units_per_em, font_size)?,
                y_offset: LayoutUnit::from_font_units(position.y_offset, units_per_em, font_size)?,
                unsafe_to_break: info.unsafe_to_break(),
            })
        })
        .collect::<Result<Vec<_>, LayoutError>>()?;
    Ok(TextGlyphRun {
        source: source_range(text, start, end)?,
        face_id: face_id.to_owned(),
        direction,
        glyphs,
    })
}

fn collect_break_opportunities(
    text: &str,
    grapheme_boundaries: &[usize],
    unsafe_clusters: &BTreeSet<u32>,
    hyphenator: Option<&UkrainianHyphenation>,
) -> Result<Vec<BreakOpportunity>, LayoutError> {
    let mut breaks = BTreeMap::<usize, BreakKind>::new();
    breaks.insert(0, BreakKind::Mandatory);
    let segmenter = LineSegmenter::new_for_non_complex_scripts(LineBreakOptions::default());
    for offset in segmenter.segment_str(text) {
        let kind = if offset == text.len()
            || text[..offset]
                .chars()
                .next_back()
                .is_some_and(|character| character == '\n' || character == '\r')
        {
            BreakKind::Mandatory
        } else {
            BreakKind::Unicode
        };
        breaks
            .entry(offset)
            .and_modify(|existing| *existing = stronger_break(*existing, kind))
            .or_insert(kind);
    }
    if let Some(hyphenator) = hyphenator {
        for (start, end) in word_ranges(text) {
            let word = &text[start..end];
            for relative in hyphenator.opportunities(word) {
                let offset = start.checked_add(relative).ok_or(LayoutError::TextLimit)?;
                breaks
                    .entry(offset)
                    .and_modify(|existing| {
                        *existing = stronger_break(*existing, BreakKind::UkrainianHyphenation)
                    })
                    .or_insert(BreakKind::UkrainianHyphenation);
            }
        }
    }
    let grapheme_set: BTreeSet<usize> = grapheme_boundaries.iter().copied().collect();
    breaks
        .into_iter()
        .map(|(offset, kind)| {
            let offset_u32 = u32::try_from(offset).map_err(|_| LayoutError::TextLimit)?;
            Ok(BreakOpportunity {
                offset_utf8: offset_u32,
                offset_utf16: utf16_offset(text, offset)?,
                kind,
                legal: grapheme_set.contains(&offset) && !unsafe_clusters.contains(&offset_u32),
            })
        })
        .collect()
}

fn stronger_break(existing: BreakKind, candidate: BreakKind) -> BreakKind {
    match (existing, candidate) {
        (BreakKind::Mandatory, _) | (_, BreakKind::Mandatory) => BreakKind::Mandatory,
        (BreakKind::UkrainianHyphenation, _) | (_, BreakKind::UkrainianHyphenation) => {
            BreakKind::UkrainianHyphenation
        }
        _ => BreakKind::Unicode,
    }
}

fn break_lines(
    text: &str,
    width: LayoutUnit,
    grapheme_boundaries: &[usize],
    opportunities: &[BreakOpportunity],
    glyph_runs: &[TextGlyphRun],
) -> Result<Vec<TextLine>, LayoutError> {
    if text.is_empty() {
        return Ok(vec![TextLine {
            source: source_range(text, 0, 0)?,
            width: LayoutUnit::default(),
            break_kind: BreakKind::Mandatory,
            discretionary_hyphen: false,
        }]);
    }
    let mut candidates: Vec<&BreakOpportunity> =
        opportunities.iter().filter(|item| item.legal).collect();
    candidates.sort_by_key(|item| item.offset_utf8);
    let mut lines = Vec::new();
    let mut line_start = 0_usize;
    let mut candidate_index = 0_usize;
    while line_start < text.len() {
        while candidate_index < candidates.len()
            && usize::try_from(candidates[candidate_index].offset_utf8)
                .map_err(|_| LayoutError::TextLimit)?
                <= line_start
        {
            candidate_index += 1;
        }
        let mut last_fit = None;
        let mut scan = candidate_index;
        while scan < candidates.len() {
            let candidate = candidates[scan];
            let end = usize::try_from(candidate.offset_utf8).map_err(|_| LayoutError::TextLimit)?;
            let candidate_width = width_between(glyph_runs, line_start, end)?;
            if candidate_width.raw() <= width.raw() {
                last_fit = Some((scan, candidate, candidate_width, end));
                scan += 1;
                continue;
            }
            break;
        }
        if let Some((fit_index, candidate, line_width, end)) = last_fit {
            lines.push(TextLine {
                source: source_range(text, line_start, end)?,
                width: line_width,
                break_kind: candidate.kind,
                discretionary_hyphen: candidate.kind == BreakKind::UkrainianHyphenation,
            });
            line_start = end;
            candidate_index = fit_index + 1;
            continue;
        }
        let forced_end = grapheme_boundaries
            .iter()
            .copied()
            .find(|boundary| *boundary > line_start)
            .unwrap_or(text.len());
        let line_width = width_between(glyph_runs, line_start, forced_end)?;
        lines.push(TextLine {
            source: source_range(text, line_start, forced_end)?,
            width: line_width,
            break_kind: BreakKind::Overflow,
            discretionary_hyphen: false,
        });
        line_start = forced_end;
        candidate_index = candidates
            .iter()
            .position(|candidate| usize::try_from(candidate.offset_utf8).unwrap_or(0) > line_start)
            .unwrap_or(candidates.len());
    }
    Ok(lines)
}

fn width_between(
    glyph_runs: &[TextGlyphRun],
    start: usize,
    end: usize,
) -> Result<LayoutUnit, LayoutError> {
    let mut width = LayoutUnit::default();
    for run in glyph_runs {
        for glyph in &run.glyphs {
            let cluster =
                usize::try_from(glyph.cluster_utf8).map_err(|_| LayoutError::TextLimit)?;
            if cluster >= start && cluster < end {
                width = width.checked_add(glyph.x_advance)?;
            }
        }
    }
    Ok(width)
}

fn word_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (offset, character) in text.char_indices() {
        if character.is_alphabetic() {
            start.get_or_insert(offset);
        } else if let Some(word_start) = start.take() {
            ranges.push((word_start, offset));
        }
    }
    if let Some(word_start) = start {
        ranges.push((word_start, text.len()));
    }
    ranges
}

fn source_range(text: &str, start: usize, end: usize) -> Result<SourceRange, LayoutError> {
    Ok(SourceRange {
        utf8_start: u32::try_from(start).map_err(|_| LayoutError::TextLimit)?,
        utf8_end: u32::try_from(end).map_err(|_| LayoutError::TextLimit)?,
        utf16_start: utf16_offset(text, start)?,
        utf16_end: utf16_offset(text, end)?,
    })
}

fn utf16_offset(text: &str, byte_offset: usize) -> Result<u32, LayoutError> {
    let prefix = text.get(..byte_offset).ok_or(LayoutError::InvalidRequest)?;
    u32::try_from(prefix.encode_utf16().count()).map_err(|_| LayoutError::TextLimit)
}

fn result_hash(result: &TextLayoutResult) -> Result<String, LayoutError> {
    serde_json::to_vec(&(
        &result.request_fingerprint,
        &result.source_text_hash,
        &result.glyph_runs,
        &result.break_opportunities,
        &result.lines,
    ))
    .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
    .map_err(|_| LayoutError::Serialization)
}

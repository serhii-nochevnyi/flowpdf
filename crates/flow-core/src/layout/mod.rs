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

use crate::{
    canonical::{canonical_bytes, canonical_hash},
    model::{
        Alignment, BlockKind, ContentNode, FlowDocument, HeaderFooterSettings, InlineRun, NodeId,
        PageOrientation, PageSettings, PageSize, SectionId,
    },
    schema::validate_document,
};

const ENGINE_VERSION: &str = "flowpdf-layout-text-0.1";
const LAYOUT_UNITS_PER_POINT: i64 = 64;
const MILLIPOINTS_PER_POINT: i64 = 1_000;
const MAX_FONT_FACES: usize = 16;
const MAX_FONT_FACE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 512 * 1024;
const MAX_REQUEST_HASH_BYTES: usize = 128;
const MAX_PAGINATION_PAGES: u32 = 2_048;
/// Version of the closed serialized layout contract consumed by the WASM
/// boundary. It is intentionally independent of the semantic schema version.
pub const LAYOUT_WASM_SCHEMA_VERSION: u32 = 1;
/// Aggregate serialized request budget. This covers canonical document JSON,
/// explicit font bytes, and the bounded request envelope before execution.
pub const MAX_LAYOUT_WASM_REQUEST_BYTES: usize = 96 * 1024 * 1024;
/// Aggregate explicit font/data budget admitted by one layout request.
pub const MAX_LAYOUT_WASM_FONT_DATA_BYTES: usize = 32 * 1024 * 1024;
/// Maximum serialized response budget. A response is rejected before it can
/// become an accepted worker result when the derived page tree is too large.
pub const MAX_LAYOUT_WASM_RESULT_BYTES: usize = 64 * 1024 * 1024;
const MAX_LAYOUT_WASM_REQUEST_ID_BYTES: usize = 128;
const MAX_LAYOUT_WASM_VIEWPORT_PAGES: u32 = 256;
const MAX_STATIC_LINES: usize = 64;
const DEFAULT_LINE_HEIGHT_NUMERATOR: u32 = 6;
const DEFAULT_LINE_HEIGHT_DENOMINATOR: u32 = 5;
const LIST_INDENT_MILLIPOINTS: u32 = 18_000;
const TABLE_CELL_PADDING_MILLIPOINTS: u32 = 2_000;
const IMAGE_PLACEHOLDER_WIDTH_MILLIMETRES: u32 = 80;
const IMAGE_PLACEHOLDER_HEIGHT_MILLIMETRES: u32 = 40;

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

    /// Converts whole millimetres to points through one checked rational
    /// boundary. PDF/page geometry is admitted only through this conversion;
    /// no floating-point value enters the paginator.
    pub fn from_millimetres(value: u32) -> Result<Self, LayoutError> {
        let numerator = i128::from(value)
            .checked_mul(72)
            .and_then(|value| value.checked_mul(i128::from(LAYOUT_UNITS_PER_POINT)))
            .and_then(|value| value.checked_mul(10))
            .ok_or(LayoutError::NumericOverflow)?;
        Self::from_ratio(numerator, 254)
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

    /// Subtracts two fixed-point values with overflow detection.
    pub fn checked_sub(self, other: Self) -> Result<Self, LayoutError> {
        self.0
            .checked_sub(other.0)
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

/// Stable identity for an admitted font face. The ordinary text-layout DTO
/// carries this identity only; the closed WASM ingress has a separate,
/// bounded byte field that is consumed into a request-local catalog and never
/// returned in derived results.
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

/// A fixed-point rectangle in page coordinates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutRect {
    pub x: LayoutUnit,
    pub y: LayoutUnit,
    pub width: LayoutUnit,
    pub height: LayoutUnit,
}

/// The semantic kind represented by one derived fragment.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum FragmentKind {
    Paragraph,
    Heading,
    Line,
    List,
    ListItem,
    Image,
    Table,
    TableRow,
    TableCell,
    PageBreak,
    Header,
    Footer,
    Unsupported,
}

/// Why a page or fragment boundary was selected. This is intentionally a
/// closed vocabulary so consumers cannot confuse a browser-specific reason
/// with an engine decision.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum BreakReason {
    DocumentStart,
    Natural,
    ExplicitPageBreak,
    SectionBoundary,
    KeepWithNext,
    WidowOrphanFallback,
    TableRow,
    OverflowFallback,
    UnsupportedFallback,
}

/// Stable diagnostics emitted when a representable source construct needs a
/// bounded fallback. Diagnostic fields never contain authored text or bytes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum LayoutDiagnosticCode {
    ConstraintFallback,
    HeaderFooterOverflow,
    OverflowFallback,
    UnsupportedNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutDiagnostic {
    pub code: LayoutDiagnosticCode,
    pub page_index: Option<u32>,
    pub source_node_id: Option<NodeId>,
    pub value: Option<u32>,
}

/// Immutable identity inputs for one pagination run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaginationRequest {
    pub source_revision: u32,
    pub source_hash: String,
    pub max_pages: u32,
}

impl PaginationRequest {
    pub fn new(
        source_revision: u32,
        source_hash: impl Into<String>,
        max_pages: u32,
    ) -> Result<Self, LayoutError> {
        let source_hash = source_hash.into();
        if source_hash.trim().is_empty()
            || source_hash.len() > MAX_REQUEST_HASH_BYTES
            || !(1..=MAX_PAGINATION_PAGES).contains(&max_pages)
        {
            return Err(LayoutError::PaginationRequestInvalid);
        }
        Ok(Self {
            source_revision,
            source_hash,
            max_pages,
        })
    }

    /// Creates a request bound to the exact canonical bytes that will be
    /// paginated.
    pub fn for_document(document: &FlowDocument) -> Result<Self, LayoutError> {
        let bytes = canonical_bytes(document).map_err(|_| LayoutError::InvalidDocument)?;
        Self::new(
            document.revision,
            canonical_hash(&bytes),
            MAX_PAGINATION_PAGES,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutFragment {
    pub id: String,
    pub kind: FragmentKind,
    pub source_node_id: Option<NodeId>,
    pub source: Option<SourceRange>,
    pub rect: LayoutRect,
    pub break_reason: Option<BreakReason>,
    pub derived: bool,
    pub repeat_index: u16,
    pub children: Vec<Self>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutPage {
    pub page_index: u32,
    pub section_id: SectionId,
    pub page_settings: PageSettings,
    pub bounds: LayoutRect,
    pub content_rect: LayoutRect,
    pub start_reason: BreakReason,
    pub header: Option<LayoutFragment>,
    pub footer: Option<LayoutFragment>,
    pub fragments: Vec<LayoutFragment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaginationResult {
    pub source_revision: u32,
    pub source_hash: String,
    pub layout_settings_fingerprint: String,
    pub font_catalog_identity: String,
    pub hyphenation_data_identity: Option<String>,
    pub pages: Vec<LayoutPage>,
    pub diagnostics: Vec<LayoutDiagnostic>,
    pub result_hash: String,
}

impl PaginationResult {
    #[must_use]
    pub fn result_hash(&self) -> &str {
        &self.result_hash
    }
}

/// One explicit font face admitted by the serialized WASM request. Bytes are
/// ingress-only data: they are consumed into a request-local `FontCatalog` and
/// are never copied into a layout result or diagnostic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutWasmFontInput {
    pub id: String,
    pub family: String,
    pub face_index: u32,
    pub bytes: Vec<u8>,
}

/// Explicit Ukrainian dictionary data admitted by the serialized WASM
/// request. Its identity is checked against the decoded bytes before layout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutWasmHyphenationInput {
    pub identity: String,
    pub bytes: Vec<u8>,
}

/// A bounded viewport hint. Pagination remains a full Rust-owned derivation;
/// the hint lets the worker request a bounded interactive window without
/// allowing an unbounded page-range value across the boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutWasmViewport {
    pub first_page: u32,
    pub page_count: u32,
}

/// Closed, versioned request DTO for the Rust/WASM layout boundary.
///
/// `canonical_json` is the only authored document payload admitted here. All
/// other fields are explicit identities, bounds, or request-local byte data;
/// no mutable Rust handle, system-font lookup, or browser layout object is
/// represented by this contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutWasmRequest {
    pub schema_version: u32,
    pub request_id: String,
    pub canonical_json: String,
    pub source_revision: u32,
    pub source_hash: String,
    pub max_pages: u32,
    pub viewport: LayoutWasmViewport,
    pub font_catalog_identity: String,
    pub hyphenation_data_identity: Option<String>,
    pub expected_layout_settings_fingerprint: Option<String>,
    pub fonts: Vec<LayoutWasmFontInput>,
    pub ukrainian_hyphenation: Option<LayoutWasmHyphenationInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutWasmError {
    pub code: String,
}

/// Closed, privacy-safe response DTO. The nested result contains only derived
/// page/fragment geometry, source ranges, stable IDs, bounded numeric context,
/// and allowlisted diagnostic codes. Authored text, font bytes, and system
/// state are deliberately absent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutWasmResponse {
    pub schema_version: u32,
    pub request_id: String,
    pub ok: bool,
    pub source_revision: Option<u32>,
    pub source_hash: Option<String>,
    pub layout_settings_fingerprint: Option<String>,
    pub font_catalog_identity: Option<String>,
    pub hyphenation_data_identity: Option<String>,
    pub result_hash: Option<String>,
    pub result: Option<PaginationResult>,
    pub error: Option<LayoutWasmError>,
}

impl LayoutWasmResponse {
    fn failure(request_id: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            schema_version: LAYOUT_WASM_SCHEMA_VERSION,
            request_id: request_id.into(),
            ok: false,
            source_revision: None,
            source_hash: None,
            layout_settings_fingerprint: None,
            font_catalog_identity: None,
            hyphenation_data_identity: None,
            result_hash: None,
            result: None,
            error: Some(LayoutWasmError { code: code.into() }),
        }
    }

    fn success(request_id: String, result: PaginationResult) -> Self {
        Self {
            schema_version: LAYOUT_WASM_SCHEMA_VERSION,
            request_id,
            ok: true,
            source_revision: Some(result.source_revision),
            source_hash: Some(result.source_hash.clone()),
            layout_settings_fingerprint: Some(result.layout_settings_fingerprint.clone()),
            font_catalog_identity: Some(result.font_catalog_identity.clone()),
            hyphenation_data_identity: result.hyphenation_data_identity.clone(),
            result_hash: Some(result.result_hash.clone()),
            result: Some(result),
            error: None,
        }
    }
}

/// Executes one serialized, immutable layout request. Every failure is
/// represented by a stable code and no partial page tree is returned.
pub fn execute_layout_wasm_json(input: &str) -> LayoutWasmResponse {
    if input.len() > MAX_LAYOUT_WASM_REQUEST_BYTES {
        return LayoutWasmResponse::failure("", "FLOW_LAYOUT_WASM_REQUEST_SIZE_LIMIT");
    }

    let request = match serde_json::from_str::<LayoutWasmRequest>(input) {
        Ok(request) => request,
        Err(_) => return LayoutWasmResponse::failure("", "FLOW_LAYOUT_WASM_REQUEST_DECODE"),
    };
    if request.schema_version != LAYOUT_WASM_SCHEMA_VERSION {
        return LayoutWasmResponse::failure(
            request.request_id,
            "FLOW_LAYOUT_WASM_VERSION_UNSUPPORTED",
        );
    }
    let request_id = request.request_id.clone();
    if request_id.trim().is_empty() || request_id.len() > MAX_LAYOUT_WASM_REQUEST_ID_BYTES {
        return LayoutWasmResponse::failure(request_id, "FLOW_LAYOUT_WASM_REQUEST_ID_INVALID");
    }
    if request.font_catalog_identity.trim().is_empty()
        || request
            .expected_layout_settings_fingerprint
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
    {
        return LayoutWasmResponse::failure(request_id, "FLOW_LAYOUT_WASM_REQUEST_INVALID");
    }
    if request.viewport.page_count == 0
        || request.viewport.page_count > MAX_LAYOUT_WASM_VIEWPORT_PAGES
        || request.viewport.first_page >= request.max_pages
        || request
            .viewport
            .first_page
            .checked_add(request.viewport.page_count)
            .is_none_or(|end| end > request.max_pages)
    {
        return LayoutWasmResponse::failure(request_id, "FLOW_LAYOUT_WASM_VIEWPORT_INVALID");
    }

    let mut explicit_data_bytes = 0_usize;
    for bytes in request.fonts.iter().map(|font| font.bytes.len()) {
        explicit_data_bytes = match explicit_data_bytes.checked_add(bytes) {
            Some(total) if total <= MAX_LAYOUT_WASM_FONT_DATA_BYTES => total,
            _ => {
                return LayoutWasmResponse::failure(request_id, "FLOW_LAYOUT_WASM_FONT_DATA_LIMIT");
            }
        };
    }
    if let Some(data) = request.ukrainian_hyphenation.as_ref() {
        explicit_data_bytes = match explicit_data_bytes.checked_add(data.bytes.len()) {
            Some(total) if total <= MAX_LAYOUT_WASM_FONT_DATA_BYTES => total,
            _ => {
                return LayoutWasmResponse::failure(request_id, "FLOW_LAYOUT_WASM_FONT_DATA_LIMIT");
            }
        };
    }
    if explicit_data_bytes > MAX_LAYOUT_WASM_FONT_DATA_BYTES {
        return LayoutWasmResponse::failure(request_id, "FLOW_LAYOUT_WASM_FONT_DATA_LIMIT");
    }

    let canonical_bytes = request.canonical_json.as_bytes();
    if crate::canonical::preflight_canonical_bytes(canonical_bytes).is_err() {
        return LayoutWasmResponse::failure(request_id, "FLOW_LAYOUT_WASM_DOCUMENT_INVALID");
    }
    let document = match crate::canonical::decode_canonical(canonical_bytes) {
        Ok(document) => document,
        Err(_) => {
            return LayoutWasmResponse::failure(request_id, "FLOW_LAYOUT_WASM_DOCUMENT_INVALID");
        }
    };

    let faces = match request
        .fonts
        .into_iter()
        .map(|font| FontFace::new(font.id, font.family, font.face_index, font.bytes))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(faces) => faces,
        Err(error) => return LayoutWasmResponse::failure(request_id, error.code()),
    };
    let catalog = match FontCatalog::new(faces) {
        Ok(catalog) => catalog,
        Err(error) => return LayoutWasmResponse::failure(request_id, error.code()),
    };
    if catalog.identity() != request.font_catalog_identity {
        return LayoutWasmResponse::failure(request_id, "FLOW_LAYOUT_WASM_IDENTITY_MISMATCH");
    }

    let hyphenation = match request.ukrainian_hyphenation {
        Some(data) => match UkrainianHyphenation::from_bincode(data.identity, &data.bytes) {
            Ok(data) => Some(data),
            Err(error) => return LayoutWasmResponse::failure(request_id, error.code()),
        },
        None => None,
    };
    let actual_hyphenation_identity = hyphenation.as_ref().map(|data| data.identity().to_owned());
    if actual_hyphenation_identity != request.hyphenation_data_identity {
        return LayoutWasmResponse::failure(request_id, "FLOW_LAYOUT_WASM_IDENTITY_MISMATCH");
    }

    let pagination_request = match PaginationRequest::new(
        request.source_revision,
        request.source_hash,
        request.max_pages,
    ) {
        Ok(request) => request,
        Err(error) => return LayoutWasmResponse::failure(request_id, error.code()),
    };
    let result = match paginate_document(
        &document,
        &pagination_request,
        &catalog,
        hyphenation.as_ref(),
    ) {
        Ok(result) => result,
        Err(error) => return LayoutWasmResponse::failure(request_id, error.code()),
    };
    if request
        .expected_layout_settings_fingerprint
        .is_some_and(|expected| expected != result.layout_settings_fingerprint)
    {
        return LayoutWasmResponse::failure(request_id, "FLOW_LAYOUT_WASM_IDENTITY_MISMATCH");
    }

    let response = LayoutWasmResponse::success(request_id, result);
    let serialized_size = serde_json::to_vec(&response)
        .map(|bytes| bytes.len())
        .unwrap_or(MAX_LAYOUT_WASM_RESULT_BYTES.saturating_add(1));
    if serialized_size > MAX_LAYOUT_WASM_RESULT_BYTES {
        return LayoutWasmResponse::failure(
            response.request_id,
            "FLOW_LAYOUT_WASM_RESULT_SIZE_LIMIT",
        );
    }
    response
}

/// Serializes one boundary response without exposing a serializer-specific
/// `JsValue` shape to the Rust core.
pub fn layout_wasm_response_json(input: &str) -> String {
    serde_json::to_string(&execute_layout_wasm_json(input))
        .expect("the closed layout response must serialize")
}

/// Recomputes the Rust-owned result hash for a complete response. The worker
/// uses this narrow boolean check before an otherwise valid result can be
/// published; malformed or partial responses fail closed.
pub fn verify_layout_wasm_response_json(input: &str) -> bool {
    let Ok(response) = serde_json::from_str::<LayoutWasmResponse>(input) else {
        return false;
    };
    if response.schema_version != LAYOUT_WASM_SCHEMA_VERSION
        || !response.ok
        || response.error.is_some()
        || response.result.is_none()
    {
        return false;
    }
    let Some(result) = response.result else {
        return false;
    };
    let Ok(expected_hash) = pagination_result_hash(&result) else {
        return false;
    };
    response.result_hash.as_deref() == Some(expected_hash.as_str())
        && result.result_hash == expected_hash
        && response.source_revision == Some(result.source_revision)
        && response.source_hash.as_deref() == Some(result.source_hash.as_str())
        && response.layout_settings_fingerprint.as_deref()
            == Some(result.layout_settings_fingerprint.as_str())
        && response.font_catalog_identity.as_deref() == Some(result.font_catalog_identity.as_str())
        && response.hyphenation_data_identity == result.hyphenation_data_identity
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
    #[error("The canonical document is invalid for layout")]
    InvalidDocument,
    #[error("The pagination request is invalid")]
    PaginationRequestInvalid,
    #[error("The pagination page limit was exceeded")]
    PaginationPageLimit,
    #[error("The page geometry cannot produce a usable layout area")]
    InvalidPaginationGeometry,
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
            Self::InvalidDocument => "FLOW_LAYOUT_DOCUMENT_INVALID",
            Self::PaginationRequestInvalid => "FLOW_LAYOUT_PAGINATION_REQUEST_INVALID",
            Self::PaginationPageLimit => "FLOW_LAYOUT_PAGINATION_PAGE_LIMIT",
            Self::InvalidPaginationGeometry => "FLOW_LAYOUT_PAGINATION_GEOMETRY_INVALID",
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
    let advances = AdvancePrefix::new(glyph_runs)?;
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
            let candidate_width = advances.width_between(line_start, end)?;
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
        let line_width = advances.width_between(line_start, forced_end)?;
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

struct AdvancePrefix {
    points: Vec<(usize, LayoutUnit)>,
}

impl AdvancePrefix {
    fn new(glyph_runs: &[TextGlyphRun]) -> Result<Self, LayoutError> {
        let mut contributions = BTreeMap::<usize, LayoutUnit>::new();
        for run in glyph_runs {
            for glyph in &run.glyphs {
                let cluster =
                    usize::try_from(glyph.cluster_utf8).map_err(|_| LayoutError::TextLimit)?;
                let current = contributions.get(&cluster).copied().unwrap_or_default();
                contributions.insert(cluster, current.checked_add(glyph.x_advance)?);
            }
        }
        let mut total = LayoutUnit::default();
        let mut points = Vec::with_capacity(contributions.len());
        for (cluster, advance) in contributions {
            total = total.checked_add(advance)?;
            points.push((cluster, total));
        }
        Ok(Self { points })
    }

    fn width_between(&self, start: usize, end: usize) -> Result<LayoutUnit, LayoutError> {
        let before_start = self.cumulative_before(start);
        let before_end = self.cumulative_before(end);
        before_end.checked_sub(before_start)
    }

    fn cumulative_before(&self, offset: usize) -> LayoutUnit {
        let mut low = 0_usize;
        let mut high = self.points.len();
        while low < high {
            let middle = low + (high - low) / 2;
            if self.points[middle].0 < offset {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low.checked_sub(1)
            .and_then(|index| self.points.get(index).map(|(_, total)| *total))
            .unwrap_or_default()
    }
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

/// Paginates a validated canonical document into a deterministic derived page
/// tree. The document is borrowed and never mutated; all output is bound to
/// the caller's canonical revision/hash and explicit font/data identities.
pub fn paginate_document(
    document: &FlowDocument,
    request: &PaginationRequest,
    catalog: &FontCatalog,
    ukrainian_hyphenation: Option<&UkrainianHyphenation>,
) -> Result<PaginationResult, LayoutError> {
    if request.source_revision != document.revision {
        return Err(LayoutError::PaginationRequestInvalid);
    }
    validate_document(document).map_err(|_| LayoutError::InvalidDocument)?;
    let canonical = canonical_bytes(document).map_err(|_| LayoutError::InvalidDocument)?;
    if canonical_hash(&canonical) != request.source_hash {
        return Err(LayoutError::PaginationRequestInvalid);
    }
    if document.sections.is_empty() {
        return Err(LayoutError::InvalidDocument);
    }
    let layout_settings_fingerprint = serde_json::to_vec(&(
        &document.page_settings,
        &document.sections,
        request.source_revision,
    ))
    .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
    .map_err(|_| LayoutError::Serialization)?;

    let mut paginator = Paginator::new(document, request, catalog, ukrainian_hyphenation);
    paginator.run()?;
    let mut result = PaginationResult {
        source_revision: request.source_revision,
        source_hash: request.source_hash.clone(),
        layout_settings_fingerprint,
        font_catalog_identity: catalog.identity().to_owned(),
        hyphenation_data_identity: ukrainian_hyphenation.map(|data| data.identity().to_owned()),
        pages: paginator.pages,
        diagnostics: paginator.diagnostics,
        result_hash: String::new(),
    };
    result.result_hash = pagination_result_hash(&result)?;
    Ok(result)
}

/// Result of the revision-safe incremental entry point. The current Phase 3
/// implementation intentionally uses the full paginator as its oracle for
/// every changed revision; only a verified exact no-op cache is reused. This
/// makes the reuse boundary observable and fail-closed before prefix/suffix
/// splicing is introduced in a later optimization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IncrementalPaginationResult {
    pub result: PaginationResult,
    pub invalidation: crate::invalidation::InvalidationPlan,
    pub reused: bool,
    pub recomputed_from: usize,
}

pub fn paginate_incremental_document(
    document: &FlowDocument,
    request: &PaginationRequest,
    catalog: &FontCatalog,
    ukrainian_hyphenation: Option<&UkrainianHyphenation>,
    cache: Option<&crate::invalidation::IncrementalLayoutCache>,
    changes: &crate::invalidation::ChangeSet,
) -> Result<IncrementalPaginationResult, LayoutError> {
    if request.source_revision != document.revision {
        return Err(LayoutError::PaginationRequestInvalid);
    }
    let canonical = canonical_bytes(document).map_err(|_| LayoutError::InvalidDocument)?;
    if canonical_hash(&canonical) != request.source_hash {
        return Err(LayoutError::PaginationRequestInvalid);
    }
    let invalidation = crate::invalidation::plan_for_document(document, changes);
    if invalidation.reason == crate::invalidation::InvalidationReason::NoChanges
        && cache.is_some_and(|cache| cache.matches(request, catalog, ukrainian_hyphenation))
    {
        let result = cache.ok_or(LayoutError::InvalidDocument)?.result.clone();
        return Ok(IncrementalPaginationResult {
            result,
            invalidation,
            reused: true,
            recomputed_from: document.content.len(),
        });
    }

    let result = paginate_document(document, request, catalog, ukrainian_hyphenation)?;
    Ok(IncrementalPaginationResult {
        result,
        recomputed_from: invalidation.earliest_boundary,
        invalidation,
        reused: false,
    })
}

#[derive(Debug, Clone, Copy)]
struct PageMetrics {
    content_left: LayoutUnit,
    content_width: LayoutUnit,
    content_top: LayoutUnit,
    content_bottom: LayoutUnit,
}

#[derive(Debug, Clone)]
struct TextBlockPlan {
    result: TextLayoutResult,
    line_height: LayoutUnit,
    spacing_before: LayoutUnit,
    spacing_after: LayoutUnit,
    alignment: Alignment,
}

#[derive(Debug, Clone)]
struct TableCellPlan {
    node_id: NodeId,
    result: TextLayoutResult,
    line_height: LayoutUnit,
}

#[derive(Debug, Clone)]
struct TableRowPlan {
    cells: Vec<TableCellPlan>,
    column_width: LayoutUnit,
    height: LayoutUnit,
}

struct Paginator<'a> {
    document: &'a FlowDocument,
    request: &'a PaginationRequest,
    catalog: &'a FontCatalog,
    ukrainian_hyphenation: Option<&'a UkrainianHyphenation>,
    pages: Vec<LayoutPage>,
    diagnostics: Vec<LayoutDiagnostic>,
    current_section_index: Option<usize>,
    metrics: Option<PageMetrics>,
    cursor_y: LayoutUnit,
}

impl<'a> Paginator<'a> {
    fn new(
        document: &'a FlowDocument,
        request: &'a PaginationRequest,
        catalog: &'a FontCatalog,
        ukrainian_hyphenation: Option<&'a UkrainianHyphenation>,
    ) -> Self {
        Self {
            document,
            request,
            catalog,
            ukrainian_hyphenation,
            pages: Vec::new(),
            diagnostics: Vec::new(),
            current_section_index: None,
            metrics: None,
            cursor_y: LayoutUnit::default(),
        }
    }

    fn run(&mut self) -> Result<(), LayoutError> {
        for index in 0..self.document.content.len() {
            let node = self.document.content[index].clone();
            let section_index = self.section_for_node(&node);
            if self.current_section_index != Some(section_index) {
                let reason = if self.pages.is_empty() {
                    BreakReason::DocumentStart
                } else {
                    BreakReason::SectionBoundary
                };
                self.start_page(section_index, reason)?;
            }
            if let Some(next) = self.document.content.get(index + 1)
                && matches!(node.body, BlockKind::Heading { .. })
                && self.current_has_content()
            {
                let required = self
                    .estimate_node_height(&node, 0)?
                    .checked_add(self.estimate_node_height(next, 0)?)?;
                if required > self.remaining_height()? {
                    self.start_current_section_page(BreakReason::KeepWithNext)?;
                }
            }
            self.place_node(&node, 0)?;
        }
        if self.pages.is_empty() {
            self.start_page(0, BreakReason::DocumentStart)?;
        }
        Ok(())
    }

    fn section_for_node(&self, node: &ContentNode) -> usize {
        self.document
            .sections
            .iter()
            .enumerate()
            .find_map(|(index, section)| {
                (section.start_node_id.as_ref() == Some(&node.id)).then_some(index)
            })
            .or(self.current_section_index)
            .unwrap_or(0)
    }

    fn start_current_section_page(&mut self, reason: BreakReason) -> Result<(), LayoutError> {
        let section_index = self
            .current_section_index
            .ok_or(LayoutError::InvalidDocument)?;
        self.start_page(section_index, reason)
    }

    fn start_page(&mut self, section_index: usize, reason: BreakReason) -> Result<(), LayoutError> {
        if self.pages.len() as u32 >= self.request.max_pages {
            return Err(LayoutError::PaginationPageLimit);
        }
        let section = self
            .document
            .sections
            .get(section_index)
            .cloned()
            .ok_or(LayoutError::InvalidDocument)?;
        let page_index =
            u32::try_from(self.pages.len()).map_err(|_| LayoutError::PaginationPageLimit)?;
        let (width, height) = page_dimensions(&section.page_settings)?;
        let left = LayoutUnit::from_millimetres(u32::from(
            section.page_settings.margins_millimetres.left,
        ))?;
        let right = LayoutUnit::from_millimetres(u32::from(
            section.page_settings.margins_millimetres.right,
        ))?;
        let top =
            LayoutUnit::from_millimetres(u32::from(section.page_settings.margins_millimetres.top))?;
        let bottom = LayoutUnit::from_millimetres(u32::from(
            section.page_settings.margins_millimetres.bottom,
        ))?;
        let content_width = width.checked_sub(left)?.checked_sub(right)?;
        if content_width.raw() <= 0 {
            return Err(LayoutError::InvalidPaginationGeometry);
        }

        let header = self.static_band(
            &section.header,
            FragmentKind::Header,
            page_index,
            left,
            top,
            content_width,
            height,
        )?;
        let footer_height = self.static_band(
            &section.footer,
            FragmentKind::Footer,
            page_index,
            left,
            height.checked_sub(bottom)?,
            content_width,
            height,
        )?;
        let footer = footer_height
            .as_ref()
            .map(|(fragment, _, _)| fragment.clone());
        let header = header.map(|(fragment, height, overflow)| {
            if overflow {
                self.diagnostics.push(LayoutDiagnostic {
                    code: LayoutDiagnosticCode::HeaderFooterOverflow,
                    page_index: Some(page_index),
                    source_node_id: None,
                    value: Some(height.raw().unsigned_abs().min(u64::from(u32::MAX)) as u32),
                });
            }
            fragment
        });
        if let Some((_, height, overflow)) = footer_height.as_ref()
            && *overflow
        {
            self.diagnostics.push(LayoutDiagnostic {
                code: LayoutDiagnosticCode::HeaderFooterOverflow,
                page_index: Some(page_index),
                source_node_id: None,
                value: Some(height.raw().unsigned_abs().min(u64::from(u32::MAX)) as u32),
            });
        }

        let header_extent = header
            .as_ref()
            .map(|fragment| fragment.rect.height)
            .unwrap_or_default()
            .checked_add(if header.is_some() {
                LayoutUnit::from_millimetres(u32::from(section.header.distance_millimetres))?
            } else {
                LayoutUnit::default()
            })?;
        let footer_extent = footer
            .as_ref()
            .map(|fragment| fragment.rect.height)
            .unwrap_or_default()
            .checked_add(if footer.is_some() {
                LayoutUnit::from_millimetres(u32::from(section.footer.distance_millimetres))?
            } else {
                LayoutUnit::default()
            })?;
        let content_top = top.checked_add(header_extent)?;
        let content_bottom = height.checked_sub(bottom)?.checked_sub(footer_extent)?;
        if content_top >= content_bottom {
            return Err(LayoutError::InvalidPaginationGeometry);
        }
        let metrics = PageMetrics {
            content_left: left,
            content_width,
            content_top,
            content_bottom,
        };
        let content_rect = LayoutRect {
            x: left,
            y: content_top,
            width: content_width,
            height: content_bottom.checked_sub(content_top)?,
        };
        self.pages.push(LayoutPage {
            page_index,
            section_id: section.id,
            page_settings: section.page_settings,
            bounds: LayoutRect {
                x: LayoutUnit::default(),
                y: LayoutUnit::default(),
                width,
                height,
            },
            content_rect,
            start_reason: reason,
            header,
            footer,
            fragments: Vec::new(),
        });
        self.current_section_index = Some(section_index);
        self.metrics = Some(metrics);
        self.cursor_y = content_top;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn static_band(
        &self,
        settings: &HeaderFooterSettings,
        kind: FragmentKind,
        page_index: u32,
        x: LayoutUnit,
        fallback_y: LayoutUnit,
        width: LayoutUnit,
        page_height: LayoutUnit,
    ) -> Result<Option<(LayoutFragment, LayoutUnit, bool)>, LayoutError> {
        if !settings.enabled {
            return Ok(None);
        }
        let text = settings
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        let font_size = settings
            .runs
            .iter()
            .map(|run| run.style.font_size_millipoints)
            .max()
            .unwrap_or(12_000);
        let language = language_for_locale(&settings.locale, &[]);
        let (result, line_height) =
            self.layout_text_value(&text, width, font_size, language, settings.locale.as_str())?;
        let normal_height = multiply_units(line_height, result.lines.len())?;
        let overflow = result.lines.len() > MAX_STATIC_LINES || normal_height > page_height;
        let height = if overflow { line_height } else { normal_height };
        let y = match kind {
            FragmentKind::Header => fallback_y,
            FragmentKind::Footer => fallback_y.checked_sub(height)?,
            _ => fallback_y,
        };
        let source = source_range(&text, 0, text.len())?;
        let mut children = Vec::new();
        if !overflow {
            for (index, line) in result.lines.iter().enumerate() {
                children.push(LayoutFragment {
                    id: format!(
                        "flow-fragment:{}:{}:{}:{}:{}",
                        fragment_kind_tag(kind),
                        page_index,
                        line.source.utf8_start,
                        line.source.utf8_end,
                        index
                    ),
                    kind: FragmentKind::Line,
                    source_node_id: None,
                    source: Some(line.source),
                    rect: LayoutRect {
                        x,
                        y: y.checked_add(multiply_units(line_height, index)?)?,
                        width: line.width,
                        height: line_height,
                    },
                    break_reason: Some(line_break_reason(line.break_kind)),
                    derived: true,
                    repeat_index: u16::try_from(page_index).unwrap_or(u16::MAX),
                    children: Vec::new(),
                });
            }
        }
        Ok(Some((
            LayoutFragment {
                id: format!(
                    "flow-fragment:{}:{}:0:{}:{}",
                    fragment_kind_tag(kind),
                    page_index,
                    source.utf8_end,
                    u16::try_from(page_index).unwrap_or(u16::MAX)
                ),
                kind,
                source_node_id: None,
                source: Some(source),
                rect: LayoutRect {
                    x,
                    y,
                    width,
                    height,
                },
                break_reason: overflow.then_some(BreakReason::OverflowFallback),
                derived: true,
                repeat_index: u16::try_from(page_index).unwrap_or(u16::MAX),
                children,
            },
            height,
            overflow,
        )))
    }

    fn place_node(&mut self, node: &ContentNode, depth: usize) -> Result<(), LayoutError> {
        match &node.body {
            BlockKind::Paragraph { .. } => {
                self.place_text_node(node, depth, FragmentKind::Paragraph)
            }
            BlockKind::Heading { .. } => self.place_text_node(node, depth, FragmentKind::Heading),
            BlockKind::OrderedList { items } | BlockKind::UnorderedList { items } => {
                self.place_marker(node, FragmentKind::List)?;
                self.place_flow_nodes(items, depth + 1)
            }
            BlockKind::ListItem { children } => {
                self.place_marker(node, FragmentKind::ListItem)?;
                self.place_flow_nodes(children, depth)
            }
            BlockKind::Image { .. } => self.place_image(node, depth),
            BlockKind::Table { header_rows, rows } => {
                self.place_table(node, *header_rows, rows, depth)
            }
            BlockKind::PageBreak => self.place_page_break(node),
            BlockKind::TableRow { .. } | BlockKind::TableCell { .. } => {
                self.place_unsupported(node)
            }
        }
    }

    fn place_flow_nodes(&mut self, nodes: &[ContentNode], depth: usize) -> Result<(), LayoutError> {
        for index in 0..nodes.len() {
            let node = &nodes[index];
            if let Some(next) = nodes.get(index + 1)
                && matches!(node.body, BlockKind::Heading { .. })
                && self.current_has_content()
            {
                let needed = self
                    .estimate_node_height(node, depth)?
                    .checked_add(self.estimate_node_height(next, depth)?)?;
                if needed > self.remaining_height()? {
                    self.start_current_section_page(BreakReason::KeepWithNext)?;
                }
            }
            self.place_node(node, depth)?;
        }
        Ok(())
    }

    fn place_text_node(
        &mut self,
        node: &ContentNode,
        depth: usize,
        kind: FragmentKind,
    ) -> Result<(), LayoutError> {
        let plan = self.text_plan(node, depth)?;
        if plan.spacing_before.raw() > 0 {
            let desired = self.cursor_y.checked_add(plan.spacing_before)?;
            if desired.checked_add(plan.line_height)? > self.content_bottom()?
                && self.current_has_content()
            {
                self.start_current_section_page(BreakReason::Natural)?;
            } else {
                self.cursor_y = desired;
            }
        }

        let mut line_index = 0_usize;
        let mut first_part = true;
        while line_index < plan.result.lines.len() {
            let available_lines = self.available_lines(plan.line_height)?;
            let remaining_lines = plan.result.lines.len() - line_index;
            if available_lines == 0 && self.current_has_content() {
                self.start_current_section_page(BreakReason::Natural)?;
                continue;
            }
            if available_lines == 1
                && remaining_lines > 1
                && plan.result.lines.len() > 2
                && self.current_has_content()
            {
                self.diagnostics.push(LayoutDiagnostic {
                    code: LayoutDiagnosticCode::ConstraintFallback,
                    page_index: Some(self.current_page_index()?),
                    source_node_id: Some(node.id.clone()),
                    value: Some(u32::try_from(remaining_lines).unwrap_or(u32::MAX)),
                });
                self.start_current_section_page(BreakReason::WidowOrphanFallback)?;
                continue;
            }
            let overflow = available_lines == 0;
            let take = if overflow {
                1
            } else {
                available_lines.min(remaining_lines)
            };
            let first_line = &plan.result.lines[line_index];
            let last_line = &plan.result.lines[line_index + take - 1];
            let block_source = SourceRange {
                utf8_start: first_line.source.utf8_start,
                utf8_end: last_line.source.utf8_end,
                utf16_start: first_line.source.utf16_start,
                utf16_end: last_line.source.utf16_end,
            };
            let x = self.content_left_for_depth(depth)?;
            let available_width = self.content_width_for_depth(depth)?;
            let mut children = Vec::with_capacity(take);
            for offset in 0..take {
                let line = &plan.result.lines[line_index + offset];
                let line_x = aligned_x(x, available_width, line.width, &plan.alignment)?;
                children.push(LayoutFragment {
                    id: format!(
                        "flow-fragment:{}:{}:{}:{}:{}",
                        fragment_kind_tag(FragmentKind::Line),
                        node.id,
                        line.source.utf8_start,
                        line.source.utf8_end,
                        self.current_page_index()?
                    ),
                    kind: FragmentKind::Line,
                    source_node_id: Some(node.id.clone()),
                    source: Some(line.source),
                    rect: LayoutRect {
                        x: line_x,
                        y: self
                            .cursor_y
                            .checked_add(multiply_units(plan.line_height, offset)?)?,
                        width: line.width,
                        height: plan.line_height,
                    },
                    break_reason: Some(line_break_reason(line.break_kind)),
                    derived: false,
                    repeat_index: 0,
                    children: Vec::new(),
                });
            }
            let height = multiply_units(plan.line_height, take)?;
            let break_reason = if overflow {
                BreakReason::OverflowFallback
            } else if first_part {
                self.current_page()?.start_reason
            } else {
                BreakReason::Natural
            };
            let fragment = LayoutFragment {
                id: format!(
                    "flow-fragment:{}:{}:{}:{}:{}",
                    fragment_kind_tag(kind),
                    node.id,
                    block_source.utf8_start,
                    block_source.utf8_end,
                    self.current_page_index()?
                ),
                kind,
                source_node_id: Some(node.id.clone()),
                source: Some(block_source),
                rect: LayoutRect {
                    x,
                    y: self.cursor_y,
                    width: available_width,
                    height,
                },
                break_reason: Some(break_reason),
                derived: false,
                repeat_index: 0,
                children,
            };
            self.push_fragment(fragment);
            self.cursor_y = self.cursor_y.checked_add(height)?;
            if overflow {
                self.cursor_y = self.content_bottom()?;
                self.diagnostics.push(LayoutDiagnostic {
                    code: LayoutDiagnosticCode::OverflowFallback,
                    page_index: Some(self.current_page_index()?),
                    source_node_id: Some(node.id.clone()),
                    value: Some(
                        plan.line_height
                            .raw()
                            .unsigned_abs()
                            .min(u64::from(u32::MAX)) as u32,
                    ),
                });
            }
            line_index += take;
            first_part = false;
        }
        let after = self.cursor_y.checked_add(plan.spacing_after)?;
        self.cursor_y = after.min(self.content_bottom()?);
        Ok(())
    }

    fn place_image(&mut self, node: &ContentNode, depth: usize) -> Result<(), LayoutError> {
        let width = LayoutUnit::from_millimetres(IMAGE_PLACEHOLDER_WIDTH_MILLIMETRES)?;
        let height = LayoutUnit::from_millimetres(IMAGE_PLACEHOLDER_HEIGHT_MILLIMETRES)?;
        let available_width = self.content_width_for_depth(depth)?;
        let width = width.min(available_width);
        let height = if width == LayoutUnit::from_millimetres(IMAGE_PLACEHOLDER_WIDTH_MILLIMETRES)?
        {
            height
        } else {
            multiply_units(height, 1)?
        };
        self.place_fixed_block(
            node,
            FragmentKind::Image,
            depth,
            width,
            height,
            BreakReason::Natural,
        )
    }

    fn place_table(
        &mut self,
        node: &ContentNode,
        header_rows: u8,
        rows: &[ContentNode],
        depth: usize,
    ) -> Result<(), LayoutError> {
        self.place_marker(node, FragmentKind::Table)?;
        let table_width = self.content_width_for_depth(depth)?;
        let header = (header_rows == 1).then(|| rows[0].clone());
        for (index, row) in rows.iter().enumerate() {
            let plan = self.table_row_plan(row, table_width)?;
            let fits = plan.height <= self.remaining_height()?;
            if !fits && self.current_has_content() {
                self.start_current_section_page(BreakReason::TableRow)?;
                if index > 0
                    && let Some(header_row) = header.as_ref()
                {
                    let header_plan = self.table_row_plan(header_row, table_width)?;
                    self.place_table_row(header_row, &header_plan, true)?;
                }
            }
            self.place_table_row(row, &plan, false)?;
        }
        Ok(())
    }

    fn place_table_row(
        &mut self,
        row: &ContentNode,
        plan: &TableRowPlan,
        derived: bool,
    ) -> Result<(), LayoutError> {
        let overflow = plan.height > self.content_rect_height()?;
        if overflow {
            self.diagnostics.push(LayoutDiagnostic {
                code: LayoutDiagnosticCode::OverflowFallback,
                page_index: Some(self.current_page_index()?),
                source_node_id: Some(row.id.clone()),
                value: Some(plan.height.raw().unsigned_abs().min(u64::from(u32::MAX)) as u32),
            });
        }
        let x = self.content_left_for_depth(0)?;
        let y = self.cursor_y;
        let mut cells = Vec::with_capacity(plan.cells.len());
        for (index, cell) in plan.cells.iter().enumerate() {
            let cell_x = x.checked_add(multiply_units(plan.column_width, index)?)?;
            let padding = LayoutUnit::from_millipoints(TABLE_CELL_PADDING_MILLIPOINTS)?;
            let mut lines = Vec::with_capacity(cell.result.lines.len());
            for (line_index, line) in cell.result.lines.iter().enumerate() {
                lines.push(LayoutFragment {
                    id: format!(
                        "flow-fragment:{}:{}:{}:{}:{}:{}",
                        fragment_kind_tag(FragmentKind::Line),
                        cell.node_id,
                        line.source.utf8_start,
                        line.source.utf8_end,
                        self.current_page_index()?,
                        u16::from(derived)
                    ),
                    kind: FragmentKind::Line,
                    source_node_id: Some(cell.node_id.clone()),
                    source: Some(line.source),
                    rect: LayoutRect {
                        x: cell_x.checked_add(padding)?,
                        y: y.checked_add(padding)?
                            .checked_add(multiply_units(cell.line_height, line_index)?)?,
                        width: line.width,
                        height: cell.line_height,
                    },
                    break_reason: Some(line_break_reason(line.break_kind)),
                    derived,
                    repeat_index: u16::from(derived),
                    children: Vec::new(),
                });
            }
            cells.push(LayoutFragment {
                id: format!(
                    "flow-fragment:{}:{}:{}:{}:{}",
                    fragment_kind_tag(FragmentKind::TableCell),
                    cell.node_id,
                    self.current_page_index()?,
                    u16::from(derived),
                    plan.column_width.raw()
                ),
                kind: FragmentKind::TableCell,
                source_node_id: Some(cell.node_id.clone()),
                source: None,
                rect: LayoutRect {
                    x: cell_x,
                    y,
                    width: plan.column_width,
                    height: plan.height,
                },
                break_reason: derived.then_some(BreakReason::TableRow),
                derived,
                repeat_index: u16::from(derived),
                children: lines,
            });
        }
        self.push_fragment(LayoutFragment {
            id: format!(
                "flow-fragment:{}:{}:{}:{}:{}",
                fragment_kind_tag(FragmentKind::TableRow),
                row.id,
                self.current_page_index()?,
                u16::from(derived),
                plan.height.raw()
            ),
            kind: FragmentKind::TableRow,
            source_node_id: Some(row.id.clone()),
            source: None,
            rect: LayoutRect {
                x,
                y,
                width: multiply_units(plan.column_width, plan.cells.len())?,
                height: plan.height,
            },
            break_reason: if overflow {
                Some(BreakReason::OverflowFallback)
            } else if derived {
                Some(BreakReason::TableRow)
            } else {
                Some(self.current_page()?.start_reason)
            },
            derived,
            repeat_index: u16::from(derived),
            children: cells,
        });
        self.cursor_y = self
            .cursor_y
            .checked_add(plan.height)?
            .min(self.content_bottom()?);
        Ok(())
    }

    fn place_fixed_block(
        &mut self,
        node: &ContentNode,
        kind: FragmentKind,
        depth: usize,
        width: LayoutUnit,
        height: LayoutUnit,
        default_reason: BreakReason,
    ) -> Result<(), LayoutError> {
        let mut overflow = false;
        if height > self.remaining_height()? && self.current_has_content() {
            self.start_current_section_page(default_reason)?;
        }
        if height > self.content_rect_height()? {
            overflow = true;
            self.diagnostics.push(LayoutDiagnostic {
                code: LayoutDiagnosticCode::OverflowFallback,
                page_index: Some(self.current_page_index()?),
                source_node_id: Some(node.id.clone()),
                value: Some(height.raw().unsigned_abs().min(u64::from(u32::MAX)) as u32),
            });
        }
        let x = self.content_left_for_depth(depth)?;
        self.push_fragment(LayoutFragment {
            id: format!(
                "flow-fragment:{}:{}:{}",
                fragment_kind_tag(kind),
                node.id,
                self.current_page_index()?
            ),
            kind,
            source_node_id: Some(node.id.clone()),
            source: None,
            rect: LayoutRect {
                x,
                y: self.cursor_y,
                width,
                height,
            },
            break_reason: Some(if overflow {
                BreakReason::OverflowFallback
            } else {
                self.current_page()?.start_reason
            }),
            derived: false,
            repeat_index: 0,
            children: Vec::new(),
        });
        self.cursor_y = self
            .cursor_y
            .checked_add(height)?
            .min(self.content_bottom()?);
        Ok(())
    }

    fn place_page_break(&mut self, node: &ContentNode) -> Result<(), LayoutError> {
        self.place_marker(node, FragmentKind::PageBreak)?;
        if self.current_has_content() {
            self.start_current_section_page(BreakReason::ExplicitPageBreak)?;
        }
        Ok(())
    }

    fn place_unsupported(&mut self, node: &ContentNode) -> Result<(), LayoutError> {
        self.diagnostics.push(LayoutDiagnostic {
            code: LayoutDiagnosticCode::UnsupportedNode,
            page_index: Some(self.current_page_index()?),
            source_node_id: Some(node.id.clone()),
            value: None,
        });
        self.place_fixed_block(
            node,
            FragmentKind::Unsupported,
            0,
            self.content_width_for_depth(0)?,
            LayoutUnit::from_millipoints(12_000)?,
            BreakReason::UnsupportedFallback,
        )
    }

    fn place_marker(&mut self, node: &ContentNode, kind: FragmentKind) -> Result<(), LayoutError> {
        self.push_fragment(LayoutFragment {
            id: format!(
                "flow-fragment:{}:{}:{}",
                fragment_kind_tag(kind),
                node.id,
                self.current_page_index()?
            ),
            kind,
            source_node_id: Some(node.id.clone()),
            source: None,
            rect: LayoutRect {
                x: self.content_left_for_depth(0)?,
                y: self.cursor_y,
                width: self.content_width_for_depth(0)?,
                height: LayoutUnit::default(),
            },
            break_reason: None,
            derived: false,
            repeat_index: 0,
            children: Vec::new(),
        });
        Ok(())
    }

    fn text_plan(&self, node: &ContentNode, depth: usize) -> Result<TextBlockPlan, LayoutError> {
        let runs = node.runs().ok_or(LayoutError::InvalidDocument)?;
        let text = runs.iter().map(|run| run.text.as_str()).collect::<String>();
        let width = self.content_width_for_depth(depth)?;
        let font_size = self.font_size_for_node(node);
        let language = language_for_locale(&self.document.locale, runs);
        let (result, line_height) = self.layout_text_value(
            &text,
            width,
            font_size,
            language,
            self.document.locale.as_str(),
        )?;
        let (alignment, spacing_before, spacing_after) = match &node.body {
            BlockKind::Paragraph { attrs, .. } | BlockKind::Heading { attrs, .. } => (
                attrs.alignment.clone(),
                LayoutUnit::from_millipoints(attrs.spacing_before_millipoints)?,
                LayoutUnit::from_millipoints(attrs.spacing_after_millipoints)?,
            ),
            _ => return Err(LayoutError::InvalidDocument),
        };
        Ok(TextBlockPlan {
            result,
            line_height,
            spacing_before,
            spacing_after,
            alignment,
        })
    }

    fn layout_text_value(
        &self,
        text: &str,
        width: LayoutUnit,
        font_size: u32,
        language: TextLanguage,
        locale: &str,
    ) -> Result<(TextLayoutResult, LayoutUnit), LayoutError> {
        let dictionary_identity = self
            .ukrainian_hyphenation
            .map(|data| data.identity().to_owned());
        let request = TextLayoutRequest::new(
            self.request.source_revision,
            blake3::hash(text.as_bytes()).to_hex().to_string(),
            width,
            font_size,
            TextDirection::Auto,
            language,
            self.catalog,
            dictionary_identity,
        )?;
        let result = layout_text(
            text,
            &request,
            self.catalog,
            if language == TextLanguage::Ukrainian && locale.starts_with("uk") {
                self.ukrainian_hyphenation
            } else {
                None
            },
        )?;
        Ok((result, line_height(font_size)?))
    }

    fn font_size_for_node(&self, node: &ContentNode) -> u32 {
        let style_size = node
            .style_id
            .as_ref()
            .and_then(|style_id| {
                self.document
                    .styles
                    .iter()
                    .find(|style| &style.id == style_id)
            })
            .map_or(12_000, |style| style.font_size_millipoints);
        node.runs()
            .into_iter()
            .flatten()
            .filter_map(|run| run.marks.font_size_millipoints)
            .fold(style_size, u32::max)
    }

    fn estimate_node_height(
        &self,
        node: &ContentNode,
        depth: usize,
    ) -> Result<LayoutUnit, LayoutError> {
        match &node.body {
            BlockKind::Paragraph { .. } | BlockKind::Heading { .. } => {
                let plan = self.text_plan(node, depth)?;
                multiply_units(plan.line_height, plan.result.lines.len())?
                    .checked_add(plan.spacing_before)?
                    .checked_add(plan.spacing_after)
            }
            BlockKind::OrderedList { items } | BlockKind::UnorderedList { items } => items
                .iter()
                .try_fold(LayoutUnit::default(), |height, item| {
                    height.checked_add(self.estimate_node_height(item, depth + 1)?)
                }),
            BlockKind::ListItem { children } => children
                .iter()
                .try_fold(LayoutUnit::default(), |height, child| {
                    height.checked_add(self.estimate_node_height(child, depth)?)
                }),
            BlockKind::Image { .. } => {
                LayoutUnit::from_millimetres(IMAGE_PLACEHOLDER_HEIGHT_MILLIMETRES)
            }
            BlockKind::Table { rows, .. } => {
                let width = self.content_width_for_depth(depth)?;
                rows.iter().try_fold(LayoutUnit::default(), |height, row| {
                    height.checked_add(self.table_row_plan(row, width)?.height)
                })
            }
            BlockKind::PageBreak => Ok(LayoutUnit::default()),
            BlockKind::TableRow { .. } | BlockKind::TableCell { .. } => {
                Ok(LayoutUnit::from_millipoints(12_000)?)
            }
        }
    }

    fn table_row_plan(
        &self,
        row: &ContentNode,
        table_width: LayoutUnit,
    ) -> Result<TableRowPlan, LayoutError> {
        let cells = row.children();
        if cells.is_empty() {
            return Err(LayoutError::InvalidDocument);
        }
        let column_width = LayoutUnit::from_raw(
            table_width
                .raw()
                .checked_div(i64::try_from(cells.len()).map_err(|_| LayoutError::NumericOverflow)?)
                .ok_or(LayoutError::InvalidPaginationGeometry)?,
        );
        if column_width.raw() <= 0 {
            return Err(LayoutError::InvalidPaginationGeometry);
        }
        let padding = LayoutUnit::from_millipoints(TABLE_CELL_PADDING_MILLIPOINTS)?;
        let inner_width = column_width.checked_sub(padding.checked_add(padding)?)?;
        let mut plans = Vec::with_capacity(cells.len());
        let mut height = LayoutUnit::default();
        for cell in cells {
            let text = node_text_recursive(cell);
            let font_size = self.font_size_for_node_recursive(cell);
            let language = language_for_locale(&self.document.locale, &[]);
            let (result, line_height) = self.layout_text_value(
                &text,
                inner_width,
                font_size,
                language,
                self.document.locale.as_str(),
            )?;
            let cell_height = multiply_units(line_height, result.lines.len())?
                .checked_add(padding.checked_add(padding)?)?;
            height = height.max(cell_height);
            plans.push(TableCellPlan {
                node_id: cell.id.clone(),
                result,
                line_height,
            });
        }
        Ok(TableRowPlan {
            cells: plans,
            column_width,
            height,
        })
    }

    fn font_size_for_node_recursive(&self, node: &ContentNode) -> u32 {
        let own = self.font_size_for_node(node);
        node.children()
            .iter()
            .map(|child| self.font_size_for_node_recursive(child))
            .fold(own, u32::max)
    }

    fn current_page(&self) -> Result<&LayoutPage, LayoutError> {
        self.pages.last().ok_or(LayoutError::InvalidDocument)
    }

    fn current_page_index(&self) -> Result<u32, LayoutError> {
        Ok(self.current_page()?.page_index)
    }

    fn content_bottom(&self) -> Result<LayoutUnit, LayoutError> {
        Ok(self
            .metrics
            .ok_or(LayoutError::InvalidDocument)?
            .content_bottom)
    }

    fn content_rect_height(&self) -> Result<LayoutUnit, LayoutError> {
        let metrics = self.metrics.ok_or(LayoutError::InvalidDocument)?;
        metrics.content_bottom.checked_sub(metrics.content_top)
    }

    fn content_left_for_depth(&self, depth: usize) -> Result<LayoutUnit, LayoutError> {
        let indent = LayoutUnit::from_millipoints(
            u32::try_from(depth)
                .ok()
                .and_then(|depth| depth.checked_mul(LIST_INDENT_MILLIPOINTS))
                .ok_or(LayoutError::NumericOverflow)?,
        )?;
        self.metrics
            .ok_or(LayoutError::InvalidDocument)?
            .content_left
            .checked_add(indent)
    }

    fn content_width_for_depth(&self, depth: usize) -> Result<LayoutUnit, LayoutError> {
        let indent = LayoutUnit::from_millipoints(
            u32::try_from(depth)
                .ok()
                .and_then(|depth| depth.checked_mul(LIST_INDENT_MILLIPOINTS))
                .ok_or(LayoutError::NumericOverflow)?,
        )?;
        self.metrics
            .ok_or(LayoutError::InvalidDocument)?
            .content_width
            .checked_sub(indent)
            .and_then(|width| {
                (width.raw() > 0)
                    .then_some(width)
                    .ok_or(LayoutError::InvalidPaginationGeometry)
            })
    }

    fn remaining_height(&self) -> Result<LayoutUnit, LayoutError> {
        self.content_bottom()?
            .checked_sub(self.cursor_y)
            .map(|value| {
                if value.raw() < 0 {
                    LayoutUnit::default()
                } else {
                    value
                }
            })
    }

    fn available_lines(&self, line_height: LayoutUnit) -> Result<usize, LayoutError> {
        if line_height.raw() <= 0 {
            return Err(LayoutError::InvalidPaginationGeometry);
        }
        Ok(usize::try_from(self.remaining_height()?.raw() / line_height.raw()).unwrap_or(0))
    }

    fn current_has_content(&self) -> bool {
        self.pages.last().is_some_and(|page| {
            page.fragments.iter().any(|fragment| {
                matches!(
                    fragment.kind,
                    FragmentKind::Paragraph
                        | FragmentKind::Heading
                        | FragmentKind::Line
                        | FragmentKind::Image
                        | FragmentKind::TableRow
                        | FragmentKind::TableCell
                        | FragmentKind::Unsupported
                )
            })
        })
    }

    fn push_fragment(&mut self, fragment: LayoutFragment) {
        if let Some(page) = self.pages.last_mut() {
            page.fragments.push(fragment);
        }
    }
}

fn page_dimensions(settings: &PageSettings) -> Result<(LayoutUnit, LayoutUnit), LayoutError> {
    let (width, height) = match (&settings.page_size, &settings.orientation) {
        (PageSize::A4, PageOrientation::Portrait) => (210, 297),
        (PageSize::A4, PageOrientation::Landscape) => (297, 210),
        (PageSize::Letter, PageOrientation::Portrait) => (216, 279),
        (PageSize::Letter, PageOrientation::Landscape) => (279, 216),
    };
    Ok((
        LayoutUnit::from_millimetres(width)?,
        LayoutUnit::from_millimetres(height)?,
    ))
}

fn multiply_units(value: LayoutUnit, count: usize) -> Result<LayoutUnit, LayoutError> {
    let count = i128::try_from(count).map_err(|_| LayoutError::NumericOverflow)?;
    let product = i128::from(value.raw())
        .checked_mul(count)
        .ok_or(LayoutError::NumericOverflow)?;
    Ok(LayoutUnit::from_raw(
        i64::try_from(product).map_err(|_| LayoutError::NumericOverflow)?,
    ))
}

fn line_height(font_size_millipoints: u32) -> Result<LayoutUnit, LayoutError> {
    let scaled = font_size_millipoints
        .checked_mul(DEFAULT_LINE_HEIGHT_NUMERATOR)
        .ok_or(LayoutError::NumericOverflow)?
        / DEFAULT_LINE_HEIGHT_DENOMINATOR;
    LayoutUnit::from_millipoints(scaled.max(1))
}

fn aligned_x(
    start: LayoutUnit,
    width: LayoutUnit,
    line_width: LayoutUnit,
    alignment: &Alignment,
) -> Result<LayoutUnit, LayoutError> {
    let free = width.checked_sub(line_width).unwrap_or_default();
    match alignment {
        Alignment::Center => start.checked_add(LayoutUnit::from_raw(free.raw() / 2)),
        Alignment::End => start.checked_add(free),
        Alignment::Start | Alignment::Justify => Ok(start),
    }
}

fn line_break_reason(kind: BreakKind) -> BreakReason {
    match kind {
        BreakKind::Overflow => BreakReason::OverflowFallback,
        BreakKind::Mandatory | BreakKind::Unicode | BreakKind::UkrainianHyphenation => {
            BreakReason::Natural
        }
    }
}

fn fragment_kind_tag(kind: FragmentKind) -> &'static str {
    match kind {
        FragmentKind::Paragraph => "paragraph",
        FragmentKind::Heading => "heading",
        FragmentKind::Line => "line",
        FragmentKind::List => "list",
        FragmentKind::ListItem => "listItem",
        FragmentKind::Image => "image",
        FragmentKind::Table => "table",
        FragmentKind::TableRow => "tableRow",
        FragmentKind::TableCell => "tableCell",
        FragmentKind::PageBreak => "pageBreak",
        FragmentKind::Header => "header",
        FragmentKind::Footer => "footer",
        FragmentKind::Unsupported => "unsupported",
    }
}

fn language_for_locale(locale: &str, runs: &[InlineRun]) -> TextLanguage {
    runs.first()
        .and_then(|run| run.marks.language.as_ref())
        .map_or_else(
            || {
                if locale.starts_with("uk") {
                    TextLanguage::Ukrainian
                } else {
                    TextLanguage::English
                }
            },
            |language| match language {
                crate::model::RunLanguage::Ukrainian => TextLanguage::Ukrainian,
                crate::model::RunLanguage::English => TextLanguage::English,
            },
        )
}

fn node_text_recursive(node: &ContentNode) -> String {
    if let Some(runs) = node.runs() {
        return runs.iter().map(|run| run.text.as_str()).collect::<String>();
    }
    node.children()
        .iter()
        .map(node_text_recursive)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn pagination_result_hash(result: &PaginationResult) -> Result<String, LayoutError> {
    serde_json::to_vec(&(
        result.source_revision,
        &result.source_hash,
        &result.layout_settings_fingerprint,
        &result.font_catalog_identity,
        &result.hyphenation_data_identity,
        &result.pages,
        &result.diagnostics,
    ))
    .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
    .map_err(|_| LayoutError::Serialization)
}

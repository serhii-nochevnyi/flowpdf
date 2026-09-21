//! Deterministic glyph-resource, bounded TrueType subsetting, and Unicode
//! mapping preparation.
//!
//! Font bytes are admitted by the Rust-owned [`FontCatalog`]. This adapter
//! never discovers or consults host fonts. The first supported PDF font
//! profile is a bounded TrueType outline subset: composite dependencies are
//! retained, glyph IDs are compacted deterministically, and unsupported
//! OpenType/CFF variants fail closed.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use super::display_list::{PdfDisplayList, PdfGlyphPlacement, PdfTextLine};
use crate::layout::FontCatalog;

const MAX_RESOURCE_GLYPHS: usize = 4_096;
const MAX_CMAP_ENTRIES: usize = 8_192;
const MAX_CMAP_BYTES: usize = 256 * 1024;
const MAX_SUBSET_BYTES: usize = 16 * 1024 * 1024;
const MAX_FONT_TABLES: usize = 64;
const SFNT_VERSION_TRUE_TYPE: u32 = 0x0001_0000;
const SFNT_CHECKSUM_MAGIC: u32 = 0xB1B0_AFBA;
const COMPOSITE_ARG_WORDS: u16 = 0x0001;
const COMPOSITE_MORE_COMPONENTS: u16 = 0x0020;
const COMPOSITE_SCALE: u16 = 0x0008;
const COMPOSITE_XY_SCALE: u16 = 0x0040;
const COMPOSITE_TWO_BY_TWO: u16 = 0x0080;
const COMPOSITE_INSTRUCTIONS: u16 = 0x0100;

/// One observed glyph mapped to a compact PDF code and source Unicode
/// sequence. The PDF code is also the glyph ID in the embedded subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfSubsetGlyph {
    pub pdf_code: u16,
    pub original_glyph_id: u16,
    pub unicode: Vec<u32>,
    pub width_font_units: u16,
}

/// A deterministic font resource prepared from the admitted font catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfFontResource {
    pub resource_name: String,
    pub face_id: String,
    pub subset_identity: String,
    pub embedded_font_bytes: Vec<u8>,
    pub to_unicode_cmap: Vec<u8>,
    pub glyphs: Vec<PdfSubsetGlyph>,
}

impl PdfFontResource {
    /// Resolves an original shaped glyph ID to the compact subset/PDF code.
    #[must_use]
    pub fn pdf_code_for_glyph(&self, glyph_id: u32) -> Option<u16> {
        let glyph_id = u16::try_from(glyph_id).ok()?;
        self.glyphs
            .iter()
            .find(|glyph| glyph.original_glyph_id == glyph_id)
            .map(|glyph| glyph.pdf_code)
    }
}

/// Stable failure taxonomy for font-resource preparation.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PdfFontError {
    #[error("the display list has no glyphs")]
    EmptyDisplayList,
    #[error("a display-list glyph references an unadmitted font")]
    FontMissing,
    #[error("a display-list glyph has no Unicode source cluster")]
    UnicodeMappingMissing,
    #[error("one glyph was observed with conflicting Unicode mappings")]
    UnicodeMappingConflict,
    #[error("the font has no usable glyph metric")]
    FontMetricMissing,
    #[error("the resource glyph count exceeds the limit")]
    GlyphLimit,
    #[error("the cmap entry count exceeds the limit")]
    CmapLimit,
    #[error("the cmap bytes exceed the limit")]
    CmapBytesLimit,
    #[error("the admitted font uses an unsupported format")]
    UnsupportedFontFormat,
    #[error("the admitted font table data is malformed")]
    MalformedFont,
    #[error("the compact font subset exceeds the byte limit")]
    SubsetBytesLimit,
}

impl PdfFontError {
    /// Stable diagnostic code for export/worker callers.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyDisplayList => "FLOW_PDF_FONT_DISPLAY_EMPTY",
            Self::FontMissing => "FLOW_PDF_FONT_MISSING",
            Self::UnicodeMappingMissing => "FLOW_PDF_FONT_UNICODE_MISSING",
            Self::UnicodeMappingConflict => "FLOW_PDF_FONT_UNICODE_CONFLICT",
            Self::FontMetricMissing => "FLOW_PDF_FONT_METRIC_MISSING",
            Self::GlyphLimit => "FLOW_PDF_FONT_GLYPH_LIMIT",
            Self::CmapLimit => "FLOW_PDF_FONT_CMAP_LIMIT",
            Self::CmapBytesLimit => "FLOW_PDF_FONT_CMAP_BYTES_LIMIT",
            Self::UnsupportedFontFormat => "FLOW_PDF_FONT_FORMAT_UNSUPPORTED",
            Self::MalformedFont => "FLOW_PDF_FONT_MALFORMED",
            Self::SubsetBytesLimit => "FLOW_PDF_FONT_SUBSET_BYTES_LIMIT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GlyphObservation {
    unicode: Vec<u32>,
    width_font_units: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubsetResult {
    bytes: Vec<u8>,
    original_to_subset: BTreeMap<u16, u16>,
}

/// Builds one deterministic resource per face used by the display list.
pub fn build_font_resources(
    display_list: &PdfDisplayList,
    catalog: &FontCatalog,
) -> Result<Vec<PdfFontResource>, PdfFontError> {
    let mut observations: BTreeMap<String, BTreeMap<u16, GlyphObservation>> = BTreeMap::new();
    for page in &display_list.pages {
        for item in &page.items {
            for line in &item.lines {
                collect_line_glyphs(line, &mut observations, catalog)?;
            }
        }
    }
    if observations.is_empty() {
        return Err(PdfFontError::EmptyDisplayList);
    }

    let mut resources = Vec::with_capacity(observations.len());
    for (index, (face_id, glyphs)) in observations.into_iter().enumerate() {
        let face = catalog
            .faces()
            .iter()
            .find(|face| face.identity().id == face_id)
            .ok_or(PdfFontError::FontMissing)?;
        let used_glyphs = glyphs.keys().copied().collect::<BTreeSet<_>>();
        let subset = build_true_type_subset(
            face.bytes_for_derived_adapter(),
            face.face_index_for_derived_adapter(),
            &used_glyphs,
            &glyphs,
        )?;
        let mut subset_glyphs = Vec::with_capacity(glyphs.len());
        for (original_glyph_id, observation) in &glyphs {
            let pdf_code = subset
                .original_to_subset
                .get(original_glyph_id)
                .copied()
                .ok_or(PdfFontError::MalformedFont)?;
            subset_glyphs.push(PdfSubsetGlyph {
                pdf_code,
                original_glyph_id: *original_glyph_id,
                unicode: observation.unicode.clone(),
                width_font_units: observation.width_font_units,
            });
        }
        let to_unicode_cmap = build_to_unicode_cmap(&subset_glyphs)?;
        let mut identity_bytes = Vec::new();
        identity_bytes.extend_from_slice(catalog.identity().as_bytes());
        identity_bytes.extend_from_slice(face.identity().id.as_bytes());
        identity_bytes.extend_from_slice(face.identity().content_hash.as_bytes());
        identity_bytes.extend_from_slice(&subset.bytes);
        identity_bytes.extend_from_slice(&to_unicode_cmap);
        let subset_identity = blake3::hash(&identity_bytes).to_hex().to_string();
        resources.push(PdfFontResource {
            resource_name: format!("F{}", index + 1),
            face_id,
            subset_identity,
            embedded_font_bytes: subset.bytes,
            to_unicode_cmap,
            glyphs: subset_glyphs,
        });
    }
    Ok(resources)
}

fn collect_line_glyphs(
    line: &PdfTextLine,
    observations: &mut BTreeMap<String, BTreeMap<u16, GlyphObservation>>,
    catalog: &FontCatalog,
) -> Result<(), PdfFontError> {
    let clusters = line
        .glyphs
        .iter()
        .map(|glyph| glyph.cluster_utf8)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    for glyph in &line.glyphs {
        let original_glyph_id =
            u16::try_from(glyph.glyph_id).map_err(|_| PdfFontError::GlyphLimit)?;
        let unicode = unicode_for_cluster(line, glyph, &clusters)?;
        if unicode.is_empty() {
            return Err(PdfFontError::UnicodeMappingMissing);
        }
        let face = catalog
            .faces()
            .iter()
            .find(|face| face.identity().id == glyph.face_id)
            .ok_or(PdfFontError::FontMissing)?;
        let (width_font_units, _) = face
            .glyph_metrics_for_derived_adapter(glyph.glyph_id)
            .map_err(|_| PdfFontError::FontMetricMissing)?;
        let by_glyph = observations.entry(glyph.face_id.clone()).or_default();
        if let Some(existing) = by_glyph.get(&original_glyph_id) {
            if existing.unicode != unicode || existing.width_font_units != width_font_units {
                return Err(PdfFontError::UnicodeMappingConflict);
            }
        } else {
            if by_glyph.len() >= MAX_RESOURCE_GLYPHS {
                return Err(PdfFontError::GlyphLimit);
            }
            by_glyph.insert(
                original_glyph_id,
                GlyphObservation {
                    unicode,
                    width_font_units,
                },
            );
        }
    }
    Ok(())
}

fn unicode_for_cluster(
    line: &PdfTextLine,
    glyph: &PdfGlyphPlacement,
    clusters: &[u32],
) -> Result<Vec<u32>, PdfFontError> {
    let start = glyph
        .cluster_utf8
        .checked_sub(line.source.utf8_start)
        .ok_or(PdfFontError::UnicodeMappingMissing)?;
    let end_absolute = clusters
        .iter()
        .copied()
        .find(|cluster| *cluster > glyph.cluster_utf8)
        .unwrap_or(line.source.utf8_end);
    let end = end_absolute
        .checked_sub(line.source.utf8_start)
        .ok_or(PdfFontError::UnicodeMappingMissing)?;
    let start = usize::try_from(start).map_err(|_| PdfFontError::UnicodeMappingMissing)?;
    let end = usize::try_from(end).map_err(|_| PdfFontError::UnicodeMappingMissing)?;
    let text = line
        .text
        .get(start..end)
        .ok_or(PdfFontError::UnicodeMappingMissing)?;
    Ok(text.chars().map(u32::from).collect())
}

fn build_to_unicode_cmap(glyphs: &[PdfSubsetGlyph]) -> Result<Vec<u8>, PdfFontError> {
    if glyphs.len() > MAX_CMAP_ENTRIES {
        return Err(PdfFontError::CmapLimit);
    }
    let mappings = glyphs
        .iter()
        .filter(|glyph| !glyph.unicode.is_empty())
        .collect::<Vec<_>>();
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapType 2 def\n",
    );
    cmap.push_str(&format!("/CMapName /FlowPDF-{} def\n", mappings.len()));
    cmap.push_str("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
    cmap.push_str(&format!("{} beginbfchar\n", mappings.len()));
    for glyph in mappings {
        cmap.push_str(&format!(
            "<{:04X}> <{}>\n",
            glyph.pdf_code,
            unicode_utf16_hex(&glyph.unicode)
        ));
    }
    cmap.push_str("endbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let bytes = cmap.into_bytes();
    if bytes.len() > MAX_CMAP_BYTES {
        return Err(PdfFontError::CmapBytesLimit);
    }
    Ok(bytes)
}

fn unicode_utf16_hex(codepoints: &[u32]) -> String {
    let mut output = String::new();
    for codepoint in codepoints {
        if let Some(character) = char::from_u32(*codepoint) {
            let mut units = [0_u16; 2];
            for unit in character.encode_utf16(&mut units) {
                output.push_str(&format!("{unit:04X}"));
            }
        }
    }
    output
}

#[derive(Debug, Clone, Copy)]
struct TableRecord {
    offset: usize,
    length: usize,
}

#[derive(Debug, Clone, Copy)]
struct CompositeComponent {
    glyph_id: u16,
    glyph_id_offset: usize,
}

fn build_true_type_subset(
    bytes: &[u8],
    face_index: u32,
    used_glyphs: &BTreeSet<u16>,
    observations: &BTreeMap<u16, GlyphObservation>,
) -> Result<SubsetResult, PdfFontError> {
    if face_index != 0 {
        return Err(PdfFontError::UnsupportedFontFormat);
    }
    let tables = parse_table_directory(bytes)?;
    let head = table_bytes(bytes, &tables, *b"head")?;
    let hhea = table_bytes(bytes, &tables, *b"hhea")?;
    let maxp = table_bytes(bytes, &tables, *b"maxp")?;
    let hmtx = table_bytes(bytes, &tables, *b"hmtx")?;
    let loca = table_bytes(bytes, &tables, *b"loca")?;
    let glyf = table_bytes(bytes, &tables, *b"glyf")?;
    if head.len() < 54 || hhea.len() < 36 || maxp.len() < 6 {
        return Err(PdfFontError::MalformedFont);
    }
    let glyph_count = read_u16(maxp, 4).ok_or(PdfFontError::MalformedFont)?;
    if glyph_count == 0 {
        return Err(PdfFontError::MalformedFont);
    }
    let location_format = read_i16(head, 50).ok_or(PdfFontError::MalformedFont)?;
    let locations = parse_locations(loca, glyf.len(), glyph_count, location_format)?;
    let metric_count = read_u16(hhea, 34).ok_or(PdfFontError::MalformedFont)?;
    if metric_count == 0 || metric_count > glyph_count {
        return Err(PdfFontError::MalformedFont);
    }
    let metrics = parse_horizontal_metrics(hmtx, glyph_count, metric_count)?;

    let mut glyph_set = BTreeSet::new();
    glyph_set.insert(0);
    for glyph_id in used_glyphs {
        if *glyph_id >= glyph_count {
            return Err(PdfFontError::MalformedFont);
        }
        glyph_set.insert(*glyph_id);
    }
    let mut pending = glyph_set.iter().copied().collect::<Vec<_>>();
    while let Some(glyph_id) = pending.pop() {
        let glyph = glyph_slice(glyf, &locations, glyph_id)?;
        for component in composite_components(glyph)? {
            if component.glyph_id >= glyph_count {
                return Err(PdfFontError::MalformedFont);
            }
            if glyph_set.insert(component.glyph_id) {
                if glyph_set.len() > MAX_RESOURCE_GLYPHS {
                    return Err(PdfFontError::GlyphLimit);
                }
                pending.push(component.glyph_id);
            }
        }
    }
    if glyph_set.len() > MAX_RESOURCE_GLYPHS || glyph_set.len() > usize::from(u16::MAX) {
        return Err(PdfFontError::GlyphLimit);
    }
    let original_to_subset = glyph_set
        .iter()
        .enumerate()
        .map(|(index, glyph_id)| {
            Ok((
                *glyph_id,
                u16::try_from(index).map_err(|_| PdfFontError::GlyphLimit)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, PdfFontError>>()?;

    let mut subset_glyf = Vec::new();
    let mut subset_locations = Vec::with_capacity(glyph_set.len() + 1);
    for glyph_id in &glyph_set {
        let glyph = glyph_slice(glyf, &locations, *glyph_id)?;
        subset_locations
            .push(u32::try_from(subset_glyf.len()).map_err(|_| PdfFontError::SubsetBytesLimit)?);
        let rewritten = rewrite_composite(glyph, &original_to_subset)?;
        subset_glyf.extend_from_slice(&rewritten);
        while subset_glyf.len() % 4 != 0 {
            subset_glyf.push(0);
        }
    }
    subset_locations
        .push(u32::try_from(subset_glyf.len()).map_err(|_| PdfFontError::SubsetBytesLimit)?);
    let subset_loca = encode_long_locations(&subset_locations)?;
    let subset_hmtx = encode_horizontal_metrics(&glyph_set, &metrics)?;
    let mut subset_head = head.to_vec();
    write_u32(&mut subset_head, 8, 0)?;
    write_i16(&mut subset_head, 50, 1)?;
    let mut subset_hhea = hhea.to_vec();
    write_u16(
        &mut subset_hhea,
        34,
        u16::try_from(glyph_set.len()).map_err(|_| PdfFontError::GlyphLimit)?,
    )?;
    let mut subset_maxp = maxp.to_vec();
    write_u16(
        &mut subset_maxp,
        4,
        u16::try_from(glyph_set.len()).map_err(|_| PdfFontError::GlyphLimit)?,
    )?;

    let mut output_tables = BTreeMap::<[u8; 4], Vec<u8>>::new();
    let mut cmap_mappings = Vec::new();
    for (original_glyph_id, observation) in observations {
        let subset_glyph_id = original_to_subset
            .get(original_glyph_id)
            .copied()
            .ok_or(PdfFontError::MalformedFont)?;
        for codepoint in &observation.unicode {
            cmap_mappings.push((*codepoint, subset_glyph_id));
        }
    }
    output_tables.insert(*b"cmap", build_font_cmap(&cmap_mappings)?);
    output_tables.insert(*b"glyf", subset_glyf);
    output_tables.insert(*b"head", subset_head);
    output_tables.insert(*b"hhea", subset_hhea);
    output_tables.insert(*b"hmtx", subset_hmtx);
    output_tables.insert(*b"loca", subset_loca);
    output_tables.insert(*b"maxp", subset_maxp);
    output_tables.insert(*b"post", minimal_post_table());
    for tag in [*b"OS/2", *b"name", *b"fpgm", *b"prep", *b"cvt ", *b"gasp"] {
        if let Some(table) = tables.get(&tag) {
            output_tables.insert(tag, table_bytes_from_record(bytes, *table)?.to_vec());
        }
    }

    let subset_bytes = build_sfnt(output_tables)?;
    if subset_bytes.len() > MAX_SUBSET_BYTES {
        return Err(PdfFontError::SubsetBytesLimit);
    }
    Ok(SubsetResult {
        bytes: subset_bytes,
        original_to_subset,
    })
}

fn parse_table_directory(bytes: &[u8]) -> Result<BTreeMap<[u8; 4], TableRecord>, PdfFontError> {
    if bytes.len() < 12 {
        return Err(PdfFontError::MalformedFont);
    }
    let version = read_u32(bytes, 0).ok_or(PdfFontError::MalformedFont)?;
    if version == u32::from_be_bytes(*b"ttcf") {
        return Err(PdfFontError::UnsupportedFontFormat);
    }
    if version != SFNT_VERSION_TRUE_TYPE {
        return Err(PdfFontError::UnsupportedFontFormat);
    }
    let table_count = usize::from(read_u16(bytes, 4).ok_or(PdfFontError::MalformedFont)?);
    if table_count == 0 || table_count > MAX_FONT_TABLES {
        return Err(PdfFontError::MalformedFont);
    }
    let directory_bytes = table_count
        .checked_mul(16)
        .and_then(|value| value.checked_add(12))
        .ok_or(PdfFontError::MalformedFont)?;
    if directory_bytes > bytes.len() {
        return Err(PdfFontError::MalformedFont);
    }
    let mut tables = BTreeMap::new();
    for index in 0..table_count {
        let offset = 12 + index * 16;
        let tag = bytes
            .get(offset..offset + 4)
            .and_then(|value| value.try_into().ok())
            .ok_or(PdfFontError::MalformedFont)?;
        let table_offset =
            usize::try_from(read_u32(bytes, offset + 8).ok_or(PdfFontError::MalformedFont)?)
                .map_err(|_| PdfFontError::MalformedFont)?;
        let table_length =
            usize::try_from(read_u32(bytes, offset + 12).ok_or(PdfFontError::MalformedFont)?)
                .map_err(|_| PdfFontError::MalformedFont)?;
        let end = table_offset
            .checked_add(table_length)
            .ok_or(PdfFontError::MalformedFont)?;
        if end > bytes.len()
            || tables
                .insert(
                    tag,
                    TableRecord {
                        offset: table_offset,
                        length: table_length,
                    },
                )
                .is_some()
        {
            return Err(PdfFontError::MalformedFont);
        }
    }
    Ok(tables)
}

fn table_bytes<'a>(
    bytes: &'a [u8],
    tables: &BTreeMap<[u8; 4], TableRecord>,
    tag: [u8; 4],
) -> Result<&'a [u8], PdfFontError> {
    let record = tables
        .get(&tag)
        .ok_or(PdfFontError::UnsupportedFontFormat)?;
    table_bytes_from_record(bytes, *record)
}

fn table_bytes_from_record(bytes: &[u8], record: TableRecord) -> Result<&[u8], PdfFontError> {
    let end = record
        .offset
        .checked_add(record.length)
        .ok_or(PdfFontError::MalformedFont)?;
    bytes
        .get(record.offset..end)
        .ok_or(PdfFontError::MalformedFont)
}

fn parse_locations(
    loca: &[u8],
    glyf_len: usize,
    glyph_count: u16,
    format: i16,
) -> Result<Vec<usize>, PdfFontError> {
    let count = usize::from(glyph_count) + 1;
    let entry_size = match format {
        0 => 2,
        1 => 4,
        _ => return Err(PdfFontError::UnsupportedFontFormat),
    };
    let expected = count
        .checked_mul(entry_size)
        .ok_or(PdfFontError::MalformedFont)?;
    if loca.len() < expected {
        return Err(PdfFontError::MalformedFont);
    }
    let mut locations = Vec::with_capacity(count);
    for index in 0..count {
        let offset = index * entry_size;
        let location = match format {
            0 => usize::from(read_u16(loca, offset).ok_or(PdfFontError::MalformedFont)?) * 2,
            1 => usize::try_from(read_u32(loca, offset).ok_or(PdfFontError::MalformedFont)?)
                .map_err(|_| PdfFontError::MalformedFont)?,
            _ => return Err(PdfFontError::UnsupportedFontFormat),
        };
        if location > glyf_len || locations.last().is_some_and(|last| location < *last) {
            return Err(PdfFontError::MalformedFont);
        }
        locations.push(location);
    }
    Ok(locations)
}

fn parse_horizontal_metrics(
    hmtx: &[u8],
    glyph_count: u16,
    metric_count: u16,
) -> Result<Vec<(u16, i16)>, PdfFontError> {
    let metric_count = usize::from(metric_count);
    let glyph_count = usize::from(glyph_count);
    let required = metric_count
        .checked_mul(4)
        .and_then(|value| {
            value.checked_add(glyph_count.saturating_sub(metric_count).checked_mul(2)?)
        })
        .ok_or(PdfFontError::MalformedFont)?;
    if required > hmtx.len() {
        return Err(PdfFontError::MalformedFont);
    }
    let mut metrics = Vec::with_capacity(glyph_count);
    let mut advances = Vec::with_capacity(metric_count);
    for index in 0..metric_count {
        let offset = index * 4;
        let advance = read_u16(hmtx, offset).ok_or(PdfFontError::MalformedFont)?;
        let bearing = read_i16(hmtx, offset + 2).ok_or(PdfFontError::MalformedFont)?;
        advances.push(advance);
        metrics.push((advance, bearing));
    }
    for index in metric_count..glyph_count {
        let offset = metric_count * 4 + (index - metric_count) * 2;
        let bearing = read_i16(hmtx, offset).ok_or(PdfFontError::MalformedFont)?;
        metrics.push((advances[metric_count - 1], bearing));
    }
    Ok(metrics)
}

fn glyph_slice<'a>(
    glyf: &'a [u8],
    locations: &[usize],
    glyph_id: u16,
) -> Result<&'a [u8], PdfFontError> {
    let index = usize::from(glyph_id);
    let start = *locations.get(index).ok_or(PdfFontError::MalformedFont)?;
    let end = *locations
        .get(index + 1)
        .ok_or(PdfFontError::MalformedFont)?;
    glyf.get(start..end).ok_or(PdfFontError::MalformedFont)
}

fn composite_components(glyph: &[u8]) -> Result<Vec<CompositeComponent>, PdfFontError> {
    if glyph.is_empty() {
        return Ok(Vec::new());
    }
    if glyph.len() < 10 {
        return Err(PdfFontError::MalformedFont);
    }
    if read_i16(glyph, 0) != Some(-1) {
        return Ok(Vec::new());
    }
    let mut cursor = 10_usize;
    let mut components = Vec::new();
    loop {
        let flags = read_u16(glyph, cursor).ok_or(PdfFontError::MalformedFont)?;
        let glyph_id_offset = cursor.checked_add(2).ok_or(PdfFontError::MalformedFont)?;
        let glyph_id = read_u16(glyph, glyph_id_offset).ok_or(PdfFontError::MalformedFont)?;
        components.push(CompositeComponent {
            glyph_id,
            glyph_id_offset,
        });
        cursor = glyph_id_offset
            .checked_add(2)
            .ok_or(PdfFontError::MalformedFont)?;
        cursor = cursor
            .checked_add(if flags & COMPOSITE_ARG_WORDS != 0 {
                4
            } else {
                2
            })
            .ok_or(PdfFontError::MalformedFont)?;
        cursor = cursor
            .checked_add(if flags & COMPOSITE_TWO_BY_TWO != 0 {
                8
            } else if flags & COMPOSITE_XY_SCALE != 0 {
                4
            } else if flags & COMPOSITE_SCALE != 0 {
                2
            } else {
                0
            })
            .ok_or(PdfFontError::MalformedFont)?;
        if flags & COMPOSITE_MORE_COMPONENTS == 0 {
            if flags & COMPOSITE_INSTRUCTIONS != 0 {
                let instruction_count =
                    usize::from(read_u16(glyph, cursor).ok_or(PdfFontError::MalformedFont)?);
                cursor = cursor
                    .checked_add(2)
                    .and_then(|value| value.checked_add(instruction_count))
                    .ok_or(PdfFontError::MalformedFont)?;
            }
            if cursor > glyph.len() {
                return Err(PdfFontError::MalformedFont);
            }
            break;
        }
        if cursor >= glyph.len() {
            return Err(PdfFontError::MalformedFont);
        }
    }
    Ok(components)
}

fn rewrite_composite(
    glyph: &[u8],
    original_to_subset: &BTreeMap<u16, u16>,
) -> Result<Vec<u8>, PdfFontError> {
    let components = composite_components(glyph)?;
    if components.is_empty() || glyph.is_empty() || read_i16(glyph, 0) != Some(-1) {
        return Ok(glyph.to_vec());
    }
    let mut rewritten = glyph.to_vec();
    for component in components {
        let replacement = original_to_subset
            .get(&component.glyph_id)
            .copied()
            .ok_or(PdfFontError::MalformedFont)?;
        write_u16(&mut rewritten, component.glyph_id_offset, replacement)?;
    }
    Ok(rewritten)
}

fn encode_long_locations(locations: &[u32]) -> Result<Vec<u8>, PdfFontError> {
    let mut output = Vec::with_capacity(
        locations
            .len()
            .checked_mul(4)
            .ok_or(PdfFontError::SubsetBytesLimit)?,
    );
    for location in locations {
        output.extend_from_slice(&location.to_be_bytes());
    }
    Ok(output)
}

fn encode_horizontal_metrics(
    glyphs: &BTreeSet<u16>,
    metrics: &[(u16, i16)],
) -> Result<Vec<u8>, PdfFontError> {
    let mut output = Vec::with_capacity(
        glyphs
            .len()
            .checked_mul(4)
            .ok_or(PdfFontError::SubsetBytesLimit)?,
    );
    for glyph_id in glyphs {
        let (advance, bearing) = *metrics
            .get(usize::from(*glyph_id))
            .ok_or(PdfFontError::MalformedFont)?;
        output.extend_from_slice(&advance.to_be_bytes());
        output.extend_from_slice(&bearing.to_be_bytes());
    }
    Ok(output)
}

fn build_font_cmap(mappings: &[(u32, u16)]) -> Result<Vec<u8>, PdfFontError> {
    let mut sorted = BTreeMap::new();
    for (codepoint, glyph_id) in mappings {
        if char::from_u32(*codepoint).is_none() {
            return Err(PdfFontError::UnicodeMappingMissing);
        }
        if let Some(existing) = sorted.get(codepoint) {
            // Shaping can legitimately use distinct glyphs for one Unicode
            // scalar (contextual forms, bidi runs, or a ligature boundary).
            // The embedded font cmap is only a scalar lookup aid; PDF
            // extraction uses the explicit ToUnicode CMap below. Keep the
            // first deterministic mapping and do not invent a conflicting
            // scalar entry.
            if *existing != *glyph_id {
                continue;
            }
        } else {
            sorted.insert(*codepoint, *glyph_id);
        }
    }
    let mut groups = Vec::<(u32, u32, u32)>::new();
    for (codepoint, glyph_id) in sorted {
        if let Some(last) = groups.last_mut()
            && last.1.checked_add(1) == Some(codepoint)
            && last.2.checked_add(1) == Some(u32::from(glyph_id))
        {
            last.1 = codepoint;
        } else {
            groups.push((codepoint, codepoint, u32::from(glyph_id)));
        }
    }
    let subtable_length = 16_usize
        .checked_add(
            groups
                .len()
                .checked_mul(12)
                .ok_or(PdfFontError::CmapBytesLimit)?,
        )
        .ok_or(PdfFontError::CmapBytesLimit)?;
    let total_length = 12_usize
        .checked_add(subtable_length)
        .ok_or(PdfFontError::CmapBytesLimit)?;
    if total_length > MAX_CMAP_BYTES || total_length > usize::try_from(u32::MAX).unwrap_or(0) {
        return Err(PdfFontError::CmapBytesLimit);
    }
    let mut output = Vec::with_capacity(total_length);
    output.extend_from_slice(&0_u16.to_be_bytes());
    output.extend_from_slice(&1_u16.to_be_bytes());
    output.extend_from_slice(&3_u16.to_be_bytes());
    output.extend_from_slice(&10_u16.to_be_bytes());
    output.extend_from_slice(&12_u32.to_be_bytes());
    output.extend_from_slice(&12_u16.to_be_bytes());
    output.extend_from_slice(&0_u16.to_be_bytes());
    output.extend_from_slice(
        &u32::try_from(subtable_length)
            .map_err(|_| PdfFontError::CmapBytesLimit)?
            .to_be_bytes(),
    );
    output.extend_from_slice(&0_u32.to_be_bytes());
    output.extend_from_slice(
        &u32::try_from(groups.len())
            .map_err(|_| PdfFontError::CmapLimit)?
            .to_be_bytes(),
    );
    for (start, end, glyph_id) in groups {
        output.extend_from_slice(&start.to_be_bytes());
        output.extend_from_slice(&end.to_be_bytes());
        output.extend_from_slice(&glyph_id.to_be_bytes());
    }
    Ok(output)
}

fn minimal_post_table() -> Vec<u8> {
    let mut output = vec![0_u8; 32];
    output[0..4].copy_from_slice(&0x0003_0000_u32.to_be_bytes());
    output
}

fn build_sfnt(tables: BTreeMap<[u8; 4], Vec<u8>>) -> Result<Vec<u8>, PdfFontError> {
    if tables.is_empty() || tables.len() > MAX_FONT_TABLES {
        return Err(PdfFontError::MalformedFont);
    }
    let table_count = u16::try_from(tables.len()).map_err(|_| PdfFontError::MalformedFont)?;
    let max_power = (0..=16)
        .rev()
        .find(|power| (1_usize << power) <= tables.len())
        .ok_or(PdfFontError::MalformedFont)?;
    let search_range = (1_u16 << max_power) * 16;
    let entry_selector = u16::try_from(max_power).map_err(|_| PdfFontError::MalformedFont)?;
    let range_shift = table_count
        .checked_mul(16)
        .and_then(|value| value.checked_sub(search_range))
        .ok_or(PdfFontError::MalformedFont)?;
    let directory_size = 12_usize
        .checked_add(usize::from(table_count) * 16)
        .ok_or(PdfFontError::SubsetBytesLimit)?;
    let mut output = vec![0_u8; directory_size];
    write_u32(&mut output, 0, SFNT_VERSION_TRUE_TYPE)?;
    write_u16(&mut output, 4, table_count)?;
    write_u16(&mut output, 6, search_range)?;
    write_u16(&mut output, 8, entry_selector)?;
    write_u16(&mut output, 10, range_shift)?;

    let mut records = Vec::with_capacity(tables.len());
    for (tag, table) in tables {
        while output.len() % 4 != 0 {
            output.push(0);
        }
        let offset = u32::try_from(output.len()).map_err(|_| PdfFontError::SubsetBytesLimit)?;
        let length = u32::try_from(table.len()).map_err(|_| PdfFontError::SubsetBytesLimit)?;
        let checksum = sfnt_checksum(&table);
        output.extend_from_slice(&table);
        while output.len() % 4 != 0 {
            output.push(0);
        }
        records.push((tag, checksum, offset, length));
        if output.len() > MAX_SUBSET_BYTES {
            return Err(PdfFontError::SubsetBytesLimit);
        }
    }
    for (index, (tag, checksum, offset, length)) in records.iter().enumerate() {
        let record_offset = 12 + index * 16;
        output[record_offset..record_offset + 4].copy_from_slice(tag);
        write_u32(&mut output, record_offset + 4, *checksum)?;
        write_u32(&mut output, record_offset + 8, *offset)?;
        write_u32(&mut output, record_offset + 12, *length)?;
    }
    let head_offset = records
        .iter()
        .find(|(tag, _, _, _)| tag == b"head")
        .map(|(_, _, offset, _)| usize::try_from(*offset).unwrap_or(usize::MAX))
        .ok_or(PdfFontError::MalformedFont)?;
    let adjustment_offset = head_offset
        .checked_add(8)
        .ok_or(PdfFontError::MalformedFont)?;
    write_u32(&mut output, adjustment_offset, 0)?;
    let checksum = sfnt_checksum(&output);
    let adjustment = SFNT_CHECKSUM_MAGIC.wrapping_sub(checksum);
    write_u32(&mut output, adjustment_offset, adjustment)?;
    Ok(output)
}

fn sfnt_checksum(bytes: &[u8]) -> u32 {
    let mut sum = 0_u32;
    for chunk in bytes.chunks(4) {
        let mut word = [0_u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let value = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_be_bytes(value.try_into().ok()?))
}

fn read_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    let value = bytes.get(offset..offset.checked_add(2)?)?;
    Some(i16::from_be_bytes(value.try_into().ok()?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let value = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_be_bytes(value.try_into().ok()?))
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) -> Result<(), PdfFontError> {
    let target = bytes
        .get_mut(offset..offset.checked_add(2).ok_or(PdfFontError::MalformedFont)?)
        .ok_or(PdfFontError::MalformedFont)?;
    target.copy_from_slice(&value.to_be_bytes());
    Ok(())
}

fn write_i16(bytes: &mut [u8], offset: usize, value: i16) -> Result<(), PdfFontError> {
    let target = bytes
        .get_mut(offset..offset.checked_add(2).ok_or(PdfFontError::MalformedFont)?)
        .ok_or(PdfFontError::MalformedFont)?;
    target.copy_from_slice(&value.to_be_bytes());
    Ok(())
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) -> Result<(), PdfFontError> {
    let target = bytes
        .get_mut(offset..offset.checked_add(4).ok_or(PdfFontError::MalformedFont)?)
        .ok_or(PdfFontError::MalformedFont)?;
    target.copy_from_slice(&value.to_be_bytes());
    Ok(())
}

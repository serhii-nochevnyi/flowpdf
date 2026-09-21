use std::collections::BTreeSet;

use flow_core::layout::{
    BreakKind, FontCatalog, FontFace, LayoutError, LayoutUnit, TextDirection, TextLanguage,
    TextLayoutRequest, UkrainianHyphenation, layout_text,
};
use icu_segmenter::GraphemeClusterSegmenter;

const NOTO_SANS: &[u8] = include_bytes!("../data/NotoSans-Regular.ttf");
const UKRAINIAN_DATA: &[u8] = include_bytes!("../data/uk.standard.bincode");

fn catalog() -> FontCatalog {
    FontCatalog::new(vec![
        FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).unwrap(),
    ])
    .unwrap()
}

fn request(
    catalog: &FontCatalog,
    language: TextLanguage,
    direction: TextDirection,
    width_millipoints: u32,
    hyphenation_id: Option<String>,
) -> TextLayoutRequest {
    TextLayoutRequest::new(
        7,
        "0123456789abcdef",
        LayoutUnit::from_millipoints(width_millipoints).unwrap(),
        12_000,
        direction,
        language,
        catalog,
        hyphenation_id,
    )
    .unwrap()
}

fn hyphenation() -> UkrainianHyphenation {
    UkrainianHyphenation::from_bincode("hyphenation-0.8.4-uk-standard", UKRAINIAN_DATA).unwrap()
}

#[test]
fn ukrainian_layout_preserves_source_and_reports_hyphenation_without_soft_hyphens() {
    let catalog = catalog();
    let dictionary = hyphenation();
    let text = "Документування українського договору має бути послідовним.";
    let request = request(
        &catalog,
        TextLanguage::Ukrainian,
        TextDirection::Auto,
        45_000,
        Some(dictionary.identity().to_owned()),
    );

    let first = layout_text(text, &request, &catalog, Some(&dictionary)).unwrap();
    let second = layout_text(text, &request, &catalog, Some(&dictionary)).unwrap();

    assert_eq!(first, second);
    assert_eq!(
        first.source_text_hash,
        blake3::hash(text.as_bytes()).to_hex().to_string()
    );
    assert!(
        first.lines.len() > 1,
        "narrow width should produce multiple lines"
    );
    assert!(
        first
            .break_opportunities
            .iter()
            .any(|item| item.kind == BreakKind::UkrainianHyphenation)
    );
    assert!(!text.contains('\u{00ad}'));
    assert!(
        first
            .break_opportunities
            .iter()
            .all(|item| item.legal || item.kind != BreakKind::UkrainianHyphenation)
    );

    let expected_boundaries: BTreeSet<usize> =
        GraphemeClusterSegmenter::new().segment_str(text).collect();
    assert!(first.break_opportunities.iter().all(|item| {
        expected_boundaries.contains(&(item.offset_utf8 as usize))
            || item.kind == BreakKind::UkrainianHyphenation
    }));
    assert!(!first.result_hash().is_empty());
}

#[test]
fn utf16_cluster_provenance_and_forced_direction_are_deterministic() {
    let catalog = catalog();
    let text = "Cafe\u{301} FlowPDF";
    let request = request(
        &catalog,
        TextLanguage::English,
        TextDirection::RightToLeft,
        100_000,
        None,
    );
    let result = layout_text(text, &request, &catalog, None).unwrap();

    assert_eq!(result.glyph_runs.len(), 1);
    assert_eq!(result.glyph_runs[0].direction, TextDirection::RightToLeft);
    assert!(
        result.glyph_runs[0]
            .glyphs
            .iter()
            .all(|glyph| text.is_char_boundary(glyph.cluster_utf8 as usize))
    );
    let clusters: BTreeSet<u32> = result
        .glyph_runs
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .map(|glyph| glyph.cluster_utf16)
        .collect();
    assert!(
        clusters.contains(&3),
        "the decomposed e cluster starts at UTF-16 3"
    );
    assert!(clusters.contains(&6), "the following F starts at UTF-16 6");
    assert_eq!(result.lines.len(), 1);
}

#[test]
fn missing_or_mismatched_ukrainian_data_fails_closed() {
    let catalog = catalog();
    let missing = request(
        &catalog,
        TextLanguage::Ukrainian,
        TextDirection::Auto,
        100_000,
        Some("missing".to_owned()),
    );
    assert_eq!(
        layout_text("Договір", &missing, &catalog, None)
            .unwrap_err()
            .code(),
        "FLOW_LAYOUT_HYPHENATION_MISSING"
    );

    let dictionary = hyphenation();
    let mismatched = request(
        &catalog,
        TextLanguage::Ukrainian,
        TextDirection::Auto,
        100_000,
        Some("other".to_owned()),
    );
    assert_eq!(
        layout_text("Договір", &mismatched, &catalog, Some(&dictionary))
            .unwrap_err()
            .code(),
        "FLOW_LAYOUT_REQUEST_INVALID"
    );
}

#[test]
fn catalog_identity_mismatch_is_rejected_before_shaping() {
    let catalog = catalog();
    let mut request = request(
        &catalog,
        TextLanguage::English,
        TextDirection::Auto,
        100_000,
        None,
    );
    request.font_catalog_identity = "other-catalog".to_owned();
    assert_eq!(
        layout_text("contract", &request, &catalog, None)
            .unwrap_err()
            .code(),
        "FLOW_LAYOUT_REQUEST_INVALID"
    );
}

#[test]
fn unsupported_glyph_is_explicit_and_does_not_consult_system_fonts() {
    let catalog = catalog();
    let request = request(
        &catalog,
        TextLanguage::English,
        TextDirection::Auto,
        100_000,
        None,
    );
    let error = layout_text("🧪", &request, &catalog, None).unwrap_err();
    assert_eq!(error, LayoutError::UnsupportedGlyph);
    assert_eq!(error.code(), "FLOW_LAYOUT_GLYPH_UNSUPPORTED");
}

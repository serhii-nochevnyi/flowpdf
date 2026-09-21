use flow_core::{
    canonical::canonical_hash,
    layout::{
        FontCatalog, FontFace, PaginationRequest, SourceRange, UkrainianHyphenation,
        paginate_document,
    },
    model::{
        ContentNode, FlowDocument, FontFamily, HeaderFooterRun, HeaderFooterSettings,
        HeaderFooterStyle, NodeId,
    },
    pdf::{
        PdfDisplayDiagnosticCode, PdfDisplayListError, build_display_list, build_font_resources,
    },
};

const NOTO_SANS: &[u8] = include_bytes!("../data/NotoSans-Regular.ttf");
const UKRAINIAN_DATA: &[u8] = include_bytes!("../data/uk.standard.bincode");

fn catalog() -> FontCatalog {
    FontCatalog::new(vec![
        FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).unwrap(),
    ])
    .unwrap()
}

fn document() -> FlowDocument {
    document_with_text(
        "en-US",
        "English source text remains selectable through the Rust display list.",
    )
}

fn document_with_text(locale: &str, text: &str) -> FlowDocument {
    let mut document = FlowDocument::deterministic_sample("en-US").unwrap();
    document.locale = locale.to_owned();
    let style_id = document.styles.first().map(|style| style.id.clone());
    document.content = vec![ContentNode::paragraph(
        NodeId::new("00000000-0000-4000-8000-000000009901").unwrap(),
        style_id,
        text.to_owned(),
    )];
    document.assets.clear();
    document.fields.clear();
    document
}

fn pagination(document: &FlowDocument) -> flow_core::layout::PaginationResult {
    pagination_with(document, None)
}

fn pagination_with(
    document: &FlowDocument,
    hyphenation: Option<&UkrainianHyphenation>,
) -> flow_core::layout::PaginationResult {
    let catalog = catalog();
    let request = PaginationRequest::for_document(document).unwrap();
    paginate_document(document, &request, &catalog, hyphenation).unwrap()
}

fn static_band(text: &str) -> HeaderFooterSettings {
    HeaderFooterSettings {
        enabled: true,
        distance_millimetres: 3,
        locale: "en-US".to_owned(),
        runs: vec![HeaderFooterRun {
            text: text.to_owned(),
            style: HeaderFooterStyle {
                font_family: FontFamily::noto_sans(),
                font_size_millipoints: 10_000,
                bold: false,
                italic: false,
            },
        }],
    }
}

#[test]
fn display_list_preserves_source_ranges_and_shaped_glyphs() {
    let document = document();
    let result = pagination(&document);
    let display = build_display_list(&document, &result, &catalog(), None).unwrap();

    assert_eq!(display.source_revision, document.revision);
    assert_eq!(
        display.source_hash,
        canonical_hash(&flow_core::canonical::canonical_bytes(&document).unwrap())
    );
    assert_eq!(display.pages.len(), result.pages.len());
    let item = display.pages[0].items.first().expect("paragraph item");
    assert!(item.text.contains("selectable"));
    assert_eq!(item.source_node_id, Some(document.content[0].id.clone()));
    assert_eq!(item.lines.len(), 1);
    assert_eq!(item.lines[0].text, item.text);
    assert!(!item.lines[0].glyphs.is_empty());
    assert!(item.lines[0].glyphs.iter().all(|glyph| {
        glyph.cluster_utf8 <= u32::try_from(item.text.len()).unwrap()
            && glyph.cluster_utf16 <= u32::try_from(item.text.encode_utf16().count()).unwrap()
    }));
    assert!(display.diagnostics.is_empty());
    assert!(!display.result_hash.is_empty());
}

#[test]
fn display_list_is_repeatable_and_uses_the_pagination_identity() {
    let document = document();
    let result = pagination(&document);
    let first = build_display_list(&document, &result, &catalog(), None).unwrap();
    let second = build_display_list(&document, &result, &catalog(), None).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.pagination_result_hash, result.result_hash);
}

#[test]
fn repeated_bands_keep_derived_provenance_and_page_order() {
    let text = std::iter::repeat_n(
        "The following contract paragraph remains source-backed and selectable.",
        160,
    )
    .collect::<Vec<_>>()
    .join(" ");
    let mut document = document_with_text("en-US", &text);
    document.sections[0].header = static_band("FlowPDF contract header");
    document.sections[0].footer = static_band("Confidential footer");
    let result = pagination(&document);
    assert!(result.pages.len() > 1);

    let display = build_display_list(&document, &result, &catalog(), None).unwrap();
    assert_eq!(display.pages.len(), result.pages.len());
    for (page_index, page) in display.pages.iter().enumerate() {
        assert_eq!(page.page_index, u32::try_from(page_index).unwrap());
        let bands = page
            .items
            .iter()
            .filter(|item| item.derived)
            .collect::<Vec<_>>();
        assert_eq!(bands.len(), 2);
        assert!(bands.iter().all(|item| item.source_node_id.is_none()));
        assert!(
            bands
                .iter()
                .all(|item| item.repeat_index == page.page_index as u16)
        );
        assert!(
            bands
                .iter()
                .any(|item| item.text == "FlowPDF contract header")
        );
        assert!(bands.iter().any(|item| item.text == "Confidential footer"));
    }
}

#[test]
fn stale_source_and_catalog_are_rejected_before_projection() {
    let document = document();
    let result = pagination(&document);

    let mut stale = document.clone();
    stale.revision = stale.revision.checked_add(1).unwrap();
    assert_eq!(
        build_display_list(&stale, &result, &catalog(), None).unwrap_err(),
        PdfDisplayListError::StaleRevision
    );

    let other_catalog = FontCatalog::new(vec![
        FontFace::new("other-font", "Noto Sans", 0, NOTO_SANS.to_vec()).unwrap(),
    ])
    .unwrap();
    assert_eq!(
        build_display_list(&document, &result, &other_catalog, None).unwrap_err(),
        PdfDisplayListError::FontCatalogMismatch
    );
}

#[test]
fn font_resources_preserve_glyph_metrics_and_unicode_cmap() {
    let document = document();
    let result = pagination(&document);
    let catalog = catalog();
    let display = build_display_list(&document, &result, &catalog, None).unwrap();
    let resources = build_font_resources(&display, &catalog).unwrap();

    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].resource_name, "F1");
    assert_eq!(resources[0].face_id, "noto-sans");
    assert!(resources[0].embedded_font_bytes.len() < NOTO_SANS.len());
    let original_face = ttf_parser::Face::parse(NOTO_SANS, 0).unwrap();
    let subset_face = ttf_parser::Face::parse(&resources[0].embedded_font_bytes, 0).unwrap();
    assert!(subset_face.number_of_glyphs() < original_face.number_of_glyphs());
    assert!(!resources[0].glyphs.is_empty());
    assert!(resources[0].glyphs.iter().all(|glyph| {
        glyph.width_font_units > 0
            && subset_face.glyph_hor_advance(ttf_parser::GlyphId(glyph.pdf_code))
                == Some(glyph.width_font_units)
            && resources[0].pdf_code_for_glyph(u32::from(glyph.original_glyph_id))
                == Some(glyph.pdf_code)
    }));
    assert!(String::from_utf8_lossy(&resources[0].to_unicode_cmap).contains("beginbfchar"));
    assert!(!resources[0].subset_identity.is_empty());
}

#[test]
fn unicode_resource_matrix_preserves_ukrainian_and_combining_source_mappings() {
    let ukrainian = document_with_text(
        "uk-UA",
        "Договір набирає чинності після підписання сторонами.",
    );
    let dictionary =
        UkrainianHyphenation::from_bincode("hyphenation-0.8.4-uk-standard", UKRAINIAN_DATA)
            .unwrap();
    let uk_pagination = pagination_with(&ukrainian, Some(&dictionary));
    let uk_display =
        build_display_list(&ukrainian, &uk_pagination, &catalog(), Some(&dictionary)).unwrap();
    let uk_resource = build_font_resources(&uk_display, &catalog())
        .unwrap()
        .remove(0);
    let uk_cmap = String::from_utf8_lossy(&uk_resource.to_unicode_cmap);
    assert!(uk_cmap.contains("0414"));
    assert!(uk_resource.glyphs.iter().all(|glyph| {
        uk_resource.pdf_code_for_glyph(u32::from(glyph.original_glyph_id)) == Some(glyph.pdf_code)
    }));

    let combining = document_with_text("en-US", "Cafe\u{301} review — stable source ranges.");
    let combining_pagination = pagination(&combining);
    let combining_display =
        build_display_list(&combining, &combining_pagination, &catalog(), None).unwrap();
    let combining_resource = build_font_resources(&combining_display, &catalog())
        .unwrap()
        .remove(0);
    let combining_cmap = String::from_utf8_lossy(&combining_resource.to_unicode_cmap);
    assert!(combining_cmap.contains("0301"));
    assert!(
        combining_display.pages[0].items[0].lines[0]
            .source
            .utf16_end
            < combining_display.pages[0].items[0].lines[0].source.utf8_end
    );
}

#[test]
fn unsupported_glyphs_are_bounded_diagnostics_without_authored_text() {
    let supported = document();
    let supported_pagination = pagination(&supported);
    let unsupported = document_with_text("en-US", "Unsupported glyph: 👩‍💻");
    let mut pagination = supported_pagination;
    pagination.source_hash =
        canonical_hash(&flow_core::canonical::canonical_bytes(&unsupported).unwrap());
    let source = SourceRange {
        utf8_start: 0,
        utf8_end: u32::try_from("Unsupported glyph: 👩‍💻".len()).unwrap(),
        utf16_start: 0,
        utf16_end: u32::try_from("Unsupported glyph: 👩‍💻".encode_utf16().count()).unwrap(),
    };
    let paragraph = pagination.pages[0].fragments.first_mut().unwrap();
    paragraph.source = Some(source);
    paragraph.children[0].source = Some(source);

    let display = build_display_list(&unsupported, &pagination, &catalog(), None).unwrap();
    assert_eq!(display.pages[0].items.len(), 0);
    assert_eq!(display.diagnostics.len(), 1);
    assert_eq!(
        display.diagnostics[0].code,
        PdfDisplayDiagnosticCode::UnsupportedGlyph
    );
    let serialized = serde_json::to_string(&display).unwrap();
    assert!(!serialized.contains("Unsupported glyph"));
    assert!(!serialized.contains('👩'));
}

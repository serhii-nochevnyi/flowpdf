use flow_core::{
    canonical::canonical_hash,
    layout::{FontCatalog, FontFace, PaginationRequest, paginate_document},
    model::{ContentNode, FlowDocument, NodeId},
    pdf::{PdfDisplayListError, build_display_list},
};

const NOTO_SANS: &[u8] = include_bytes!("../data/NotoSans-Regular.ttf");

fn catalog() -> FontCatalog {
    FontCatalog::new(vec![
        FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).unwrap(),
    ])
    .unwrap()
}

fn document() -> FlowDocument {
    let mut document = FlowDocument::deterministic_sample("en-US").unwrap();
    let style_id = document.styles.first().map(|style| style.id.clone());
    document.content = vec![ContentNode::paragraph(
        NodeId::new("00000000-0000-4000-8000-000000009901").unwrap(),
        style_id,
        "English source text remains selectable through the Rust display list.".to_owned(),
    )];
    document.assets.clear();
    document.fields.clear();
    document
}

fn pagination(document: &FlowDocument) -> flow_core::layout::PaginationResult {
    let catalog = catalog();
    let request = PaginationRequest::for_document(document).unwrap();
    paginate_document(document, &request, &catalog, None).unwrap()
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

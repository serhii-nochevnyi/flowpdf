use flow_core::layout::{
    BreakReason, FontCatalog, FontFace, FragmentKind, LayoutFragment, LayoutUnit,
    PaginationRequest, paginate_document,
};
use flow_core::model::{
    BlockKind, ContentNode, FlowDocument, HeaderFooterRun, HeaderFooterSettings, HeaderFooterStyle,
    NodeId, PageOrientation, PageSettings, PageSize, SectionId,
};

const NOTO_SANS: &[u8] = include_bytes!("../data/NotoSans-Regular.ttf");

fn catalog() -> FontCatalog {
    FontCatalog::new(vec![
        FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).unwrap(),
    ])
    .unwrap()
}

fn id(value: u32) -> NodeId {
    NodeId::new(format!("00000000-0000-4000-8000-{value:012x}")).unwrap()
}

fn section_id(value: u32) -> SectionId {
    SectionId::new(format!("00000000-0000-4000-8000-{value:012x}")).unwrap()
}

fn base_document() -> FlowDocument {
    let mut document = FlowDocument::deterministic_sample("en-US").unwrap();
    let style_id = document.styles.first().map(|style| style.id.clone());
    document.content = vec![ContentNode::paragraph(
        id(900),
        style_id,
        "Base semantic paragraph.".to_owned(),
    )];
    document.assets.clear();
    document.fields.clear();
    document
}

fn long_text(seed: &str, repeats: usize) -> String {
    std::iter::repeat_n(seed, repeats)
        .collect::<Vec<_>>()
        .join(" ")
}

fn add_long_paragraphs(document: &mut FlowDocument, first: u32, count: usize) -> Vec<NodeId> {
    let style_id = document.styles.first().map(|style| style.id.clone());
    let mut ids = Vec::new();
    for offset in 0..count {
        let node_id = id(first + u32::try_from(offset).unwrap());
        ids.push(node_id.clone());
        document.content.push(ContentNode::paragraph(
            node_id,
            style_id.clone(),
            long_text(
                "The following contract paragraph remains semantic and selectable.",
                55,
            ),
        ));
    }
    ids
}

fn static_band(text: &str, locale: &str) -> HeaderFooterSettings {
    HeaderFooterSettings {
        enabled: true,
        distance_millimetres: 3,
        locale: locale.to_owned(),
        runs: vec![HeaderFooterRun {
            text: text.to_owned(),
            style: HeaderFooterStyle {
                font_family: flow_core::model::FontFamily::noto_sans(),
                font_size_millipoints: 10_000,
                bold: false,
                italic: false,
            },
        }],
    }
}

fn paginate(document: &FlowDocument) -> flow_core::layout::PaginationResult {
    let request = PaginationRequest::for_document(document).unwrap();
    paginate_document(document, &request, &catalog(), None).unwrap()
}

fn top_level_fragments(
    result: &flow_core::layout::PaginationResult,
) -> Vec<(u32, &LayoutFragment)> {
    result
        .pages
        .iter()
        .flat_map(|page| {
            page.fragments
                .iter()
                .map(move |fragment| (page.page_index, fragment))
        })
        .collect()
}

#[test]
fn ordinary_flow_reflows_only_the_affected_suffix_and_repeats_static_bands() {
    let mut document = base_document();
    document.sections[0].header = static_band("FlowPDF contract", "en-US");
    document.sections[0].footer = static_band("Confidential", "en-US");
    let node_ids = add_long_paragraphs(&mut document, 1_000, 24);
    let before = document.clone();
    let first = paginate(&document);
    let second = paginate(&document);

    assert_eq!(first, second);
    assert_eq!(first.pages.len(), 25);
    assert_eq!(
        first.pages[0].bounds.width,
        LayoutUnit::from_millimetres(210).unwrap()
    );
    assert_eq!(
        first.pages[0].bounds.height,
        LayoutUnit::from_millimetres(297).unwrap()
    );
    assert_eq!(
        first.pages[0].content_rect.x,
        LayoutUnit::from_millimetres(20).unwrap()
    );
    assert_eq!(
        first.pages[0].content_rect.y,
        LayoutUnit::from_millimetres(20)
            .unwrap()
            .checked_add(LayoutUnit::from_millipoints(12_000).unwrap())
            .unwrap()
            .checked_add(LayoutUnit::from_millimetres(3).unwrap())
            .unwrap()
    );
    assert!(first.pages.iter().all(|page| page.header.is_some()));
    assert!(first.pages.iter().all(|page| page.footer.is_some()));
    assert_eq!(first.pages[0].start_reason, BreakReason::DocumentStart);
    assert!(first.pages.iter().any(|page| {
        page.fragments.iter().any(|fragment| {
            fragment.source_node_id.as_ref() == Some(&node_ids[0])
                && fragment.kind == FragmentKind::Paragraph
        })
    }));

    let mut edited = document.clone();
    let edited_node = node_ids[8].clone();
    let edited_text = long_text(
        "The changed paragraph grows and repaginates its suffix.",
        240,
    );
    edited
        .content
        .iter_mut()
        .find(|node| node.id == edited_node)
        .unwrap()
        .set_plain_text(edited_text)
        .unwrap();
    let after = paginate(&edited);
    assert_ne!(first.result_hash, after.result_hash);
    assert_ne!(first.source_hash, after.source_hash);
    assert_eq!(
        document, before,
        "pagination must not mutate canonical state"
    );

    let before_positions = top_level_fragments(&first)
        .into_iter()
        .filter(|(_, fragment)| {
            fragment
                .source_node_id
                .as_ref()
                .is_some_and(|id| id < &edited_node)
        })
        .map(|(page, fragment)| {
            (
                page,
                fragment.source_node_id.clone(),
                fragment.source,
                fragment.rect,
            )
        })
        .collect::<Vec<_>>();
    let after_positions = top_level_fragments(&after)
        .into_iter()
        .filter(|(_, fragment)| {
            fragment
                .source_node_id
                .as_ref()
                .is_some_and(|id| id < &edited_node)
        })
        .map(|(page, fragment)| {
            (
                page,
                fragment.source_node_id.clone(),
                fragment.source,
                fragment.rect,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(before_positions, after_positions);
}

#[test]
fn explicit_breaks_and_section_boundaries_select_stable_page_geometry() {
    let mut document = base_document();
    let section_start = id(2_001);
    document.content.push(ContentNode {
        id: id(2_000),
        style_id: None,
        body: BlockKind::PageBreak,
    });
    document.content.push(ContentNode::paragraph(
        section_start.clone(),
        None,
        "Landscape section".to_owned(),
    ));
    let page_settings = PageSettings {
        page_size: PageSize::Letter,
        orientation: PageOrientation::Landscape,
        margins_millimetres: document.page_settings.margins_millimetres.clone(),
    };
    document.sections.push(flow_core::model::SectionSettings {
        id: section_id(2_002),
        start_node_id: Some(section_start.clone()),
        page_settings,
        header: static_band("Section header", "en-US"),
        footer: HeaderFooterSettings::empty("en-US"),
    });

    let result = paginate(&document);
    assert!(result.pages.iter().any(|page| {
        page.start_reason == BreakReason::ExplicitPageBreak
            && page.section_id == document.sections[0].id
    }));
    let section_page = result
        .pages
        .iter()
        .find(|page| page.section_id == document.sections[1].id)
        .unwrap();
    assert_eq!(section_page.start_reason, BreakReason::SectionBoundary);
    assert_eq!(section_page.page_settings.page_size, PageSize::Letter);
    assert_eq!(
        section_page.page_settings.orientation,
        PageOrientation::Landscape
    );
    assert!(section_page.header.is_some());
}

#[test]
fn pagination_corpus_is_checked_in_and_fragment_order_is_repeatable() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/layout/phase3-pagination-corpus.json"
    ))
    .unwrap();
    assert_eq!(corpus["schemaVersion"], 1);
    let document = base_document();
    let first = paginate(&document);
    let second = paginate(&document);
    let first_order = first
        .pages
        .iter()
        .flat_map(|page| page.fragments.iter())
        .filter_map(|fragment| fragment.source_node_id.as_ref().map(ToString::to_string))
        .collect::<Vec<_>>();
    let second_order = second
        .pages
        .iter()
        .flat_map(|page| page.fragments.iter())
        .filter_map(|fragment| fragment.source_node_id.as_ref().map(ToString::to_string))
        .collect::<Vec<_>>();
    assert_eq!(first_order, second_order);
    assert_eq!(first.result_hash, second.result_hash);
}

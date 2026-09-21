use flow_core::layout::{
    BreakReason, FontCatalog, FontFace, LayoutDiagnosticCode, PaginationRequest, paginate_document,
};
use flow_core::model::{BlockKind, ContentNode, FlowDocument, NodeId, ParagraphAttrs};

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

fn paragraph(document: &FlowDocument, node_id: NodeId, text: String) -> ContentNode {
    ContentNode::paragraph(
        node_id,
        document.styles.first().map(|style| style.id.clone()),
        text,
    )
}

fn paginate(
    document: &FlowDocument,
    max_pages: u32,
) -> Result<flow_core::layout::PaginationResult, flow_core::layout::LayoutError> {
    let base = PaginationRequest::for_document(document).unwrap();
    let request =
        PaginationRequest::new(base.source_revision, base.source_hash, max_pages).unwrap();
    paginate_document(document, &request, &catalog(), None)
}

fn fragment_page(result: &flow_core::layout::PaginationResult, node_id: &NodeId) -> Option<u32> {
    result.pages.iter().find_map(|page| {
        page.fragments
            .iter()
            .any(|fragment| fragment.source_node_id.as_ref() == Some(node_id))
            .then_some(page.page_index)
    })
}

fn table(document: &FlowDocument, table_id: u32, rows: usize, text_repeats: usize) -> ContentNode {
    let table_rows = (0..rows)
        .map(|row_index| {
            let row_id = id(table_id + 1 + u32::try_from(row_index).unwrap());
            let cell_id = id(table_id + 100 + u32::try_from(row_index).unwrap());
            let cell_paragraph_id = id(table_id + 200 + u32::try_from(row_index).unwrap());
            let text = if row_index == 0 {
                "Column header".to_owned()
            } else {
                long_text(
                    "A bounded table row remains represented exactly once.",
                    text_repeats,
                )
            };
            let paragraph = paragraph(document, cell_paragraph_id, text);
            ContentNode {
                id: row_id,
                style_id: None,
                body: BlockKind::TableRow {
                    cells: vec![ContentNode {
                        id: cell_id,
                        style_id: None,
                        body: BlockKind::TableCell {
                            children: vec![paragraph],
                        },
                    }],
                },
            }
        })
        .collect();
    ContentNode {
        id: id(table_id),
        style_id: None,
        body: BlockKind::Table {
            header_rows: 1,
            rows: table_rows,
        },
    }
}

#[test]
fn heading_keep_with_next_and_widow_orphan_fallback_are_bounded() {
    let mut document = base_document();
    for index in 0..18 {
        document.content.push(paragraph(
            &document,
            id(4_000 + index),
            long_text("Filler content consumes deterministic page height.", 50),
        ));
    }
    let heading_id = id(4_100);
    let following_id = id(4_101);
    document.content.push(ContentNode {
        id: heading_id.clone(),
        style_id: document.styles.get(1).map(|style| style.id.clone()),
        body: BlockKind::Heading {
            level: 2,
            attrs: ParagraphAttrs::default(),
            runs: vec![flow_core::model::InlineRun {
                text: "Heading stays with its following block".to_owned(),
                marks: flow_core::model::MarkSet::default(),
            }],
        },
    });
    document.content.push(paragraph(
        &document,
        following_id.clone(),
        "The following paragraph is the keep-with-next target.".to_owned(),
    ));

    let result = paginate(&document, 128).unwrap();
    assert_eq!(
        fragment_page(&result, &heading_id),
        fragment_page(&result, &following_id)
    );
    assert!(result.diagnostics.iter().all(|diagnostic| {
        matches!(
            diagnostic.code,
            LayoutDiagnosticCode::ConstraintFallback | LayoutDiagnosticCode::OverflowFallback
        )
    }));
}

#[test]
fn simple_tables_repeat_header_rows_and_never_split_inside_a_row() {
    let mut document = base_document();
    for index in 0..12 {
        document.content.push(paragraph(
            &document,
            id(5_000 + index),
            long_text("Content before the table exercises continuation pages.", 60),
        ));
    }
    let table = table(&document, 5_100, 8, 36);
    let header_id = table.children().first().unwrap().id.clone();
    document.content.push(table);

    let result = paginate(&document, 128).unwrap();
    let header_repetitions = result
        .pages
        .iter()
        .flat_map(|page| page.fragments.iter())
        .filter(|fragment| fragment.source_node_id.as_ref() == Some(&header_id) && fragment.derived)
        .count();
    assert!(
        header_repetitions > 0,
        "the table should continue and repeat its header"
    );
    assert!(result.pages.iter().all(|page| {
        page.fragments.iter().all(|fragment| {
            fragment.kind != flow_core::layout::FragmentKind::TableRow
                || fragment.rect.height.raw() > 0
        })
    }));
    assert!(result.pages.iter().any(|page| {
        page.fragments.iter().any(|fragment| {
            fragment.source_node_id.as_ref() == Some(&header_id)
                && fragment.break_reason == Some(BreakReason::TableRow)
        })
    }));
}

#[test]
fn oversized_rows_emit_explicit_fallback_and_page_limit_stops_without_looping() {
    let mut document = base_document();
    let oversized = table(&document, 6_100, 2, 200);
    let row_id = oversized.children().get(1).unwrap().id.clone();
    document.content.push(oversized);

    let result = paginate(&document, 128).unwrap();
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == LayoutDiagnosticCode::OverflowFallback
            && diagnostic.source_node_id.as_ref() == Some(&row_id)
    }));
    assert!(
        result
            .pages
            .iter()
            .flat_map(|page| page.fragments.iter())
            .any(|fragment| {
                fragment.source_node_id.as_ref() == Some(&row_id)
                    && fragment.break_reason == Some(BreakReason::OverflowFallback)
            })
    );

    let error = paginate(&document, 1).unwrap_err();
    assert_eq!(error.code(), "FLOW_LAYOUT_PAGINATION_PAGE_LIMIT");
}

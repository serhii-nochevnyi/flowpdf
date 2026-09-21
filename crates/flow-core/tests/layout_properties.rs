use flow_core::{
    diff_documents,
    invalidation::IncrementalLayoutCache,
    layout::{
        FontCatalog, FontFace, PaginationRequest, paginate_document, paginate_incremental_document,
    },
    model::{
        AssetDescriptor, BlockKind, ContentNode, FlowDocument, HeaderFooterSettings,
        ImageAccessibility, NodeId, PageOrientation, PageSettings, PageSize, SectionId,
        SectionSettings,
    },
};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

const NOTO_SANS: &[u8] = include_bytes!("../data/NotoSans-Regular.ttf");

fn catalog() -> FontCatalog {
    FontCatalog::new(vec![
        FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).unwrap(),
    ])
    .unwrap()
}

fn node_id(value: u32) -> NodeId {
    NodeId::new(format!("00000000-0000-4000-8000-{value:012x}")).unwrap()
}

fn make_document(shapes: &[u8], repeats: u8) -> (FlowDocument, NodeId) {
    let mut document = FlowDocument::deterministic_sample("en-US").unwrap();
    let style_id = document.styles.first().map(|style| style.id.clone());
    let mut content = Vec::new();
    let mut assets = Vec::new();
    for (index, shape) in shapes.iter().enumerate() {
        let serial = 9_000 + u32::try_from(index).unwrap() * 10;
        let node = match shape % 6 {
            0 => ContentNode::paragraph(
                node_id(serial),
                style_id.clone(),
                format!(
                    "paragraph {index} {}",
                    "contract ".repeat(usize::from(repeats))
                ),
            ),
            1 => ContentNode {
                id: node_id(serial),
                style_id: document.styles.get(1).map(|style| style.id.clone()),
                body: BlockKind::Heading {
                    level: 2,
                    attrs: Default::default(),
                    runs: vec![flow_core::model::InlineRun {
                        text: format!("heading {index}"),
                        marks: Default::default(),
                    }],
                },
            },
            2 => ContentNode {
                id: node_id(serial),
                style_id: None,
                body: BlockKind::UnorderedList {
                    items: vec![ContentNode {
                        id: node_id(serial + 1),
                        style_id: None,
                        body: BlockKind::ListItem {
                            children: vec![ContentNode::paragraph(
                                node_id(serial + 2),
                                style_id.clone(),
                                format!("list item {index}"),
                            )],
                        },
                    }],
                },
            },
            3 => ContentNode {
                id: node_id(serial),
                style_id: None,
                body: BlockKind::PageBreak,
            },
            4 => ContentNode {
                id: node_id(serial),
                style_id: None,
                body: BlockKind::Table {
                    header_rows: 1,
                    rows: vec![ContentNode {
                        id: node_id(serial + 1),
                        style_id: None,
                        body: BlockKind::TableRow {
                            cells: vec![ContentNode {
                                id: node_id(serial + 2),
                                style_id: None,
                                body: BlockKind::TableCell {
                                    children: vec![ContentNode::paragraph(
                                        node_id(serial + 3),
                                        style_id.clone(),
                                        format!("table cell {index}"),
                                    )],
                                },
                            }],
                        },
                    }],
                },
            },
            _ => {
                let asset_id = flow_core::model::AssetId::new(format!(
                    "00000000-0000-4000-8000-{:012x}",
                    7_000 + u32::try_from(index).unwrap()
                ))
                .unwrap();
                assets.push(AssetDescriptor {
                    id: asset_id.clone(),
                    content_hash: format!("blake3:{}", blake3::hash(&[]).to_hex()),
                    media_type: "image/png".to_owned(),
                    byte_length: 0,
                    alt_text: String::new(),
                });
                ContentNode {
                    id: node_id(serial),
                    style_id: None,
                    body: BlockKind::Image {
                        asset_id,
                        accessibility: ImageAccessibility::Decorative,
                    },
                }
            }
        };
        content.push(node);
    }
    let edit_id = node_id(9_800);
    content.push(ContentNode::paragraph(
        edit_id.clone(),
        style_id,
        "editable suffix".to_owned(),
    ));
    document.content = content;
    document.assets = assets;
    document.fields.clear();
    if shapes.len() >= 3 {
        let boundary_index = shapes.len() / 2;
        document.sections.push(SectionSettings {
            id: SectionId::new("00000000-0000-4000-8000-000000000702").unwrap(),
            start_node_id: Some(document.content[boundary_index].id.clone()),
            page_settings: PageSettings {
                page_size: PageSize::Letter,
                orientation: PageOrientation::Landscape,
                margins_millimetres: document.page_settings.margins_millimetres.clone(),
            },
            header: HeaderFooterSettings::empty("en-US"),
            footer: HeaderFooterSettings::empty("en-US"),
        });
    }
    (document, edit_id)
}

fn property_config() -> ProptestConfig {
    let mut config = ProptestConfig::with_cases(16);
    config.failure_persistence = Some(Box::new(FileFailurePersistence::Direct(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/layout_properties.txt"
    ))));
    config
}

proptest! {
    #![proptest_config(property_config())]

    #[test]
    fn generated_bounded_documents_have_full_incremental_equivalence(
        shapes in prop::collection::vec(0_u8..=5, 1..=7),
        repeats in 1_u8..=8,
        edited_repeats in 1_u8..=14,
    ) {
        let (before, edit_id) = make_document(&shapes, repeats);
        let fonts = catalog();
        let before_request = PaginationRequest::for_document(&before).unwrap();
        let before_result = paginate_document(&before, &before_request, &fonts, None).unwrap();
        let cache = IncrementalLayoutCache::from_result(before_result, 0).unwrap();

        let mut after = before.clone();
        after.revision += 1;
        let edit_text = format!("editable suffix {}", "contract ".repeat(usize::from(edited_repeats)));
        after
            .content
            .iter_mut()
            .find(|node| node.id == edit_id)
            .unwrap()
            .set_plain_text(edit_text)
            .unwrap();
        let changes = diff_documents(&before, &after);
        let request = PaginationRequest::for_document(&after).unwrap();
        let incremental = paginate_incremental_document(
            &after,
            &request,
            &fonts,
            None,
            Some(&cache),
            &changes,
        )
        .unwrap();
        let full = paginate_document(&after, &request, &fonts, None).unwrap();

        prop_assert_eq!(&incremental.result, &full);
        prop_assert_eq!(incremental.result.result_hash, full.result_hash);
        prop_assert_eq!(incremental.result.diagnostics, full.diagnostics);
        prop_assert!(!incremental.reused);
        prop_assert_eq!(incremental.recomputed_from, changes.node_ids.iter()
            .filter_map(|id| after.content.iter().position(|node| &node.id == id))
            .min()
            .unwrap_or(0));
    }
}

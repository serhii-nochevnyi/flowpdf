use flow_core::{
    ChangeKind, ChangeSet, IncrementalLayoutCache, InvalidationReason, diff_documents,
    layout::{
        FontCatalog, FontFace, PaginationRequest, paginate_document, paginate_incremental_document,
    },
    model::{BlockKind, ContentNode, FlowDocument, NodeId},
    plan_for_document,
};

const NOTO_SANS: &[u8] = include_bytes!("../data/NotoSans-Regular.ttf");

fn catalog(id: &str) -> FontCatalog {
    FontCatalog::new(vec![
        FontFace::new(id, "Noto Sans", 0, NOTO_SANS.to_vec()).unwrap(),
    ])
    .unwrap()
}

fn id(value: u32) -> NodeId {
    NodeId::new(format!("00000000-0000-4000-8000-{value:012x}")).unwrap()
}

fn document() -> FlowDocument {
    let mut document = FlowDocument::deterministic_sample("en-US").unwrap();
    let style_id = document.styles.first().map(|style| style.id.clone());
    document.content = (0..6)
        .map(|index| {
            ContentNode::paragraph(
                id(8_000 + index),
                style_id.clone(),
                format!("Paragraph {index} carries a stable source range."),
            )
        })
        .collect();
    document.assets.clear();
    document.fields.clear();
    document
}

fn full(
    document: &FlowDocument,
    catalog: &FontCatalog,
) -> (PaginationRequest, flow_core::layout::PaginationResult) {
    let request = PaginationRequest::for_document(document).unwrap();
    let result = paginate_document(document, &request, catalog, None).unwrap();
    (request, result)
}

#[test]
fn no_op_reuses_only_a_verified_exact_result() {
    let document = document();
    let fonts = catalog("noto-sans");
    let (request, full_result) = full(&document, &fonts);
    let cache = IncrementalLayoutCache::from_result(full_result.clone(), 1).unwrap();
    let reused = paginate_incremental_document(
        &document,
        &request,
        &fonts,
        None,
        Some(&cache),
        &ChangeSet::empty(),
    )
    .unwrap();

    assert!(reused.reused);
    assert_eq!(reused.invalidation.reason, InvalidationReason::NoChanges);
    assert_eq!(reused.result, full_result);
    assert_eq!(reused.recomputed_from, document.content.len());

    let mut forged = cache.clone();
    forged.carry.carry_hash = "forged".to_owned();
    let recomputed = paginate_incremental_document(
        &document,
        &request,
        &fonts,
        None,
        Some(&forged),
        &ChangeSet::empty(),
    )
    .unwrap();
    assert!(!recomputed.reused);
    assert_eq!(recomputed.result, full_result);
}

#[test]
fn node_changes_invalidate_the_earliest_owner_and_equal_the_full_oracle() {
    let before = document();
    let mut after = before.clone();
    after.revision += 1;
    let changed_id = id(8_003);
    after
        .content
        .iter_mut()
        .find(|node| node.id == changed_id)
        .unwrap()
        .set_plain_text("The changed block has a different height and text.".to_owned())
        .unwrap();

    let fonts = catalog("noto-sans");
    let (_, before_result) = full(&before, &fonts);
    let cache = IncrementalLayoutCache::from_result(before_result, 3).unwrap();
    let request = PaginationRequest::for_document(&after).unwrap();
    let changes = diff_documents(&before, &after);
    assert!(changes.kinds.contains(&ChangeKind::Text));
    assert!(!changes.kinds.contains(&ChangeKind::Revision));
    let plan = plan_for_document(&after, &changes);
    assert_eq!(plan.earliest_boundary, 3);
    assert!(plan.reusable_prefix);

    let incremental =
        paginate_incremental_document(&after, &request, &fonts, None, Some(&cache), &changes)
            .unwrap();
    let full_result = paginate_document(&after, &request, &fonts, None).unwrap();
    assert!(!incremental.reused);
    assert_eq!(incremental.recomputed_from, 3);
    assert_eq!(incremental.result, full_result);
}

#[test]
fn global_inputs_and_ambiguity_disable_prefix_reuse_from_zero() {
    let document = document();
    let node = document.content[4].id.clone();
    let style_plan = plan_for_document(&document, &ChangeSet::node(node, ChangeKind::Style));
    assert_eq!(style_plan.earliest_boundary, 4);
    assert!(style_plan.reusable_prefix);
    for changes in [
        ChangeSet::global(ChangeKind::Section),
        ChangeSet::global(ChangeKind::Geometry),
        ChangeSet::global(ChangeKind::FontData),
        ChangeSet::global(ChangeKind::Constraint),
        ChangeSet::global(ChangeKind::Asset),
        ChangeSet::ambiguous(),
        ChangeSet::global(ChangeKind::Unknown),
    ] {
        let plan = plan_for_document(&document, &changes);
        assert_eq!(plan.earliest_boundary, 0);
        assert!(!plan.reusable_prefix);
        assert!(matches!(
            plan.reason,
            InvalidationReason::GlobalLayoutInput | InvalidationReason::Ambiguous
        ));
    }
}

#[test]
fn stale_revision_and_cross_font_identity_cannot_publish_a_reused_result() {
    let before = document();
    let fonts = catalog("noto-sans");
    let (request, result) = full(&before, &fonts);
    let cache = IncrementalLayoutCache::from_result(result, 0).unwrap();
    let mut changed = before.clone();
    changed.revision += 1;
    changed.content.push(ContentNode {
        id: id(8_100),
        style_id: None,
        body: BlockKind::PageBreak,
    });
    let stale = paginate_incremental_document(
        &changed,
        &request,
        &fonts,
        None,
        Some(&cache),
        &ChangeSet::empty(),
    )
    .unwrap_err();
    assert_eq!(stale.code(), "FLOW_LAYOUT_PAGINATION_REQUEST_INVALID");

    let other_fonts = catalog("other-noto-sans");
    let current_request = PaginationRequest::for_document(&before).unwrap();
    let recomputed = paginate_incremental_document(
        &before,
        &current_request,
        &other_fonts,
        None,
        Some(&cache),
        &ChangeSet::empty(),
    )
    .unwrap();
    assert!(!recomputed.reused);
    assert_eq!(
        recomputed.result,
        paginate_document(&before, &current_request, &other_fonts, None).unwrap()
    );
}

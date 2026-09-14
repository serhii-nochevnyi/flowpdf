use flow_core::{
    CommandKind, DirectionalSelection, Operation, SourceModality,
    anchor::{AnchorMapResult, AnchorTransformation, Utf16Offset},
    model::{
        Affinity, BlockKind, CommandId, ContentNode, FlowDocument, InlineRun, LogicalPosition,
        MarkSet, NodeId,
    },
    transaction::{EditorState, TransactionService, semantic_hash},
};

fn id(value: u32) -> CommandId {
    CommandId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("valid command id")
}

fn node_id(value: u32) -> NodeId {
    NodeId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("valid node id")
}

fn command(state: &EditorState, serial: u32, kind: CommandKind) -> flow_core::transaction::Command {
    flow_core::transaction::Command {
        command_id: id(serial),
        base_revision: state.document().revision,
        modality: SourceModality::Keyboard,
        issued_at: "2026-09-14T17:00:00Z".to_owned(),
        kind,
    }
}

fn position(node_id: NodeId, offset: u32, affinity: Affinity) -> LogicalPosition {
    LogicalPosition {
        node_id,
        utf16_offset: offset.into(),
        affinity,
    }
}

fn equal_semantics(left: &FlowDocument, right: &FlowDocument) {
    assert_eq!(
        semantic_hash(left).expect("left hash"),
        semantic_hash(right).expect("right hash")
    );
}

#[test]
fn structural_split_merge_preserves_ids_marks_and_directional_affinity() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let original_hash = semantic_hash(&document).expect("original hash");
    let first_id = document.content[0].id.clone();
    let generated_id = node_id(9001);
    let initial = EditorState::new(document.clone()).expect("state");

    let split = TransactionService::apply(
        &initial,
        command(
            &initial,
            9002,
            CommandKind::SplitTextBlock {
                node_id: first_id.clone(),
                utf16_offset: Utf16Offset::new(5),
                new_node_id: generated_id.clone(),
            },
        ),
    )
    .expect("split");
    assert_eq!(split.state.document().content[0].id, first_id);
    assert_eq!(split.state.document().content[1].id, generated_id);
    assert_eq!(
        split.state.document().content[0].text() + &split.state.document().content[1].text(),
        document.content[0].text()
    );

    let backward = split.transaction.anchor_mapping.map(&position(
        document.content[0].id.clone(),
        5,
        Affinity::Backward,
    ));
    assert_eq!(
        backward,
        AnchorMapResult::Mapped(position(first_id.clone(), 5, Affinity::Backward))
    );
    let forward = split.transaction.anchor_mapping.map(&position(
        document.content[0].id.clone(),
        5,
        Affinity::Forward,
    ));
    assert_eq!(
        forward,
        AnchorMapResult::Mapped(position(generated_id.clone(), 0, Affinity::Forward))
    );
    assert!(split.transaction.anchor_mapping.transformations.iter().any(
        |transformation| matches!(transformation, AnchorTransformation::SplitTextBlock { .. })
    ));

    let merged = TransactionService::apply(
        &split.state,
        command(
            &split.state,
            9003,
            CommandKind::MergeTextBlocks {
                first_node_id: first_id.clone(),
                second_node_id: generated_id.clone(),
            },
        ),
    )
    .expect("merge");
    equal_semantics(merged.state.document(), &document);

    let undone_merge = TransactionService::apply(
        &merged.state,
        command(&merged.state, 9004, CommandKind::Undo),
    )
    .expect("undo merge");
    assert_eq!(
        undone_merge.state.document().content.len(),
        split.state.document().content.len()
    );
    let undone_split = TransactionService::apply(
        &undone_merge.state,
        command(&undone_merge.state, 9005, CommandKind::Undo),
    )
    .expect("undo split");
    assert_eq!(
        semantic_hash(undone_split.state.document()).expect("undo hash"),
        original_hash
    );
}

#[test]
fn incompatible_or_nonadjacent_structural_targets_reject_atomically() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let state = EditorState::new(document.clone()).expect("state");
    let selection = DirectionalSelection {
        anchor: position(document.content[0].id.clone(), 0, Affinity::Forward),
        focus: position(document.content[1].id.clone(), 0, Affinity::Forward),
    };
    let error = TransactionService::apply(
        &state,
        command(
            &state,
            9010,
            CommandKind::ReplaceSelection {
                selection,
                text: "x".to_owned(),
            },
        ),
    )
    .expect_err("paragraph and heading are incompatible");
    assert_eq!(error.code(), "FLOW_INCOMPATIBLE_STRUCTURE");
    assert_eq!(state.document(), &document);

    let error = TransactionService::apply(
        &state,
        command(
            &state,
            9011,
            CommandKind::MergeTextBlocks {
                first_node_id: document.content[0].id.clone(),
                second_node_id: document.content[2].id.clone(),
            },
        ),
    )
    .expect_err("nonadjacent atomic target");
    assert_eq!(error.code(), "FLOW_INCOMPATIBLE_STRUCTURE");
    assert_eq!(state.document(), &document);
}

#[test]
fn compatible_cross_block_replace_preserves_exact_unicode_and_restores_inverse() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let second_id = node_id(9020);
    document.content.insert(
        1,
        ContentNode::paragraph(
            second_id.clone(),
            Some(document.styles[0].id.clone()),
            "другий 😀 é".to_owned(),
        ),
    );
    let original = document.clone();
    let state = EditorState::new(document).expect("state");
    let first = state.document().content[0].clone();
    let selection = DirectionalSelection {
        anchor: position(
            first.id.clone(),
            first.text().encode_utf16().count() as u32,
            Affinity::Forward,
        ),
        focus: position(second_id.clone(), 0, Affinity::Forward),
    };
    let replacement = " — український é 😀";
    let applied = TransactionService::apply(
        &state,
        command(
            &state,
            9021,
            CommandKind::ReplaceSelection {
                selection,
                text: replacement.to_owned(),
            },
        ),
    )
    .expect("cross-block replacement");
    assert_eq!(
        applied.state.document().content.len(),
        original.content.len() - 1
    );
    assert!(
        applied.state.document().content[0]
            .text()
            .contains(replacement)
    );
    assert!(
        applied
            .transaction
            .anchor_mapping
            .transformations
            .iter()
            .any(|transformation| matches!(
                transformation,
                AnchorTransformation::CrossBlockReplace { .. }
            ))
    );

    let undone = TransactionService::apply(
        &applied.state,
        command(&applied.state, 9022, CommandKind::Undo),
    )
    .expect("undo cross-block replacement");
    equal_semantics(undone.state.document(), &original);
}

#[test]
fn rich_run_replacement_preserves_unselected_marks_and_normalizes_only_adjacent_equal_runs() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let node = &mut document.content[0];
    let first_marks = MarkSet {
        bold: true,
        ..MarkSet::default()
    };
    let second_marks = MarkSet {
        italic: true,
        ..MarkSet::default()
    };
    node.body = BlockKind::Paragraph {
        attrs: Default::default(),
        runs: vec![
            InlineRun {
                text: "Україна".to_owned(),
                marks: first_marks.clone(),
            },
            InlineRun {
                text: " é".to_owned(),
                marks: second_marks.clone(),
            },
        ],
    };
    let state = EditorState::new(document.clone()).expect("rich state");
    let node_id = document.content[0].id.clone();
    let applied = TransactionService::apply(
        &state,
        command(
            &state,
            9025,
            CommandKind::ReplaceSelection {
                selection: DirectionalSelection {
                    anchor: position(node_id.clone(), 2, Affinity::Forward),
                    focus: position(node_id.clone(), 7, Affinity::Backward),
                },
                text: "ї".to_owned(),
            },
        ),
    )
    .expect("rich replace");
    let runs = applied.state.document().content[0].runs().expect("runs");
    assert_eq!(runs[0].text, "Укї".to_owned());
    assert_eq!(runs[0].marks, first_marks);
    assert_eq!(runs[1].text, " é".to_owned());
    assert_eq!(runs[1].marks, second_marks);
    let undone = TransactionService::apply(
        &applied.state,
        command(&applied.state, 9026, CommandKind::Undo),
    )
    .expect("undo rich replace");
    equal_semantics(undone.state.document(), &document);
}

#[test]
fn split_and_merge_use_the_same_grapheme_contract_inside_a_list_item() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    document.fields.clear();
    let paragraph_id = node_id(9061);
    let item_id = node_id(9062);
    let list_id = node_id(9063);
    document.content = vec![ContentNode {
        id: list_id,
        style_id: None,
        body: BlockKind::UnorderedList {
            items: vec![ContentNode {
                id: item_id,
                style_id: None,
                body: BlockKind::ListItem {
                    children: vec![ContentNode::paragraph(
                        paragraph_id.clone(),
                        None,
                        "список".to_owned(),
                    )],
                },
            }],
        },
    }];
    let state = EditorState::new(document).expect("list state");
    let split = TransactionService::apply(
        &state,
        command(
            &state,
            9064,
            CommandKind::SplitTextBlock {
                node_id: paragraph_id.clone(),
                utf16_offset: Utf16Offset::new(3),
                new_node_id: node_id(9065),
            },
        ),
    )
    .expect("list split");
    let item = &split.state.document().content[0].children()[0];
    assert_eq!(item.children().len(), 2);
    assert_eq!(
        item.children()[0].text() + &item.children()[1].text(),
        "список"
    );
    let merged = TransactionService::apply(
        &split.state,
        command(
            &split.state,
            9066,
            CommandKind::MergeTextBlocks {
                first_node_id: paragraph_id,
                second_node_id: node_id(9065),
            },
        ),
    )
    .expect("list merge");
    assert_eq!(
        merged.state.document().content[0].children()[0]
            .children()
            .len(),
        1
    );
    assert_eq!(
        merged.state.document().content[0].children()[0].children()[0].text(),
        "список"
    );
}

#[test]
fn deleting_the_last_editable_root_node_retains_a_new_empty_paragraph() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    document.content.truncate(1);
    document.fields.clear();
    let state = EditorState::new(document).expect("state");
    let applied = TransactionService::apply(
        &state,
        command(
            &state,
            9030,
            CommandKind::DeleteSubtree {
                node_id: state.document().content[0].id.clone(),
            },
        ),
    )
    .expect("last editable delete");
    assert_eq!(applied.state.document().content.len(), 1);
    assert!(applied.state.document().content[0].runs().is_some());
    assert!(applied.state.document().content[0].text().is_empty());
    assert_ne!(
        applied.state.document().content[0].id,
        state.document().content[0].id
    );
}

#[test]
fn deletion_tombstones_are_deterministic_and_private_preimage_is_bounded() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let state = EditorState::new(document.clone()).expect("state");
    let target = document.content[0].id.clone();
    let applied = TransactionService::apply(
        &state,
        command(
            &state,
            9040,
            CommandKind::DeleteSubtree {
                node_id: target.clone(),
            },
        ),
    )
    .expect("delete with field preimage");
    let Operation::ReplaceChildren { preimage, .. } = &applied.transaction.forward_operations[0]
    else {
        panic!("expected structural delete operation");
    };
    let preimage = preimage.as_ref().expect("private preimage");
    assert_eq!(preimage.root_node_id, target);
    assert_eq!(preimage.tombstone.command_id, id(9040));
    assert_eq!(preimage.tombstone.slot, 0);
    assert_eq!(preimage.owners.len(), document.fields.len());
    assert!(serde_json::to_vec(preimage).expect("preimage bytes").len() < 8 * 1024 * 1024);
    assert!(applied.state.document().fields.iter().all(|field| matches!(
        field.anchor,
        flow_core::model::FieldAnchorState::TargetDeleted { .. }
    )));
}

#[test]
fn delete_undo_redo_undo_restores_document_fields_ids_and_history_exactly() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let original_hash = semantic_hash(&document).expect("original hash");
    let original_fields = document.fields.clone();
    let state = EditorState::new(document.clone()).expect("state");
    let deleted = TransactionService::apply(
        &state,
        command(
            &state,
            9050,
            CommandKind::DeleteSubtree {
                node_id: document.content[0].id.clone(),
            },
        ),
    )
    .expect("delete");
    let undone = TransactionService::apply(
        &deleted.state,
        command(&deleted.state, 9051, CommandKind::Undo),
    )
    .expect("undo");
    assert_eq!(
        semantic_hash(undone.state.document()).expect("undo hash"),
        original_hash
    );
    assert_eq!(undone.state.document().fields, original_fields);
    let redone = TransactionService::apply(
        &undone.state,
        command(&undone.state, 9052, CommandKind::Redo),
    )
    .expect("redo");
    assert_eq!(
        redone.state.document().fields,
        deleted.state.document().fields
    );
    let restored = TransactionService::apply(
        &redone.state,
        command(&redone.state, 9053, CommandKind::Undo),
    )
    .expect("second undo");
    assert_eq!(
        semantic_hash(restored.state.document()).expect("restored hash"),
        original_hash
    );
    assert_eq!(restored.state.document().fields, original_fields);
    assert_eq!(restored.state.history().cursor, 0);
}

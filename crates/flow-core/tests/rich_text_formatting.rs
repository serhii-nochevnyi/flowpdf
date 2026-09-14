use flow_core::{
    ApiResponse, ApplyCommandRequest, CommandDto, CommandKind, CreateSampleRequest,
    DirectionalSelection, SourceModality,
    anchor::Utf16Offset,
    apply_command, create_sample,
    editor_view::{EditorBlockViewDto, EditorSessionAction, EditorSessionRequest},
    model::{
        Affinity, BlockAttributes, BlockKind, BlockStyle, ContentNode, FlowDocument, InlineMark,
        ListKind, LogicalPosition, MarkSet, NodeId,
    },
    recover,
    store::{CommitPlanner, DocumentStore, InMemoryDocumentStore, SnapshotPolicy, SnapshotReason},
    transaction::{Command, EditorState, TransactionService, semantic_hash},
};

fn node_id(value: u32) -> NodeId {
    NodeId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("valid node id")
}

fn command_id(value: u32) -> flow_core::model::CommandId {
    flow_core::model::CommandId::new(format!("00000000-0000-4000-8000-{value:012}"))
        .expect("valid command id")
}

fn command(state: &EditorState, serial: u32, kind: CommandKind) -> Command {
    Command {
        command_id: command_id(serial),
        base_revision: state.document().revision,
        modality: SourceModality::Keyboard,
        issued_at: "2026-09-14T18:00:00Z".to_owned(),
        kind,
    }
}

fn position(id: NodeId, offset: u32, affinity: Affinity) -> LogicalPosition {
    LogicalPosition {
        node_id: id,
        utf16_offset: Utf16Offset::new(offset),
        affinity,
    }
}

fn selection(id: NodeId, start: u32, end: u32) -> DirectionalSelection {
    DirectionalSelection {
        anchor: position(id.clone(), start, Affinity::Forward),
        focus: position(id, end, Affinity::Backward),
    }
}

fn simple_document(texts: &[&str]) -> FlowDocument {
    let mut document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    document.fields.clear();
    let style_id = document.styles[0].id.clone();
    document.content = texts
        .iter()
        .enumerate()
        .map(|(index, text)| {
            ContentNode::paragraph(
                node_id(10_000 + index as u32),
                Some(style_id.clone()),
                (*text).to_owned(),
            )
        })
        .collect();
    document
}

fn apply(
    state: &EditorState,
    serial: u32,
    kind: CommandKind,
) -> Result<flow_core::transaction::AppliedCommand, flow_core::transaction::CommandError> {
    TransactionService::apply(state, command(state, serial, kind))
}

fn success<T>(response: ApiResponse<T>) -> T {
    assert!(response.ok, "unexpected response: {:?}", response.error);
    response.value.expect("successful response value")
}

#[test]
fn formatting_changes_exact_grapheme_range_and_inverse_restores_runs() {
    let document = simple_document(&["a😀éb"]);
    let node_id = document.content[0].id.clone();
    let state = EditorState::new(document.clone()).expect("state");
    let applied = apply(
        &state,
        10_001,
        CommandKind::SetInlineMark {
            selection: selection(node_id.clone(), 1, 3),
            mark: InlineMark::Bold { value: true },
        },
    )
    .expect("range format");
    let runs = applied.state.document().content[0].runs().expect("runs");
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].text, "a");
    assert!(!runs[0].marks.bold);
    assert_eq!(runs[1].text, "😀");
    assert!(runs[1].marks.bold);
    assert_eq!(runs[2].text, "éb");
    assert!(!runs[2].marks.bold);

    let undone = apply(&applied.state, 10_002, CommandKind::Undo).expect("undo");
    assert_eq!(
        semantic_hash(undone.state.document()),
        semantic_hash(&document)
    );
}

#[test]
fn reverse_selection_has_the_same_formatting_and_keeps_direction() {
    let document = simple_document(&["abcdef"]);
    let node_id = document.content[0].id.clone();
    let state = EditorState::new(document.clone()).expect("state");
    let forward = selection(node_id.clone(), 1, 5);
    let reverse = DirectionalSelection {
        anchor: position(node_id.clone(), 5, Affinity::Backward),
        focus: position(node_id.clone(), 1, Affinity::Forward),
    };
    let left = apply(
        &state,
        10_010,
        CommandKind::SetInlineMarks {
            selection: forward,
            marks: MarkSet {
                italic: true,
                ..MarkSet::default()
            },
        },
    )
    .expect("forward format");
    let right = apply(
        &state,
        10_011,
        CommandKind::SetInlineMarks {
            selection: reverse.clone(),
            marks: MarkSet {
                italic: true,
                ..MarkSet::default()
            },
        },
    )
    .expect("reverse format");
    assert_eq!(
        semantic_hash(left.state.document()),
        semantic_hash(right.state.document())
    );
    assert_eq!(right.selection, Some(reverse));
}

#[test]
fn formatting_noop_stale_and_duplicate_delivery_are_non_mutating() {
    let document = simple_document(&["abc"]);
    let id = document.content[0].id.clone();
    let state = EditorState::new(document).expect("state");
    let no_op_kind = CommandKind::SetInlineMark {
        selection: selection(id.clone(), 0, 3),
        mark: InlineMark::Underline { value: false },
    };
    let no_op = apply(&state, 10_020, no_op_kind).expect_err("explicit no-op");
    assert_eq!(no_op.code(), "FLOW_NO_OP");

    let kind = CommandKind::SetInlineMark {
        selection: selection(id.clone(), 0, 3),
        mark: InlineMark::Underline { value: true },
    };
    let mut stale = command(&state, 10_021, kind.clone());
    stale.base_revision = 0;
    let stale_error = TransactionService::apply(&state, stale).expect_err("stale");
    assert_eq!(stale_error.code(), "FLOW_STALE_REVISION");

    let applied = TransactionService::apply(&state, command(&state, 10_022, kind)).expect("format");
    let duplicate = Command {
        command_id: command_id(10_022),
        base_revision: applied.state.document().revision,
        modality: SourceModality::Keyboard,
        issued_at: "2026-09-14T18:00:00Z".to_owned(),
        kind: CommandKind::SetInlineMark {
            selection: selection(id, 0, 3),
            mark: InlineMark::Underline { value: false },
        },
    };
    assert_eq!(
        TransactionService::apply(&applied.state, duplicate)
            .expect_err("duplicate")
            .code(),
        "FLOW_DUPLICATE_COMMAND"
    );
}

#[test]
fn closed_formatting_bounds_and_encoding_reject_without_clamping() {
    let document = simple_document(&["abc"]);
    let id = document.content[0].id.clone();
    let state = EditorState::new(document).expect("state");
    let range = selection(id.clone(), 0, 3);

    let size = apply(
        &state,
        10_030,
        CommandKind::SetInlineMark {
            selection: range.clone(),
            mark: InlineMark::FontSize {
                value: Some(288_000),
            },
        },
    )
    .expect("maximum font size");
    assert_eq!(
        size.state.document().content[0].runs().expect("runs")[0]
            .marks
            .font_size_millipoints,
        Some(288_000)
    );
    let too_large = apply(
        &state,
        10_031,
        CommandKind::SetInlineMark {
            selection: range.clone(),
            mark: InlineMark::FontSize {
                value: Some(288_001),
            },
        },
    )
    .expect_err("font size max+1");
    assert_eq!(too_large.code(), "FLOW_INVALID_FORMATTING");

    let colored = apply(
        &state,
        10_032,
        CommandKind::SetInlineMark {
            selection: range.clone(),
            mark: InlineMark::Color {
                value: Some("#A1B2C3".to_owned()),
            },
        },
    )
    .expect("uppercase color");
    assert_eq!(
        colored.state.document().content[0].runs().expect("runs")[0]
            .marks
            .color,
        Some([0xA1, 0xB2, 0xC3])
    );
    let lower = apply(
        &state,
        10_033,
        CommandKind::SetInlineMark {
            selection: range,
            mark: InlineMark::Color {
                value: Some("#a1b2c3".to_owned()),
            },
        },
    )
    .expect_err("lowercase color rejected");
    assert_eq!(lower.code(), "FLOW_INVALID_FORMATTING");
}

#[test]
fn block_style_attributes_pending_marks_and_exact_bounds_are_rust_owned() {
    let document = simple_document(&["abc"]);
    let id = document.content[0].id.clone();
    let state = EditorState::new(document.clone()).expect("state");
    let styled = apply(
        &state,
        10_040,
        CommandKind::SetBlockStyle {
            selection: selection(id.clone(), 0, 0),
            style: BlockStyle::Heading { level: 6 },
        },
    )
    .expect("heading six");
    assert!(matches!(
        styled.state.document().content[0].body,
        BlockKind::Heading { level: 6, .. }
    ));
    let attrs = apply(
        &styled.state,
        10_041,
        CommandKind::SetBlockAttributes {
            selection: selection(id.clone(), 0, 0),
            attributes: BlockAttributes {
                alignment: Some(flow_core::model::Alignment::Justify),
                spacing_before_millipoints: Some(144_000),
                spacing_after_millipoints: Some(0),
            },
        },
    )
    .expect("exact block bounds");
    let too_large = apply(
        &styled.state,
        10_042,
        CommandKind::SetBlockAttributes {
            selection: selection(id.clone(), 0, 0),
            attributes: BlockAttributes {
                spacing_before_millipoints: Some(144_001),
                ..BlockAttributes::default()
            },
        },
    )
    .expect_err("spacing max+1");
    assert_eq!(too_large.code(), "FLOW_INVALID_FORMATTING");
    assert_eq!(attrs.state.history().cursor, 2);

    let canonical_json =
        String::from_utf8(flow_core::canonical::canonical_bytes(&document).expect("canonical"))
            .expect("utf8");
    let session = flow_core::EditorSessionState::from_document(&document).expect("session");
    let response = flow_core::apply_editor_session(EditorSessionRequest {
        canonical_json,
        session,
        action: EditorSessionAction::SetPendingMarks {
            marks: MarkSet {
                bold: true,
                ..MarkSet::default()
            },
        },
    });
    assert!(response.ok, "{response:?}");
    assert!(
        response
            .value
            .expect("session response")
            .session
            .pending_marks
            .bold
    );
}

#[test]
fn list_conversion_continuation_exit_indent_and_outdent_are_reversible() {
    let document = simple_document(&["one", "two"]);
    let first = document.content[0].id.clone();
    let second = document.content[1].id.clone();
    let state = EditorState::new(document.clone()).expect("state");
    let list = apply(
        &state,
        10_050,
        CommandKind::SetListKind {
            selection: DirectionalSelection {
                anchor: position(first.clone(), 0, Affinity::Forward),
                focus: position(second.clone(), 3, Affinity::Backward),
            },
            kind: ListKind::Unordered,
        },
    )
    .expect("list conversion");
    let list_node = &list.state.document().content[0];
    assert!(matches!(list_node.body, BlockKind::UnorderedList { .. }));
    assert_eq!(list_node.children().len(), 2);
    assert_eq!(list_node.children()[0].children()[0].id, first);
    assert_eq!(list_node.children()[1].children()[0].id, second);

    let continued = apply(
        &list.state,
        10_051,
        CommandKind::ContinueListItem {
            selection: selection(first.clone(), 3, 3),
        },
    )
    .expect("continue list");
    let continued_list = &continued.state.document().content[0];
    assert_eq!(continued_list.children().len(), 3);
    assert_eq!(continued_list.children()[0].children()[0].text(), "one");
    assert_eq!(continued_list.children()[1].children()[0].text(), "");
    assert_eq!(continued_list.children()[2].children()[0].text(), "two");
    assert!(continued.selection.as_ref().is_some_and(|selection| {
        continued
            .state
            .document()
            .content
            .iter()
            .any(|node| node.id == selection.anchor.node_id)
            || continued.state.document().content[0]
                .children()
                .iter()
                .any(|item| {
                    item.children()
                        .iter()
                        .any(|child| child.id == selection.anchor.node_id)
                })
    }));

    let empty_id = continued_list.children()[1].children()[0].id.clone();
    let exited = apply(
        &continued.state,
        10_052,
        CommandKind::ExitListItem {
            selection: selection(empty_id, 0, 0),
        },
    )
    .expect("exit empty list item");
    assert!(
        exited
            .state
            .document()
            .content
            .iter()
            .any(|node| node.runs().is_some() && node.text().is_empty())
    );
    assert!(exited.selection.as_ref().is_some_and(|selection| {
        exited
            .state
            .document()
            .content
            .iter()
            .any(|node| node.id == selection.anchor.node_id)
    }));

    let indented = apply(
        &list.state,
        10_053,
        CommandKind::IndentListItem {
            item_id: list.state.document().content[0].children()[1].id.clone(),
        },
    )
    .expect("indent");
    assert_eq!(
        indented.state.document().content[0].children()[0]
            .children()
            .len(),
        2
    );
    let outdented = apply(
        &indented.state,
        10_054,
        CommandKind::OutdentListItem {
            item_id: indented.state.document().content[0].children()[0].children()[1].children()[0]
                .id
                .clone(),
        },
    )
    .expect("outdent");
    assert_eq!(
        semantic_hash(outdented.state.document()),
        semantic_hash(list.state.document())
    );

    let undo = apply(&list.state, 10_055, CommandKind::Undo).expect("undo list");
    assert_eq!(
        semantic_hash(undo.state.document()),
        semantic_hash(&document)
    );
}

#[test]
fn formatting_commit_replays_through_the_durable_store() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-09-14T18:10:00Z".to_owned(),
    }));
    let planner = CommitPlanner::new(SnapshotPolicy::EveryTransaction);
    let mut store = InMemoryDocumentStore::default();
    planner
        .commit(&mut store, created.commit, SnapshotReason::Creation)
        .expect("creation commit");
    let before = success(recover(store.load_records().expect("records")));
    let (node_id, text_length) = match &before.editor.view.document.blocks[0] {
        EditorBlockViewDto::Paragraph { node_id, text, .. }
        | EditorBlockViewDto::Heading { node_id, text, .. } => {
            (node_id.clone(), text.encode_utf16().count() as u32)
        }
        _ => panic!("sample starts with a text block"),
    };
    let applied = success(apply_command(ApplyCommandRequest {
        canonical_json: before.session.canonical_json,
        history: before.session.history,
        command: CommandDto {
            command_id: command_id(10_070),
            base_revision: before.session.revision,
            modality: SourceModality::Ui,
            issued_at: "2026-09-14T18:10:01Z".to_owned(),
            kind: CommandKind::SetInlineMark {
                selection: DirectionalSelection {
                    anchor: position(node_id.clone(), 0, Affinity::Forward),
                    focus: position(node_id, text_length, Affinity::Backward),
                },
                mark: InlineMark::Bold { value: true },
            },
        },
    }));
    let post_document: FlowDocument =
        serde_json::from_str(&applied.session.canonical_json).expect("post document");
    EditorState::with_history(post_document, applied.session.history.clone()).expect("history");
    planner
        .commit(
            &mut store,
            applied.commit,
            SnapshotReason::CommittedTransaction,
        )
        .expect("formatting commit");
    let recovered = success(recover(store.load_records().expect("formatted records")));
    assert_eq!(recovered.session.revision, before.session.revision + 1);
    assert_eq!(
        recovered.editor.view.formatting.bold,
        flow_core::FormattingState::On
    );
}

#[test]
fn list_depth_nine_rejects_before_mutation() {
    let mut document = simple_document(&["deep", "sibling"]);
    let mut nested = ContentNode::paragraph(node_id(11_000), None, "deep".to_owned());
    for level in (0..8).rev() {
        let direct =
            ContentNode::paragraph(node_id(11_300 + level), None, format!("level-{level}"));
        let item = ContentNode {
            id: node_id(11_100 + level),
            style_id: None,
            body: BlockKind::ListItem {
                children: vec![direct, nested],
            },
        };
        if level == 7 {
            let second_item = ContentNode {
                id: node_id(11_500),
                style_id: None,
                body: BlockKind::ListItem {
                    children: vec![ContentNode::paragraph(
                        node_id(11_501),
                        None,
                        "second-deep".to_owned(),
                    )],
                },
            };
            nested = ContentNode {
                id: node_id(11_200 + level),
                style_id: None,
                body: BlockKind::UnorderedList {
                    items: vec![item, second_item],
                },
            };
            continue;
        }
        nested = ContentNode {
            id: node_id(11_200 + level),
            style_id: None,
            body: BlockKind::UnorderedList { items: vec![item] },
        };
    }
    document.content[0] = nested;
    let state = EditorState::new(document).expect("depth-eight document");
    let mut deepest = &state.document().content[0];
    for _ in 0..7 {
        deepest = &deepest.children()[0].children()[1];
    }
    let item_id = deepest.children()[1].id.clone();
    let error =
        apply(&state, 10_060, CommandKind::IndentListItem { item_id }).expect_err("depth nine");
    assert_eq!(error.code(), "FLOW_LIMIT_LIST_DEPTH");
    assert_eq!(state.document().revision, 1);
}

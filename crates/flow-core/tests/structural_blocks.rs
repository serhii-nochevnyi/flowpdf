use flow_core::{
    ApiResponse, ApplyCommandRequest, CommandDto, CommandKind, CreateSampleRequest,
    DirectionalSelection, EditorBlockViewDto, EditorCapability, EditorSessionState, SourceModality,
    StructuralPlacement,
    anchor::Utf16Offset,
    apply_command, create_sample,
    model::{Affinity, BlockKind, ContentNode, FlowDocument, LogicalPosition, NodeId},
    recover,
    store::{CommitPlanner, DocumentStore, InMemoryDocumentStore, SnapshotPolicy, SnapshotReason},
    transaction::{Command, EditorState, TransactionService, semantic_hash},
};

fn id(value: u32) -> flow_core::model::CommandId {
    flow_core::model::CommandId::new(format!("00000000-0000-4000-8000-{value:012}"))
        .expect("valid command id")
}

fn node_id(value: u32) -> NodeId {
    NodeId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("valid node id")
}

fn command(state: &EditorState, serial: u32, kind: CommandKind) -> Command {
    Command {
        command_id: id(serial),
        base_revision: state.document().revision,
        modality: SourceModality::Keyboard,
        issued_at: "2026-09-14T20:00:00Z".to_owned(),
        kind,
    }
}

fn position(node_id: NodeId, offset: u32, affinity: Affinity) -> LogicalPosition {
    LogicalPosition {
        node_id,
        utf16_offset: Utf16Offset::new(offset),
        affinity,
    }
}

fn collapsed(node_id: NodeId) -> DirectionalSelection {
    let position = position(node_id, 0, Affinity::Forward);
    DirectionalSelection {
        anchor: position.clone(),
        focus: position,
    }
}

fn document_with_content(content: Vec<ContentNode>) -> FlowDocument {
    let mut document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    document.fields.clear();
    document.content = content;
    document
}

fn paragraph(value: u32, text: &str) -> ContentNode {
    ContentNode::paragraph(
        node_id(value),
        Some(
            FlowDocument::deterministic_sample("uk-UA")
                .expect("sample")
                .styles[0]
                .id
                .clone(),
        ),
        text.to_owned(),
    )
}

fn table(value: u32, rows: usize, columns: usize, header: bool) -> ContentNode {
    let style_id = FlowDocument::deterministic_sample("uk-UA")
        .expect("sample")
        .styles[0]
        .id
        .clone();
    let rows = (0..rows)
        .map(|row_index| ContentNode {
            id: node_id(value + 1 + row_index as u32),
            style_id: None,
            body: BlockKind::TableRow {
                cells: (0..columns)
                    .map(|column_index| ContentNode {
                        id: node_id(value + 100 + (row_index * columns + column_index) as u32),
                        style_id: None,
                        body: BlockKind::TableCell {
                            children: vec![ContentNode::paragraph(
                                node_id(
                                    value + 1_000 + (row_index * columns + column_index) as u32,
                                ),
                                Some(style_id.clone()),
                                String::new(),
                            )],
                        },
                    })
                    .collect(),
            },
        })
        .collect();
    ContentNode {
        id: node_id(value),
        style_id: None,
        body: BlockKind::Table {
            header_rows: u8::from(header),
            rows,
        },
    }
}

fn table_at(document: &FlowDocument, index: usize) -> &ContentNode {
    &document.content[index]
}

fn cell_text_id(table: &ContentNode, row: usize, column: usize) -> NodeId {
    table.children()[row].children()[column].children()[0]
        .id
        .clone()
}

fn equal_semantics(left: &FlowDocument, right: &FlowDocument) {
    assert_eq!(
        semantic_hash(left).expect("left hash"),
        semantic_hash(right).expect("right hash")
    );
}

fn success<T>(response: ApiResponse<T>) -> T {
    assert!(response.ok, "unexpected response: {:?}", response.error);
    response.value.expect("successful response")
}

#[test]
fn page_break_placement_is_atomic_and_exactly_reversible() {
    let original = document_with_content(vec![paragraph(10, "before"), paragraph(11, "after")]);
    let state = EditorState::new(original.clone()).expect("valid state");

    for (serial, index) in [(100, 0_u32), (110, 1), (120, 2)] {
        let applied = TransactionService::apply(
            &state,
            command(
                &state,
                serial,
                CommandKind::InsertPageBreak {
                    placement: StructuralPlacement {
                        parent_id: None,
                        index,
                    },
                },
            ),
        )
        .expect("page break insertion");
        assert_eq!(applied.state.document().content.len(), 4);
        assert!(matches!(
            applied.state.document().content[index as usize].body,
            BlockKind::PageBreak
        ));
        assert!(applied.selection.is_some());
        assert_eq!(
            applied.selection.as_ref().expect("focus").anchor.node_id,
            applied.state.document().content[index as usize + 1].id
        );
    }

    let inserted = TransactionService::apply(
        &state,
        command(
            &state,
            130,
            CommandKind::InsertPageBreak {
                placement: StructuralPlacement {
                    parent_id: None,
                    index: 1,
                },
            },
        ),
    )
    .expect("middle insertion");
    let page_break_id = inserted
        .state
        .document()
        .content
        .iter()
        .find(|node| matches!(node.body, BlockKind::PageBreak))
        .expect("page break")
        .id
        .clone();
    let removed = TransactionService::apply(
        &inserted.state,
        command(
            &inserted.state,
            131,
            CommandKind::RemovePageBreak {
                page_break_id: page_break_id.clone(),
            },
        ),
    )
    .expect("explicit removal");
    assert_eq!(removed.state.document().content.len(), 3);
    assert_eq!(removed.state.document().content[1].text(), "");
    assert_eq!(removed.state.document().content[2].text(), "after");
    assert_eq!(
        removed
            .selection
            .as_ref()
            .expect("nearby focus")
            .anchor
            .node_id,
        inserted.state.document().content[2].id
    );

    let undone = TransactionService::apply(
        &removed.state,
        command(&removed.state, 132, CommandKind::Undo),
    )
    .expect("undo removal");
    equal_semantics(undone.state.document(), inserted.state.document());
    let redone = TransactionService::apply(
        &undone.state,
        command(&undone.state, 133, CommandKind::Redo),
    )
    .expect("redo removal");
    equal_semantics(redone.state.document(), removed.state.document());

    let ordinary_delete = TransactionService::apply(
        &inserted.state,
        command(
            &inserted.state,
            134,
            CommandKind::DeleteText {
                range: flow_core::transaction::TextRange::collapsed(position(
                    page_break_id.clone(),
                    0,
                    Affinity::Forward,
                )),
            },
        ),
    )
    .expect_err("text deletion cannot consume an atomic page break");
    assert_eq!(ordinary_delete.code(), "FLOW_INVALID_TARGET");
    let unchanged = inserted.state.document().clone();
    equal_semantics(inserted.state.document(), &unchanged);

    let merge = TransactionService::apply(
        &inserted.state,
        command(
            &inserted.state,
            135,
            CommandKind::MergeTextBlocks {
                first_node_id: inserted.state.document().content[0].id.clone(),
                second_node_id: page_break_id,
            },
        ),
    )
    .expect_err("merge cannot consume an atomic page break");
    assert_eq!(merge.code(), "FLOW_INCOMPATIBLE_STRUCTURE");

    let stale_command = command(
        &inserted.state,
        136,
        CommandKind::RemovePageBreak {
            page_break_id: inserted.state.document().content[1].id.clone(),
        },
    );
    let stale =
        TransactionService::apply(&state, stale_command).expect_err("stale page-break command");
    assert_eq!(stale.code(), "FLOW_STALE_REVISION");
}

#[test]
fn table_commands_preserve_ids_focus_headers_and_exact_history() {
    let original = document_with_content(vec![paragraph(20, "before"), paragraph(21, "after")]);
    let state = EditorState::new(original.clone()).expect("valid state");
    let inserted = TransactionService::apply(
        &state,
        command(
            &state,
            200,
            CommandKind::InsertTable {
                placement: StructuralPlacement {
                    parent_id: None,
                    index: 1,
                },
                rows: 2,
                columns: 2,
                header_row: true,
            },
        ),
    )
    .expect("table insertion");
    let inserted_table = table_at(inserted.state.document(), 1);
    assert!(matches!(
        inserted_table.body,
        BlockKind::Table { header_rows: 1, .. }
    ));
    assert_eq!(inserted_table.children().len(), 2);
    assert_eq!(inserted_table.children()[0].children().len(), 2);
    assert_eq!(
        inserted
            .selection
            .as_ref()
            .expect("table focus")
            .anchor
            .node_id,
        cell_text_id(inserted_table, 0, 0)
    );
    let original_row_ids = inserted_table
        .children()
        .iter()
        .map(|row| row.id.clone())
        .collect::<Vec<_>>();
    let original_cell_ids = inserted_table.children()[0]
        .children()
        .iter()
        .map(|cell| cell.id.clone())
        .collect::<Vec<_>>();
    let table_id = inserted_table.id.clone();

    let header_off = TransactionService::apply(
        &inserted.state,
        command(
            &inserted.state,
            201,
            CommandKind::SetTableHeaderRow {
                table_id: table_id.clone(),
                enabled: false,
            },
        ),
    )
    .expect("header off");
    assert!(matches!(
        table_at(header_off.state.document(), 1).body,
        BlockKind::Table { header_rows: 0, .. }
    ));
    let no_op = TransactionService::apply(
        &header_off.state,
        command(
            &header_off.state,
            202,
            CommandKind::SetTableHeaderRow {
                table_id: table_id.clone(),
                enabled: false,
            },
        ),
    )
    .expect_err("same header state is a no-op");
    assert_eq!(no_op.code(), "FLOW_NO_OP");
    let header_on = TransactionService::apply(
        &header_off.state,
        command(
            &header_off.state,
            203,
            CommandKind::SetTableHeaderRow {
                table_id: table_id.clone(),
                enabled: true,
            },
        ),
    )
    .expect("header on");
    assert!(matches!(
        table_at(header_on.state.document(), 1).body,
        BlockKind::Table { header_rows: 1, .. }
    ));

    let selected = DirectionalSelection {
        anchor: position(cell_text_id(inserted_table, 0, 0), 0, Affinity::Forward),
        focus: position(cell_text_id(inserted_table, 0, 0), 0, Affinity::Forward),
    };
    let added_row = TransactionService::apply(
        &inserted.state,
        command(
            &inserted.state,
            204,
            CommandKind::AddTableRow {
                selection: selected,
            },
        ),
    )
    .expect("add row");
    let table_after_row = table_at(added_row.state.document(), 1);
    assert_eq!(table_after_row.children().len(), 3);
    assert_eq!(table_after_row.children()[0].id, original_row_ids[0]);
    assert_eq!(table_after_row.children()[2].id, original_row_ids[1]);
    assert_eq!(
        added_row
            .selection
            .as_ref()
            .expect("new row focus")
            .anchor
            .node_id,
        cell_text_id(table_after_row, 1, 0)
    );

    let added_column = TransactionService::apply(
        &added_row.state,
        command(
            &added_row.state,
            205,
            CommandKind::AddTableColumn {
                selection: added_row.selection.clone().expect("row focus"),
            },
        ),
    )
    .expect("add column");
    let table_after_column = table_at(added_column.state.document(), 1);
    assert!(
        table_after_column
            .children()
            .iter()
            .all(|row| row.children().len() == 3)
    );
    assert_eq!(
        table_after_column.children()[0].children()[0].id,
        original_cell_ids[0]
    );
    assert_eq!(
        table_after_column.children()[0].children()[2].id,
        original_cell_ids[1]
    );
    assert_eq!(
        added_column
            .selection
            .as_ref()
            .expect("new column focus")
            .anchor
            .node_id,
        cell_text_id(table_after_column, 1, 1)
    );

    let removed_row = TransactionService::apply(
        &added_column.state,
        command(
            &added_column.state,
            206,
            CommandKind::RemoveTableRow {
                selection: added_column.selection.clone().expect("column focus"),
            },
        ),
    )
    .expect("remove row");
    assert_eq!(
        table_at(removed_row.state.document(), 1).children().len(),
        2
    );
    assert_eq!(
        removed_row
            .selection
            .as_ref()
            .expect("nearest row focus")
            .anchor
            .node_id,
        cell_text_id(table_at(removed_row.state.document(), 1), 1, 1)
    );

    let removed_column = TransactionService::apply(
        &removed_row.state,
        command(
            &removed_row.state,
            207,
            CommandKind::RemoveTableColumn {
                selection: removed_row.selection.clone().expect("row focus"),
            },
        ),
    )
    .expect("remove column");
    assert!(
        table_at(removed_column.state.document(), 1)
            .children()
            .iter()
            .all(|row| row.children().len() == 2)
    );

    let undone = TransactionService::apply(
        &removed_column.state,
        command(&removed_column.state, 208, CommandKind::Undo),
    )
    .expect("undo column");
    let undone = TransactionService::apply(
        &undone.state,
        command(&undone.state, 209, CommandKind::Undo),
    )
    .expect("undo row");
    let undone = TransactionService::apply(
        &undone.state,
        command(&undone.state, 210, CommandKind::Undo),
    )
    .expect("undo column insertion");
    let undone = TransactionService::apply(
        &undone.state,
        command(&undone.state, 211, CommandKind::Undo),
    )
    .expect("undo row insertion");
    equal_semantics(undone.state.document(), inserted.state.document());
}

#[test]
fn table_bounds_and_last_dimension_removal_fail_closed_before_mutation() {
    let document = document_with_content(vec![paragraph(30, "text")]);
    let state = EditorState::new(document.clone()).expect("valid state");
    for (serial, rows, columns) in [(300, 0_u32, 2_u32), (301, 51, 1), (302, 2, 0), (303, 2, 21)] {
        let error = TransactionService::apply(
            &state,
            command(
                &state,
                serial,
                CommandKind::InsertTable {
                    placement: StructuralPlacement {
                        parent_id: None,
                        index: 1,
                    },
                    rows,
                    columns,
                    header_row: false,
                },
            ),
        )
        .expect_err("bounded table dimensions");
        assert_eq!(error.code(), "FLOW_LIMIT_TABLE");
        assert_eq!(state.document(), &document);
    }

    let one_by_one = document_with_content(vec![table(40, 1, 1, false)]);
    let one_state = EditorState::new(one_by_one.clone()).expect("one by one table");
    let cell = cell_text_id(&one_state.document().content[0], 0, 0);
    let selection = collapsed(cell);
    let row_error = TransactionService::apply(
        &one_state,
        command(
            &one_state,
            304,
            CommandKind::RemoveTableRow {
                selection: selection.clone(),
            },
        ),
    )
    .expect_err("last row escalates");
    assert_eq!(row_error.code(), "FLOW_CONFIRMATION_REQUIRED");
    let column_error = TransactionService::apply(
        &one_state,
        command(
            &one_state,
            305,
            CommandKind::RemoveTableColumn { selection },
        ),
    )
    .expect_err("last column escalates");
    assert_eq!(column_error.code(), "FLOW_CONFIRMATION_REQUIRED");

    let missing_confirmation = TransactionService::apply(
        &one_state,
        command(
            &one_state,
            306,
            CommandKind::RemoveTable {
                table_id: one_state.document().content[0].id.clone(),
                confirmed: false,
            },
        ),
    )
    .expect_err("table removal requires confirmation");
    assert_eq!(missing_confirmation.code(), "FLOW_CONFIRMATION_REQUIRED");
    assert_eq!(one_state.document(), &one_by_one);

    let removed = TransactionService::apply(
        &one_state,
        command(
            &one_state,
            307,
            CommandKind::RemoveTable {
                table_id: one_state.document().content[0].id.clone(),
                confirmed: true,
            },
        ),
    )
    .expect("confirmed table removal");
    assert_eq!(removed.state.document().content.len(), 1);
    assert!(removed.state.document().content[0].runs().is_some());
    assert!(removed.state.document().content[0].text().is_empty());
    let restored = TransactionService::apply(
        &removed.state,
        command(&removed.state, 308, CommandKind::Undo),
    )
    .expect("undo table removal");
    equal_semantics(restored.state.document(), &one_by_one);
}

#[test]
fn structural_command_duplicates_and_stale_bases_do_not_mutate() {
    let document = document_with_content(vec![paragraph(50, "text")]);
    let state = EditorState::new(document.clone()).expect("valid state");
    let insert = command(
        &state,
        400,
        CommandKind::InsertPageBreak {
            placement: StructuralPlacement {
                parent_id: None,
                index: 1,
            },
        },
    );
    let applied = TransactionService::apply(&state, insert.clone()).expect("insert");
    let duplicate = TransactionService::apply(&applied.state, insert).expect_err("duplicate");
    assert_eq!(duplicate.code(), "FLOW_DUPLICATE_COMMAND");
    assert_eq!(
        applied.state.document().revision,
        state.document().revision + 1
    );

    let stale = TransactionService::apply(
        &state,
        command(
            &applied.state,
            401,
            CommandKind::InsertTable {
                placement: StructuralPlacement {
                    parent_id: None,
                    index: 1,
                },
                rows: 1,
                columns: 1,
                header_row: false,
            },
        ),
    )
    .expect_err("stale base");
    assert_eq!(stale.code(), "FLOW_STALE_REVISION");
    equal_semantics(state.document(), &document);
}

#[test]
fn table_capabilities_project_shared_confirmation_metadata_and_header_state() {
    let document = document_with_content(vec![table(60, 1, 1, true)]);
    let table_id = document.content[0].id.clone();
    let cell_id = cell_text_id(&document.content[0], 0, 0);
    let session = EditorSessionState::from_document_with_selection(&document, collapsed(cell_id))
        .expect("table cell session");
    let remove_table = session
        .capabilities
        .iter()
        .find(|capability| capability.name == EditorCapability::RemoveTable)
        .expect("remove table capability");
    assert!(remove_table.enabled);
    let remove_row = session
        .capabilities
        .iter()
        .find(|capability| capability.name == EditorCapability::RemoveTableRow)
        .expect("remove row capability");
    let remove_column = session
        .capabilities
        .iter()
        .find(|capability| capability.name == EditorCapability::RemoveTableColumn)
        .expect("remove column capability");
    assert!(!remove_row.enabled);
    assert!(!remove_column.enabled);
    assert_eq!(
        remove_row.reason_key.as_deref(),
        Some("tableRemovalConfirmationRequired")
    );
    assert_eq!(
        remove_column.reason_key.as_deref(),
        Some("tableRemovalConfirmationRequired")
    );
    assert_eq!(remove_row.confirmation, remove_table.confirmation);
    assert_eq!(remove_column.confirmation, remove_table.confirmation);
    let view = session.view_for_document(&document);
    let header = view.document.blocks.first().expect("table projection");
    match header {
        flow_core::EditorBlockViewDto::Atomic {
            node_id,
            node_kind,
            table_header_rows,
            ..
        } => {
            assert_eq!(node_id, &table_id);
            assert_eq!(node_kind, "table");
            assert_eq!(*table_header_rows, Some(1));
        }
        _ => panic!("expected atomic table projection"),
    }
}

#[test]
fn table_projection_publishes_rust_owned_row_major_focus_order() {
    let document = document_with_content(vec![table(100, 2, 2, true)]);
    let first_cell_text = cell_text_id(table_at(&document, 0), 0, 0);
    let session =
        EditorSessionState::from_document_with_selection(&document, collapsed(first_cell_text))
            .expect("table cell session");
    let view = session.view_for_document(&document);
    let flow_core::EditorBlockViewDto::Atomic {
        table_cell_focus_order,
        ..
    } = view.document.blocks.first().expect("table projection")
    else {
        panic!("expected atomic table projection");
    };
    assert_eq!(table_cell_focus_order.len(), 4);
    let cells = table_at(&document, 0)
        .children()
        .iter()
        .flat_map(|row| row.children().iter())
        .collect::<Vec<_>>();
    for (index, focus) in table_cell_focus_order.iter().enumerate() {
        let cell = cells[index];
        assert_eq!(focus.cell_id, cell.id);
        assert_eq!(focus.selection.anchor.node_id, cell.children()[0].id);
        assert_eq!(focus.selection.anchor.utf16_offset.get(), 0);
        assert_eq!(focus.selection.anchor.affinity, Affinity::Forward);
        assert_eq!(
            focus.previous_cell_id.as_ref(),
            index
                .checked_sub(1)
                .and_then(|previous| cells.get(previous))
                .map(|cell| &cell.id)
        );
        assert_eq!(
            focus.next_cell_id.as_ref(),
            cells.get(index + 1).map(|cell| &cell.id)
        );
    }
}

#[test]
fn structural_transactions_replay_through_the_durable_store() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-09-14T20:10:00Z".to_owned(),
    }));
    let planner = CommitPlanner::new(SnapshotPolicy::EveryTransaction);
    let mut store = InMemoryDocumentStore::default();
    planner
        .commit(&mut store, created.commit, SnapshotReason::Creation)
        .expect("creation commit");
    let before = success(recover(store.load_records().expect("records")));
    let applied = success(apply_command(ApplyCommandRequest {
        canonical_json: before.session.canonical_json,
        history: before.session.history,
        command: CommandDto {
            command_id: id(500),
            base_revision: before.session.revision,
            modality: SourceModality::Ui,
            issued_at: "2026-09-14T20:10:01Z".to_owned(),
            kind: CommandKind::InsertTable {
                placement: StructuralPlacement {
                    parent_id: None,
                    index: 1,
                },
                rows: 2,
                columns: 2,
                header_row: true,
            },
        },
    }));
    planner
        .commit(
            &mut store,
            applied.commit,
            SnapshotReason::CommittedTransaction,
        )
        .expect("table commit");
    let recovered = success(recover(store.load_records().expect("table records")));
    assert_eq!(recovered.session.revision, before.session.revision + 1);
    assert_eq!(
        recovered.session.canonical_json,
        applied.session.canonical_json
    );
    assert!(recovered.editor.view.document.blocks.iter().any(|block| {
        matches!(
            block,
            EditorBlockViewDto::Atomic {
                node_kind,
                table_header_rows: Some(1),
                ..
            } if node_kind == "table"
        )
    }));
}

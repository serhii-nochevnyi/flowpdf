use flow_core::{
    ApiResponse, ApplyCommandRequest, CommandDto, CommandKind, CreateSampleRequest, RecoverRequest,
    SourceModality, apply_command, create_sample,
    model::{
        Affinity, FieldAnchorState, FieldDescriptor, FieldId, FieldKind, FieldValue, FlowDocument,
        LegacyAnchorReason, LogicalPosition, TextInputHint,
    },
    recover,
    store::{CommitPlanner, DocumentStore, InMemoryDocumentStore, SnapshotPolicy, SnapshotReason},
    transaction::{EditorState, Operation, TransactionService},
};

fn success<T>(response: ApiResponse<T>) -> T {
    assert!(response.ok, "unexpected error: {:?}", response.error);
    response.value.expect("successful response has a value")
}

fn command_id(value: u32) -> flow_core::model::CommandId {
    flow_core::model::CommandId::new(format!("00000000-0000-4000-8000-{value:012}"))
        .expect("command ID")
}

fn new_field(position: LogicalPosition) -> FieldDescriptor {
    FieldDescriptor {
        id: FieldId::new("00000000-0000-4000-8000-000000000507").expect("field ID"),
        name: "authored-new-field".to_owned(),
        label: Some("Нове поле".to_owned()),
        anchor: FieldAnchorState::GraphemeSafe { original: position },
        kind: FieldKind::Text {
            multiline: false,
            input_hint: TextInputHint::Plain,
        },
        required: false,
        read_only: false,
        default_value: FieldValue::Empty,
        options: Vec::new(),
    }
}

#[test]
fn insert_field_is_reversible_recoverable_and_capability_catalogued() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-09-22T00:00:00Z".to_owned(),
    }));
    let target = created.editor.view.selection.anchor.clone();
    let field = new_field(target.clone());
    let inserted = success(apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5071),
            base_revision: created.session.revision,
            modality: SourceModality::Ui,
            issued_at: "2026-09-22T00:00:01Z".to_owned(),
            kind: CommandKind::InsertField {
                field: field.clone(),
            },
        },
    }));

    assert_eq!(inserted.session.revision, created.session.revision + 1);
    assert_eq!(inserted.view.field_count, created.view.field_count + 1);
    assert_eq!(
        inserted.session.history.cursor,
        created.session.history.cursor + 1
    );
    assert!(inserted
        .commit
        .transaction
        .forward_operations
        .iter()
        .any(|operation| matches!(operation, Operation::InsertField { field: value, .. } if value.as_ref() == &field)));
    let inserted_view = inserted
        .editor
        .view
        .document
        .fields
        .iter()
        .find(|candidate| candidate.descriptor.id == field.id)
        .expect("inserted field projection");
    assert_eq!(inserted_view.descriptor, field);
    assert!(inserted.editor.view.capabilities.iter().any(|capability| {
        capability.name.command_type() == Some("insertField") && capability.enabled
    }));

    let undone = success(apply_command(ApplyCommandRequest {
        canonical_json: inserted.session.canonical_json.clone(),
        history: inserted.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5072),
            base_revision: inserted.session.revision,
            modality: SourceModality::Keyboard,
            issued_at: "2026-09-22T00:00:02Z".to_owned(),
            kind: CommandKind::Undo,
        },
    }));
    assert_eq!(undone.view.field_count, created.view.field_count);

    let redone = success(apply_command(ApplyCommandRequest {
        canonical_json: undone.session.canonical_json.clone(),
        history: undone.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5073),
            base_revision: undone.session.revision,
            modality: SourceModality::Keyboard,
            issued_at: "2026-09-22T00:00:03Z".to_owned(),
            kind: CommandKind::Redo,
        },
    }));
    assert_eq!(
        redone.editor.view.document.fields,
        inserted.editor.view.document.fields
    );
    assert_eq!(redone.view.field_count, inserted.view.field_count);

    let planner = CommitPlanner::new(SnapshotPolicy::EveryTransaction);
    let mut store = InMemoryDocumentStore::default();
    planner
        .commit(&mut store, created.commit.clone(), SnapshotReason::Creation)
        .expect("durable creation commit");
    planner
        .commit(
            &mut store,
            inserted.commit.clone(),
            SnapshotReason::CommittedTransaction,
        )
        .unwrap_or_else(|error| panic!("durable field insertion commit: {error:?}"));
    let recovered = success(recover(store.load_records().expect("durable records")));
    assert_eq!(
        recovered.session.canonical_json,
        inserted.session.canonical_json
    );
    assert_eq!(recovered.session.history, inserted.session.history);
    assert_eq!(recovered.view.field_count, inserted.view.field_count);
}

#[test]
fn duplicate_or_invalid_insert_field_requests_are_atomic() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-09-22T00:00:00Z".to_owned(),
    }));
    let original_json = created.session.canonical_json.clone();
    let original_hash = created.session.canonical_hash.clone();
    let duplicate = created.editor.view.document.fields[0].descriptor.clone();
    let duplicate_result = apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5081),
            base_revision: created.session.revision,
            modality: SourceModality::Api,
            issued_at: "2026-09-22T00:00:01Z".to_owned(),
            kind: CommandKind::InsertField { field: duplicate },
        },
    });
    assert!(!duplicate_result.ok);
    assert_eq!(
        duplicate_result.error.expect("duplicate error").code,
        "FLOW_BROKEN_INVARIANT"
    );

    let invalid = new_field(LogicalPosition {
        node_id: flow_core::model::NodeId::new("00000000-0000-4000-8000-000000009999")
            .expect("unknown node ID"),
        utf16_offset: 0.into(),
        affinity: Affinity::Forward,
    });
    let invalid_result = apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5082),
            base_revision: 1,
            modality: SourceModality::Api,
            issued_at: "2026-09-22T00:00:02Z".to_owned(),
            kind: CommandKind::InsertField { field: invalid },
        },
    });
    assert!(!invalid_result.ok);
    assert_eq!(
        invalid_result.error.expect("invalid error").code,
        "FLOW_DANGLING_REFERENCE"
    );
    assert_eq!(created.session.canonical_json, original_json);
    assert_eq!(created.session.canonical_hash, original_hash);
}

#[test]
fn remove_field_requires_confirmation_restores_middle_order_and_recovers() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-09-22T00:00:00Z".to_owned(),
    }));
    let original_document: FlowDocument =
        serde_json::from_str(&created.session.canonical_json).expect("canonical document");
    let original_json = created.session.canonical_json.clone();
    assert!(original_document.fields.len() >= 3);
    let target = original_document.fields[1].clone();

    let unconfirmed = apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5091),
            base_revision: created.session.revision,
            modality: SourceModality::Ui,
            issued_at: "2026-09-22T00:00:01Z".to_owned(),
            kind: CommandKind::RemoveField {
                field_id: target.id.clone(),
                confirmed: false,
            },
        },
    });
    assert!(!unconfirmed.ok);
    assert_eq!(
        unconfirmed.error.expect("confirmation error").code,
        "FLOW_CONFIRMATION_REQUIRED"
    );
    assert_eq!(created.session.canonical_json, original_json);
    assert_eq!(created.session.history.cursor, 0);

    let removed = success(apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5092),
            base_revision: created.session.revision,
            modality: SourceModality::Ui,
            issued_at: "2026-09-22T00:00:02Z".to_owned(),
            kind: CommandKind::RemoveField {
                field_id: target.id.clone(),
                confirmed: true,
            },
        },
    }));
    let mut expected_fields = original_document.fields.clone();
    expected_fields.remove(1);
    let removed_document: FlowDocument =
        serde_json::from_str(&removed.session.canonical_json).expect("removed document");
    assert_eq!(removed_document.fields, expected_fields);
    assert_eq!(removed.session.revision, created.session.revision + 1);
    assert_eq!(removed.commit.transaction.command_type, "removeField");
    assert!(removed
        .commit
        .transaction
        .forward_operations
        .iter()
        .any(|operation| matches!(operation, Operation::RemoveField { index: 1, field } if field.as_ref() == &target)));

    let undone = success(apply_command(ApplyCommandRequest {
        canonical_json: removed.session.canonical_json.clone(),
        history: removed.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5093),
            base_revision: removed.session.revision,
            modality: SourceModality::Keyboard,
            issued_at: "2026-09-22T00:00:03Z".to_owned(),
            kind: CommandKind::Undo,
        },
    }));
    let undone_document: FlowDocument =
        serde_json::from_str(&undone.session.canonical_json).expect("undone document");
    assert_eq!(undone_document.fields, original_document.fields);

    let redone = success(apply_command(ApplyCommandRequest {
        canonical_json: undone.session.canonical_json.clone(),
        history: undone.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5094),
            base_revision: undone.session.revision,
            modality: SourceModality::Keyboard,
            issued_at: "2026-09-22T00:00:04Z".to_owned(),
            kind: CommandKind::Redo,
        },
    }));
    let redone_document: FlowDocument =
        serde_json::from_str(&redone.session.canonical_json).expect("redone document");
    assert_eq!(redone_document.fields, expected_fields);

    let planner = CommitPlanner::new(SnapshotPolicy::EveryTransaction);
    let mut store = InMemoryDocumentStore::default();
    planner
        .commit(&mut store, created.commit, SnapshotReason::Creation)
        .expect("durable creation commit");
    planner
        .commit(
            &mut store,
            removed.commit,
            SnapshotReason::CommittedTransaction,
        )
        .expect("durable field removal commit");
    let recovered = success(recover(store.load_records().expect("durable records")));
    let recovered_document: FlowDocument =
        serde_json::from_str(&recovered.session.canonical_json).expect("recovered document");
    assert_eq!(recovered_document.fields, expected_fields);
    assert_eq!(recovered.session.history.cursor, 1);
}

#[test]
fn move_field_reorders_valid_tab_order_and_recovers_exactly() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-09-22T00:10:00Z".to_owned(),
    }));
    let original_document: FlowDocument =
        serde_json::from_str(&created.session.canonical_json).expect("canonical document");
    assert!(original_document.fields.len() >= 4);
    let moved_field = original_document.fields[1].clone();
    let mut expected_fields = original_document.fields.clone();
    let moved = expected_fields.remove(1);
    expected_fields.insert(3, moved);

    let same_index = apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5101),
            base_revision: created.session.revision,
            modality: SourceModality::Api,
            issued_at: "2026-09-22T00:10:01Z".to_owned(),
            kind: CommandKind::MoveField {
                field_id: moved_field.id.clone(),
                target_index: 1,
            },
        },
    });
    assert!(!same_index.ok);
    assert_eq!(
        same_index.error.expect("same-index error").code,
        "FLOW_BROKEN_INVARIANT"
    );

    let reordered = success(apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5102),
            base_revision: created.session.revision,
            modality: SourceModality::Ui,
            issued_at: "2026-09-22T00:10:02Z".to_owned(),
            kind: CommandKind::MoveField {
                field_id: moved_field.id.clone(),
                target_index: 3,
            },
        },
    }));
    let reordered_document: FlowDocument =
        serde_json::from_str(&reordered.session.canonical_json).expect("reordered document");
    assert_eq!(reordered_document.fields, expected_fields);
    assert_eq!(reordered.session.revision, created.session.revision + 1);
    assert_eq!(reordered.commit.transaction.command_type, "moveField");
    assert!(
        reordered
            .commit
            .transaction
            .forward_operations
            .iter()
            .any(|operation| matches!(
                operation,
                Operation::MoveField {
                    from_index: 1,
                    to_index: 3,
                    field,
                } if field.as_ref() == &moved_field
            ))
    );

    let out_of_range = apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5103),
            base_revision: created.session.revision,
            modality: SourceModality::Api,
            issued_at: "2026-09-22T00:10:03Z".to_owned(),
            kind: CommandKind::MoveField {
                field_id: moved_field.id.clone(),
                target_index: 99,
            },
        },
    });
    assert!(!out_of_range.ok);
    assert_eq!(
        out_of_range.error.expect("range error").code,
        "FLOW_BROKEN_INVARIANT"
    );
    assert_eq!(created.session.revision, 1);

    let undone = success(apply_command(ApplyCommandRequest {
        canonical_json: reordered.session.canonical_json.clone(),
        history: reordered.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5104),
            base_revision: reordered.session.revision,
            modality: SourceModality::Keyboard,
            issued_at: "2026-09-22T00:10:04Z".to_owned(),
            kind: CommandKind::Undo,
        },
    }));
    let undone_document: FlowDocument =
        serde_json::from_str(&undone.session.canonical_json).expect("undone document");
    assert_eq!(undone_document.fields, original_document.fields);

    let redone = success(apply_command(ApplyCommandRequest {
        canonical_json: undone.session.canonical_json.clone(),
        history: undone.session.history.clone(),
        command: CommandDto {
            command_id: command_id(5105),
            base_revision: undone.session.revision,
            modality: SourceModality::Keyboard,
            issued_at: "2026-09-22T00:10:05Z".to_owned(),
            kind: CommandKind::Redo,
        },
    }));
    let redone_document: FlowDocument =
        serde_json::from_str(&redone.session.canonical_json).expect("redone document");
    assert_eq!(redone_document.fields, expected_fields);

    let planner = CommitPlanner::new(SnapshotPolicy::EveryTransaction);
    let mut store = InMemoryDocumentStore::default();
    planner
        .commit(&mut store, created.commit.clone(), SnapshotReason::Creation)
        .expect("durable creation commit");
    planner
        .commit(
            &mut store,
            reordered.commit.clone(),
            SnapshotReason::CommittedTransaction,
        )
        .expect("durable field reorder commit");
    let recovered = success(recover(store.load_records().expect("durable records")));
    let recovered_document: FlowDocument =
        serde_json::from_str(&recovered.session.canonical_json).expect("recovered document");
    assert_eq!(recovered_document.fields, expected_fields);
    assert_eq!(recovered.session.history.cursor, 1);

    let mut forged_transaction = reordered.commit.transaction.clone();
    match &mut forged_transaction.forward_operations[0] {
        Operation::MoveField { to_index, .. } => *to_index = 0,
        operation => panic!("unexpected move operation: {operation:?}"),
    }
    let forged_recovery = recover(RecoverRequest {
        snapshots: vec![
            created.commit.snapshot.clone(),
            reordered.commit.snapshot.clone(),
        ],
        transactions: vec![created.commit.transaction.clone(), forged_transaction],
        audits: vec![created.commit.audit.clone(), reordered.commit.audit.clone()],
        assets: created.commit.assets.clone(),
        sources: Vec::new(),
    });
    assert!(!forged_recovery.ok);
    assert_eq!(
        forged_recovery.error.expect("forged replay error").code,
        "FLOW_RECOVERY_GAP"
    );
}

#[test]
fn move_field_rejects_review_and_stale_requests_without_mutating_state() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").expect("sample document");
    let review_field_id = document.fields[0].id.clone();
    let original = document.fields[0].anchor.original().clone();
    document.fields[0].anchor = FieldAnchorState::LegacyInvalid {
        original,
        reason: LegacyAnchorReason::NonGraphemeBoundary,
    };
    let state = EditorState::new(document).expect("editor state");
    let before = state.clone();

    let review = TransactionService::apply(
        &state,
        CommandDto {
            command_id: command_id(5111),
            base_revision: state.document().revision,
            modality: SourceModality::Ui,
            issued_at: "2026-09-22T00:11:00Z".to_owned(),
            kind: CommandKind::MoveField {
                field_id: review_field_id,
                target_index: 0,
            },
        },
    )
    .expect_err("review field cannot be reordered");
    assert_eq!(review.code(), "FLOW_INVALID_TARGET");
    assert_eq!(state, before);

    let valid_field_id = state.document().fields[1].id.clone();
    let stale = TransactionService::apply(
        &state,
        CommandDto {
            command_id: command_id(5112),
            base_revision: state.document().revision.saturating_sub(1),
            modality: SourceModality::Api,
            issued_at: "2026-09-22T00:11:01Z".to_owned(),
            kind: CommandKind::MoveField {
                field_id: valid_field_id,
                target_index: 0,
            },
        },
    )
    .expect_err("stale move cannot publish");
    assert_eq!(stale.code(), "FLOW_STALE_REVISION");
    assert_eq!(state, before);
}

use flow_core::{
    ApiResponse, ApplyCommandRequest, CommandDto, CommandKind, CreateSampleRequest, SourceModality,
    apply_command, create_sample,
    model::{
        Affinity, FieldAnchorState, FieldDescriptor, FieldId, FieldKind, FieldValue, FlowDocument,
        LogicalPosition, TextInputHint,
    },
    recover,
    store::{CommitPlanner, DocumentStore, InMemoryDocumentStore, SnapshotPolicy, SnapshotReason},
    transaction::Operation,
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

use flow_core::{
    ApiResponse, ApplyCommandRequest, CommandDto, CommandKind, CreateSampleRequest,
    EditorFieldReviewStatusDto, EditorFieldValueSummaryDto, EditorSessionState, RecoverRequest,
    SourceModality, apply_command, create_sample,
    model::{Affinity, FieldAnchorState, FlowDocument, LogicalPosition, TombstoneToken},
    recover,
    transaction::{EditorState, TransactionService},
};

fn success<T>(response: ApiResponse<T>) -> T {
    assert!(response.ok, "unexpected error: {:?}", response.error);
    response.value.expect("successful response has a value")
}

fn command_id(value: u32) -> flow_core::model::CommandId {
    flow_core::model::CommandId::new(format!("00000000-0000-4000-8000-{value:012}"))
        .expect("command id")
}

fn sample() -> FlowDocument {
    FlowDocument::deterministic_sample("uk-UA").expect("sample document")
}

fn position(node_id: flow_core::model::NodeId, offset: u32) -> LogicalPosition {
    LogicalPosition {
        node_id,
        utf16_offset: offset.into(),
        affinity: Affinity::Forward,
    }
}

#[test]
fn empty_one_and_many_fields_project_complete_descriptors_and_summaries() {
    let mut empty = sample();
    empty.fields.clear();
    let empty_session = EditorSessionState::from_document(&empty).expect("empty session");
    let empty_view = empty_session.view_for_document(&empty);
    assert!(empty_view.document.fields.is_empty());
    assert!(empty_view.document.field_review.is_empty());

    let mut one = sample();
    let first = one.fields[0].clone();
    one.fields = vec![first.clone()];
    let one_session = EditorSessionState::from_document(&one).expect("one-field session");
    let one_view = one_session.view_for_document(&one);
    assert_eq!(one_view.document.fields.len(), 1);
    assert_eq!(one_view.document.fields[0].descriptor, first);
    assert_eq!(
        one_view.document.fields[0].value_summary,
        EditorFieldValueSummaryDto::Text {
            value: "Тест".to_owned()
        }
    );
    assert!(one_view.document.field_review.is_empty());

    let many = sample();
    let many_session = EditorSessionState::from_document(&many).expect("many-field session");
    let many_view = many_session.view_for_document(&many);
    assert_eq!(many_view.document.fields.len(), many.fields.len());
    assert!(many_view.document.field_review.is_empty());
    assert!(many_view.document.fields.iter().all(|field| {
        field.descriptor.label.is_some() && field.descriptor.required
            || !field.descriptor.required && field.descriptor.name.starts_with("sample-")
    }));
    assert_eq!(
        many_view.document.fields[0].descriptor.kind,
        many.fields[0].kind
    );
    assert!(matches!(
        many_view.document.fields[1].value_summary,
        EditorFieldValueSummaryDto::Checked { .. }
    ));
    assert!(matches!(
        many_view.document.fields[2].value_summary,
        EditorFieldValueSummaryDto::Selected { .. }
    ));
    assert!(matches!(
        many_view.document.fields[4].value_summary,
        EditorFieldValueSummaryDto::Empty
    ));
}

#[test]
fn valid_fields_are_sorted_by_document_position_then_offset_and_id() {
    let mut document = sample();
    let first_node_id = document.content[0].id.clone();
    let second_node_id = document.content[1].id.clone();
    document.fields[0].anchor = FieldAnchorState::GraphemeSafe {
        original: position(first_node_id.clone(), 5),
    };
    document.fields[1].anchor = FieldAnchorState::GraphemeSafe {
        original: position(first_node_id, 0),
    };
    document.fields[2].anchor = FieldAnchorState::GraphemeSafe {
        original: position(second_node_id, 0),
    };
    document.fields.truncate(3);

    let session = EditorSessionState::from_document(&document).expect("session");
    let view = session.view_for_document(&document);
    let ordered_ids = view
        .document
        .fields
        .iter()
        .map(|field| field.descriptor.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(ordered_ids[0], document.fields[1].id);
    assert_eq!(ordered_ids[1], document.fields[0].id);
    assert_eq!(ordered_ids[2], document.fields[2].id);
    assert_eq!(view.document.fields[0].tab_order, 1);
    assert_eq!(view.document.fields[1].tab_order, 0);
    assert_eq!(view.document.fields[2].tab_order, 2);
}

#[test]
fn legacy_invalid_and_deleted_fields_remain_in_review_with_exact_state() {
    let mut document = sample();
    let first_position = document.fields[0].anchor.original().clone();
    let second_position = document.fields[1].anchor.original().clone();
    let tombstone = TombstoneToken {
        command_id: command_id(701),
        slot: 3,
    };
    document.fields[0].anchor = FieldAnchorState::LegacyInvalid {
        original: first_position,
        reason: flow_core::model::LegacyAnchorReason::NonGraphemeBoundary,
    };
    document.fields[1].anchor = FieldAnchorState::TargetDeleted {
        original: second_position,
        tombstone: tombstone.clone(),
    };

    let session = EditorSessionState::from_document(&document).expect("session");
    let view = session.view_for_document(&document);
    assert_eq!(view.document.fields.len(), document.fields.len() - 2);
    assert_eq!(view.document.field_review.len(), 2);
    assert!(matches!(
        view.document.field_review[0].status,
        EditorFieldReviewStatusDto::LegacyInvalid { .. }
    ));
    assert!(matches!(
        view.document.field_review[1].status,
        EditorFieldReviewStatusDto::TargetDeleted { .. }
    ));
    assert_eq!(
        view.document.field_review[1].descriptor.anchor,
        document.fields[1].anchor
    );
    assert_eq!(
        view.document.field_review[1].value_summary,
        EditorFieldValueSummaryDto::Checked { value: false }
    );
    if let EditorFieldReviewStatusDto::TargetDeleted { tombstone: actual } =
        &view.document.field_review[1].status
    {
        assert_eq!(actual, &tombstone);
    } else {
        panic!("deleted field review status was not preserved");
    }
}

#[test]
fn delete_undo_and_recovery_restore_the_exact_field_projection_without_field_authoring() {
    let document = sample();
    let first_node_id = document.content[0].id.clone();
    let state = EditorState::new(document.clone()).expect("state");
    let delete = CommandDto {
        command_id: command_id(702),
        base_revision: state.document().revision,
        modality: SourceModality::Keyboard,
        issued_at: "2026-09-15T00:00:00Z".to_owned(),
        kind: CommandKind::DeleteSubtree {
            node_id: first_node_id.clone(),
        },
    };
    let deleted = TransactionService::apply(&state, delete)
        .expect("delete")
        .state;
    let deleted_session = EditorSessionState::from_document(deleted.document()).expect("session");
    let deleted_view = deleted_session.view_for_document(deleted.document());
    assert!(deleted_view.document.fields.is_empty());
    assert_eq!(
        deleted_view.document.field_review.len(),
        document.fields.len()
    );
    assert!(
        deleted_view
            .document
            .field_review
            .iter()
            .all(|field| matches!(
                field.status,
                EditorFieldReviewStatusDto::TargetDeleted { .. }
            ))
    );

    let undo = CommandDto {
        command_id: command_id(703),
        base_revision: deleted.document().revision,
        modality: SourceModality::Keyboard,
        issued_at: "2026-09-15T00:00:01Z".to_owned(),
        kind: CommandKind::Undo,
    };
    let restored = TransactionService::apply(&deleted, undo)
        .expect("undo")
        .state;
    let restored_session = EditorSessionState::from_document(restored.document()).expect("session");
    let restored_view = restored_session.view_for_document(restored.document());
    assert_eq!(restored_view.document.fields.len(), document.fields.len());
    assert!(restored_view.document.field_review.is_empty());
    assert_eq!(
        restored_view.document.fields,
        EditorSessionState::from_document(&document)
            .expect("original session")
            .view_for_document(&document)
            .document
            .fields
    );

    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-09-15T00:00:00Z".to_owned(),
    }));
    let deleted_result = success(apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: command_id(704),
            base_revision: created.session.revision,
            modality: SourceModality::Api,
            issued_at: "2026-09-15T00:00:02Z".to_owned(),
            kind: CommandKind::DeleteSubtree {
                node_id: first_node_id,
            },
        },
    }));
    let recovered = success(recover(RecoverRequest {
        snapshots: vec![
            created.commit.snapshot.clone(),
            deleted_result.commit.snapshot.clone(),
        ],
        transactions: vec![
            created.commit.transaction.clone(),
            deleted_result.commit.transaction.clone(),
        ],
        audits: vec![
            created.commit.audit.clone(),
            deleted_result.commit.audit.clone(),
        ],
        assets: created.commit.assets.clone(),
        sources: Vec::new(),
    }));
    assert!(recovered.editor.view.document.fields.is_empty());
    assert_eq!(
        recovered.editor.view.document.field_review.len(),
        document.fields.len()
    );
    assert!(
        recovered
            .editor
            .view
            .capabilities
            .iter()
            .all(|capability| { capability.name.command_type() != Some("setField") })
    );
}

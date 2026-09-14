use flow_core::{
    ApiResponse, ApplyCommandRequest, CommandDto, CommandKind, CreateSampleRequest,
    DirectionalSelection, OperationResult, SourceModality, apply_command, create_sample,
    model::{Affinity, CommandId, LogicalPosition},
    recover,
    store::{CommitPlanner, DocumentStore, InMemoryDocumentStore, SnapshotPolicy, SnapshotReason},
};

fn success<T>(response: ApiResponse<T>) -> T {
    assert!(
        response.ok,
        "unexpected response error: {:?}",
        response.error
    );
    response.value.expect("successful response has a value")
}

fn command_id(value: u32) -> CommandId {
    CommandId::new(format!("00000000-0000-4000-8000-{value:012}"))
        .expect("valid deterministic command id")
}

fn command(
    id: u32,
    base_revision: u32,
    canonical_json: &str,
    history: &flow_core::HistoryState,
    kind: CommandKind,
) -> ApplyCommandRequest {
    ApplyCommandRequest {
        canonical_json: canonical_json.to_owned(),
        history: history.clone(),
        command: CommandDto {
            command_id: command_id(id),
            base_revision,
            modality: SourceModality::Ui,
            issued_at: "2026-08-14T21:00:00Z".to_owned(),
            kind,
        },
    }
}

fn whole_first_paragraph(result: &OperationResult) -> DirectionalSelection {
    let target = result
        .session
        .next_command_target
        .clone()
        .expect("the sample has an editable paragraph");
    DirectionalSelection {
        anchor: LogicalPosition {
            node_id: target.node_id.clone(),
            utf16_offset: 0.into(),
            affinity: Affinity::Forward,
        },
        focus: target,
    }
}

fn replace_request(result: &OperationResult, id: u32) -> ApplyCommandRequest {
    command(
        id,
        result.session.revision,
        &result.session.canonical_json,
        &result.session.history,
        CommandKind::ReplaceSelection {
            selection: whole_first_paragraph(result),
            text: "English contract — український текст.".to_owned(),
        },
    )
}

#[test]
fn paragraph_tracer() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
    }));
    let created_text = created.editor.view.document.blocks[0].clone();

    let replace = replace_request(&created, 9_201);
    let applied = success(apply_command(replace.clone()));
    assert_eq!(applied.session.revision, 2);
    assert_eq!(applied.commit.transaction.command_type, "replaceSelection");
    assert_eq!(applied.commit.transaction.forward_operations.len(), 1);
    assert_eq!(applied.commit.transaction.inverse_operations.len(), 1);
    assert_eq!(
        applied.editor.view.selection.anchor,
        applied.editor.view.selection.focus
    );
    assert_eq!(
        applied.editor.view.selection.focus.utf16_offset.get(),
        "English contract — український текст."
            .encode_utf16()
            .count() as u32
    );
    assert_ne!(applied.editor.view.document.blocks[0], created_text);

    let stale = command(
        9_202,
        created.session.revision,
        &applied.session.canonical_json,
        &applied.session.history,
        CommandKind::ReplaceSelection {
            selection: applied.editor.session.selection.clone(),
            text: "stale".to_owned(),
        },
    );
    let stale_response = apply_command(stale);
    assert!(!stale_response.ok);
    assert_eq!(
        stale_response.error.expect("stale error").code,
        "FLOW_STALE_REVISION"
    );
    assert!(stale_response.value.is_none());

    let duplicate = apply_command(ApplyCommandRequest {
        canonical_json: applied.session.canonical_json.clone(),
        history: applied.session.history.clone(),
        command: replace.command.clone(),
    });
    assert!(!duplicate.ok);
    assert_eq!(
        duplicate.error.expect("duplicate error").code,
        "FLOW_DUPLICATE_COMMAND"
    );
    assert!(duplicate.value.is_none());

    let combining_text = created
        .editor
        .view
        .document
        .blocks
        .iter()
        .find_map(|block| match block {
            flow_core::EditorBlockViewDto::Paragraph { node_id, text, .. } => {
                Some((node_id.clone(), text.clone()))
            }
            _ => None,
        })
        .expect("the sample paragraph projection");
    let combining_byte = combining_text
        .1
        .find("и\u{0306}")
        .expect("combining sample text")
        + "и".len();
    let combining_offset = combining_text.1[..combining_byte].encode_utf16().count() as u32;
    let invalid_grapheme = command(
        9_203,
        created.session.revision,
        &created.session.canonical_json,
        &created.session.history,
        CommandKind::ReplaceSelection {
            selection: DirectionalSelection {
                anchor: LogicalPosition {
                    node_id: combining_text.0.clone(),
                    utf16_offset: combining_offset.into(),
                    affinity: Affinity::Forward,
                },
                focus: LogicalPosition {
                    node_id: combining_text.0,
                    utf16_offset: (combining_offset + 1).into(),
                    affinity: Affinity::Forward,
                },
            },
            text: "x".to_owned(),
        },
    );
    let invalid_grapheme_response = apply_command(invalid_grapheme);
    assert!(!invalid_grapheme_response.ok);
    assert_eq!(
        invalid_grapheme_response
            .error
            .expect("grapheme error")
            .code,
        "FLOW_INVALID_GRAPHEME_BOUNDARY"
    );
    assert!(invalid_grapheme_response.value.is_none());

    let no_op = command(
        9_204,
        created.session.revision,
        &created.session.canonical_json,
        &created.session.history,
        CommandKind::ReplaceSelection {
            selection: created.editor.session.selection.clone(),
            text: String::new(),
        },
    );
    let no_op_response = apply_command(no_op);
    assert!(!no_op_response.ok);
    assert_eq!(
        no_op_response.error.expect("no-op error").code,
        "FLOW_NO_OP"
    );
    assert!(no_op_response.value.is_none());

    let undone = success(apply_command(command(
        9_205,
        applied.session.revision,
        &applied.session.canonical_json,
        &applied.session.history,
        CommandKind::Undo,
    )));
    assert_eq!(undone.session.revision, 3);
    assert_eq!(undone.editor.view.document.blocks[0], created_text);

    let redone = success(apply_command(command(
        9_206,
        undone.session.revision,
        &undone.session.canonical_json,
        &undone.session.history,
        CommandKind::Redo,
    )));
    assert_eq!(redone.session.revision, 4);
    assert_eq!(
        redone.editor.view.document.blocks[0],
        applied.editor.view.document.blocks[0]
    );
    assert_eq!(redone.editor.view.selection, created.editor.view.selection);

    let planner = CommitPlanner::new(SnapshotPolicy::EveryTransaction);
    let mut store = InMemoryDocumentStore::default();
    for (index, result) in [&created, &applied, &undone, &redone]
        .into_iter()
        .enumerate()
    {
        let reason = if index == 0 {
            SnapshotReason::Creation
        } else {
            SnapshotReason::CommittedTransaction
        };
        planner
            .commit(&mut store, result.commit.clone(), reason)
            .unwrap_or_else(|error| panic!("durable transaction commit at {index}: {error:?}"));
    }

    let recovered = success(recover(store.load_records().expect("records")));
    assert_eq!(recovered.session.revision, redone.session.revision);
    assert_eq!(
        recovered.session.canonical_json,
        redone.session.canonical_json
    );
    assert_eq!(
        recovered.session.canonical_hash,
        redone.session.canonical_hash
    );
    assert_eq!(recovered.session.history, redone.session.history);
    assert_eq!(
        recovered.editor.session.selection,
        redone.editor.session.selection
    );
    assert_eq!(recovered.editor.view.document, redone.editor.view.document);
}

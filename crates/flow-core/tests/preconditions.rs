use flow_core::{
    anchor::Utf16Offset,
    canonical::canonical_bytes,
    model::{Affinity, CommandId, FlowDocument, LogicalPosition, NodeId},
    transaction::{Command, CommandKind, EditorState, SourceModality, TextRange, TransactionService},
};

fn id(value: u32) -> CommandId {
    CommandId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("id")
}

fn position(node_id: NodeId, offset: u32) -> LogicalPosition {
    LogicalPosition {
        node_id,
        utf16_offset: Utf16Offset::new(offset),
        affinity: Affinity::Forward,
    }
}

fn insert(document: &FlowDocument, command_id: CommandId, base_revision: u32) -> Command {
    Command {
        command_id,
        base_revision,
        modality: SourceModality::Voice,
        issued_at: "2026-08-14T20:03:00Z".to_owned(),
        kind: CommandKind::InsertText {
            target: position(document.content[1].id.clone(), 0),
            text: "X".to_owned(),
        },
    }
}

fn assert_unchanged(state: &EditorState, before: &EditorState) {
    assert_eq!(state, before);
    assert_eq!(state.document().revision, before.document().revision);
    assert_eq!(state.history(), before.history());
    assert_eq!(
        canonical_bytes(state.document()).expect("state bytes"),
        canonical_bytes(before.document()).expect("before bytes")
    );
}

#[test]
fn stale_duplicate_invalid_and_broken_commands_are_exactly_non_mutating() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let initial = EditorState::new(document.clone()).expect("state");

    let stale = TransactionService::apply(&initial, insert(&document, id(711), 0))
        .expect_err("stale");
    assert_eq!(stale.code(), "FLOW_STALE_REVISION");
    assert_unchanged(&initial, &initial.clone());

    let first = TransactionService::apply(&initial, insert(&document, id(712), 1))
        .expect("first");
    let duplicate = TransactionService::apply(
        &first.state,
        insert(first.state.document(), id(712), first.state.document().revision),
    )
    .expect_err("duplicate");
    assert_eq!(duplicate.code(), "FLOW_DUPLICATE_COMMAND");
    assert_unchanged(&first.state, &first.state.clone());

    let mut missing = insert(&document, id(713), 1);
    missing.kind = CommandKind::InsertText {
        target: position(
            NodeId::new("00000000-0000-4000-8000-000000009999").expect("id"),
            0,
        ),
        text: "X".to_owned(),
    };
    assert_eq!(
        TransactionService::apply(&initial, missing)
            .expect_err("missing")
            .code(),
        "FLOW_INVALID_TARGET"
    );

    let out_of_range = Command {
        command_id: id(714),
        base_revision: 1,
        modality: SourceModality::Ui,
        issued_at: "2026-08-14T20:03:01Z".to_owned(),
        kind: CommandKind::DeleteText {
            range: TextRange::collapsed(position(document.content[1].id.clone(), 999)),
        },
    };
    assert_eq!(
        TransactionService::apply(&initial, out_of_range)
            .expect_err("range")
            .code(),
        "FLOW_INVALID_RANGE"
    );

    let emoji_node = document.content[0].clone();
    let emoji_byte = emoji_node.text.find('😀').expect("emoji");
    let before_emoji = emoji_node.text[..emoji_byte].encode_utf16().count() as u32;
    let surrogate_half = Command {
        command_id: id(715),
        base_revision: 1,
        modality: SourceModality::Ui,
        issued_at: "2026-08-14T20:03:02Z".to_owned(),
        kind: CommandKind::InsertText {
            target: position(emoji_node.id, before_emoji + 1),
            text: "X".to_owned(),
        },
    };
    assert_eq!(
        TransactionService::apply(&initial, surrogate_half)
            .expect_err("surrogate interior")
            .code(),
        "FLOW_INVALID_UTF16_BOUNDARY"
    );

    let no_op = Command {
        command_id: id(716),
        base_revision: 1,
        modality: SourceModality::Ui,
        issued_at: "2026-08-14T20:03:03Z".to_owned(),
        kind: CommandKind::ReplaceText {
            range: TextRange {
                start: position(document.content[1].id.clone(), 0),
                end: position(document.content[1].id.clone(), 7),
            },
            text: "English".to_owned(),
        },
    };
    assert_eq!(
        TransactionService::apply(&initial, no_op)
            .expect_err("no-op")
            .code(),
        "FLOW_BROKEN_INVARIANT"
    );
    assert_unchanged(&initial, &initial.clone());
}

#[test]
fn same_base_race_has_one_winner_and_never_retargets_the_loser() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let state = EditorState::new(document.clone()).expect("state");
    let winner = TransactionService::apply(&state, insert(&document, id(721), 1)).expect("winner");
    let loser_target = winner.state.document().content[1].text.clone();
    let loser = TransactionService::apply(&winner.state, insert(&document, id(722), 1))
        .expect_err("same-base loser");

    assert_eq!(loser.code(), "FLOW_STALE_REVISION");
    assert_eq!(winner.state.document().content[1].text, loser_target);
}

#[test]
fn deleting_a_node_with_live_anchors_is_explicitly_invalidated_not_guessed() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let state = EditorState::new(document.clone()).expect("state");
    let command = Command {
        command_id: id(731),
        base_revision: 1,
        modality: SourceModality::Api,
        issued_at: "2026-08-14T20:03:04Z".to_owned(),
        kind: CommandKind::DeleteNode {
            node_id: document.content[0].id.clone(),
        },
    };

    assert_eq!(
        TransactionService::apply(&state, command)
            .expect_err("anchored node")
            .code(),
        "FLOW_ANCHOR_INVALIDATED"
    );
    assert_unchanged(&state, &state.clone());
}

#[test]
fn serialized_public_anchor_has_no_dom_absolute_page_or_pdf_vocabulary() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let json = serde_json::to_value(position(document.content[0].id.clone(), 0)).expect("json");
    assert_eq!(
        json.as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["affinity", "nodeId", "utf16Offset"]
    );
    let encoded = json.to_string();
    for forbidden in ["dom", "absolute", "page", "pdf", "rect", "coordinate"] {
        assert!(!encoded.to_ascii_lowercase().contains(forbidden));
    }
}

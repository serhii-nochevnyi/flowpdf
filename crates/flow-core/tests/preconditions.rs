use flow_core::{
    ApplyCommandRequest, CreateSampleRequest, RecoverRequest,
    anchor::{
        AnchorError, AnchorInvalidation, AnchorMapResult, AnchorMapping, AnchorTransformation,
        NativeByteOffset, Utf16Offset, byte_to_utf16_offset, resolve_utf16_offset,
    },
    canonical::canonical_bytes,
    model::{Affinity, CommandId, FlowDocument, LogicalPosition, NodeId},
    transaction::{
        Command, CommandKind, EditorState, Mutation, Operation, SourceModality, TextRange,
        TransactionService,
    },
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

    let before_stale = initial.clone();
    let stale =
        TransactionService::apply(&initial, insert(&document, id(711), 0)).expect_err("stale");
    assert_eq!(stale.code(), "FLOW_STALE_REVISION");
    assert_unchanged(&initial, &before_stale);

    let first = TransactionService::apply(&initial, insert(&document, id(712), 1)).expect("first");
    let before_duplicate = first.state.clone();
    let duplicate = TransactionService::apply(
        &first.state,
        insert(
            first.state.document(),
            id(712),
            first.state.document().revision,
        ),
    )
    .expect_err("duplicate");
    assert_eq!(duplicate.code(), "FLOW_DUPLICATE_COMMAND");
    assert_unchanged(&first.state, &before_duplicate);

    let mut missing = insert(&document, id(713), 1);
    missing.kind = CommandKind::InsertText {
        target: position(
            NodeId::new("00000000-0000-4000-8000-000000009999").expect("id"),
            0,
        ),
        text: "X".to_owned(),
    };
    let before_missing = initial.clone();
    assert_eq!(
        TransactionService::apply(&initial, missing)
            .expect_err("missing")
            .code(),
        "FLOW_INVALID_TARGET"
    );
    assert_unchanged(&initial, &before_missing);

    let out_of_range = Command {
        command_id: id(714),
        base_revision: 1,
        modality: SourceModality::Ui,
        issued_at: "2026-08-14T20:03:01Z".to_owned(),
        kind: CommandKind::DeleteText {
            range: TextRange::collapsed(position(document.content[1].id.clone(), 999)),
        },
    };
    let before_out_of_range = initial.clone();
    assert_eq!(
        TransactionService::apply(&initial, out_of_range)
            .expect_err("range")
            .code(),
        "FLOW_INVALID_RANGE"
    );
    assert_unchanged(&initial, &before_out_of_range);

    let emoji_node = document.content[0].clone();
    let emoji_byte = emoji_node.text().find('😀').expect("emoji");
    let before_emoji = emoji_node.text()[..emoji_byte].encode_utf16().count() as u32;
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
    let before_surrogate_half = initial.clone();
    assert_eq!(
        TransactionService::apply(&initial, surrogate_half)
            .expect_err("surrogate interior")
            .code(),
        "FLOW_INVALID_UTF16_BOUNDARY"
    );
    assert_unchanged(&initial, &before_surrogate_half);

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
    let before_no_op = initial.clone();
    assert_eq!(
        TransactionService::apply(&initial, no_op)
            .expect_err("no-op")
            .code(),
        "FLOW_BROKEN_INVARIANT"
    );
    assert_unchanged(&initial, &before_no_op);
}

#[test]
fn same_base_race_has_one_winner_and_never_retargets_the_loser() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let state = EditorState::new(document.clone()).expect("state");
    let winner = TransactionService::apply(&state, insert(&document, id(721), 1)).expect("winner");
    let loser_target = winner.state.document().content[1].text().clone();
    let loser = TransactionService::apply(&winner.state, insert(&document, id(722), 1))
        .expect_err("same-base loser");

    assert_eq!(loser.code(), "FLOW_STALE_REVISION");
    assert_eq!(winner.state.document().content[1].text(), loser_target);
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

    let before = state.clone();
    assert_eq!(
        TransactionService::apply(&state, command)
            .expect_err("anchored node")
            .code(),
        "FLOW_ANCHOR_INVALIDATED"
    );
    assert_unchanged(&state, &before);
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

#[test]
fn utf16_and_native_offsets_round_trip_every_scalar_boundary_without_a_grapheme_claim() {
    let text = "Aи\u{0306}😀𝄞Б";
    for byte_index in text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
    {
        let native = NativeByteOffset::new(byte_index);
        let utf16 = byte_to_utf16_offset(text, native).expect("native scalar boundary");
        assert_eq!(
            resolve_utf16_offset(text, utf16).expect("UTF-16 scalar boundary"),
            native
        );
    }

    let combining_mark_byte = text.find('\u{0306}').expect("combining mark");
    let between_base_and_mark =
        byte_to_utf16_offset(text, NativeByteOffset::new(combining_mark_byte)).expect("boundary");
    assert!(
        resolve_utf16_offset(text, between_base_and_mark).is_ok(),
        "Phase 1 validates scalar/UTF-16 boundaries but intentionally does not claim grapheme safety"
    );

    let emoji_units = text[..text.find('😀').expect("emoji")]
        .encode_utf16()
        .count() as u32;
    assert_eq!(
        resolve_utf16_offset(text, Utf16Offset::new(emoji_units + 1)),
        Err(AnchorError::InvalidUtf16Boundary)
    );
    assert_eq!(
        resolve_utf16_offset(text, Utf16Offset::new(999)),
        Err(AnchorError::OutOfRange)
    );
}

#[test]
fn insertion_mapping_uses_affinity_and_composes_in_declared_order() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let node_id = document.content[1].id.clone();
    let mapping = AnchorMapping {
        transformations: vec![
            AnchorTransformation::TextEdit {
                node_id: node_id.clone(),
                start: Utf16Offset::new(2),
                removed_utf16_length: 0,
                inserted_utf16_length: 3,
            },
            AnchorTransformation::TextEdit {
                node_id: node_id.clone(),
                start: Utf16Offset::new(5),
                removed_utf16_length: 0,
                inserted_utf16_length: 1,
            },
        ],
    };
    let map = |offset, affinity| {
        mapping.map(&LogicalPosition {
            node_id: node_id.clone(),
            utf16_offset: Utf16Offset::new(offset),
            affinity,
        })
    };

    assert_eq!(
        map(1, Affinity::Forward),
        AnchorMapResult::Mapped(position(node_id.clone(), 1))
    );
    assert_eq!(
        map(2, Affinity::Backward),
        AnchorMapResult::Mapped(LogicalPosition {
            node_id: node_id.clone(),
            utf16_offset: Utf16Offset::new(2),
            affinity: Affinity::Backward,
        })
    );
    assert_eq!(
        map(2, Affinity::Forward),
        AnchorMapResult::Mapped(position(node_id.clone(), 6))
    );
    assert_eq!(
        map(3, Affinity::Forward),
        AnchorMapResult::Mapped(position(node_id, 7))
    );
}

#[test]
fn replace_delete_and_node_invalidation_never_guess_an_interior_target() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let node_id = document.content[1].id.clone();
    let replacement = AnchorMapping {
        transformations: vec![AnchorTransformation::TextEdit {
            node_id: node_id.clone(),
            start: Utf16Offset::new(2),
            removed_utf16_length: 4,
            inserted_utf16_length: 1,
        }],
    };
    let mapped = |offset, affinity| {
        replacement.map(&LogicalPosition {
            node_id: node_id.clone(),
            utf16_offset: Utf16Offset::new(offset),
            affinity,
        })
    };

    assert_eq!(
        mapped(1, Affinity::Forward),
        AnchorMapResult::Mapped(position(node_id.clone(), 1))
    );
    assert_eq!(
        mapped(3, Affinity::Forward),
        AnchorMapResult::Invalid(AnchorInvalidation::DeletedText)
    );
    assert_eq!(
        mapped(7, Affinity::Forward),
        AnchorMapResult::Mapped(position(node_id.clone(), 4))
    );
    assert_eq!(
        mapped(2, Affinity::Backward),
        AnchorMapResult::Mapped(LogicalPosition {
            node_id: node_id.clone(),
            utf16_offset: Utf16Offset::new(2),
            affinity: Affinity::Backward,
        })
    );
    assert_eq!(
        mapped(2, Affinity::Forward),
        AnchorMapResult::Mapped(LogicalPosition {
            node_id: node_id.clone(),
            utf16_offset: Utf16Offset::new(3),
            affinity: Affinity::Backward,
        })
    );
    assert_eq!(
        mapped(6, Affinity::Backward),
        AnchorMapResult::Mapped(LogicalPosition {
            node_id: node_id.clone(),
            utf16_offset: Utf16Offset::new(2),
            affinity: Affinity::Forward,
        })
    );
    assert_eq!(
        mapped(6, Affinity::Forward),
        AnchorMapResult::Mapped(position(node_id.clone(), 3))
    );

    let deletion = AnchorMapping {
        transformations: vec![AnchorTransformation::TextEdit {
            node_id: node_id.clone(),
            start: Utf16Offset::new(2),
            removed_utf16_length: 4,
            inserted_utf16_length: 0,
        }],
    };
    assert_eq!(
        deletion.map(&position(node_id.clone(), 2)),
        AnchorMapResult::Invalid(AnchorInvalidation::DeletedText)
    );
    assert_eq!(
        deletion.map(&LogicalPosition {
            node_id: node_id.clone(),
            utf16_offset: Utf16Offset::new(6),
            affinity: Affinity::Backward,
        }),
        AnchorMapResult::Invalid(AnchorInvalidation::DeletedText)
    );

    let deleted_node = AnchorMapping {
        transformations: vec![AnchorTransformation::NodeInvalidated {
            node_id: node_id.clone(),
        }],
    };
    assert_eq!(
        deleted_node.map(&position(node_id, 0)),
        AnchorMapResult::Invalid(AnchorInvalidation::DeletedNode)
    );
}

#[test]
fn oversized_batch_is_rejected_before_any_target_is_examined() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let state = EditorState::new(document).expect("state");
    let missing = position(
        NodeId::new("00000000-0000-4000-8000-000000009998").expect("id"),
        0,
    );
    let mutations = (0..5_001)
        .map(|_| Mutation::InsertText {
            target: missing.clone(),
            text: "X".to_owned(),
        })
        .collect();
    let command = Command {
        command_id: id(741),
        base_revision: 1,
        modality: SourceModality::Api,
        issued_at: "2026-08-14T20:03:05Z".to_owned(),
        kind: CommandKind::Batch { mutations },
    };

    assert_eq!(
        TransactionService::apply(&state, command)
            .expect_err("operation ceiling")
            .code(),
        "FLOW_LIMIT_TRANSACTION_OPERATIONS"
    );
}

#[test]
fn recovery_replays_transaction_meaning_and_rejects_tampered_inverse() {
    let created = flow_core::create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
    })
    .value
    .expect("sample");
    let applied = flow_core::apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: Command {
            command_id: id(751),
            base_revision: created.session.revision,
            modality: SourceModality::Api,
            issued_at: "2026-08-14T20:03:06Z".to_owned(),
            kind: CommandKind::InsertText {
                target: created
                    .session
                    .next_command_target
                    .clone()
                    .expect("sample command target"),
                text: "X".to_owned(),
            },
        },
    })
    .value
    .expect("applied command");

    let mut tampered = applied.commit.transaction.clone();
    match &mut tampered.inverse_operations[0] {
        Operation::ReplaceText { replacement, .. } => replacement.push_str("tampered"),
        operation => panic!("unexpected inverse operation: {operation:?}"),
    }
    let response = flow_core::recover(RecoverRequest {
        snapshots: vec![created.commit.snapshot, applied.commit.snapshot],
        transactions: vec![created.commit.transaction, tampered],
        audits: vec![created.commit.audit, applied.commit.audit],
        assets: created.commit.assets,
        sources: Vec::new(),
    });

    assert!(!response.ok);
    assert!(response.value.is_none());
    assert_eq!(
        response.error.expect("stable recovery error").code,
        "FLOW_RECOVERY_GAP"
    );
}

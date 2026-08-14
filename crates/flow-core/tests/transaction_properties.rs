use flow_core::{
    anchor::Utf16Offset,
    model::{Affinity, CommandId, ContentNode, FlowDocument, LogicalPosition, NodeId},
    transaction::{
        Command, CommandError, CommandKind, EditorState, HistoryEffect, SourceModality, TextRange,
        Transaction, TransactionService, semantic_hash,
    },
};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

fn command_id(serial: u32) -> CommandId {
    CommandId::new(format!("10000000-0000-4000-8000-{serial:012}")).expect("command id")
}

fn command(state: &EditorState, serial: u32, kind: CommandKind) -> Command {
    Command {
        command_id: command_id(serial),
        base_revision: state.document().revision,
        modality: SourceModality::Api,
        issued_at: format!("2026-08-14T20:{:02}:00Z", serial % 60),
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

fn apply(
    state: &EditorState,
    serial: u32,
    kind: CommandKind,
) -> Result<(EditorState, Transaction), CommandError> {
    TransactionService::apply(state, command(state, serial, kind))
        .map(|applied| (applied.state, applied.transaction))
}

#[test]
fn empty_single_and_repeated_history_commands_are_atomic_and_idempotent() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let initial = EditorState::new(document.clone()).expect("state");
    assert_eq!(
        apply(&initial, 801, CommandKind::Undo)
            .expect_err("empty undo")
            .code(),
        "FLOW_UNDO_EMPTY"
    );
    assert_eq!(
        apply(&initial, 802, CommandKind::Redo)
            .expect_err("empty redo")
            .code(),
        "FLOW_REDO_EMPTY"
    );

    let target = position(document.content[1].id.clone(), 0, Affinity::Forward);
    let (inserted, _) = apply(
        &initial,
        803,
        CommandKind::InsertText {
            target,
            text: "X".to_owned(),
        },
    )
    .expect("insert");
    let (undone, undo_transaction) = apply(&inserted, 804, CommandKind::Undo).expect("undo");
    assert!(matches!(
        undo_transaction.history_effect,
        HistoryEffect::Undo { .. }
    ));
    assert_eq!(
        semantic_hash(undone.document()).expect("hash"),
        semantic_hash(&document).expect("original")
    );
    assert_eq!(
        apply(&undone, 805, CommandKind::Undo)
            .expect_err("cannot undo twice")
            .code(),
        "FLOW_UNDO_EMPTY"
    );

    let duplicate = Command {
        command_id: command_id(804),
        base_revision: undone.document().revision,
        modality: SourceModality::Api,
        issued_at: "2026-08-14T20:04:01Z".to_owned(),
        kind: CommandKind::Redo,
    };
    assert_eq!(
        TransactionService::apply(&undone, duplicate)
            .expect_err("duplicate undo id")
            .code(),
        "FLOW_DUPLICATE_COMMAND"
    );

    let (redone, redo_transaction) = apply(&undone, 806, CommandKind::Redo).expect("redo");
    assert!(matches!(
        redo_transaction.history_effect,
        HistoryEffect::Redo { .. }
    ));
    assert_eq!(
        semantic_hash(redone.document()).expect("redone"),
        semantic_hash(inserted.document()).expect("inserted")
    );
    assert_eq!(redone.document().revision, 4);
}

#[test]
fn replacement_boundary_affinity_makes_field_anchor_undo_exactly_reversible() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    assert_eq!(document.fields[0].anchor.utf16_offset.get(), 0);
    assert_eq!(document.fields[0].anchor.affinity, Affinity::Forward);
    let initial_hash = semantic_hash(&document).expect("initial hash");
    let initial = EditorState::new(document.clone()).expect("state");
    let range = TextRange {
        start: position(document.content[0].id.clone(), 0, Affinity::Backward),
        end: position(document.content[0].id.clone(), 10, Affinity::Forward),
    };
    let (replaced, _) = apply(
        &initial,
        811,
        CommandKind::ReplaceText {
            range,
            text: "X".to_owned(),
        },
    )
    .expect("replace");
    let (undone, _) = apply(&replaced, 812, CommandKind::Undo).expect("undo exact anchor mapping");

    assert_eq!(
        semantic_hash(undone.document()).expect("undo hash"),
        initial_hash
    );
    assert_eq!(undone.document().fields, document.fields);
}

#[test]
fn content_style_structure_and_field_transactions_all_undo_and_redo_in_order() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let original_hash = semantic_hash(&document).expect("original");
    let original_image = document.content[2].clone();
    let mut state = EditorState::new(document.clone()).expect("state");

    let target = position(document.content[1].id.clone(), 0, Affinity::Forward);
    (state, _) = apply(
        &state,
        821,
        CommandKind::InsertText {
            target,
            text: "Зміст ".to_owned(),
        },
    )
    .expect("content");
    (state, _) = apply(
        &state,
        822,
        CommandKind::SetNodeStyle {
            node_id: document.content[1].id.clone(),
            style_id: Some(document.styles[0].id.clone()),
        },
    )
    .expect("style");
    (state, _) = apply(
        &state,
        823,
        CommandKind::DeleteNode {
            node_id: original_image.id.clone(),
        },
    )
    .expect("structure");
    let mut replacement_field = state.document().fields[0].clone();
    replacement_field.label = Some("Оновлена мітка".to_owned());
    (state, _) = apply(
        &state,
        824,
        CommandKind::SetField {
            field_id: replacement_field.id.clone(),
            field: replacement_field,
        },
    )
    .expect("field");
    let final_hash = semantic_hash(state.document()).expect("final");

    for serial in 831..835 {
        (state, _) = apply(&state, serial, CommandKind::Undo).expect("ordered undo");
    }
    assert_eq!(
        semantic_hash(state.document()).expect("undone"),
        original_hash
    );
    assert_eq!(
        state.document(),
        &FlowDocument {
            revision: state.document().revision,
            ..document.clone()
        }
    );

    for serial in 841..845 {
        (state, _) = apply(&state, serial, CommandKind::Redo).expect("ordered redo");
    }
    assert_eq!(semantic_hash(state.document()).expect("redone"), final_hash);
    assert!(!state.document().content.contains(&original_image));
}

#[test]
fn a_new_commit_after_undo_discards_redo_without_reusing_old_command_ids() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let initial = EditorState::new(document.clone()).expect("state");
    let (first, _) = apply(
        &initial,
        851,
        CommandKind::InsertText {
            target: position(document.content[1].id.clone(), 0, Affinity::Forward),
            text: "A".to_owned(),
        },
    )
    .expect("first");
    let (undone, _) = apply(&first, 852, CommandKind::Undo).expect("undo");
    let (branched, _) = apply(
        &undone,
        853,
        CommandKind::InsertText {
            target: position(document.content[1].id.clone(), 0, Affinity::Forward),
            text: "B".to_owned(),
        },
    )
    .expect("branch");

    assert_eq!(branched.history().entries.len(), 1);
    assert_eq!(branched.history().cursor, 1);
    assert_eq!(
        apply(&branched, 854, CommandKind::Redo)
            .expect_err("redo branch cleared")
            .code(),
        "FLOW_REDO_EMPTY"
    );
}

fn run_mixed_sequence(actions: &[u8]) -> (EditorState, Vec<Transaction>, String, String) {
    let original = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let original_hash = semantic_hash(&original).expect("original");
    let text_node_id = original.content[1].id.clone();
    let image: ContentNode = original.content[2].clone();
    let mut state = EditorState::new(original.clone()).expect("state");
    let mut transactions = Vec::new();

    for (index, action) in actions.iter().enumerate() {
        let kind = match action % 4 {
            0 => {
                let node = state
                    .document()
                    .content
                    .iter()
                    .find(|node| node.id == text_node_id)
                    .expect("text node");
                CommandKind::InsertText {
                    target: position(
                        text_node_id.clone(),
                        node.text.encode_utf16().count() as u32,
                        Affinity::Forward,
                    ),
                    text: char::from(b'a' + (*action % 26)).to_string(),
                }
            }
            1 => {
                let node = state
                    .document()
                    .content
                    .iter()
                    .find(|node| node.id == text_node_id)
                    .expect("style node");
                let style_id = if node.style_id.as_ref() == Some(&original.styles[0].id) {
                    original.styles[1].id.clone()
                } else {
                    original.styles[0].id.clone()
                };
                CommandKind::SetNodeStyle {
                    node_id: text_node_id.clone(),
                    style_id: Some(style_id),
                }
            }
            2 => {
                if state
                    .document()
                    .content
                    .iter()
                    .any(|node| node.id == image.id)
                {
                    CommandKind::DeleteNode {
                        node_id: image.id.clone(),
                    }
                } else {
                    CommandKind::InsertNode {
                        index: state.document().content.len() as u32,
                        node: image.clone(),
                    }
                }
            }
            _ => {
                let current = state.document().fields[0].clone();
                let mut field = current.clone();
                field.label = if current.label.as_deref() == Some("Варіант A") {
                    Some("Варіант B".to_owned())
                } else {
                    Some("Варіант A".to_owned())
                };
                CommandKind::SetField {
                    field_id: field.id.clone(),
                    field,
                }
            }
        };
        let applied =
            TransactionService::apply(&state, command(&state, 1_000 + index as u32, kind))
                .expect("generated valid mutation");
        state = applied.state;
        transactions.push(applied.transaction);
    }
    let final_hash = semantic_hash(state.document()).expect("final");

    for index in 0..actions.len() {
        let applied = TransactionService::apply(
            &state,
            command(&state, 2_000 + index as u32, CommandKind::Undo),
        )
        .expect("generated undo");
        state = applied.state;
        transactions.push(applied.transaction);
    }
    assert_eq!(
        semantic_hash(state.document()).expect("fully undone"),
        original_hash
    );

    for index in 0..actions.len() {
        let applied = TransactionService::apply(
            &state,
            command(&state, 3_000 + index as u32, CommandKind::Redo),
        )
        .expect("generated redo");
        state = applied.state;
        transactions.push(applied.transaction);
    }
    assert_eq!(
        semantic_hash(state.document()).expect("fully redone"),
        final_hash
    );
    (state, transactions, original_hash, final_hash)
}

fn property_config() -> ProptestConfig {
    let mut config = ProptestConfig::with_cases(64);
    config.failure_persistence = Some(Box::new(FileFailurePersistence::Direct(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/transaction_properties.txt"
    ))));
    config
}

proptest! {
    #![proptest_config(property_config())]

    #[test]
    fn generated_mixed_histories_are_reversible_replayable_and_deterministic(
        actions in prop::collection::vec(0_u8..=31, 1..24)
    ) {
        let first = run_mixed_sequence(&actions);
        let second = run_mixed_sequence(&actions);
        prop_assert_eq!(first, second);
    }
}

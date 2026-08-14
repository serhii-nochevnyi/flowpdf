use flow_core::{
    anchor::Utf16Offset,
    canonical::canonical_hash,
    model::{Affinity, CommandId, FlowDocument, LogicalPosition},
    transaction::{
        Command, CommandKind, EditorState, Operation, SourceModality, TextRange, TransactionService,
    },
};

fn command_id(value: u32) -> CommandId {
    CommandId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("command id")
}

fn replace_command(document: &FlowDocument, id: u32) -> Command {
    let node = document.content[1].id.clone();
    Command {
        command_id: command_id(id),
        base_revision: document.revision,
        modality: SourceModality::Keyboard,
        issued_at: "2026-08-14T20:02:00Z".to_owned(),
        kind: CommandKind::ReplaceText {
            range: TextRange {
                start: LogicalPosition {
                    node_id: node.clone(),
                    utf16_offset: Utf16Offset::new(0),
                    affinity: Affinity::Backward,
                },
                end: LogicalPosition {
                    node_id: node,
                    utf16_offset: Utf16Offset::new(7),
                    affinity: Affinity::Forward,
                },
            },
            text: "Англійський".to_owned(),
        },
    }
}

#[test]
fn typed_replace_becomes_one_immutable_transaction_with_executable_inverse() {
    let original = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let original_bytes = flow_core::canonical::canonical_bytes(&original).expect("bytes");
    let original_semantic_hash = flow_core::transaction::semantic_hash(&original).expect("hash");
    let state = EditorState::new(original.clone()).expect("state");

    let applied = TransactionService::apply(&state, replace_command(&original, 701))
        .expect("replace applies");

    assert_eq!(state.document(), &original, "input state is immutable");
    assert_eq!(
        flow_core::canonical::canonical_bytes(state.document()).expect("bytes"),
        original_bytes
    );
    assert_eq!(applied.state.document().revision, 2);
    assert_eq!(applied.transaction.base_revision, 1);
    assert_eq!(applied.transaction.new_revision, 2);
    assert_eq!(applied.transaction.forward_operations.len(), 1);
    assert_eq!(applied.transaction.inverse_operations.len(), 1);
    assert_eq!(applied.transaction.anchor_mapping.transformations.len(), 1);
    assert_eq!(
        applied.transaction.before_hash,
        canonical_hash(&original_bytes)
    );
    assert_eq!(
        applied.transaction.after_hash,
        applied.state.canonical_hash()
    );
    assert_ne!(
        applied.transaction.before_hash,
        applied.transaction.after_hash
    );
    assert!(
        applied.state.document().content[1]
            .text
            .starts_with("Англійський")
    );

    match (
        &applied.transaction.forward_operations[0],
        &applied.transaction.inverse_operations[0],
    ) {
        (
            Operation::ReplaceText {
                expected_text,
                replacement,
                ..
            },
            Operation::ReplaceText {
                expected_text: inverse_expected,
                replacement: inverse_replacement,
                ..
            },
        ) => {
            assert_eq!(expected_text, "English");
            assert_eq!(replacement, "Англійський");
            assert_eq!(inverse_expected, "Англійський");
            assert_eq!(inverse_replacement, "English");
        }
        other => panic!("unexpected operation pair: {other:?}"),
    }

    let undone = TransactionService::apply(
        &applied.state,
        Command {
            command_id: command_id(702),
            base_revision: 2,
            modality: SourceModality::Ui,
            issued_at: "2026-08-14T20:02:01Z".to_owned(),
            kind: CommandKind::Undo,
        },
    )
    .expect("stored inverse executes");
    assert_eq!(
        flow_core::transaction::semantic_hash(undone.state.document()).expect("semantic hash"),
        original_semantic_hash
    );
    assert_eq!(undone.state.document().revision, 3);
}

#[test]
fn adjacent_mutations_keep_declared_forward_order_and_reverse_rollback_order() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let node_id = document.content[1].id.clone();
    let state = EditorState::new(document.clone()).expect("state");
    let at = |offset, affinity| LogicalPosition {
        node_id: node_id.clone(),
        utf16_offset: Utf16Offset::new(offset),
        affinity,
    };
    let command = Command {
        command_id: command_id(703),
        base_revision: 1,
        modality: SourceModality::Api,
        issued_at: "2026-08-14T20:02:02Z".to_owned(),
        kind: CommandKind::Batch {
            mutations: vec![
                flow_core::transaction::Mutation::InsertText {
                    target: at(0, Affinity::Forward),
                    text: "A".to_owned(),
                },
                flow_core::transaction::Mutation::InsertText {
                    target: at(1, Affinity::Forward),
                    text: "B".to_owned(),
                },
            ],
        },
    };

    let applied = TransactionService::apply(&state, command).expect("batch");
    assert_eq!(applied.transaction.forward_operations.len(), 2);
    assert_eq!(applied.transaction.inverse_operations.len(), 2);
    assert!(applied.state.document().content[1].text.starts_with("AB"));
    let undone = TransactionService::apply(
        &applied.state,
        Command {
            command_id: command_id(704),
            base_revision: 2,
            modality: SourceModality::Api,
            issued_at: "2026-08-14T20:02:03Z".to_owned(),
            kind: CommandKind::Undo,
        },
    )
    .expect("undo batch");
    assert_eq!(
        undone.state.document().content[1].text,
        document.content[1].text
    );
}

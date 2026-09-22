use std::collections::BTreeSet;

use flow_core::{
    ApiResponse, ApplyCommandRequest, CommandDto, CommandKind, CreateSampleRequest,
    DirectionalSelection, MutationFamily, OperationResult, SourceModality, apply_command,
    command_capabilities, command_capability_for_command_kind, command_capability_for_family,
    command_capability_for_voice_intent, command_parity_contract, command_with_modality,
    create_sample,
    model::{Affinity, InlineMark, LogicalPosition},
    transaction::{Command, Mutation},
};
use serde_json::Value;

fn success<T>(response: ApiResponse<T>) -> T {
    assert!(
        response.ok,
        "unexpected response error: {:?}",
        response.error
    );
    response.value.expect("successful response has a value")
}

fn command_id(value: u32) -> flow_core::model::CommandId {
    flow_core::model::CommandId::new(format!("00000000-0000-4000-8000-{value:012}"))
        .expect("valid deterministic command id")
}

#[test]
fn catalog_is_exhaustive_non_vacuous_and_voice_safe() {
    let catalog = command_capabilities();
    assert_eq!(catalog.len(), MutationFamily::ALL.len());
    assert_eq!(catalog.len(), 35);
    assert!(!catalog.is_empty());

    let command_types = catalog
        .iter()
        .map(|capability| capability.command_type)
        .collect::<Vec<_>>();
    let unique_command_types = command_types.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(unique_command_types.len(), catalog.len());

    for family in MutationFamily::ALL {
        let capability = command_capability_for_family(family);
        assert_eq!(capability.family, family);
        assert_eq!(capability.command_type, family.command_type());
        assert!(!capability.intent.is_empty());
        assert!(!capability.visible.label_key.is_empty());
        assert!(!capability.visible.route.is_empty());
        assert!(!capability.keyboard.label_key.is_empty());
        assert!(!capability.keyboard.route.is_empty());
        assert_eq!(capability.future_voice.intent, capability.intent);
        assert_eq!(
            capability.future_voice.command_type,
            capability.command_type
        );

        let voice = command_capability_for_voice_intent(capability.intent)
            .expect("every command has a future voice intent");
        assert_eq!(voice.family, capability.family);
        assert_eq!(voice.risk, capability.risk);
        assert_eq!(voice.confirmation, capability.confirmation);
        assert_eq!(voice.undo, capability.undo);
    }

    assert!(command_capability_for_voice_intent("editor.intent.unknown").is_none());
    assert!(command_capability_for_command_kind(&CommandKind::Undo).is_none());
    assert!(command_capability_for_command_kind(&CommandKind::Redo).is_none());
    assert!(
        command_capability_for_command_kind(&CommandKind::Batch {
            mutations: Vec::new(),
        })
        .is_none()
    );

    let insert = Mutation::InsertText {
        target: LogicalPosition {
            node_id: flow_core::model::NodeId::new("00000000-0000-4000-8000-000000000101")
                .expect("node id"),
            utf16_offset: 0.into(),
            affinity: Affinity::Forward,
        },
        text: "x".to_owned(),
    };
    assert_eq!(
        MutationFamily::from_mutation(&insert),
        MutationFamily::InsertText
    );
}

#[test]
fn generated_parity_contract_is_deterministic_and_complete() {
    let contract = command_parity_contract();
    assert_eq!(contract.mutation_count, 35);
    assert_eq!(contract.commands, command_capabilities());

    let expected = serde_json::to_string_pretty(&contract).expect("contract serializes");
    let checked_in = include_str!("../../../tests/contracts/phase2-command-parity.json");
    assert_eq!(checked_in.trim_end(), expected);

    let value: Value = serde_json::from_str(checked_in).expect("checked-in contract JSON");
    assert_eq!(value["mutationCount"], 35);
    assert_eq!(value["commands"].as_array().map(Vec::len), Some(35));
}

#[test]
fn visible_keyboard_and_future_voice_traces_are_semantically_equal() {
    let ui = run_trace(SourceModality::Ui);
    let keyboard = run_trace(SourceModality::Keyboard);
    let voice = run_trace(SourceModality::Voice);

    assert_ne!(
        ui, keyboard,
        "audit/transaction modality must remain observable"
    );
    assert_eq!(
        normalize_modalities(ui),
        normalize_modalities(keyboard.clone())
    );
    assert_eq!(normalize_modalities(keyboard), normalize_modalities(voice));
}

#[test]
fn future_voice_translation_reuses_the_same_command_constructor() {
    let document =
        flow_core::model::FlowDocument::deterministic_sample("uk-UA").expect("sample document");
    let node_id = document.content[0].id.clone();
    let template = Command {
        command_id: command_id(9_901),
        base_revision: document.revision,
        modality: SourceModality::Keyboard,
        issued_at: "2026-09-15T20:00:00Z".to_owned(),
        kind: CommandKind::ReplaceSelection {
            selection: selection(node_id, 0, 1),
            text: "П".to_owned(),
        },
    };
    let voice = command_with_modality(&template, SourceModality::Voice);
    assert_eq!(voice.kind, template.kind);
    assert_eq!(voice.command_id, template.command_id);
    assert_eq!(voice.base_revision, template.base_revision);
    assert_eq!(
        command_capability_for_command_kind(&voice.kind),
        command_capability_for_voice_intent("editor.intent.replaceSelection")
    );
}

fn run_trace(modality: SourceModality) -> Vec<Value> {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-09-15T20:00:00Z".to_owned(),
    }));
    let node_id = created.editor.view.selection.anchor.node_id.clone();
    let first_selection = whole_selection(&created, node_id.clone());
    let first = success(apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: command_id(9_902),
            base_revision: created.session.revision,
            modality: modality.clone(),
            issued_at: "2026-09-15T20:00:01Z".to_owned(),
            kind: CommandKind::ReplaceSelection {
                selection: first_selection,
                text: "Parity text".to_owned(),
            },
        },
    }));
    let second = success(apply_command(ApplyCommandRequest {
        canonical_json: first.session.canonical_json.clone(),
        history: first.session.history.clone(),
        command: CommandDto {
            command_id: command_id(9_903),
            base_revision: first.session.revision,
            modality,
            issued_at: "2026-09-15T20:00:02Z".to_owned(),
            kind: CommandKind::SetInlineMark {
                selection: selection(node_id, 0, 1),
                mark: InlineMark::Bold { value: true },
            },
        },
    }));
    vec![
        serde_json::to_value(first).expect("first trace serializes"),
        serde_json::to_value(second).expect("second trace serializes"),
    ]
}

fn whole_selection(
    result: &OperationResult,
    node_id: flow_core::model::NodeId,
) -> DirectionalSelection {
    let text_length = match &result.editor.view.document.blocks[0] {
        flow_core::EditorBlockViewDto::Paragraph { text, .. }
        | flow_core::EditorBlockViewDto::Heading { text, .. } => text.encode_utf16().count() as u32,
        flow_core::EditorBlockViewDto::Atomic { .. } => panic!("sample starts with text"),
    };
    selection(node_id, 0, text_length)
}

fn selection(node_id: flow_core::model::NodeId, start: u32, end: u32) -> DirectionalSelection {
    DirectionalSelection {
        anchor: LogicalPosition {
            node_id: node_id.clone(),
            utf16_offset: start.into(),
            affinity: Affinity::Forward,
        },
        focus: LogicalPosition {
            node_id,
            utf16_offset: end.into(),
            affinity: Affinity::Backward,
        },
    }
}

fn normalize_modalities(mut value: Vec<Value>) -> Vec<Value> {
    for item in &mut value {
        normalize_value(item);
    }
    value
}

fn normalize_value(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(normalize_value),
        Value::Object(object) => {
            if object.contains_key("modality") {
                object.insert(
                    "modality".to_owned(),
                    Value::String("normalized".to_owned()),
                );
            }
            object.values_mut().for_each(normalize_value);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

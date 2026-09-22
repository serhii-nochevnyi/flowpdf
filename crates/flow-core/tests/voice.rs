use flow_core::model::{Affinity, FieldId, InlineMark, LogicalPosition};
use flow_core::{
    ConfirmationPolicy, DirectionalSelection, FieldNavigationDirection, MutationFamily, RiskLevel,
    VOICE_PROTOCOL_VERSION, VoiceAction, VoiceCapabilityMetadata, VoiceCommandRequest, VoiceError,
    VoiceLocale, resolve_voice_command,
};

fn selection() -> DirectionalSelection {
    DirectionalSelection {
        anchor: LogicalPosition {
            node_id: flow_core::model::NodeId::new("00000000-0000-4000-8000-000000000901")
                .expect("node id"),
            utf16_offset: 2.into(),
            affinity: Affinity::Forward,
        },
        focus: LogicalPosition {
            node_id: flow_core::model::NodeId::new("00000000-0000-4000-8000-000000000901")
                .expect("node id"),
            utf16_offset: 8.into(),
            affinity: Affinity::Forward,
        },
    }
}

fn request(locale: VoiceLocale, transcript: &str) -> VoiceCommandRequest {
    VoiceCommandRequest {
        protocol_version: VOICE_PROTOCOL_VERSION,
        locale,
        transcript: transcript.to_owned(),
        source_revision: 7,
        selection: Some(selection()),
        active_field_id: None,
    }
}

fn assert_catalog_metadata(
    metadata: &VoiceCapabilityMetadata,
    family: MutationFamily,
    command_type: &str,
    risk: RiskLevel,
    confirmation: ConfirmationPolicy,
) {
    assert_eq!(metadata.family, Some(family));
    assert_eq!(metadata.command_type, command_type);
    assert_eq!(metadata.risk, risk);
    assert_eq!(metadata.confirmation, confirmation);
    assert!(metadata.undo.is_some());
}

#[test]
fn english_and_ukrainian_phrases_resolve_to_closed_actions() {
    let english =
        resolve_voice_command(request(VoiceLocale::EnUs, "  MAKE   BOLD ")).expect("bold intent");
    assert_eq!(english.source_revision, 7);
    assert_eq!(english.locale, VoiceLocale::EnUs);
    assert_eq!(
        english.action,
        VoiceAction::SetInlineMark {
            mark: InlineMark::Bold { value: true }
        }
    );
    assert_catalog_metadata(
        &english.capability,
        MutationFamily::SetInlineMark,
        "setInlineMark",
        RiskLevel::Formatting,
        ConfirmationPolicy::None,
    );

    let ukrainian = resolve_voice_command(request(VoiceLocale::UkUa, "зробити жирним"))
        .expect("Ukrainian bold intent");
    assert_eq!(
        ukrainian.action,
        VoiceAction::SetInlineMark {
            mark: InlineMark::Bold { value: true }
        }
    );
    assert_eq!(ukrainian.capability, english.capability);
}

#[test]
fn resolver_covers_history_selection_structure_and_field_actions() {
    let undo = resolve_voice_command(request(VoiceLocale::EnUs, "undo")).expect("undo");
    assert_eq!(undo.action, VoiceAction::Undo);
    assert_eq!(undo.capability.family, None);
    assert_eq!(undo.capability.command_type, "undo");

    let delete =
        resolve_voice_command(request(VoiceLocale::UkUa, "видалити виділене")).expect("delete");
    assert_eq!(delete.action, VoiceAction::DeleteSelection);
    assert_catalog_metadata(
        &delete.capability,
        MutationFamily::ReplaceSelection,
        "replaceSelection",
        RiskLevel::Text,
        ConfirmationPolicy::Explicit,
    );

    let insert = resolve_voice_command(request(VoiceLocale::EnUs, "insert page break"))
        .expect("insert break");
    assert_eq!(insert.action, VoiceAction::InsertPageBreak);
    assert_catalog_metadata(
        &insert.capability,
        MutationFamily::InsertPageBreak,
        "insertPageBreak",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
    );

    let remove = resolve_voice_command(request(VoiceLocale::UkUa, "прибрати розрив сторінки"))
        .expect("remove break");
    assert!(matches!(remove.action, VoiceAction::RemovePageBreak { .. }));
    assert_eq!(remove.capability.confirmation, ConfirmationPolicy::Explicit);

    let mut next_request = request(VoiceLocale::EnUs, "next form field");
    next_request.selection = None;
    let next = resolve_voice_command(next_request).expect("next field");
    assert_eq!(
        next.action,
        VoiceAction::NavigateField {
            direction: FieldNavigationDirection::Next
        }
    );
    assert_eq!(next.capability.command_type, "navigateField");

    let mut clear_request = request(VoiceLocale::UkUa, "очистити поточне поле");
    clear_request.selection = None;
    clear_request.active_field_id =
        Some(FieldId::new("00000000-0000-4000-8000-000000000501").expect("field id"));
    let clear = resolve_voice_command(clear_request).expect("clear field");
    assert_eq!(
        clear.action,
        VoiceAction::ClearField {
            field_id: FieldId::new("00000000-0000-4000-8000-000000000501").expect("field id")
        }
    );
    assert_eq!(clear.capability.family, Some(MutationFamily::SetField));
    assert_eq!(clear.capability.confirmation, ConfirmationPolicy::Explicit);
}

#[test]
fn every_initial_locale_phrase_family_remains_exact_and_allowlisted() {
    for phrase in ["redo", "redo action"] {
        assert_eq!(
            resolve_voice_command(request(VoiceLocale::EnUs, phrase))
                .expect("English redo")
                .action,
            VoiceAction::Redo
        );
    }
    for phrase in ["повторити", "повторити дію"] {
        assert_eq!(
            resolve_voice_command(request(VoiceLocale::UkUa, phrase))
                .expect("Ukrainian redo")
                .action,
            VoiceAction::Redo
        );
    }

    for (locale, phrase, expected) in [
        (
            VoiceLocale::EnUs,
            "italic",
            InlineMark::Italic { value: true },
        ),
        (
            VoiceLocale::EnUs,
            "make underline",
            InlineMark::Underline { value: true },
        ),
        (
            VoiceLocale::UkUa,
            "курсив",
            InlineMark::Italic { value: true },
        ),
        (
            VoiceLocale::UkUa,
            "підкреслити",
            InlineMark::Underline { value: true },
        ),
    ] {
        assert_eq!(
            resolve_voice_command(request(locale, phrase))
                .expect("formatting phrase")
                .action,
            VoiceAction::SetInlineMark { mark: expected }
        );
    }

    for (locale, phrase, direction) in [
        (
            VoiceLocale::EnUs,
            "previous field",
            FieldNavigationDirection::Previous,
        ),
        (
            VoiceLocale::UkUa,
            "наступне поле",
            FieldNavigationDirection::Next,
        ),
    ] {
        let mut field_request = request(locale, phrase);
        field_request.selection = None;
        assert_eq!(
            resolve_voice_command(field_request)
                .expect("field navigation phrase")
                .action,
            VoiceAction::NavigateField { direction }
        );
    }

    let mut clear_request = request(VoiceLocale::EnUs, "clear field");
    clear_request.selection = None;
    clear_request.active_field_id =
        Some(FieldId::new("00000000-0000-4000-8000-000000000501").expect("field id"));
    let clear = resolve_voice_command(clear_request).expect("English clear field");
    assert!(matches!(clear.action, VoiceAction::ClearField { .. }));
    assert_eq!(clear.capability.confirmation, ConfirmationPolicy::Explicit);
}

#[test]
fn bounded_failures_are_stable_and_privacy_safe() {
    let cases = [
        ("", VoiceError::TranscriptEmpty),
        ("format", VoiceError::AmbiguousCommand),
        ("please do something", VoiceError::UnsupportedCommand),
    ];
    for (transcript, expected) in cases {
        let error = resolve_voice_command(request(VoiceLocale::EnUs, transcript))
            .expect_err("phrase must fail closed");
        assert_eq!(error, expected);
        if !transcript.is_empty() {
            assert!(!error.to_string().contains(transcript));
        }
    }

    let mut future = request(VoiceLocale::EnUs, "undo");
    future.protocol_version += 1;
    assert_eq!(
        resolve_voice_command(future).expect_err("future protocol"),
        VoiceError::ProtocolVersion
    );

    let mut oversized = request(VoiceLocale::EnUs, "undo");
    oversized.transcript = "x".repeat(flow_core::MAX_VOICE_TRANSCRIPT_BYTES + 1);
    assert_eq!(
        resolve_voice_command(oversized).expect_err("oversized transcript"),
        VoiceError::TranscriptSizeLimit
    );
}

#[test]
fn selection_and_field_preconditions_are_required_before_dispatch() {
    let mut selection_missing = request(VoiceLocale::EnUs, "delete selection");
    selection_missing.selection = None;
    assert_eq!(
        resolve_voice_command(selection_missing).expect_err("selection required"),
        VoiceError::SelectionRequired
    );

    let mut field_missing = request(VoiceLocale::EnUs, "clear current field");
    field_missing.selection = None;
    assert_eq!(
        resolve_voice_command(field_missing).expect_err("field required"),
        VoiceError::FieldRequired
    );
}

#[test]
fn returned_intent_has_no_raw_transcript_or_document_mutation_surface() {
    let transcript = "secret transcript must not cross the intent boundary";
    let request = request(VoiceLocale::EnUs, "undo");
    let original = request.clone();
    let intent = resolve_voice_command(request).expect("intent");
    assert_eq!(original.transcript, "undo");
    assert_ne!(original.transcript, transcript);
    let encoded = serde_json::to_string(&intent).expect("intent json");
    assert!(!encoded.contains(transcript));
    assert!(!encoded.contains("canonicalJson"));
}

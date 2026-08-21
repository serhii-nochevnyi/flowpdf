use flow_core::{
    audit::{
        AuditAction, AuditCommandKind, AuditErrorCode, AuditEvent, AuditMetadata, AuditTimestamp,
        sort_events,
    },
    model::{CommandId, FlowDocument},
    transaction::SourceModality,
};

fn command_id(value: u32) -> CommandId {
    CommandId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("valid command ID")
}

fn event(sequence: u64, command: u32, timestamp: &str) -> AuditEvent {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let command_id = command_id(command);
    AuditEvent::success(
        command_id.clone(),
        document.document_id,
        command_id.clone(),
        command_id,
        1,
        2,
        sequence,
        AuditTimestamp::parse(timestamp).expect("safe timestamp"),
        AuditAction::Command {
            command_kind: AuditCommandKind::ReplaceText,
        },
        SourceModality::Keyboard,
        vec![AuditMetadata::SchemaVersion { value: 1 }],
    )
    .expect("safe audit event")
}

#[test]
fn audit_serialization_and_debug_are_closed_allowlists() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let command = command_id(501);
    let event = AuditEvent::failure(
        command.clone(),
        document.document_id,
        command.clone(),
        command,
        2,
        2,
        7,
        AuditTimestamp::parse("2026-08-14T20:50:00Z").expect("timestamp"),
        AuditAction::Command {
            command_kind: AuditCommandKind::InsertText,
        },
        SourceModality::Voice,
        AuditErrorCode::StaleRevision,
        vec![AuditMetadata::SchemaVersion { value: 1 }],
    )
    .expect("failure event");

    let serialized = serde_json::to_string(&event).expect("audit serializes");
    let debug = format!("{event:?}");
    for forbidden in [
        "SENSITIVE_DOCUMENT_TEXT",
        "RAW_AUDIO_BYTES",
        "VOICE_TRANSCRIPT",
        "commandArguments",
        "forwardOperations",
        "storagePayload",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "serialized audit leaked {forbidden}"
        );
        assert!(!debug.contains(forbidden), "debug audit leaked {forbidden}");
    }
    assert!(serialized.contains("staleRevision"));
    assert!(serialized.contains("schemaVersion") || serialized.contains("recoveryVerified"));

    let mut unknown = serde_json::to_value(&event).expect("audit JSON value");
    unknown.as_object_mut().expect("object").insert(
        "documentText".to_owned(),
        serde_json::json!("SENSITIVE_DOCUMENT_TEXT"),
    );
    assert!(serde_json::from_value::<AuditEvent>(unknown).is_err());
}

#[test]
fn audit_deserialization_rejects_invalid_ids_and_normalizes_uuid_spellings() {
    let original = event(8, 508, "2026-08-14T20:50:01Z");
    let mut invalid = serde_json::to_value(&original).expect("audit JSON value");
    invalid["auditId"] = serde_json::json!("not-a-uuid");
    assert!(serde_json::from_value::<AuditEvent>(invalid).is_err());

    let mut alternate = serde_json::to_value(&original).expect("audit JSON value");
    for field in ["auditId", "transactionId", "commandId"] {
        let id = alternate[field].as_str().expect("serialized ID");
        alternate[field] = serde_json::json!(id.replace('-', ""));
    }
    assert_eq!(
        serde_json::from_value::<AuditEvent>(alternate).expect("normalized audit"),
        original
    );
}

#[test]
fn audit_timestamps_require_real_utc_calendar_seconds() {
    for timestamp in [
        "2024-02-29T23:59:59Z",
        "2000-02-29T00:00:00Z",
        "2026-08-15T00:00:00Z",
    ] {
        assert!(AuditTimestamp::parse(timestamp).is_ok(), "{timestamp}");
    }

    for timestamp in [
        "2025-02-29T12:00:00Z",
        "2026-04-31T12:00:00Z",
        "2026-13-01T12:00:00Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T12:60:00Z",
        "2026-01-01T12:00:60Z",
        "0000-01-01T00:00:00Z",
    ] {
        assert!(AuditTimestamp::parse(timestamp).is_err(), "{timestamp}");
    }
}

#[test]
fn audit_actions_enforce_revision_and_metadata_semantics() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let timestamp = AuditTimestamp::parse("2026-08-15T00:00:00Z").expect("timestamp");
    let id = command_id(601);

    let transaction_success = || {
        AuditEvent::success(
            id.clone(),
            document.document_id.clone(),
            id.clone(),
            id.clone(),
            4,
            5,
            8,
            timestamp.clone(),
            AuditAction::Command {
                command_kind: AuditCommandKind::ReplaceText,
            },
            SourceModality::Keyboard,
            vec![AuditMetadata::SchemaVersion { value: 1 }],
        )
    };
    assert!(transaction_success().is_ok());

    assert!(
        AuditEvent::success(
            id.clone(),
            document.document_id.clone(),
            id.clone(),
            id.clone(),
            4,
            5,
            9,
            timestamp.clone(),
            AuditAction::Create,
            SourceModality::System,
            vec![AuditMetadata::SchemaVersion { value: 1 }],
        )
        .is_ok()
    );

    assert!(
        AuditEvent::failure(
            id.clone(),
            document.document_id.clone(),
            id.clone(),
            id.clone(),
            4,
            4,
            10,
            timestamp.clone(),
            AuditAction::Command {
                command_kind: AuditCommandKind::DeleteText,
            },
            SourceModality::Keyboard,
            AuditErrorCode::InvalidTarget,
            vec![AuditMetadata::SchemaVersion { value: 1 }],
        )
        .is_ok()
    );

    assert!(
        AuditEvent::success(
            id.clone(),
            document.document_id.clone(),
            id.clone(),
            id.clone(),
            4,
            5,
            11,
            timestamp.clone(),
            AuditAction::Command {
                command_kind: AuditCommandKind::Recovery,
            },
            SourceModality::System,
            vec![AuditMetadata::SchemaVersion { value: 1 }],
        )
        .is_err()
    );

    assert!(
        AuditEvent::success(
            id.clone(),
            document.document_id.clone(),
            id.clone(),
            id.clone(),
            4,
            6,
            12,
            timestamp.clone(),
            AuditAction::Create,
            SourceModality::System,
            vec![AuditMetadata::SchemaVersion { value: 1 }],
        )
        .is_err()
    );

    assert!(
        AuditEvent::failure(
            id.clone(),
            document.document_id.clone(),
            id.clone(),
            id.clone(),
            4,
            5,
            13,
            timestamp.clone(),
            AuditAction::Command {
                command_kind: AuditCommandKind::DeleteText,
            },
            SourceModality::Keyboard,
            AuditErrorCode::InvalidTarget,
            vec![AuditMetadata::SchemaVersion { value: 1 }],
        )
        .is_err()
    );

    let recovery_verified = AuditMetadata::RecoveryVerified;
    assert!(
        AuditEvent::failure(
            id.clone(),
            document.document_id.clone(),
            id.clone(),
            id.clone(),
            4,
            4,
            14,
            timestamp.clone(),
            AuditAction::Command {
                command_kind: AuditCommandKind::DeleteText,
            },
            SourceModality::Keyboard,
            AuditErrorCode::InvalidTarget,
            vec![recovery_verified],
        )
        .is_err()
    );

    assert!(
        AuditEvent::success(
            id.clone(),
            document.document_id.clone(),
            id.clone(),
            id.clone(),
            4,
            4,
            15,
            timestamp.clone(),
            AuditAction::Migration,
            SourceModality::System,
            vec![AuditMetadata::Migration {
                from_schema_version: 0,
                to_schema_version: 1,
            }],
        )
        .is_ok()
    );

    assert!(
        AuditEvent::success(
            id.clone(),
            document.document_id.clone(),
            id.clone(),
            id.clone(),
            4,
            4,
            16,
            timestamp.clone(),
            AuditAction::Migration,
            SourceModality::System,
            vec![AuditMetadata::Migration {
                from_schema_version: 1,
                to_schema_version: 1,
            }],
        )
        .is_err()
    );

    assert!(
        AuditEvent::success(
            id.clone(),
            document.document_id.clone(),
            id.clone(),
            id.clone(),
            4,
            4,
            17,
            timestamp.clone(),
            AuditAction::Recovery,
            SourceModality::System,
            vec![AuditMetadata::RecoveryVerified],
        )
        .is_ok()
    );

    assert!(
        AuditEvent::failure(
            id.clone(),
            document.document_id.clone(),
            id.clone(),
            id,
            4,
            4,
            18,
            timestamp.clone(),
            AuditAction::Recovery,
            SourceModality::System,
            AuditErrorCode::HashMismatch,
            vec![],
        )
        .is_ok()
    );

    assert!(
        AuditEvent::failure(
            command_id(602),
            document.document_id,
            command_id(602),
            command_id(602),
            4,
            4,
            19,
            timestamp,
            AuditAction::Recovery,
            SourceModality::System,
            AuditErrorCode::HashMismatch,
            vec![AuditMetadata::RecoveryVerified],
        )
        .is_err()
    );
}

#[test]
fn audit_order_uses_durable_sequence_then_revision_then_identity() {
    let mut events = vec![
        event(3, 503, "2026-08-14T20:50:00Z"),
        event(2, 502, "2026-08-14T20:50:00Z"),
        event(2, 501, "2026-08-14T20:50:00Z"),
    ];
    sort_events(&mut events);

    assert_eq!(events[0].durable_sequence(), 2);
    assert_eq!(events[0].audit_id().as_str(), command_id(501).as_str());
    assert_eq!(events[1].durable_sequence(), 2);
    assert_eq!(events[2].durable_sequence(), 3);
    assert_eq!(events.len(), 3, "queries never coalesce adjacent events");
}

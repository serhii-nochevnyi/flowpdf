use flow_core::model::CommandId;
use flow_core::{
    ApiResponse, ApplyCommandRequest, AuditRecord, AuditedRecoverRequest, CommandDto, CommandKind,
    CreateSampleRequest, MigrateDocumentRequest, OperationResult, RecoverResult,
    RecoveryAuditContext, SourceModality, apply_command,
    audit::{AuditAction, AuditMetadata, AuditTimestamp},
    create_sample, migrate_document, recover, recover_audited,
    store::{CommitDisposition, DocumentStore, InMemoryDocumentStore, StoreError},
};

fn success<T>(response: ApiResponse<T>) -> T {
    assert!(response.ok, "unexpected error: {:?}", response.error);
    response.value.expect("successful response has a value")
}

#[test]
fn repeated_create_is_idempotent_and_divergent_same_identity_never_replaces_truth() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
    }));
    let mut store = InMemoryDocumentStore::default();
    assert_eq!(
        store
            .commit_atomic(created.commit.clone())
            .expect("initial create"),
        CommitDisposition::Committed
    );
    assert_eq!(
        store
            .commit_atomic(created.commit.clone())
            .expect("identical create"),
        CommitDisposition::Idempotent
    );

    let mut divergent = created.commit.clone();
    divergent.transaction.issued_at = "2026-08-14T00:00:02Z".to_owned();
    let command_id = divergent.transaction.command_id.clone();
    divergent.audit = AuditRecord::success(
        command_id.clone(),
        divergent.snapshot.document_id.clone(),
        command_id.clone(),
        command_id,
        0,
        1,
        1,
        AuditTimestamp::parse("2026-08-14T00:00:02Z").expect("timestamp"),
        AuditAction::Create,
        SourceModality::System,
        vec![AuditMetadata::SchemaVersion { value: 1 }],
    )
    .expect("safe divergent audit");
    assert_eq!(
        store
            .commit_atomic(divergent)
            .expect_err("divergent identity must conflict"),
        StoreError::IdentityConflict
    );

    let records = store.load_records().expect("unchanged store");
    assert_eq!(records.snapshots.len(), 1);
    assert_eq!(records.transactions.len(), 1);
    assert_eq!(records.audits.len(), 1);
}

#[test]
fn store_replays_a_mutation_before_acknowledging_its_atomic_commit() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
    }));
    let applied = apply_one(&created);
    let mut store = InMemoryDocumentStore::default();
    store
        .commit_atomic(created.commit.clone())
        .expect("initial create");

    let mut corrupt = applied.commit;
    corrupt.transaction.inverse_operations = corrupt.transaction.forward_operations.clone();
    assert_eq!(
        store
            .commit_atomic(corrupt)
            .expect_err("unreplayable inverse must not be acknowledged"),
        StoreError::InvalidCommit
    );
    assert_eq!(store.load_records().expect("old prefix").snapshots.len(), 1);
}

#[test]
fn a_deserialized_exact_plus_divergent_identity_is_always_a_conflict() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
    }));
    let mut original = InMemoryDocumentStore::default();
    original
        .commit_atomic(created.commit.clone())
        .expect("initial create");
    let mut stored = serde_json::to_value(&original).expect("store JSON");
    let mut divergent = created.commit.transaction.clone();
    divergent.issued_at = "2026-08-14T00:00:02Z".to_owned();
    stored["transactions"]
        .as_array_mut()
        .expect("transaction array")
        .push(serde_json::to_value(divergent).expect("transaction JSON"));
    let mut hostile: InMemoryDocumentStore =
        serde_json::from_value(stored).expect("hostile physical image");

    assert_eq!(
        hostile
            .commit_atomic(created.commit)
            .expect_err("any divergent match wins over an exact match"),
        StoreError::IdentityConflict
    );
}

#[test]
fn rejected_command_audit_is_redacted_atomic_and_idempotent_without_a_transaction() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
    }));
    let rejected = apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: CommandId::new("00000000-0000-4000-8000-000000000902").expect("attempt ID"),
            base_revision: 0,
            modality: SourceModality::Voice,
            issued_at: "2026-08-14T20:40:02Z".to_owned(),
            kind: CommandKind::InsertText {
                target: created.session.next_command_target.clone(),
                text: "SENSITIVE_REJECTED_TEXT".to_owned(),
            },
        },
    });
    assert!(!rejected.ok);
    let audit = rejected
        .error
        .expect("rejection")
        .audit
        .expect("standalone audit");
    assert!(
        !serde_json::to_string(&audit)
            .expect("audit JSON")
            .contains("SENSITIVE_REJECTED_TEXT")
    );

    let mut store = InMemoryDocumentStore::default();
    store.commit_atomic(created.commit).expect("create commit");
    assert_eq!(
        store
            .commit_standalone_audit_atomic(audit.clone())
            .expect("failure audit"),
        CommitDisposition::Committed
    );
    assert_eq!(
        store
            .commit_standalone_audit_atomic(audit)
            .expect("same attempt"),
        CommitDisposition::Idempotent
    );
    let recovered = success(recover(store.load_records().expect("records")));
    assert_eq!(recovered.view.audit.len(), 2);
    assert_eq!(recovered.session.revision, 1);
}

fn apply_one(created: &OperationResult) -> OperationResult {
    success(apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: CommandId::new("00000000-0000-4000-8000-000000000901").expect("command ID"),
            base_revision: created.session.revision,
            modality: SourceModality::Voice,
            issued_at: "2026-08-14T20:40:01Z".to_owned(),
            kind: CommandKind::InsertText {
                target: created.session.next_command_target.clone(),
                text: " — приватний вміст команди".to_owned(),
            },
        },
    }))
}

#[test]
fn atomic_store_commit_recovers_the_exact_immutable_revision_and_redacted_audit() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
    }));
    let applied = apply_one(&created);
    let mut store = InMemoryDocumentStore::default();

    assert_eq!(
        store
            .commit_atomic(created.commit.clone())
            .expect("create commit"),
        CommitDisposition::Committed
    );
    assert_eq!(
        store
            .commit_atomic(applied.commit.clone())
            .expect("mutation commit"),
        CommitDisposition::Committed
    );

    let records = store.load_records().expect("physical record read");
    assert_eq!(records.snapshots.len(), 2);
    assert_eq!(records.transactions.len(), 2);
    assert_eq!(records.audits.len(), 2);
    assert_eq!(records.assets.len(), 1);
    assert!(records.assets[0].bytes.is_empty());
    assert_eq!(
        records.assets[0].content_hash,
        "blake3:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
    );

    let recovered: RecoverResult = success(recover(records.clone()));
    assert_eq!(recovered.session.revision, applied.session.revision);
    assert_eq!(
        recovered.session.canonical_json,
        applied.session.canonical_json
    );
    assert_eq!(
        recovered.session.canonical_hash,
        applied.session.canonical_hash
    );
    assert_eq!(recovered.session.history, applied.session.history);
    assert_eq!(
        recovered.view.revision_provenance.revision(),
        applied.session.revision
    );
    assert_eq!(
        recovered.view.revision_provenance.canonical_hash().as_str(),
        applied.session.canonical_hash
    );

    let audit_json = serde_json::to_string(&records.audits).expect("audit JSON");
    for forbidden in [
        "приватний вміст",
        "forwardOperations",
        "inverseOperations",
        "transcript",
        "audio",
    ] {
        assert!(!audit_json.contains(forbidden), "leaked {forbidden}");
    }
}

#[test]
fn verified_recovery_audit_is_nonmutating_and_idempotent_by_attempt_identity() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
    }));
    let applied = apply_one(&created);
    let mut store = InMemoryDocumentStore::default();
    store.commit_atomic(created.commit).expect("create commit");
    store
        .commit_atomic(applied.commit)
        .expect("mutation commit");
    let context = RecoveryAuditContext {
        attempt_id: CommandId::new("00000000-0000-4000-8000-000000000903").expect("recovery ID"),
        document_id: applied.session.document_id.clone(),
        expected_revision: applied.session.revision,
        issued_at: "2026-08-14T20:40:03Z".to_owned(),
    };
    let audited = success(recover_audited(AuditedRecoverRequest {
        records: store.load_records().expect("records"),
        audit_context: context,
    }));
    assert_eq!(audited.recovered.session.revision, applied.session.revision);
    assert_eq!(
        store
            .commit_standalone_audit_atomic(audited.audit.clone())
            .expect("recovery audit"),
        CommitDisposition::Committed
    );
    assert_eq!(
        store
            .commit_standalone_audit_atomic(audited.audit)
            .expect("same recovery attempt"),
        CommitDisposition::Idempotent
    );
    let reopened = success(recover(store.load_records().expect("reopen records")));
    assert_eq!(reopened.session.revision, applied.session.revision);
    assert_eq!(reopened.view.audit.len(), 3);
}

#[test]
fn migration_atomically_retains_and_revalidates_exact_source_bytes() {
    let older_file = include_str!("../../../fixtures/flowdoc/older.json");
    let older = older_file.strip_suffix('\n').unwrap_or(older_file);
    let migrated = success(migrate_document(MigrateDocumentRequest {
        canonical_json: older.to_owned(),
        migration_id: CommandId::new("00000000-0000-4000-8000-000000000904").expect("migration ID"),
        issued_at: "2026-08-14T20:40:04Z".to_owned(),
        assets: vec![flow_core::store::AssetRecord {
            record_format_version: flow_core::RECORD_FORMAT_VERSION,
            content_hash: "blake3:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
                .to_owned(),
            bytes: Vec::new(),
        }],
    }));
    let commit = migrated.commit.expect("migration boundary");
    assert_eq!(commit.source.canonical_json.as_bytes(), older.as_bytes());

    let legacy_snapshot = flow_core::SnapshotRecord {
        record_format_version: flow_core::RECORD_FORMAT_VERSION,
        document_id: commit.source.document_id.clone(),
        revision: 1,
        schema_version: commit.source.schema_version,
        canonical_json: commit.source.canonical_json.clone(),
        canonical_hash: commit.source.canonical_hash.clone(),
        history: flow_core::HistoryState::default(),
        migration_boundary: None,
    };
    let mut store: InMemoryDocumentStore = serde_json::from_value(serde_json::json!({
        "snapshots": [legacy_snapshot],
        "transactions": [],
        "audits": [],
        "assets": [],
        "sources": []
    }))
    .expect("legacy physical image");
    assert_eq!(
        store
            .commit_migration_atomic(commit.clone())
            .expect("migration commit"),
        CommitDisposition::Committed
    );
    assert_eq!(
        store
            .commit_migration_atomic(commit)
            .expect("migration retry"),
        CommitDisposition::Idempotent
    );
    let records = store.load_records().expect("migration records");
    assert_eq!(records.snapshots.len(), 2, "legacy snapshot is preserved");
    assert_eq!(records.sources.len(), 1);
    assert_eq!(
        success(recover(records.clone())).session.canonical_hash,
        migrated.canonical_hash
    );

    let mut tampered = records;
    tampered.sources[0].canonical_json.push(' ');
    let rejected = recover(tampered);
    assert!(!rejected.ok);
    assert!(rejected.value.is_none());
    assert_eq!(
        rejected.error.expect("source diagnostic").code,
        "FLOW_RECOVERY_GAP"
    );
}

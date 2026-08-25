use flow_core::model::{CommandId, FlowDocument, Provenance};
use flow_core::{
    ApiResponse, ApplyCommandRequest, AuditOutcome, AuditRecord, AuditedRecoverRequest, CommandDto,
    CommandKind, CreateSampleRequest, MigrateDocumentRequest, OperationResult,
    PlanStandaloneAuditRequest, RecoverRequest, RecoverResult, RecoveryAuditContext,
    SourceModality, StandaloneAuditDerivation, apply_command,
    audit::{AuditAction, AuditCommandKind, AuditErrorCode, AuditMetadata, AuditTimestamp},
    create_sample, migrate_document, plan_standalone_audit, recover, recover_audited,
    schema::DocumentLimits,
    store::{
        CommitDisposition, CommitPlanner, DocumentStore, InMemoryDocumentStore, SnapshotPolicy,
        SnapshotReason, StoreError,
    },
};

fn success<T>(response: ApiResponse<T>) -> T {
    assert!(response.ok, "unexpected error: {:?}", response.error);
    response.value.expect("successful response has a value")
}

#[test]
fn repeated_create_is_idempotent_and_divergent_same_identity_never_replaces_truth() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
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
fn sample_creation_uses_the_caller_timestamp_for_document_transaction_and_audit() {
    let issued_at = "2026-08-14T12:34:56Z";
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: issued_at.to_owned(),
    }));
    let document: FlowDocument =
        serde_json::from_str(&created.session.canonical_json).expect("canonical document");

    assert_eq!(
        document.provenance,
        Provenance::LocalSample {
            created_at: issued_at.to_owned(),
        }
    );
    assert_eq!(created.commit.transaction.issued_at, issued_at);
    assert_eq!(created.commit.audit.timestamp().as_str(), issued_at);
}

#[test]
fn store_replays_a_mutation_before_acknowledging_its_atomic_commit() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
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
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
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
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
    }));
    let rejected_request = ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: CommandId::new("00000000-0000-4000-8000-000000000902").expect("command ID"),
            base_revision: 0,
            modality: SourceModality::Voice,
            issued_at: "2026-08-14T20:40:02Z".to_owned(),
            kind: CommandKind::InsertText {
                target: created
                    .session
                    .next_command_target
                    .clone()
                    .expect("sample command target"),
                text: "SENSITIVE_REJECTED_TEXT".to_owned(),
            },
        },
    };
    let rejected = apply_command(rejected_request.clone());
    assert!(!rejected.ok);
    let response_audit = rejected
        .error
        .expect("rejection")
        .audit
        .expect("standalone audit");
    assert!(
        !serde_json::to_string(&response_audit)
            .expect("audit JSON")
            .contains("SENSITIVE_REJECTED_TEXT")
    );

    let mut store = InMemoryDocumentStore::default();
    store.commit_atomic(created.commit).expect("create commit");
    assert_eq!(
        store
            .commit_standalone_audit_atomic(response_audit)
            .expect_err("unplanned logical-command identity must not be durable"),
        StoreError::InvalidCommit
    );
    let records = store.load_records().expect("creation records");
    let audit = success(plan_standalone_audit(PlanStandaloneAuditRequest {
        records,
        derivation: StandaloneAuditDerivation::CommandFailure {
            request: Box::new(rejected_request),
            attempt_id: CommandId::new("00000000-0000-4000-8000-000000000903").expect("attempt ID"),
        },
    }));
    assert_ne!(audit.audit_id(), audit.command_id());
    assert_eq!(
        audit.command_id().as_str(),
        "00000000-0000-4000-8000-000000000902"
    );
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

#[test]
fn standalone_audit_planning_rejects_successful_commands_and_an_over_budget_post_image() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
    }));
    let successful_request = ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: CommandId::new("00000000-0000-4000-8000-000000000904").expect("command ID"),
            base_revision: created.session.revision,
            modality: SourceModality::Ui,
            issued_at: "2026-08-14T20:40:04Z".to_owned(),
            kind: CommandKind::InsertText {
                target: created
                    .session
                    .next_command_target
                    .clone()
                    .expect("command target"),
                text: "valid command".to_owned(),
            },
        },
    };
    let forged = plan_standalone_audit(PlanStandaloneAuditRequest {
        records: RecoverRequest {
            snapshots: vec![created.commit.snapshot.clone()],
            transactions: vec![created.commit.transaction.clone()],
            audits: vec![created.commit.audit.clone()],
            assets: created.commit.assets.clone(),
            sources: Vec::new(),
        },
        derivation: StandaloneAuditDerivation::CommandFailure {
            request: Box::new(successful_request),
            attempt_id: CommandId::new("00000000-0000-4000-8000-000000000905").expect("attempt ID"),
        },
    });
    assert_eq!(
        forged.error.expect("forged audit rejection").code,
        "FLOW_UNSAFE_AUDIT_RECORD"
    );

    let stale_request = ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: CommandId::new("00000000-0000-4000-8000-000000000906").expect("command ID"),
            base_revision: 0,
            modality: SourceModality::Ui,
            issued_at: "2026-08-14T20:40:06Z".to_owned(),
            kind: CommandKind::InsertText {
                target: created
                    .session
                    .next_command_target
                    .clone()
                    .expect("command target"),
                text: "stale command".to_owned(),
            },
        },
    };
    let mut records = RecoverRequest {
        snapshots: vec![created.commit.snapshot],
        transactions: vec![created.commit.transaction],
        audits: vec![created.commit.audit],
        assets: created.commit.assets,
        sources: Vec::new(),
    };
    let current_count = records.snapshots.len()
        + records.transactions.len()
        + records.audits.len()
        + records.assets.len();
    records.assets.extend(vec![
        records.assets[0].clone();
        DocumentLimits::V1.recovery_records - current_count
    ]);
    let over_budget = plan_standalone_audit(PlanStandaloneAuditRequest {
        records,
        derivation: StandaloneAuditDerivation::CommandFailure {
            request: Box::new(stale_request),
            attempt_id: CommandId::new("00000000-0000-4000-8000-000000000907").expect("attempt ID"),
        },
    });
    assert_eq!(
        over_budget.error.expect("post-image limit").code,
        "FLOW_LIMIT_RECOVERY_RECORDS"
    );
}

#[test]
fn duplicate_command_failure_has_a_distinct_attempt_identity_and_preserves_the_success_audit() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
    }));
    let applied = apply_one(&created);
    let duplicate_request = ApplyCommandRequest {
        canonical_json: applied.session.canonical_json.clone(),
        history: applied.session.history.clone(),
        command: CommandDto {
            command_id: applied.commit.transaction.command_id.clone(),
            base_revision: applied.session.revision,
            modality: SourceModality::Voice,
            issued_at: "2026-08-14T20:40:10Z".to_owned(),
            kind: CommandKind::InsertText {
                target: applied
                    .session
                    .next_command_target
                    .clone()
                    .expect("command target"),
                text: "duplicate payload".to_owned(),
            },
        },
    };
    assert_eq!(
        apply_command(duplicate_request.clone())
            .error
            .expect("duplicate rejection")
            .code,
        "FLOW_DUPLICATE_COMMAND"
    );

    let mut store = InMemoryDocumentStore::default();
    store.commit_atomic(created.commit).expect("creation");
    store
        .commit_atomic(applied.commit.clone())
        .expect("successful command");
    let failure = success(plan_standalone_audit(PlanStandaloneAuditRequest {
        records: store.load_records().expect("durable prefix"),
        derivation: StandaloneAuditDerivation::CommandFailure {
            request: Box::new(duplicate_request),
            attempt_id: CommandId::new("00000000-0000-4000-8000-000000000910").expect("attempt ID"),
        },
    }));
    assert_eq!(failure.command_id(), applied.commit.audit.command_id());
    assert_ne!(failure.audit_id(), applied.commit.audit.audit_id());
    store
        .commit_standalone_audit_atomic(failure.clone())
        .expect("failure audit");

    let recovered = success(recover(store.load_records().expect("records")));
    assert!(recovered.view.audit.contains(&applied.commit.audit));
    assert!(recovered.view.audit.contains(&failure));
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
                target: created
                    .session
                    .next_command_target
                    .clone()
                    .expect("sample command target"),
                text: " — приватний вміст команди".to_owned(),
            },
        },
    }))
}

#[test]
fn atomic_store_commit_recovers_the_exact_immutable_revision_and_redacted_audit() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
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
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
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
        audit_context: context.clone(),
    }));
    assert_eq!(audited.recovered.session.revision, applied.session.revision);
    let authorized = success(plan_standalone_audit(PlanStandaloneAuditRequest {
        records: store.load_records().expect("authorization records"),
        derivation: StandaloneAuditDerivation::Recovery {
            audit_context: context,
        },
    }));
    assert_eq!(
        store
            .commit_standalone_audit_atomic(authorized.clone())
            .expect("recovery audit"),
        CommitDisposition::Committed
    );
    assert_eq!(
        store
            .commit_standalone_audit_atomic(authorized)
            .expect("same recovery attempt"),
        CommitDisposition::Idempotent
    );
    let reopened = success(recover(store.load_records().expect("reopen records")));
    assert_eq!(reopened.session.revision, applied.session.revision);
    assert_eq!(reopened.view.audit.len(), 3);
}

#[test]
fn recovery_context_mismatch_returns_an_authorizable_nonmutating_failure_audit() {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
    }));
    let applied = apply_one(&created);
    let mut store = InMemoryDocumentStore::default();
    store.commit_atomic(created.commit).expect("create commit");
    store
        .commit_atomic(applied.commit)
        .expect("mutation commit");

    let records = store.load_records().expect("records");
    let context = RecoveryAuditContext {
        attempt_id: CommandId::new("00000000-0000-4000-8000-000000000909").expect("recovery ID"),
        document_id: applied.session.document_id,
        expected_revision: 1,
        issued_at: "2026-08-14T20:40:09Z".to_owned(),
    };
    let response = recover_audited(AuditedRecoverRequest {
        records: records.clone(),
        audit_context: context.clone(),
    });
    assert!(!response.ok);
    let error = response.error.expect("mismatch diagnostic");
    assert_eq!(error.code, "FLOW_RECOVERY_GAP");
    let audit = error.audit.expect("mismatch failure audit");
    assert_eq!(audit.new_revision(), 1);
    assert!(matches!(audit.action(), AuditAction::Recovery));
    assert!(matches!(
        audit.outcome(),
        AuditOutcome::Failure {
            code: AuditErrorCode::RecoveryGap
        }
    ));
    let authorized = success(plan_standalone_audit(PlanStandaloneAuditRequest {
        records,
        derivation: StandaloneAuditDerivation::Recovery {
            audit_context: context,
        },
    }));
    assert_eq!(
        store
            .commit_standalone_audit_atomic(authorized)
            .expect("failure audit commit"),
        CommitDisposition::Committed
    );
    let reopened = success(recover(store.load_records().expect("reopen records")));
    assert_eq!(reopened.session.revision, 2);
    assert!(reopened.view.audit.contains(&audit));
    let audit_json = serde_json::to_string(&audit).expect("failure audit JSON");
    assert!(!audit_json.contains("typed mutation"));
    assert!(!audit_json.contains("canonicalJson"));
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

#[test]
fn migrated_documents_checkpoint_after_the_boundary_and_reject_lineage_tampering() {
    let older_file = include_str!("../../../fixtures/flowdoc/older.json");
    let older = older_file.strip_suffix('\n').unwrap_or(older_file);
    let migrated = success(migrate_document(MigrateDocumentRequest {
        canonical_json: older.to_owned(),
        migration_id: CommandId::new("00000000-0000-4000-8000-000000000905").expect("migration ID"),
        issued_at: "2026-08-14T20:40:05Z".to_owned(),
        assets: vec![flow_core::store::AssetRecord {
            record_format_version: flow_core::RECORD_FORMAT_VERSION,
            content_hash: "blake3:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
                .to_owned(),
            bytes: Vec::new(),
        }],
    }));
    let migration_commit = migrated.commit.expect("migration boundary");
    let original_boundary = migration_commit
        .snapshot
        .migration_boundary
        .clone()
        .expect("lineage boundary");
    let mut store = InMemoryDocumentStore::default();
    store
        .commit_migration_atomic(migration_commit)
        .expect("migration commit");

    let planner = CommitPlanner::new(SnapshotPolicy::Periodic {
        transaction_interval: 2,
        byte_interval: u64::MAX,
    });
    for index in 0..3_u32 {
        let before = success(recover(
            store.load_records().expect("records before command"),
        ));
        let applied = success(apply_command(ApplyCommandRequest {
            canonical_json: before.session.canonical_json,
            history: before.session.history,
            command: CommandDto {
                command_id: CommandId::new(format!("00000000-0000-4000-8000-{:012}", 906 + index))
                    .expect("command ID"),
                base_revision: before.session.revision,
                modality: SourceModality::Api,
                issued_at: format!("2026-08-14T20:41:0{}Z", index + 1),
                kind: CommandKind::InsertText {
                    target: before
                        .session
                        .next_command_target
                        .expect("sample command target"),
                    text: format!("-{index}"),
                },
            },
        }));
        assert!(
            applied.commit.snapshot.migration_boundary.is_none(),
            "the operation layer must not invent durable migration lineage"
        );
        planner
            .commit(
                &mut store,
                applied.commit,
                SnapshotReason::CommittedTransaction,
            )
            .expect("planner-backed migrated commit");
    }

    let records = store.load_records().expect("post-migration records");
    let latest_snapshot = records
        .snapshots
        .iter()
        .filter(|snapshot| snapshot.schema_version == 1)
        .max_by_key(|snapshot| snapshot.revision)
        .expect("latest current-schema checkpoint")
        .clone();
    assert_eq!(latest_snapshot.revision, original_boundary.revision + 2);
    assert_eq!(
        latest_snapshot.migration_boundary.as_ref(),
        Some(&original_boundary),
        "the planner carries the original boundary into later checkpoints"
    );
    let recovered = success(recover(records.clone()));
    assert_eq!(recovered.session.revision, latest_snapshot.revision + 1);
    assert_eq!(
        records
            .transactions
            .iter()
            .filter(|transaction| transaction.new_revision > latest_snapshot.revision)
            .count(),
        1,
        "only the bounded post-checkpoint tail is needed for final replay"
    );

    let latest_only = flow_core::RecoverRequest {
        snapshots: vec![latest_snapshot],
        transactions: records.transactions.clone(),
        audits: records.audits.clone(),
        assets: records.assets.clone(),
        sources: records.sources.clone(),
    };
    assert_eq!(
        success(recover(latest_only.clone())).session.canonical_hash,
        recovered.session.canonical_hash,
        "the later checkpoint is independently proven back to the exact source boundary"
    );

    let mut source_tampered = latest_only.clone();
    source_tampered.sources[0].canonical_json.push(' ');
    assert_eq!(
        recover(source_tampered.clone())
            .error
            .expect("source tamper diagnostic")
            .code,
        "FLOW_RECOVERY_GAP"
    );

    let mut boundary_tampered = latest_only.clone();
    boundary_tampered.snapshots[0]
        .migration_boundary
        .as_mut()
        .expect("boundary")
        .revision += 1;
    assert_eq!(
        recover(boundary_tampered.clone())
            .error
            .expect("boundary tamper diagnostic")
            .code,
        "FLOW_RECOVERY_GAP"
    );
    let mut fallback_records = records.clone();
    fallback_records
        .snapshots
        .iter_mut()
        .max_by_key(|snapshot| snapshot.revision)
        .expect("newest snapshot")
        .migration_boundary
        .as_mut()
        .expect("newest boundary")
        .revision += 1;
    assert_eq!(
        success(recover(fallback_records)).session.canonical_hash,
        recovered.session.canonical_hash,
        "recovery rejects the tampered candidate and falls back to the older proven boundary"
    );

    let ghost_id =
        CommandId::new("00000000-0000-4000-8000-000000000910").expect("ghost command ID");
    let mut ghost_transaction = records.transactions[0].clone();
    ghost_transaction.transaction_id = ghost_id.clone();
    ghost_transaction.command_id = ghost_id.clone();
    ghost_transaction.base_revision = 0;
    ghost_transaction.new_revision = original_boundary.revision;
    ghost_transaction.issued_at = "2026-08-14T20:41:10Z".to_owned();
    let ghost_audit = AuditRecord::success(
        ghost_id.clone(),
        original_boundary.document_id.clone(),
        ghost_id.clone(),
        ghost_id,
        0,
        original_boundary.revision,
        u64::from(original_boundary.revision),
        AuditTimestamp::parse("2026-08-14T20:41:10Z").expect("ghost timestamp"),
        AuditAction::Command {
            command_kind: AuditCommandKind::InsertText,
        },
        SourceModality::Api,
        vec![AuditMetadata::SchemaVersion { value: 1 }],
    )
    .expect("structurally valid ghost audit");
    let mut ghost_records = latest_only.clone();
    ghost_records.transactions.push(ghost_transaction);
    ghost_records.audits.push(ghost_audit);
    assert_eq!(
        recover(ghost_records)
            .error
            .expect("pre-boundary transaction diagnostic")
            .code,
        "FLOW_RECOVERY_GAP"
    );

    for (modality, sequence, timestamp) in [
        (
            SourceModality::Api,
            u64::from(original_boundary.revision),
            original_boundary.issued_at.as_str(),
        ),
        (
            SourceModality::System,
            u64::from(original_boundary.revision) + 1,
            original_boundary.issued_at.as_str(),
        ),
        (
            SourceModality::System,
            u64::from(original_boundary.revision),
            "2026-08-14T20:49:59Z",
        ),
    ] {
        let mut audit_tampered = latest_only.clone();
        let migration_audit = AuditRecord::success(
            original_boundary.migration_id.clone(),
            original_boundary.document_id.clone(),
            original_boundary.migration_id.clone(),
            original_boundary.migration_id.clone(),
            original_boundary.revision,
            original_boundary.revision,
            sequence,
            AuditTimestamp::parse(timestamp).expect("tampered timestamp is structurally valid"),
            AuditAction::Migration,
            modality,
            vec![AuditMetadata::Migration {
                from_schema_version: original_boundary.source_schema_version,
                to_schema_version: original_boundary.current_schema_version,
            }],
        )
        .expect("structurally valid tampered migration audit");
        let index = audit_tampered
            .audits
            .iter()
            .position(|audit| matches!(audit.action(), AuditAction::Migration))
            .expect("migration audit");
        audit_tampered.audits[index] = migration_audit;
        assert_eq!(
            recover(audit_tampered)
                .error
                .expect("audit tamper diagnostic")
                .code,
            "FLOW_RECOVERY_GAP"
        );
    }

    let final_state = success(recover(records.clone()));
    let candidate = success(apply_command(ApplyCommandRequest {
        canonical_json: final_state.session.canonical_json,
        history: final_state.session.history,
        command: CommandDto {
            command_id: CommandId::new("00000000-0000-4000-8000-000000000909")
                .expect("candidate command ID"),
            base_revision: final_state.session.revision,
            modality: SourceModality::Api,
            issued_at: "2026-08-14T20:41:04Z".to_owned(),
            kind: CommandKind::InsertText {
                target: final_state
                    .session
                    .next_command_target
                    .expect("sample command target"),
                text: "-candidate".to_owned(),
            },
        },
    }));
    let planned = planner
        .plan(
            &records,
            candidate.commit.clone(),
            SnapshotReason::CommittedTransaction,
        )
        .expect("clean planner input");
    assert_eq!(
        planned
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.migration_boundary.as_ref()),
        Some(&original_boundary),
        "the exact cadence checkpoint receives the verified lineage"
    );
    let mut forged_candidate = candidate.commit.clone();
    forged_candidate.transaction.before_hash = original_boundary.source_canonical_hash.clone();
    assert_eq!(
        planner
            .plan(
                &records,
                forged_candidate,
                SnapshotReason::CommittedTransaction,
            )
            .expect_err("planner proves the transaction against the recovered durable head"),
        StoreError::InvalidCommit
    );

    let mut rejected_newer_snapshot_records = records.clone();
    let mut rejected_newer_snapshot = rejected_newer_snapshot_records
        .snapshots
        .iter()
        .max_by_key(|snapshot| snapshot.revision)
        .expect("checkpoint")
        .clone();
    rejected_newer_snapshot.revision = final_state.session.revision;
    rejected_newer_snapshot_records
        .snapshots
        .push(rejected_newer_snapshot);
    assert!(
        planner
            .plan(
                &rejected_newer_snapshot_records,
                candidate.commit.clone(),
                SnapshotReason::CommittedTransaction,
            )
            .expect("rejected snapshot metadata cannot influence cadence")
            .snapshot
            .is_some(),
        "cadence is derived from the checkpoint recovery actually selected"
    );

    let mut source_format_tampered = records.clone();
    source_format_tampered.sources[0].record_format_version += 1;
    assert_eq!(
        planner
            .plan(
                &source_format_tampered,
                candidate.commit.clone(),
                SnapshotReason::CommittedTransaction,
            )
            .expect_err("planner validates raw migration source envelopes"),
        StoreError::PartialRecordSet
    );
    assert_eq!(
        planner
            .plan(
                &source_tampered,
                candidate.commit.clone(),
                SnapshotReason::CommittedTransaction,
            )
            .expect_err("planner rejects a tampered migration source"),
        StoreError::PartialRecordSet
    );
    assert_eq!(
        planner
            .plan(
                &boundary_tampered,
                candidate.commit,
                SnapshotReason::CommittedTransaction,
            )
            .expect_err("planner rejects a tampered migration boundary"),
        StoreError::PartialRecordSet
    );
}

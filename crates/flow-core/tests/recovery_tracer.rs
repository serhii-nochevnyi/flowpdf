use flow_core::{
    ApiResponse, ApplyCommandRequest, CommandDto, CommandKind, CreateSampleRequest,
    OperationResult, RecoverResult, SourceModality, apply_command, create_sample, recover,
    store::{CommitDisposition, DocumentStore, InMemoryDocumentStore},
};
use flow_core::model::CommandId;

fn success<T>(response: ApiResponse<T>) -> T {
    assert!(response.ok, "unexpected error: {:?}", response.error);
    response.value.expect("successful response has a value")
}

fn apply_one(created: &OperationResult) -> OperationResult {
    success(apply_command(ApplyCommandRequest {
        canonical_json: created.session.canonical_json.clone(),
        history: created.session.history.clone(),
        command: CommandDto {
            command_id: CommandId::new("00000000-0000-4000-8000-000000000901")
                .expect("command ID"),
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

    let recovered: RecoverResult = success(recover(records));
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
        recovered.view.revision_provenance.revision,
        applied.session.revision
    );
    assert_eq!(
        recovered.view.revision_provenance.canonical_hash,
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

use flow_core::{
    ApiResponse, ApplyCommandRequest, AuditRecord, CommandDto, CommandKind, CreateSampleRequest,
    OperationResult, PersistenceCommit, RecoverRequest, SourceModality, TransactionRecord,
    apply_command, create_sample,
    model::{CommandId, DocumentId},
    recover,
    schema::DocumentLimits,
    store::{
        CommitDisposition, CommitPlanner, DocumentStore, InMemoryDocumentStore, SnapshotPolicy,
        SnapshotReason,
    },
};
use serde::Deserialize;

fn success<T>(response: ApiResponse<T>) -> T {
    assert!(response.ok, "unexpected error: {:?}", response.error);
    response.value.expect("successful response has a value")
}

fn command_id(value: u32) -> CommandId {
    CommandId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("command ID")
}

fn chain_with_commands(command_count: u32) -> Vec<OperationResult> {
    let created = success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
    }));
    let mut results = vec![created];
    for index in 0..command_count {
        let previous = results.last().expect("previous revision");
        let applied = success(apply_command(ApplyCommandRequest {
            canonical_json: previous.session.canonical_json.clone(),
            history: previous.session.history.clone(),
            command: CommandDto {
                command_id: command_id(920 + index),
                base_revision: previous.session.revision,
                modality: SourceModality::Api,
                issued_at: format!("2026-08-14T20:41:{:02}Z", index + 1),
                kind: CommandKind::InsertText {
                    target: previous
                        .session
                        .next_command_target
                        .clone()
                        .expect("sample command target"),
                    text: format!("-{index}"),
                },
            },
        }));
        results.push(applied);
    }
    results
}

fn chain() -> Vec<OperationResult> {
    chain_with_commands(2)
}

#[test]
fn creation_planning_rejects_every_partial_durable_record_set() {
    let chain = chain_with_commands(0);
    let created = chain.first().expect("creation");
    let planner = CommitPlanner::new(SnapshotPolicy::EveryTransaction);
    let complete = request(&chain, &[0]);

    planner
        .plan(
            &RecoverRequest {
                snapshots: Vec::new(),
                transactions: Vec::new(),
                audits: Vec::new(),
                assets: Vec::new(),
                sources: Vec::new(),
            },
            created.commit.clone(),
            SnapshotReason::Creation,
        )
        .expect("truly empty creation");
    planner
        .plan(&complete, created.commit.clone(), SnapshotReason::Creation)
        .expect("complete exact retry");

    let mut partial_sets = Vec::new();
    let mut missing_snapshot = complete.clone();
    missing_snapshot.snapshots.clear();
    partial_sets.push(missing_snapshot);
    let mut missing_transaction = complete.clone();
    missing_transaction.transactions.clear();
    partial_sets.push(missing_transaction);
    let mut missing_audit = complete.clone();
    missing_audit.audits.clear();
    partial_sets.push(missing_audit);
    let mut missing_asset = complete;
    missing_asset.assets.clear();
    partial_sets.push(missing_asset);

    for partial in partial_sets {
        assert_eq!(
            planner
                .plan(&partial, created.commit.clone(), SnapshotReason::Creation,)
                .expect_err("partial creation must not be repaired"),
            flow_core::store::StoreError::PartialRecordSet
        );
    }
}

fn commit(
    planner: &CommitPlanner,
    store: &mut InMemoryDocumentStore,
    candidate: &PersistenceCommit,
    reason: SnapshotReason,
) -> CommitDisposition {
    planner
        .commit(store, candidate.clone(), reason)
        .expect("valid durable commit")
}

fn persist_chain(chain: &[OperationResult], policy: SnapshotPolicy) -> InMemoryDocumentStore {
    let planner = CommitPlanner::new(policy);
    let mut store = InMemoryDocumentStore::default();
    for (index, result) in chain.iter().enumerate() {
        let reason = if index == 0 {
            SnapshotReason::Creation
        } else {
            SnapshotReason::CommittedTransaction
        };
        assert_eq!(
            commit(&planner, &mut store, &result.commit, reason),
            CommitDisposition::Committed
        );
    }
    store
}

fn request(chain: &[OperationResult], snapshot_indices: &[usize]) -> RecoverRequest {
    RecoverRequest {
        snapshots: snapshot_indices
            .iter()
            .map(|index| chain[*index].commit.snapshot.clone())
            .collect(),
        transactions: chain
            .iter()
            .map(|result| result.commit.transaction.clone())
            .collect(),
        audits: chain
            .iter()
            .map(|result| result.commit.audit.clone())
            .collect(),
        assets: chain
            .iter()
            .flat_map(|result| result.commit.assets.clone())
            .collect(),
        sources: Vec::new(),
    }
}

fn assert_failure(request: RecoverRequest, expected: &str) {
    let response = recover(request);
    assert!(!response.ok);
    assert!(response.value.is_none());
    assert_eq!(
        response.error.expect("stable recovery error").code,
        expected
    );
}

fn assert_store_recovers_exactly(store: &InMemoryDocumentStore, expected: &OperationResult) {
    let records = store.load_records().expect("durable records");
    let recovered = success(recover(records));
    assert_eq!(recovered.session.revision, expected.session.revision);
    assert_eq!(
        recovered.session.canonical_json,
        expected.session.canonical_json
    );
    assert_eq!(
        recovered.session.canonical_hash,
        expected.session.canonical_hash
    );
    assert_eq!(recovered.session.history, expected.session.history);
}

#[test]
fn checkpoint_policies_change_durable_work_only_and_never_recovered_truth() {
    let chain = chain();
    let expected = chain.last().expect("final revision");
    let policies = [
        (SnapshotPolicy::NoIntermediate, 1),
        (SnapshotPolicy::EveryTransaction, 3),
        (
            SnapshotPolicy::Periodic {
                transaction_interval: 2,
                byte_interval: u64::MAX,
            },
            2,
        ),
        (
            SnapshotPolicy::Periodic {
                transaction_interval: u32::MAX,
                byte_interval: u64::MAX,
            },
            1,
        ),
    ];

    for (policy, expected_snapshot_count) in policies {
        let store = persist_chain(&chain, policy);
        let records = store.load_records().expect("durable records");
        assert_eq!(records.snapshots.len(), expected_snapshot_count);
        assert_eq!(records.transactions.len(), chain.len());
        assert_eq!(records.audits.len(), chain.len());
        assert_store_recovers_exactly(&store, expected);
    }
}

#[test]
fn periodic_transaction_threshold_snapshots_at_exact_n_then_resets_for_n_plus_one() {
    let chain = chain_with_commands(3);
    let planner = CommitPlanner::new(SnapshotPolicy::Periodic {
        transaction_interval: 2,
        byte_interval: u64::MAX,
    });
    let mut store = InMemoryDocumentStore::default();

    commit(
        &planner,
        &mut store,
        &chain[0].commit,
        SnapshotReason::Creation,
    );
    commit(
        &planner,
        &mut store,
        &chain[1].commit,
        SnapshotReason::CommittedTransaction,
    );
    assert_eq!(
        store.load_records().expect("N - 1 records").snapshots.len(),
        1
    );

    commit(
        &planner,
        &mut store,
        &chain[2].commit,
        SnapshotReason::CommittedTransaction,
    );
    assert_eq!(store.load_records().expect("N records").snapshots.len(), 2);

    commit(
        &planner,
        &mut store,
        &chain[3].commit,
        SnapshotReason::CommittedTransaction,
    );
    assert_eq!(
        store.load_records().expect("N + 1 records").snapshots.len(),
        2,
        "the checkpoint at N resets the durable cadence"
    );
    assert_store_recovers_exactly(&store, chain.last().expect("final revision"));
}

#[test]
fn periodic_byte_threshold_uses_exact_serialized_transaction_bytes() {
    let chain = chain_with_commands(1);
    let transaction_bytes = u64::try_from(
        serde_json::to_vec(&chain[1].commit.transaction)
            .unwrap()
            .len(),
    )
    .expect("serialized transaction length");

    for (byte_interval, expected_snapshot_count) in
        [(transaction_bytes + 1, 1), (transaction_bytes, 2)]
    {
        let store = persist_chain(
            &chain,
            SnapshotPolicy::Periodic {
                transaction_interval: u32::MAX,
                byte_interval,
            },
        );
        assert_eq!(
            store
                .load_records()
                .expect("byte-policy records")
                .snapshots
                .len(),
            expected_snapshot_count
        );
        assert_store_recovers_exactly(&store, chain.last().expect("final revision"));
    }
}

#[test]
fn explicit_local_save_forces_a_snapshot_under_no_intermediate_policy() {
    let chain = chain_with_commands(1);
    let planner = CommitPlanner::new(SnapshotPolicy::NoIntermediate);
    let mut store = InMemoryDocumentStore::default();
    commit(
        &planner,
        &mut store,
        &chain[0].commit,
        SnapshotReason::Creation,
    );
    commit(
        &planner,
        &mut store,
        &chain[1].commit,
        SnapshotReason::ExplicitLocalSave,
    );

    assert_eq!(
        store
            .load_records()
            .expect("explicit-save records")
            .snapshots
            .len(),
        2
    );
    assert_store_recovers_exactly(&store, chain.last().expect("final revision"));
}

#[test]
fn retrying_a_commit_that_skipped_its_checkpoint_is_idempotent() {
    let chain = chain_with_commands(1);
    let planner = CommitPlanner::new(SnapshotPolicy::NoIntermediate);
    let mut store = InMemoryDocumentStore::default();
    commit(
        &planner,
        &mut store,
        &chain[0].commit,
        SnapshotReason::Creation,
    );
    assert_eq!(
        commit(
            &planner,
            &mut store,
            &chain[1].commit,
            SnapshotReason::CommittedTransaction,
        ),
        CommitDisposition::Committed
    );
    let before_retry = store.load_records().expect("records before retry");

    let restarted_planner = CommitPlanner::new(SnapshotPolicy::NoIntermediate);
    assert_eq!(
        commit(
            &restarted_planner,
            &mut store,
            &chain[1].commit,
            SnapshotReason::CommittedTransaction,
        ),
        CommitDisposition::Idempotent
    );
    assert_eq!(
        store.load_records().expect("records after retry"),
        before_retry
    );
    assert_eq!(before_retry.snapshots.len(), 1);
    assert_store_recovers_exactly(&store, chain.last().expect("final revision"));
}

#[test]
fn identical_records_are_idempotent_and_corrupt_newest_snapshot_falls_back() {
    let chain = chain();
    let expected = chain.last().expect("final revision");
    let mut repeated = request(&chain, &[0]);
    repeated.transactions.push(repeated.transactions[1].clone());
    repeated.audits.push(repeated.audits[1].clone());
    repeated.snapshots.push(repeated.snapshots[0].clone());
    let recovered = success(recover(repeated));
    assert_eq!(
        recovered.session.canonical_hash,
        expected.session.canonical_hash
    );
    assert_eq!(recovered.view.audit.len(), 3);

    let mut fallback = request(&chain, &[0, 1]);
    fallback.snapshots[1].canonical_hash.push_str("corrupt");
    let recovered = success(recover(fallback));
    assert_eq!(
        recovered.session.canonical_hash,
        expected.session.canonical_hash
    );
}

#[test]
fn gaps_conflicts_identity_schema_hash_and_atomicity_fail_without_publication() {
    let chain = chain();

    let checkpoint_without_prefix = RecoverRequest {
        snapshots: vec![chain[2].commit.snapshot.clone()],
        transactions: vec![chain[2].commit.transaction.clone()],
        audits: vec![chain[2].commit.audit.clone()],
        assets: chain[2].commit.assets.clone(),
        sources: Vec::new(),
    };
    assert_failure(checkpoint_without_prefix, "FLOW_RECOVERY_GAP");

    let mut gap = request(&chain, &[0]);
    gap.transactions.remove(1);
    gap.audits.remove(1);
    assert_failure(gap, "FLOW_RECOVERY_GAP");

    let mut conflict = request(&chain, &[0]);
    let mut divergent = conflict.transactions[1].clone();
    divergent.issued_at = "2026-08-14T20:49:59Z".to_owned();
    conflict.transactions.push(divergent);
    assert_failure(conflict, "FLOW_RECOVERY_GAP");

    let mut corrupt = request(&chain, &[2]);
    corrupt.snapshots[0].canonical_hash.push_str("corrupt");
    assert_failure(corrupt, "FLOW_HASH_MISMATCH");

    let mut wrong_identity = request(&chain, &[0]);
    wrong_identity.transactions[1].document_id =
        DocumentId::new("00000000-0000-4000-8000-000000009999").expect("other document");
    assert_failure(wrong_identity, "FLOW_RECOVERY_GAP");

    let mut unknown_schema = request(&chain, &[0]);
    unknown_schema.transactions[1].schema_version = 99;
    assert_failure(unknown_schema, "FLOW_RECOVERY_GAP");

    let mut future_snapshot_schema = request(&chain, &[0]);
    future_snapshot_schema.snapshots[0].schema_version = 99;
    assert_failure(future_snapshot_schema, "FLOW_RECOVERY_GAP");

    let mut unknown_record_format = request(&chain, &[0]);
    unknown_record_format.snapshots[0].record_format_version = 99;
    assert_failure(unknown_record_format, "FLOW_RECOVERY_GAP");

    let mut future_transaction_below_checkpoint = request(&chain, &[2]);
    future_transaction_below_checkpoint.transactions[0].schema_version = 99;
    assert_failure(future_transaction_below_checkpoint, "FLOW_RECOVERY_GAP");

    let mut future_format_below_checkpoint = request(&chain, &[2]);
    future_format_below_checkpoint.transactions[0].record_format_version = 99;
    assert_failure(future_format_below_checkpoint, "FLOW_RECOVERY_GAP");

    let mut interrupted = request(&chain[..2], &[0, 1]);
    interrupted.audits.pop();
    assert_failure(interrupted, "FLOW_RECOVERY_GAP");
}

#[test]
fn recovery_work_hard_stops_at_n_plus_one_before_deduplication() {
    let chain = chain();
    let transaction: TransactionRecord = chain[0].commit.transaction.clone();
    let transactions = vec![transaction; DocumentLimits::V1.recovery_records + 1];
    let audit: AuditRecord = chain[0].commit.audit.clone();
    assert_failure(
        RecoverRequest {
            snapshots: vec![chain[0].commit.snapshot.clone()],
            transactions,
            audits: vec![audit],
            assets: chain[0].commit.assets.clone(),
            sources: Vec::new(),
        },
        "FLOW_LIMIT_RECOVERY_RECORDS",
    );

    let mut raw_record_overflow = request(&chain, &[2]);
    let current_count = raw_record_overflow.snapshots.len()
        + raw_record_overflow.transactions.len()
        + raw_record_overflow.audits.len()
        + raw_record_overflow.assets.len()
        + raw_record_overflow.sources.len();
    let repeated_asset = raw_record_overflow.assets[0].clone();
    raw_record_overflow.assets.extend(vec![
        repeated_asset;
        DocumentLimits::V1.recovery_records + 1 - current_count
    ]);
    assert_failure(raw_record_overflow, "FLOW_LIMIT_RECOVERY_RECORDS");
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryFixture {
    format_version: u32,
    cases: Vec<RecoveryCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryCase {
    id: String,
    expected: String,
}

#[test]
fn checked_in_recovery_case_table_names_every_locked_class() {
    let fixture: RecoveryFixture =
        serde_json::from_str(include_str!("../../../fixtures/recovery/cases.json"))
            .expect("recovery fixture");
    assert_eq!(fixture.format_version, 1);
    let ids = fixture
        .cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<Vec<_>>();
    for required in [
        "valid-tail-replay",
        "repeated-identical",
        "newest-corrupt-falls-back",
        "revision-gap",
        "divergent-duplicate",
        "corrupt-only-snapshot-hash",
        "wrong-document-identity",
        "unknown-schema-tail",
        "interrupted-atomic-commit",
        "bounded-work-n-plus-one",
    ] {
        assert!(ids.contains(&required), "missing fixture case {required}");
    }
    assert!(fixture.cases.iter().all(|case| !case.expected.is_empty()));
}

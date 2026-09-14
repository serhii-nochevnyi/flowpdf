//! Adapter-neutral durable record protocol.
//!
//! A store implementation performs physical atomic reads and writes only.
//! Canonical validation, recovery selection, replay, audit projection, and
//! repair policy remain in the Rust core.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AuditRecord, MigrationPersistenceCommit, PersistenceCommit, RecoverRequest, SnapshotRecord,
    TransactionRecord, asset,
    canonical::{asset_hash, canonical_hash, decode_canonical, verify_asset_bytes},
    model::FlowDocument,
    schema::SchemaError,
    transaction::{
        EditorState, HistoryEffect, Operation, SourceModality, replay_forward,
        validate_private_preimages,
    },
};

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "mode",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SnapshotPolicy {
    NoIntermediate,
    EveryTransaction,
    Periodic {
        transaction_interval: u32,
        byte_interval: u64,
    },
}

impl Default for SnapshotPolicy {
    fn default() -> Self {
        Self::Periodic {
            transaction_interval: 100,
            byte_interval: 4 * MIB,
        }
    }
}

impl SnapshotPolicy {
    #[must_use]
    pub const fn benchmark_candidates() -> [Self; 4] {
        [
            Self::Periodic {
                transaction_interval: 100,
                byte_interval: 4 * MIB,
            },
            Self::Periodic {
                transaction_interval: 50,
                byte_interval: 2 * MIB,
            },
            Self::Periodic {
                transaction_interval: 25,
                byte_interval: MIB,
            },
            Self::Periodic {
                transaction_interval: 10,
                byte_interval: MIB,
            },
        ]
    }

    #[must_use]
    pub const fn should_snapshot(
        self,
        reason: SnapshotReason,
        uncheckpointed_transactions: u32,
        uncheckpointed_bytes: u64,
    ) -> bool {
        match reason {
            SnapshotReason::Creation
            | SnapshotReason::ExplicitLocalSave
            | SnapshotReason::Migration => true,
            SnapshotReason::CommittedTransaction => match self {
                Self::NoIntermediate => false,
                Self::EveryTransaction => true,
                Self::Periodic {
                    transaction_interval,
                    byte_interval,
                } => {
                    uncheckpointed_transactions >= transaction_interval
                        || uncheckpointed_bytes >= byte_interval
                }
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotReason {
    Creation,
    ExplicitLocalSave,
    Migration,
    CommittedTransaction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetRecord {
    pub record_format_version: u32,
    pub content_hash: String,
    pub bytes: Vec<u8>,
}

/// Immutable source bytes retained when a schema migration creates a new
/// current-schema replay boundary. Recovery verifies this record against the
/// boundary but never replays legacy operations from it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationSourceRecord {
    pub record_format_version: u32,
    pub document_id: crate::model::DocumentId,
    pub schema_version: u32,
    pub canonical_json: String,
    pub canonical_hash: String,
}

/// Physical mutation set produced by [`CommitPlanner`].
///
/// The canonical post-state snapshot carried by [`PersistenceCommit`] is a
/// transient proof supplied to the Rust core. A physical adapter stores it
/// only when the Rust-owned policy selects a checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlannedPersistenceCommit {
    pub replace_existing: bool,
    pub snapshot: Option<SnapshotRecord>,
    pub transaction: TransactionRecord,
    pub audit: AuditRecord,
    pub assets: Vec<AssetRecord>,
}

impl From<PersistenceCommit> for PlannedPersistenceCommit {
    fn from(commit: PersistenceCommit) -> Self {
        Self {
            replace_existing: commit.replace_existing,
            snapshot: Some(commit.snapshot),
            transaction: commit.transaction,
            audit: commit.audit,
            assets: commit.assets,
        }
    }
}

/// Stateless policy engine. Progress is derived from durable records on each
/// call so a worker restart cannot reset the transaction/byte cadence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitPlanner {
    policy: SnapshotPolicy,
}

impl CommitPlanner {
    #[must_use]
    pub const fn new(policy: SnapshotPolicy) -> Self {
        Self { policy }
    }

    #[must_use]
    pub const fn policy(self) -> SnapshotPolicy {
        self.policy
    }

    pub fn plan(
        self,
        records: &RecoverRequest,
        mut commit: PersistenceCommit,
        reason: SnapshotReason,
    ) -> Result<PlannedPersistenceCommit, StoreError> {
        attach_migration_boundary(records, &mut commit)?;
        let document = if commit.transaction.base_revision == 0 {
            validate_commit_set(&commit)?
        } else {
            // A follow-up semantic transaction carries only newly redeemed
            // physical assets. Existing bytes are already durable and must
            // participate in validation without being copied into every
            // semantic response or browser request.
            let proof = commit_with_durable_assets(records, &commit)?;
            validate_commit_set(&proof)?
        };
        let checkpoint_revision = validate_commit_against_records(records, &commit, &document)?;
        validate_snapshot_reason(&commit.transaction, reason)?;

        let mut seen = std::collections::BTreeMap::<&str, &TransactionRecord>::new();
        let mut uncheckpointed_transactions = 0_u32;
        let mut uncheckpointed_bytes = 0_u64;
        for transaction in records.transactions.iter().filter(|transaction| {
            transaction.document_id == commit.transaction.document_id
                && transaction.schema_version == commit.transaction.schema_version
                && transaction.new_revision > checkpoint_revision
                && transaction.new_revision <= commit.transaction.base_revision
        }) {
            match seen.get(transaction.transaction_id.as_str()) {
                Some(existing) if *existing == transaction => continue,
                Some(_) => return Err(StoreError::PartialRecordSet),
                None => {
                    seen.insert(transaction.transaction_id.as_str(), transaction);
                }
            }
            uncheckpointed_transactions = uncheckpointed_transactions
                .checked_add(1)
                .ok_or(StoreError::InvalidCommit)?;
            uncheckpointed_bytes = uncheckpointed_bytes
                .checked_add(transaction_record_bytes(transaction)?)
                .ok_or(StoreError::InvalidCommit)?;
        }

        let already_durable = records.transactions.iter().any(|transaction| {
            transaction.transaction_id == commit.transaction.transaction_id
                && transaction == &commit.transaction
        });
        if !already_durable {
            uncheckpointed_transactions = uncheckpointed_transactions
                .checked_add(1)
                .ok_or(StoreError::InvalidCommit)?;
            uncheckpointed_bytes = uncheckpointed_bytes
                .checked_add(transaction_record_bytes(&commit.transaction)?)
                .ok_or(StoreError::InvalidCommit)?;
        }

        let keep_snapshot =
            self.policy
                .should_snapshot(reason, uncheckpointed_transactions, uncheckpointed_bytes);
        let planned = PlannedPersistenceCommit {
            replace_existing: commit.replace_existing,
            snapshot: keep_snapshot.then_some(commit.snapshot),
            transaction: commit.transaction,
            audit: commit.audit,
            assets: commit.assets,
        };
        let candidate = project_planned_records(records, &planned)?;
        crate::preflight_recovery_records(&candidate).map_err(|_| StoreError::InvalidCommit)?;
        Ok(planned)
    }

    pub fn commit<S: DocumentStore>(
        self,
        store: &mut S,
        commit: PersistenceCommit,
        reason: SnapshotReason,
    ) -> Result<CommitDisposition, StoreError> {
        let records = store.load_records()?;
        let planned = self.plan(&records, commit, reason)?;
        store.commit_planned_atomic(planned)
    }
}

fn commit_with_durable_assets(
    records: &RecoverRequest,
    commit: &PersistenceCommit,
) -> Result<PersistenceCommit, StoreError> {
    let mut proof = commit.clone();
    for durable in &records.assets {
        match proof
            .assets
            .iter()
            .find(|asset| asset.content_hash == durable.content_hash)
        {
            Some(incoming) if incoming == durable => {}
            Some(_) => return Err(StoreError::IdentityConflict),
            None => proof.assets.push(durable.clone()),
        }
    }
    Ok(proof)
}

fn validate_snapshot_reason(
    transaction: &TransactionRecord,
    reason: SnapshotReason,
) -> Result<(), StoreError> {
    match (transaction.base_revision, reason) {
        (0, SnapshotReason::Creation) => Ok(()),
        (0, _) | (_, SnapshotReason::Creation | SnapshotReason::Migration) => {
            Err(StoreError::InvalidCommit)
        }
        (_, SnapshotReason::ExplicitLocalSave | SnapshotReason::CommittedTransaction) => Ok(()),
    }
}

fn transaction_record_bytes(transaction: &TransactionRecord) -> Result<u64, StoreError> {
    let bytes = serde_json::to_vec(transaction).map_err(|_| StoreError::InvalidCommit)?;
    u64::try_from(bytes.len()).map_err(|_| StoreError::InvalidCommit)
}

fn project_planned_records(
    records: &RecoverRequest,
    planned: &PlannedPersistenceCommit,
) -> Result<RecoverRequest, StoreError> {
    let mut candidate = records.clone();
    if let Some(snapshot) = &planned.snapshot {
        project_record(&mut candidate.snapshots, snapshot, |record| {
            record.document_id == snapshot.document_id
                && record.schema_version == snapshot.schema_version
                && record.revision == snapshot.revision
        })?;
    }
    project_record(
        &mut candidate.transactions,
        &planned.transaction,
        |record| record.transaction_id == planned.transaction.transaction_id,
    )?;
    project_record(&mut candidate.audits, &planned.audit, |record| {
        record.audit_id() == planned.audit.audit_id()
    })?;
    for asset in &planned.assets {
        project_record(&mut candidate.assets, asset, |record| {
            record.content_hash == asset.content_hash
        })?;
    }
    Ok(candidate)
}

fn project_record<T: Clone + PartialEq>(
    records: &mut Vec<T>,
    candidate: &T,
    same_identity: impl Fn(&T) -> bool,
) -> Result<(), StoreError> {
    let mut found = false;
    for record in records.iter().filter(|record| same_identity(record)) {
        if record != candidate {
            return Err(StoreError::IdentityConflict);
        }
        found = true;
    }
    if !found {
        records.push(candidate.clone());
    }
    Ok(())
}

/// Operation DTOs are intentionally independent of the physical store, so a
/// post-migration operation cannot invent or rewrite its source lineage. The
/// Rust planner copies the single verified immutable boundary from durable
/// records into the candidate checkpoint before deciding whether it is due.
fn attach_migration_boundary(
    records: &RecoverRequest,
    commit: &mut PersistenceCommit,
) -> Result<(), StoreError> {
    let document = decode_canonical(commit.snapshot.canonical_json.as_bytes())
        .map_err(|_| StoreError::InvalidCommit)?;
    if !matches!(
        document.provenance,
        crate::model::Provenance::Migrated { .. }
    ) {
        return Ok(());
    }

    let mut durable_boundary = None;
    for boundary in records
        .snapshots
        .iter()
        .filter(|snapshot| {
            snapshot.document_id == document.document_id
                && snapshot.schema_version == document.schema_version
        })
        .filter_map(|snapshot| snapshot.migration_boundary.as_ref())
    {
        boundary
            .validate_against_current(&document)
            .map_err(|_| StoreError::PartialRecordSet)?;
        match &durable_boundary {
            Some(existing) if existing != boundary => {
                return Err(StoreError::PartialRecordSet);
            }
            Some(_) => {}
            None => durable_boundary = Some(boundary.clone()),
        }
    }
    let boundary = durable_boundary.ok_or(StoreError::PartialRecordSet)?;
    if commit
        .snapshot
        .migration_boundary
        .as_ref()
        .is_some_and(|supplied| supplied != &boundary)
    {
        return Err(StoreError::InvalidCommit);
    }
    crate::validate_migration_source(&document, Some(&boundary), &records.sources)
        .map_err(|_| StoreError::PartialRecordSet)?;

    let mut matching_audit = None;
    for audit in records
        .audits
        .iter()
        .filter(|audit| audit.audit_id() == &boundary.migration_id)
    {
        if audit.validate().is_err()
            || !matches!(audit.action(), crate::audit::AuditAction::Migration)
            || audit.command_id() != &boundary.migration_id
            || audit.transaction_id() != &boundary.migration_id
            || audit.document_id() != &boundary.document_id
            || audit.base_revision() != boundary.revision
            || audit.new_revision() != boundary.revision
            || audit.durable_sequence() != u64::from(boundary.revision)
            || audit.transaction_type() != "migration"
            || audit.modality() != &SourceModality::System
            || audit.timestamp().as_str() != boundary.issued_at
            || audit.metadata()
                != [crate::audit::AuditMetadata::Migration {
                    from_schema_version: boundary.source_schema_version,
                    to_schema_version: boundary.current_schema_version,
                }]
            || !matches!(audit.outcome(), crate::audit::AuditOutcome::Success)
        {
            return Err(StoreError::PartialRecordSet);
        }
        match matching_audit {
            Some(existing) if existing != audit => return Err(StoreError::PartialRecordSet),
            Some(_) => {}
            None => matching_audit = Some(audit),
        }
    }
    if matching_audit.is_none() {
        return Err(StoreError::PartialRecordSet);
    }

    commit.snapshot.migration_boundary = Some(boundary);
    Ok(())
}

/// Proves that a logical commit extends the recovered durable head (or is an
/// exact retry of that head) before a browser/native adapter receives bytes to
/// persist. Self-consistent candidate JSON is not sufficient: it must be the
/// executable result of the durable prefix.
fn validate_commit_against_records(
    records: &RecoverRequest,
    commit: &PersistenceCommit,
    document: &FlowDocument,
) -> Result<u32, StoreError> {
    if commit.transaction.base_revision == 0 {
        if recovery_records_are_empty(records) {
            return Ok(0);
        }
        if exact_creation_records(records, commit) {
            return Ok(0);
        }
        return Err(StoreError::PartialRecordSet);
    }

    let mut already_durable = false;
    for transaction in records
        .transactions
        .iter()
        .filter(|transaction| transaction.transaction_id == commit.transaction.transaction_id)
    {
        if transaction != &commit.transaction {
            return Err(StoreError::PartialRecordSet);
        }
        already_durable = true;
    }

    let (recovered, checkpoint_revision) = crate::recover_inner_with_checkpoint(records.clone())
        .map_err(|_| StoreError::PartialRecordSet)?;
    let before = decode_canonical(recovered.session.canonical_json.as_bytes())
        .map_err(|_| StoreError::PartialRecordSet)?;
    if before.document_id != recovered.session.document_id
        || before.revision != recovered.session.revision
        || canonical_hash(recovered.session.canonical_json.as_bytes())
            != recovered.session.canonical_hash
    {
        return Err(StoreError::PartialRecordSet);
    }

    if already_durable {
        if before != *document
            || recovered.session.revision != commit.transaction.new_revision
            || recovered.session.canonical_hash != commit.transaction.after_hash
            || recovered.session.history != commit.snapshot.history
        {
            return Err(StoreError::InvalidCommit);
        }
        return Ok(checkpoint_revision);
    }

    if before.document_id != commit.transaction.document_id
        || before.schema_version != commit.transaction.schema_version
        || before.revision != commit.transaction.base_revision
        || recovered.session.canonical_hash != commit.transaction.before_hash
    {
        return Err(StoreError::InvalidCommit);
    }
    let mut history = recovered.session.history;
    let replayed =
        replay_forward(&before, &commit.transaction).map_err(|_| StoreError::InvalidCommit)?;
    crate::replay_history_effect(&mut history, &commit.transaction, &before, &replayed)
        .map_err(|_| StoreError::InvalidCommit)?;
    if replayed != *document || history != commit.snapshot.history {
        return Err(StoreError::InvalidCommit);
    }
    Ok(checkpoint_revision)
}

fn recovery_records_are_empty(records: &RecoverRequest) -> bool {
    records.snapshots.is_empty()
        && records.transactions.is_empty()
        && records.audits.is_empty()
        && records.assets.is_empty()
        && records.sources.is_empty()
}

fn exact_creation_records(records: &RecoverRequest, commit: &PersistenceCommit) -> bool {
    if records.snapshots.as_slice() != [commit.snapshot.clone()]
        || records.transactions.as_slice() != [commit.transaction.clone()]
        || records.audits.as_slice() != [commit.audit.clone()]
        || !records.sources.is_empty()
    {
        return false;
    }

    let expected_assets = commit
        .assets
        .iter()
        .map(|asset| (asset.content_hash.as_str(), asset))
        .collect::<std::collections::BTreeMap<_, _>>();
    let durable_assets = records
        .assets
        .iter()
        .map(|asset| (asset.content_hash.as_str(), asset))
        .collect::<std::collections::BTreeMap<_, _>>();
    records.assets.len() == durable_assets.len()
        && expected_assets.len() == durable_assets.len()
        && expected_assets
            .iter()
            .all(|(hash, asset)| durable_assets.get(hash) == Some(asset))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitDisposition {
    Committed,
    Idempotent,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StoreError {
    #[error("The atomic commit set is internally inconsistent")]
    InvalidCommit,
    #[error("A durable identity already exists with different bytes")]
    IdentityConflict,
    #[error("The durable record set is already partial or corrupt")]
    PartialRecordSet,
}

impl StoreError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidCommit => "FLOW_STORE_INVALID_COMMIT",
            Self::IdentityConflict => "FLOW_STORE_IDENTITY_CONFLICT",
            Self::PartialRecordSet => "FLOW_STORE_PARTIAL_RECORD_SET",
        }
    }
}

/// Physical durability port. Implementations must make `commit_atomic`
/// all-or-nothing and return raw typed records without selecting a snapshot.
pub trait DocumentStore {
    fn commit_planned_atomic(
        &mut self,
        commit: PlannedPersistenceCommit,
    ) -> Result<CommitDisposition, StoreError>;

    fn commit_atomic(
        &mut self,
        commit: PersistenceCommit,
    ) -> Result<CommitDisposition, StoreError> {
        self.commit_planned_atomic(commit.into())
    }

    fn load_records(&self) -> Result<RecoverRequest, StoreError>;

    fn commit_migration_atomic(
        &mut self,
        commit: MigrationPersistenceCommit,
    ) -> Result<CommitDisposition, StoreError>;

    fn commit_standalone_audit_atomic(
        &mut self,
        audit: AuditRecord,
    ) -> Result<CommitDisposition, StoreError>;
}

/// Validates the semantic and identity constraints for an audit-only write.
/// The physical adapter must repeat the identity/anchor checks in its atomic
/// transaction because the record image can change after this pure plan.
pub fn validate_standalone_audit(
    records: &RecoverRequest,
    audit: &AuditRecord,
) -> Result<(), StoreError> {
    audit.validate().map_err(|_| StoreError::InvalidCommit)?;
    let has_revision_anchor = records.snapshots.iter().any(|snapshot| {
        snapshot.record_format_version == crate::RECORD_FORMAT_VERSION
            && snapshot.schema_version == crate::model::SCHEMA_VERSION
            && snapshot.document_id == *audit.document_id()
            && snapshot.revision == audit.new_revision()
    }) || records.transactions.iter().any(|transaction| {
        transaction.record_format_version == crate::RECORD_FORMAT_VERSION
            && transaction.schema_version == crate::model::SCHEMA_VERSION
            && transaction.document_id == *audit.document_id()
            && transaction.new_revision == audit.new_revision()
    });
    let valid_semantics = match (audit.action(), audit.outcome()) {
        (crate::audit::AuditAction::Command { .. }, crate::audit::AuditOutcome::Failure { .. }) => {
            audit.audit_id() != audit.command_id()
                && audit.metadata()
                    == [crate::audit::AuditMetadata::SchemaVersion {
                        value: crate::model::SCHEMA_VERSION,
                    }]
        }
        (
            crate::audit::AuditAction::Recovery,
            crate::audit::AuditOutcome::Success | crate::audit::AuditOutcome::Failure { .. },
        ) => audit.audit_id() == audit.command_id(),
        _ => false,
    };
    if audit.record_format_version() != crate::RECORD_FORMAT_VERSION
        || audit.transaction_id() != audit.command_id()
        || audit.durable_sequence() != u64::from(audit.new_revision()) + 1
        || !has_revision_anchor
        || !valid_semantics
    {
        return Err(StoreError::InvalidCommit);
    }
    if records
        .audits
        .iter()
        .any(|record| record.audit_id() == audit.audit_id() && record != audit)
    {
        return Err(StoreError::IdentityConflict);
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InMemoryDocumentStore {
    snapshots: Vec<SnapshotRecord>,
    transactions: Vec<TransactionRecord>,
    audits: Vec<AuditRecord>,
    assets: Vec<AssetRecord>,
    sources: Vec<MigrationSourceRecord>,
}

impl DocumentStore for InMemoryDocumentStore {
    fn commit_planned_atomic(
        &mut self,
        commit: PlannedPersistenceCommit,
    ) -> Result<CommitDisposition, StoreError> {
        validate_transaction_audit(&commit.transaction, &commit.audit)?;
        if commit.replace_existing != (commit.transaction.base_revision == 0) {
            return Err(StoreError::InvalidCommit);
        }

        let snapshot_state = commit.snapshot.as_ref().map(|snapshot| {
            record_state(
                &self.snapshots,
                |record| {
                    record.document_id == snapshot.document_id
                        && record.schema_version == snapshot.schema_version
                        && record.revision == snapshot.revision
                },
                snapshot,
            )
        });
        let transaction_state = record_state(
            &self.transactions,
            |record| record.transaction_id == commit.transaction.transaction_id,
            &commit.transaction,
        );
        let audit_state = record_state(
            &self.audits,
            |record| record.audit_id() == commit.audit.audit_id(),
            &commit.audit,
        );
        let asset_states = commit
            .assets
            .iter()
            .map(|asset| {
                record_state(
                    &self.assets,
                    |record| record.content_hash == asset.content_hash,
                    asset,
                )
            })
            .collect::<Vec<_>>();
        if snapshot_state == Some(RecordState::Conflict)
            || [transaction_state, audit_state].contains(&RecordState::Conflict)
            || asset_states.contains(&RecordState::Conflict)
        {
            return Err(StoreError::IdentityConflict);
        }
        if snapshot_state.is_none_or(|state| state == RecordState::Exact)
            && transaction_state == RecordState::Exact
            && audit_state == RecordState::Exact
            && asset_states
                .iter()
                .all(|state| *state == RecordState::Exact)
        {
            return Ok(CommitDisposition::Idempotent);
        }
        if transaction_state != audit_state
            || (transaction_state == RecordState::Missing
                && snapshot_state == Some(RecordState::Exact))
            || (transaction_state == RecordState::Exact
                && asset_states
                    .iter()
                    .any(|state| *state != RecordState::Exact))
        {
            return Err(StoreError::PartialRecordSet);
        }

        let (document, history) = if commit.transaction.base_revision == 0 {
            let snapshot = commit.snapshot.as_ref().ok_or(StoreError::InvalidCommit)?;
            let proof = PersistenceCommit {
                replace_existing: commit.replace_existing,
                snapshot: snapshot.clone(),
                transaction: commit.transaction.clone(),
                audit: commit.audit.clone(),
                assets: commit.assets.clone(),
            };
            let document = validate_commit_set(&proof)?;
            if self
                .transactions
                .iter()
                .any(|record| record.document_id == commit.transaction.document_id)
            {
                return Err(StoreError::IdentityConflict);
            }
            (document, snapshot.history.clone())
        } else if transaction_state == RecordState::Missing {
            let (before, mut history) = recover_store_state(self)?;
            if before.document_id != commit.transaction.document_id
                || before.schema_version != commit.transaction.schema_version
                || before.revision != commit.transaction.base_revision
                || canonical_hash(
                    &crate::canonical::canonical_bytes(&before)
                        .map_err(|_| StoreError::InvalidCommit)?,
                ) != commit.transaction.before_hash
            {
                return Err(StoreError::InvalidCommit);
            }
            let document = replay_forward(&before, &commit.transaction)
                .map_err(|_| StoreError::InvalidCommit)?;
            crate::replay_history_effect(&mut history, &commit.transaction, &before, &document)
                .map_err(|_| StoreError::InvalidCommit)?;
            validate_optional_snapshot(&commit.snapshot, &document, &history)?;
            (document, history)
        } else {
            // A transaction/audit pair that is already durable may be promoted
            // to a checkpoint (for example, by an explicit local save), but no
            // missing mutation/audit/asset record is silently repaired.
            let (document, history) = recover_store_state(self)?;
            if document.document_id != commit.transaction.document_id
                || document.schema_version != commit.transaction.schema_version
                || document.revision != commit.transaction.new_revision
                || canonical_hash(
                    &crate::canonical::canonical_bytes(&document)
                        .map_err(|_| StoreError::InvalidCommit)?,
                ) != commit.transaction.after_hash
            {
                return Err(StoreError::InvalidCommit);
            }
            validate_optional_snapshot(&commit.snapshot, &document, &history)?;
            (document, history)
        };

        let mut candidate = self.clone();
        if let Some(snapshot) = commit.snapshot
            && snapshot_state == Some(RecordState::Missing)
        {
            candidate.snapshots.push(snapshot);
        }
        if transaction_state == RecordState::Missing {
            candidate.transactions.push(commit.transaction);
            candidate.audits.push(commit.audit);
        }
        for asset in commit.assets {
            match record_state(
                &candidate.assets,
                |record| record.content_hash == asset.content_hash,
                &asset,
            ) {
                RecordState::Missing => candidate.assets.push(asset),
                RecordState::Exact => {}
                RecordState::Conflict => return Err(StoreError::IdentityConflict),
            }
        }
        validate_asset_records(&document, &candidate.assets)
            .map_err(|_| StoreError::InvalidCommit)?;
        let recovered = crate::recover_inner(candidate.load_records()?)
            .map_err(|_| StoreError::InvalidCommit)?;
        if recovered.session.document_id != document.document_id
            || recovered.session.revision != document.revision
            || recovered.session.canonical_hash
                != canonical_hash(
                    &crate::canonical::canonical_bytes(&document)
                        .map_err(|_| StoreError::InvalidCommit)?,
                )
            || recovered.session.history != history
        {
            return Err(StoreError::InvalidCommit);
        }
        *self = candidate;
        Ok(CommitDisposition::Committed)
    }

    fn load_records(&self) -> Result<RecoverRequest, StoreError> {
        Ok(RecoverRequest {
            snapshots: self.snapshots.clone(),
            transactions: self.transactions.clone(),
            audits: self.audits.clone(),
            assets: self.assets.clone(),
            sources: self.sources.clone(),
        })
    }

    fn commit_migration_atomic(
        &mut self,
        commit: MigrationPersistenceCommit,
    ) -> Result<CommitDisposition, StoreError> {
        let document = validate_migration_commit(&commit)?;
        let expected_hash = commit.snapshot.canonical_hash.clone();
        let snapshot_state = record_state(
            &self.snapshots,
            |record| {
                record.document_id == commit.snapshot.document_id
                    && record.schema_version == commit.snapshot.schema_version
                    && record.revision == commit.snapshot.revision
            },
            &commit.snapshot,
        );
        let audit_state = record_state(
            &self.audits,
            |record| record.audit_id() == commit.audit.audit_id(),
            &commit.audit,
        );
        let asset_states = commit
            .assets
            .iter()
            .map(|asset| {
                record_state(
                    &self.assets,
                    |record| record.content_hash == asset.content_hash,
                    asset,
                )
            })
            .collect::<Vec<_>>();
        let source_state = record_state(
            &self.sources,
            |record| {
                record.document_id == commit.source.document_id
                    && record.schema_version == commit.source.schema_version
                    && record.canonical_hash == commit.source.canonical_hash
            },
            &commit.source,
        );
        if snapshot_state == RecordState::Conflict
            || audit_state == RecordState::Conflict
            || source_state == RecordState::Conflict
            || asset_states.contains(&RecordState::Conflict)
        {
            return Err(StoreError::IdentityConflict);
        }
        if snapshot_state == RecordState::Exact
            && audit_state == RecordState::Exact
            && source_state == RecordState::Exact
            && asset_states
                .iter()
                .all(|state| *state == RecordState::Exact)
        {
            return Ok(CommitDisposition::Idempotent);
        }
        if snapshot_state != RecordState::Missing
            || audit_state != RecordState::Missing
            || source_state != RecordState::Missing
        {
            return Err(StoreError::PartialRecordSet);
        }

        let mut candidate = self.clone();
        candidate.snapshots.push(commit.snapshot);
        candidate.audits.push(commit.audit);
        candidate.sources.push(commit.source);
        for asset in commit.assets {
            match record_state(
                &candidate.assets,
                |record| record.content_hash == asset.content_hash,
                &asset,
            ) {
                RecordState::Missing => candidate.assets.push(asset),
                RecordState::Exact => {}
                RecordState::Conflict => return Err(StoreError::IdentityConflict),
            }
        }
        validate_asset_records(&document, &candidate.assets)
            .map_err(|_| StoreError::InvalidCommit)?;
        let recovered = crate::recover_inner(candidate.load_records()?)
            .map_err(|_| StoreError::InvalidCommit)?;
        if recovered.session.document_id != document.document_id
            || recovered.session.revision != document.revision
            || recovered.session.canonical_hash != expected_hash
        {
            return Err(StoreError::InvalidCommit);
        }
        *self = candidate;
        Ok(CommitDisposition::Committed)
    }

    fn commit_standalone_audit_atomic(
        &mut self,
        audit: AuditRecord,
    ) -> Result<CommitDisposition, StoreError> {
        validate_standalone_audit(&self.load_records()?, &audit)?;
        let mut candidate = self.clone();
        match record_state(
            &candidate.audits,
            |record| record.audit_id() == audit.audit_id(),
            &audit,
        ) {
            RecordState::Missing => candidate.audits.push(audit),
            RecordState::Exact => return Ok(CommitDisposition::Idempotent),
            RecordState::Conflict => return Err(StoreError::IdentityConflict),
        }
        crate::preflight_recovery_records(&candidate.load_records()?)
            .map_err(|_| StoreError::InvalidCommit)?;
        *self = candidate;
        Ok(CommitDisposition::Committed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordState {
    Missing,
    Exact,
    Conflict,
}

fn record_state<T: PartialEq>(
    records: &[T],
    identity: impl Fn(&T) -> bool,
    candidate: &T,
) -> RecordState {
    let mut found_exact = false;
    for record in records.iter().filter(|record| identity(record)) {
        if record != candidate {
            return RecordState::Conflict;
        }
        found_exact = true;
    }
    if found_exact {
        RecordState::Exact
    } else {
        RecordState::Missing
    }
}

fn recover_store_state(
    store: &InMemoryDocumentStore,
) -> Result<(FlowDocument, crate::transaction::HistoryState), StoreError> {
    let recovered =
        crate::recover_inner(store.load_records()?).map_err(|_| StoreError::PartialRecordSet)?;
    let document = decode_canonical(recovered.session.canonical_json.as_bytes())
        .map_err(|_| StoreError::PartialRecordSet)?;
    if document.document_id != recovered.session.document_id
        || document.revision != recovered.session.revision
        || canonical_hash(recovered.session.canonical_json.as_bytes())
            != recovered.session.canonical_hash
    {
        return Err(StoreError::PartialRecordSet);
    }
    Ok((document, recovered.session.history))
}

fn validate_optional_snapshot(
    snapshot: &Option<SnapshotRecord>,
    document: &FlowDocument,
    history: &crate::transaction::HistoryState,
) -> Result<(), StoreError> {
    let Some(snapshot) = snapshot else {
        return Ok(());
    };
    let (snapshot_document, snapshot_history, snapshot_hash) =
        crate::validate_snapshot(snapshot).map_err(|_| StoreError::InvalidCommit)?;
    let expected_hash = canonical_hash(
        &crate::canonical::canonical_bytes(document).map_err(|_| StoreError::InvalidCommit)?,
    );
    if snapshot_document != *document
        || snapshot_history != *history
        || snapshot_hash != expected_hash
    {
        return Err(StoreError::InvalidCommit);
    }
    Ok(())
}

fn validate_transaction_audit(
    transaction: &TransactionRecord,
    audit: &AuditRecord,
) -> Result<(), StoreError> {
    validate_private_preimages(transaction).map_err(|_| StoreError::InvalidCommit)?;
    audit.validate().map_err(|_| StoreError::InvalidCommit)?;
    if transaction.record_format_version != crate::RECORD_FORMAT_VERSION
        || audit.record_format_version() != crate::RECORD_FORMAT_VERSION
        || transaction.transaction_id != transaction.command_id
        || transaction.new_revision
            != transaction
                .base_revision
                .checked_add(1)
                .ok_or(StoreError::InvalidCommit)?
        || audit.document_id() != &transaction.document_id
        || audit.audit_id() != &transaction.command_id
        || audit.transaction_id() != &transaction.transaction_id
        || audit.command_id() != &transaction.command_id
        || audit.base_revision() != transaction.base_revision
        || audit.new_revision() != transaction.new_revision
        || audit.transaction_type() != transaction.command_type
        || audit.modality() != &transaction.modality
        || audit.timestamp().as_str() != transaction.issued_at
        || audit.durable_sequence() != u64::from(transaction.new_revision)
        || audit.metadata()
            != [crate::audit::AuditMetadata::SchemaVersion {
                value: transaction.schema_version,
            }]
        || !matches!(audit.outcome(), crate::audit::AuditOutcome::Success)
    {
        return Err(StoreError::InvalidCommit);
    }
    Ok(())
}

fn validate_commit_set(commit: &PersistenceCommit) -> Result<FlowDocument, StoreError> {
    let (document, validated_history, validated_hash) =
        crate::validate_snapshot(&commit.snapshot).map_err(|_| StoreError::InvalidCommit)?;
    let computed_hash = canonical_hash(commit.snapshot.canonical_json.as_bytes());
    validate_transaction_audit(&commit.transaction, &commit.audit)?;
    if commit.snapshot.record_format_version != crate::RECORD_FORMAT_VERSION
        || commit.snapshot.document_id != document.document_id
        || commit.snapshot.revision != document.revision
        || commit.snapshot.schema_version != document.schema_version
        || commit.snapshot.canonical_hash != computed_hash
        || commit.transaction.document_id != document.document_id
        || commit.transaction.schema_version != document.schema_version
        || commit.transaction.new_revision != document.revision
        || commit.transaction.after_hash != computed_hash
        || commit.replace_existing != (commit.transaction.base_revision == 0)
        || validated_history != commit.snapshot.history
        || validated_hash != computed_hash
    {
        return Err(StoreError::InvalidCommit);
    }
    EditorState::with_history(document.clone(), commit.snapshot.history.clone())
        .map_err(|_| StoreError::InvalidCommit)?;
    validate_asset_records(&document, &commit.assets).map_err(|_| StoreError::InvalidCommit)?;
    if commit.transaction.base_revision == 0
        && (commit.transaction.before_hash != "none"
            || commit.transaction.command_type != "createSample"
            || commit.transaction.modality != SourceModality::System
            || commit.transaction.forward_operations != [Operation::CreateDocument]
            || commit.transaction.inverse_operations != [Operation::DeleteDocument]
            || commit.transaction.anchor_mapping != crate::anchor::AnchorMapping::identity()
            || commit.transaction.history_effect != HistoryEffect::Create
            || !commit.snapshot.history.entries.is_empty()
            || commit.snapshot.history.cursor != 0
            || commit.snapshot.history.seen_command_ids != [commit.transaction.command_id.clone()])
    {
        return Err(StoreError::InvalidCommit);
    }
    Ok(document)
}

fn validate_migration_commit(
    commit: &MigrationPersistenceCommit,
) -> Result<FlowDocument, StoreError> {
    let (document, history, hash) =
        crate::validate_snapshot(&commit.snapshot).map_err(|_| StoreError::InvalidCommit)?;
    let boundary = commit
        .snapshot
        .migration_boundary
        .as_ref()
        .ok_or(StoreError::InvalidCommit)?;
    commit
        .audit
        .validate()
        .map_err(|_| StoreError::InvalidCommit)?;
    if !history.entries.is_empty()
        || history.cursor != 0
        || !history.seen_command_ids.is_empty()
        || commit.audit.record_format_version() != crate::RECORD_FORMAT_VERSION
        || commit.audit.audit_id() != &boundary.migration_id
        || commit.audit.transaction_id() != &boundary.migration_id
        || commit.audit.command_id() != &boundary.migration_id
        || commit.audit.base_revision() != document.revision
        || commit.audit.new_revision() != document.revision
        || commit.audit.durable_sequence() != u64::from(document.revision)
        || commit.audit.transaction_type() != "migration"
        || commit.audit.modality() != &SourceModality::System
        || commit.audit.timestamp().as_str() != boundary.issued_at
        || commit.audit.metadata()
            != [crate::audit::AuditMetadata::Migration {
                from_schema_version: boundary.source_schema_version,
                to_schema_version: boundary.current_schema_version,
            }]
        || !matches!(commit.audit.outcome(), crate::audit::AuditOutcome::Success)
        || hash != commit.snapshot.canonical_hash
    {
        return Err(StoreError::InvalidCommit);
    }
    let migration_output = crate::validate_migration_source(
        &document,
        commit.snapshot.migration_boundary.as_ref(),
        std::slice::from_ref(&commit.source),
    )
    .map_err(|_| StoreError::InvalidCommit)?;
    if migration_output.as_ref() != Some(&document) {
        return Err(StoreError::InvalidCommit);
    }
    validate_asset_records(&document, &commit.assets).map_err(|_| StoreError::InvalidCommit)?;
    Ok(document)
}

pub fn validate_asset_records(
    document: &FlowDocument,
    records: &[AssetRecord],
) -> Result<(), SchemaError> {
    let mut unique = std::collections::BTreeMap::<&str, &AssetRecord>::new();
    for record in records {
        if record.record_format_version != crate::RECORD_FORMAT_VERSION
            || asset_hash(&record.bytes) != record.content_hash
        {
            return Err(SchemaError::asset_hash_mismatch());
        }
        match unique.get(record.content_hash.as_str()) {
            Some(existing) if *existing == record => {}
            Some(_) => return Err(SchemaError::asset_hash_mismatch()),
            None => {
                unique.insert(record.content_hash.as_str(), record);
            }
        }
    }
    for descriptor in &document.assets {
        let record = unique
            .get(descriptor.content_hash.as_str())
            .ok_or_else(SchemaError::asset_hash_mismatch)?;
        verify_asset_bytes(descriptor, &record.bytes)?;
    }
    // Reachability is computed from the candidate semantic document, never
    // from a browser request. Physical records are deliberately retained when
    // their current reference count reaches zero: undo/recovery history may
    // still need them, and this validator never authorizes unsafe garbage
    // collection as a side effect of an image mutation.
    for content_hash in asset::reachable_asset_hashes(document) {
        if !unique.contains_key(content_hash.as_str()) {
            return Err(SchemaError::asset_hash_mismatch());
        }
    }
    Ok(())
}

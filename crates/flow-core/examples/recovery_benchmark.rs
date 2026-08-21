//! Deterministic Phase 1 recovery benchmark.
//!
//! This is deliberately a native-core measurement: it constructs the same
//! immutable command and record DTOs that browser persistence transports, then
//! measures Rust-owned recovery. The Node validator supplies host/browser
//! metadata and enforces the terminal artifact contract.

use std::{env, fs, time::Instant};

use flow_core::{
    ApiResponse, ApplyCommandRequest, CommandDto, CommandKind, CreateSampleRequest, HistoryState,
    RecoverRequest, SourceModality, apply_command, create_sample,
    model::CommandId,
    recover,
    store::{CommitPlanner, PlannedPersistenceCommit, SnapshotPolicy, SnapshotReason},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Recipe {
    format_version: u32,
    name: String,
    pages: u32,
    transactions: u32,
    bytes_per_transaction: usize,
    warmups: u32,
    measurements: u32,
    p95_target_milliseconds: f64,
    candidates: Vec<Policy>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
    transaction_interval: u32,
    byte_interval: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Measurement {
    fixture_hash: String,
    fixture_name: String,
    pages: u32,
    transactions: u32,
    warmups: u32,
    measurements: u32,
    p95_target_milliseconds: f64,
    policy: Policy,
    durations_milliseconds: Vec<f64>,
    p50_milliseconds: f64,
    p95_milliseconds: f64,
}

struct BenchmarkWorkload {
    request: RecoverRequest,
    revision: u32,
    canonical_hash: String,
    history: HistoryState,
}

fn main() -> Result<(), String> {
    let (fixture_path, policy) = parse_arguments()?;
    let fixture_bytes = fs::read(&fixture_path)
        .map_err(|error| format!("could not read benchmark recipe {fixture_path}: {error}"))?;
    let recipe: Recipe = serde_json::from_slice(&fixture_bytes)
        .map_err(|error| format!("could not parse benchmark recipe: {error}"))?;
    if recipe.format_version != 1 || recipe.pages != 200 || recipe.transactions != 1_000 {
        return Err(
            "benchmark recipe does not describe the locked 200-page/1,000-transaction workload"
                .to_owned(),
        );
    }
    if !recipe.candidates.iter().any(|candidate| {
        candidate.transaction_interval == policy.transaction_interval
            && candidate.byte_interval == policy.byte_interval
    }) {
        return Err("requested policy is not an approved benchmark candidate".to_owned());
    }

    let workload = build_workload(&recipe, &policy)?;
    for _ in 0..recipe.warmups {
        assert_recovery_truth(&workload)?;
    }

    let mut durations_milliseconds = Vec::with_capacity(recipe.measurements as usize);
    for _ in 0..recipe.measurements {
        let started = Instant::now();
        assert_recovery_truth(&workload)?;
        durations_milliseconds.push(started.elapsed().as_secs_f64() * 1_000.0);
    }

    let mut ordered = durations_milliseconds.clone();
    ordered.sort_by(f64::total_cmp);
    let p50_milliseconds = percentile(&ordered, 0.50);
    let p95_milliseconds = percentile(&ordered, 0.95);
    let measurement = Measurement {
        fixture_hash: format!("blake3:{}", blake3::hash(&fixture_bytes).to_hex()),
        fixture_name: recipe.name,
        pages: recipe.pages,
        transactions: recipe.transactions,
        warmups: recipe.warmups,
        measurements: recipe.measurements,
        p95_target_milliseconds: recipe.p95_target_milliseconds,
        policy,
        durations_milliseconds,
        p50_milliseconds,
        p95_milliseconds,
    };
    println!(
        "{}",
        serde_json::to_string(&measurement)
            .map_err(|error| format!("could not serialize benchmark result: {error}"))?
    );
    Ok(())
}

fn parse_arguments() -> Result<(String, Policy), String> {
    let mut args = env::args().skip(1);
    let fixture_path = args.next().ok_or_else(|| {
        "usage: recovery_benchmark <recipe-path> <transaction-interval> <byte-interval>".to_owned()
    })?;
    let transaction_interval = args
        .next()
        .ok_or_else(|| "missing transaction interval".to_owned())?
        .parse::<u32>()
        .map_err(|_| "transaction interval must be an unsigned integer".to_owned())?;
    let byte_interval = args
        .next()
        .ok_or_else(|| "missing byte interval".to_owned())?
        .parse::<u64>()
        .map_err(|_| "byte interval must be an unsigned integer".to_owned())?;
    if args.next().is_some() || transaction_interval == 0 || byte_interval == 0 {
        return Err("unexpected arguments or zero snapshot interval".to_owned());
    }
    Ok((
        fixture_path,
        Policy {
            transaction_interval,
            byte_interval,
        },
    ))
}

fn build_workload(recipe: &Recipe, policy: &Policy) -> Result<BenchmarkWorkload, String> {
    let snapshot_policy = SnapshotPolicy::Periodic {
        transaction_interval: policy.transaction_interval,
        byte_interval: policy.byte_interval,
    };
    let planner = CommitPlanner::new(snapshot_policy);
    let mut records = RecoverRequest {
        snapshots: Vec::new(),
        transactions: Vec::new(),
        audits: Vec::new(),
        assets: Vec::new(),
        sources: Vec::new(),
    };
    let created = successful(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
    }))?;
    let planned = planner
        .plan(&records, created.commit, SnapshotReason::Creation)
        .map_err(|error| format!("could not plan benchmark creation: {error}"))?;
    append_planned(&mut records, planned)?;
    let mut canonical_json = created.session.canonical_json;
    let mut history = created.session.history;
    let mut revision = created.session.revision;
    let mut target = created.session.next_command_target;
    let mut expected_hash = created.session.canonical_hash;
    let payload = format!(
        " {}",
        "x".repeat(recipe.bytes_per_transaction.saturating_sub(1))
    );
    for sequence in 1..=recipe.transactions {
        let result = successful(apply_command(ApplyCommandRequest {
            canonical_json,
            history,
            command: CommandDto {
                command_id: command_id(sequence)?,
                base_revision: revision,
                modality: SourceModality::System,
                // Equal timestamps are intentional: recovery/audit ordering must
                // remain stable by revision and command identity, not wall clock.
                issued_at: "2026-08-14T00:00:00Z".to_owned(),
                kind: CommandKind::InsertText {
                    target: target.clone(),
                    text: payload.clone(),
                },
            },
        }))?;
        let planned = planner
            .plan(
                &records,
                result.commit,
                SnapshotReason::CommittedTransaction,
            )
            .map_err(|error| format!("could not plan benchmark transaction: {error}"))?;
        append_planned(&mut records, planned)?;
        canonical_json = result.session.canonical_json;
        history = result.session.history;
        revision = result.session.revision;
        target = result.session.next_command_target;
        expected_hash = result.session.canonical_hash;
    }

    let expected_snapshots = expected_snapshot_count(&records, snapshot_policy)?;
    if records.snapshots.len() != expected_snapshots {
        return Err(format!(
            "CommitPlanner checkpoint count diverged: expected {expected_snapshots}, got {}",
            records.snapshots.len()
        ));
    }
    let workload = BenchmarkWorkload {
        request: records,
        revision,
        canonical_hash: expected_hash,
        history,
    };
    assert_recovery_truth(&workload)?;
    Ok(workload)
}

fn append_planned(
    records: &mut RecoverRequest,
    planned: PlannedPersistenceCommit,
) -> Result<(), String> {
    if let Some(snapshot) = planned.snapshot {
        records.snapshots.push(snapshot);
    }
    records.transactions.push(planned.transaction);
    records.audits.push(planned.audit);
    for asset in planned.assets {
        match records
            .assets
            .iter()
            .find(|existing| existing.content_hash == asset.content_hash)
        {
            Some(existing) if existing == &asset => {}
            Some(_) => return Err("benchmark asset identity has divergent bytes".to_owned()),
            None => records.assets.push(asset),
        }
    }
    Ok(())
}

fn expected_snapshot_count(
    request: &RecoverRequest,
    policy: SnapshotPolicy,
) -> Result<usize, String> {
    let mut count = 1_usize;
    let mut uncheckpointed_transactions = 0_u32;
    let mut uncheckpointed_bytes = 0_u64;
    for transaction in request.transactions.iter().skip(1) {
        uncheckpointed_transactions = uncheckpointed_transactions
            .checked_add(1)
            .ok_or_else(|| "benchmark transaction cadence overflowed".to_owned())?;
        let transaction_bytes = u64::try_from(
            serde_json::to_vec(transaction)
                .map_err(|error| format!("could not measure transaction bytes: {error}"))?
                .len(),
        )
        .map_err(|_| "benchmark transaction size overflowed".to_owned())?;
        uncheckpointed_bytes = uncheckpointed_bytes
            .checked_add(transaction_bytes)
            .ok_or_else(|| "benchmark byte cadence overflowed".to_owned())?;
        if policy.should_snapshot(
            SnapshotReason::CommittedTransaction,
            uncheckpointed_transactions,
            uncheckpointed_bytes,
        ) {
            count += 1;
            uncheckpointed_transactions = 0;
            uncheckpointed_bytes = 0;
        }
    }
    Ok(count)
}

fn assert_recovery_truth(workload: &BenchmarkWorkload) -> Result<(), String> {
    let recovered = successful(recover(workload.request.clone()))?;
    if recovered.session.revision != workload.revision
        || recovered.session.canonical_hash != workload.canonical_hash
        || recovered.session.history != workload.history
    {
        return Err("recovery did not reproduce the durable benchmark truth".to_owned());
    }
    Ok(())
}

fn command_id(sequence: u32) -> Result<CommandId, String> {
    let benchmark_identity = 900_000_000_000_u64 + u64::from(sequence);
    CommandId::new(format!("00000000-0000-4000-8000-{benchmark_identity:012}"))
        .map_err(|error| format!("could not construct command ID: {error}"))
}

fn successful<T>(response: ApiResponse<T>) -> Result<T, String> {
    if response.ok {
        response
            .value
            .ok_or_else(|| "successful DTO response lacked a value".to_owned())
    } else {
        Err(format!("core operation failed: {:?}", response.error))
    }
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    let index = ((sorted.len() as f64 * percentile).ceil() as usize).saturating_sub(1);
    sorted[index]
}

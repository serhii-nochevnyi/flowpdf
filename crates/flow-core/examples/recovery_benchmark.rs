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
    model::{CommandId, ContentNode, ContentNodeKind, FlowDocument, NodeId},
    recover,
    store::{CommitPlanner, PlannedPersistenceCommit, SnapshotPolicy, SnapshotReason},
    transaction::Mutation,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Recipe {
    format_version: u32,
    name: String,
    pages: u32,
    transactions: u32,
    background_mutation_utf8_bytes: usize,
    page_equivalent: PageEquivalentRecipe,
    warmups: u32,
    measurements: u32,
    p95_target_milliseconds: f64,
    candidates: Vec<Policy>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PageEquivalentRecipe {
    paragraphs_per_page: u32,
    words_per_paragraph: u32,
    minimum_utf8_bytes_per_page: u64,
    vocabulary: Vec<String>,
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
    workload_proof: WorkloadProof,
    durations_milliseconds: Vec<f64>,
    p50_milliseconds: f64,
    p95_milliseconds: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WorkloadProof {
    page_equivalent_count: u32,
    paragraphs_per_page: u32,
    words_per_paragraph: u32,
    generated_paragraph_count: u32,
    generated_word_count: u64,
    generated_utf8_bytes: u64,
    minimum_utf8_bytes_per_page: u64,
    initial_content_node_count: u32,
    final_content_node_count: u32,
    semantic_payload_hash: String,
    final_revision: u32,
    history_entry_count: u32,
    history_cursor: u32,
    planned_transaction_count: u32,
}

struct BenchmarkWorkload {
    request: RecoverRequest,
    revision: u32,
    canonical_hash: String,
    history: HistoryState,
    recipe: Recipe,
    proof: WorkloadProof,
}

fn main() -> Result<(), String> {
    let (fixture_path, policy) = parse_arguments()?;
    let fixture_bytes = fs::read(&fixture_path)
        .map_err(|error| format!("could not read benchmark recipe {fixture_path}: {error}"))?;
    let recipe: Recipe = serde_json::from_slice(&fixture_bytes)
        .map_err(|error| format!("could not parse benchmark recipe: {error}"))?;
    validate_recipe(&recipe)?;
    if recipe.format_version != 2 || recipe.pages != 200 || recipe.transactions != 1_000 {
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
    let p50_milliseconds = percentile(&ordered, 0.50)?;
    let p95_milliseconds = percentile(&ordered, 0.95)?;
    let measurement = Measurement {
        fixture_hash: format!("blake3:{}", blake3::hash(&fixture_bytes).to_hex()),
        fixture_name: recipe.name,
        pages: recipe.pages,
        transactions: recipe.transactions,
        warmups: recipe.warmups,
        measurements: recipe.measurements,
        p95_target_milliseconds: recipe.p95_target_milliseconds,
        policy,
        workload_proof: workload.proof,
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

fn validate_recipe(recipe: &Recipe) -> Result<(), String> {
    let expected_paragraphs = recipe
        .pages
        .checked_mul(recipe.page_equivalent.paragraphs_per_page)
        .ok_or_else(|| "page-equivalent paragraph count overflowed".to_owned())?;
    if expected_paragraphs != recipe.transactions
        || recipe.page_equivalent.words_per_paragraph < 16
        || recipe.background_mutation_utf8_bytes < 16
        || recipe.page_equivalent.minimum_utf8_bytes_per_page == 0
        || recipe.warmups == 0
        || recipe.measurements == 0
        || !recipe.p95_target_milliseconds.is_finite()
        || recipe.p95_target_milliseconds <= 0.0
        || recipe.candidates.is_empty()
        || recipe
            .candidates
            .iter()
            .any(|candidate| candidate.transaction_interval == 0 || candidate.byte_interval == 0)
        || recipe.page_equivalent.vocabulary.len() < 8
        || recipe
            .page_equivalent
            .vocabulary
            .iter()
            .any(|word| word.is_empty() || word.chars().any(char::is_whitespace) || word.len() > 64)
    {
        return Err("benchmark page-equivalent recipe is not structurally meaningful".to_owned());
    }
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
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
    }))?;
    let planned = planner
        .plan(&records, created.commit, SnapshotReason::Creation)
        .map_err(|error| format!("could not plan benchmark creation: {error}"))?;
    append_planned(&mut records, planned)?;
    let initial_document = decode_document(&created.session.canonical_json)?;
    let initial_content_node_count = u32::try_from(initial_document.content.len())
        .map_err(|_| "initial content-node count overflowed".to_owned())?;
    let body_style_id = initial_document
        .content
        .iter()
        .find(|node| node.kind == ContentNodeKind::Paragraph)
        .and_then(|node| node.style_id.clone())
        .ok_or_else(|| "sample document has no styled paragraph".to_owned())?;
    let mut canonical_json = created.session.canonical_json;
    let mut history = created.session.history;
    let mut revision = created.session.revision;
    let mut target = created
        .session
        .next_command_target
        .ok_or_else(|| "sample document has no command target".to_owned())?;
    let mut expected_hash = created.session.canonical_hash;
    for sequence in 1..=recipe.transactions {
        let kind = if sequence == recipe.transactions {
            CommandKind::Batch {
                mutations: page_equivalent_mutations(
                    recipe,
                    initial_content_node_count,
                    &body_style_id,
                )?,
            }
        } else {
            CommandKind::InsertText {
                target: target.clone(),
                text: background_mutation_text(sequence, recipe.background_mutation_utf8_bytes)?,
            }
        };
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
                kind,
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
        target = result
            .session
            .next_command_target
            .ok_or_else(|| "benchmark document lost its command target".to_owned())?;
        expected_hash = result.session.canonical_hash;
    }

    let expected_snapshots = expected_snapshot_count(&records, snapshot_policy)?;
    if records.snapshots.len() != expected_snapshots {
        return Err(format!(
            "CommitPlanner checkpoint count diverged: expected {expected_snapshots}, got {}",
            records.snapshots.len()
        ));
    }
    let final_document = decode_document(&canonical_json)?;
    let planned_transaction_count = u32::try_from(
        records
            .transactions
            .iter()
            .filter(|transaction| transaction.base_revision > 0)
            .count(),
    )
    .map_err(|_| "planned transaction count overflowed".to_owned())?;
    let proof = derive_workload_proof(
        &final_document,
        &history,
        recipe,
        initial_content_node_count,
        planned_transaction_count,
    )?;
    let workload = BenchmarkWorkload {
        request: records,
        revision,
        canonical_hash: expected_hash,
        history,
        recipe: recipe.clone(),
        proof,
    };
    assert_recovery_truth(&workload)?;
    Ok(workload)
}

fn page_equivalent_mutations(
    recipe: &Recipe,
    initial_content_node_count: u32,
    body_style_id: &flow_core::model::StyleId,
) -> Result<Vec<Mutation>, String> {
    let paragraph_count = recipe
        .pages
        .checked_mul(recipe.page_equivalent.paragraphs_per_page)
        .ok_or_else(|| "page-equivalent paragraph count overflowed".to_owned())?;
    (1..=paragraph_count)
        .map(|sequence| {
            let index = initial_content_node_count
                .checked_add(sequence - 1)
                .ok_or_else(|| "benchmark insertion index overflowed".to_owned())?;
            Ok(Mutation::InsertNode {
                index,
                node: ContentNode {
                    id: node_id(sequence)?,
                    kind: ContentNodeKind::Paragraph,
                    style_id: Some(body_style_id.clone()),
                    text: generated_paragraph(recipe, sequence)?,
                    asset_id: None,
                },
            })
        })
        .collect()
}

fn background_mutation_text(sequence: u32, utf8_bytes: usize) -> Result<String, String> {
    let prefix = format!(" edit-{sequence:04}-");
    let filler_length = utf8_bytes
        .checked_sub(prefix.len())
        .ok_or_else(|| "background mutation byte budget is too small".to_owned())?;
    Ok(format!("{prefix}{}", "r".repeat(filler_length)))
}

fn generated_paragraph(recipe: &Recipe, sequence: u32) -> Result<String, String> {
    let paragraphs_per_page = recipe.page_equivalent.paragraphs_per_page;
    let page = (sequence - 1) / paragraphs_per_page + 1;
    let paragraph_on_page = (sequence - 1) % paragraphs_per_page + 1;
    let word_count = usize::try_from(recipe.page_equivalent.words_per_paragraph)
        .map_err(|_| "paragraph word count overflowed".to_owned())?;
    let mut words = Vec::with_capacity(word_count);
    words.push(format!("page-{page:03}"));
    words.push(format!("paragraph-{paragraph_on_page:02}"));
    for word_index in 2..word_count {
        let vocabulary_index = (usize::try_from(sequence)
            .map_err(|_| "paragraph sequence overflowed".to_owned())?
            + word_index)
            % recipe.page_equivalent.vocabulary.len();
        let mut word = recipe.page_equivalent.vocabulary[vocabulary_index].clone();
        if word_index + 1 == word_count {
            word.push('.');
        }
        words.push(word);
    }
    Ok(words.join(" "))
}

fn derive_workload_proof(
    document: &FlowDocument,
    history: &HistoryState,
    recipe: &Recipe,
    initial_content_node_count: u32,
    planned_transaction_count: u32,
) -> Result<WorkloadProof, String> {
    let generated_paragraph_count = recipe
        .pages
        .checked_mul(recipe.page_equivalent.paragraphs_per_page)
        .ok_or_else(|| "generated paragraph count overflowed".to_owned())?;
    let generated_word_count = u64::from(generated_paragraph_count)
        .checked_mul(u64::from(recipe.page_equivalent.words_per_paragraph))
        .ok_or_else(|| "generated word count overflowed".to_owned())?;
    let final_content_node_count = u32::try_from(document.content.len())
        .map_err(|_| "final content-node count overflowed".to_owned())?;
    if final_content_node_count
        != initial_content_node_count
            .checked_add(generated_paragraph_count)
            .ok_or_else(|| "final content-node count overflowed".to_owned())?
    {
        return Err("final FlowDocument does not contain the page-equivalent nodes".to_owned());
    }

    let mut semantic_hasher = blake3::Hasher::new();
    let mut generated_utf8_bytes = 0_u64;
    let mut page_utf8_bytes = 0_u64;
    for sequence in 1..=generated_paragraph_count {
        let index = usize::try_from(initial_content_node_count + sequence - 1)
            .map_err(|_| "generated node index overflowed".to_owned())?;
        let node = document
            .content
            .get(index)
            .ok_or_else(|| "generated page-equivalent node is missing".to_owned())?;
        let expected_text = generated_paragraph(recipe, sequence)?;
        if node.id != node_id(sequence)?
            || node.kind != ContentNodeKind::Paragraph
            || node.style_id.is_none()
            || node.asset_id.is_some()
            || node.text != expected_text
            || node.text.split_whitespace().count()
                != usize::try_from(recipe.page_equivalent.words_per_paragraph)
                    .map_err(|_| "paragraph word count overflowed".to_owned())?
        {
            return Err("generated semantic paragraph diverged from the locked recipe".to_owned());
        }
        let node_bytes = u64::try_from(node.text.len())
            .map_err(|_| "generated paragraph byte count overflowed".to_owned())?;
        generated_utf8_bytes = generated_utf8_bytes
            .checked_add(node_bytes)
            .ok_or_else(|| "generated payload byte count overflowed".to_owned())?;
        page_utf8_bytes = page_utf8_bytes
            .checked_add(node_bytes)
            .ok_or_else(|| "page payload byte count overflowed".to_owned())?;
        semantic_hasher.update(node.id.as_str().as_bytes());
        semantic_hasher.update(&[0]);
        semantic_hasher.update(node.text.as_bytes());
        semantic_hasher.update(&[0xff]);
        if sequence % recipe.page_equivalent.paragraphs_per_page == 0 {
            if page_utf8_bytes < recipe.page_equivalent.minimum_utf8_bytes_per_page {
                return Err(format!(
                    "page-equivalent payload is too small: {page_utf8_bytes} bytes"
                ));
            }
            page_utf8_bytes = 0;
        }
    }

    let history_entry_count = u32::try_from(history.entries.len())
        .map_err(|_| "history entry count overflowed".to_owned())?;
    if history_entry_count != recipe.transactions
        || history.cursor != history_entry_count
        || planned_transaction_count != recipe.transactions
    {
        return Err("benchmark history does not cover all real transactions".to_owned());
    }
    Ok(WorkloadProof {
        page_equivalent_count: recipe.pages,
        paragraphs_per_page: recipe.page_equivalent.paragraphs_per_page,
        words_per_paragraph: recipe.page_equivalent.words_per_paragraph,
        generated_paragraph_count,
        generated_word_count,
        generated_utf8_bytes,
        minimum_utf8_bytes_per_page: recipe.page_equivalent.minimum_utf8_bytes_per_page,
        initial_content_node_count,
        final_content_node_count,
        semantic_payload_hash: format!("blake3:{}", semantic_hasher.finalize().to_hex()),
        final_revision: document.revision,
        history_entry_count,
        history_cursor: history.cursor,
        planned_transaction_count,
    })
}

fn decode_document(canonical_json: &str) -> Result<FlowDocument, String> {
    serde_json::from_str(canonical_json)
        .map_err(|error| format!("could not decode benchmark FlowDocument: {error}"))
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
    let recovered_document = decode_document(&recovered.session.canonical_json)?;
    let planned_transaction_count = u32::try_from(
        workload
            .request
            .transactions
            .iter()
            .filter(|transaction| transaction.base_revision > 0)
            .count(),
    )
    .map_err(|_| "planned transaction count overflowed".to_owned())?;
    let recovered_proof = derive_workload_proof(
        &recovered_document,
        &recovered.session.history,
        &workload.recipe,
        workload.proof.initial_content_node_count,
        planned_transaction_count,
    )?;
    if recovered_proof != workload.proof {
        return Err("recovery changed the 200-page-equivalent semantic payload".to_owned());
    }
    Ok(())
}

fn command_id(sequence: u32) -> Result<CommandId, String> {
    let benchmark_identity = 900_000_000_000_u64 + u64::from(sequence);
    CommandId::new(format!("00000000-0000-4000-8000-{benchmark_identity:012}"))
        .map_err(|error| format!("could not construct command ID: {error}"))
}

fn node_id(sequence: u32) -> Result<NodeId, String> {
    let benchmark_identity = 700_000_000_000_u64 + u64::from(sequence);
    NodeId::new(format!("00000000-0000-4000-8000-{benchmark_identity:012}"))
        .map_err(|error| format!("could not construct benchmark node ID: {error}"))
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

fn percentile(sorted: &[f64], percentile: f64) -> Result<f64, String> {
    if sorted.is_empty() || !percentile.is_finite() || !(0.0..=1.0).contains(&percentile) {
        return Err(
            "benchmark percentile requires a non-empty sample and a finite rank".to_owned(),
        );
    }
    let index = ((sorted.len() as f64 * percentile).ceil() as usize).saturating_sub(1);
    sorted
        .get(index)
        .copied()
        .ok_or_else(|| "benchmark percentile rank exceeded its sample".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Recipe, percentile, validate_recipe};

    #[test]
    fn zero_measurements_fail_without_indexing_an_empty_sample() {
        assert!(percentile(&[], 0.95).is_err());
        let mut recipe: Recipe = serde_json::from_str(include_str!(
            "../../../fixtures/recovery/benchmark-200-page.recipe.json"
        ))
        .expect("benchmark recipe");
        recipe.measurements = 0;
        assert_eq!(
            validate_recipe(&recipe).expect_err("zero measurements"),
            "benchmark page-equivalent recipe is not structurally meaningful"
        );
    }
}

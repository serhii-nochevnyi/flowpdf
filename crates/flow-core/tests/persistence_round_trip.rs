use flow_core::{
    canonical::{
        IdentityResolution, canonical_bytes, canonical_hash, decode_canonical,
        preflight_canonical_bytes, reconcile_canonical_identity, verify_asset_bytes,
    },
    model::{DocumentId, FlowDocument},
    schema::{DocumentLimits, LimitKind, MigrationRegistry},
};

const CURRENT_JSON: &[u8] = include_bytes!("../../../fixtures/flowdoc/schema-v2-current.json");
const CURRENT_HASH: &str = include_str!("../../../fixtures/flowdoc/schema-v2-current.hash");

fn current_payload() -> &'static [u8] {
    CURRENT_JSON.strip_suffix(b"\n").unwrap_or(CURRENT_JSON)
}

#[test]
fn checked_in_v2_fixture_migrates_to_an_exact_v3_round_trip_contract() {
    let source = current_payload();
    assert_eq!(canonical_hash(source), CURRENT_HASH.trim());
    let outcome = MigrationRegistry::current()
        .migrate(source)
        .expect("v2 migration");
    let expected_hash = outcome.canonical_hash.clone();
    let document = outcome.document;
    let bytes = outcome.canonical_bytes;
    assert_eq!(document.schema_version, 3);
    assert_eq!(document.sections.len(), 1);
    assert_eq!(bytes, canonical_bytes(&document).expect("canonical bytes"));
    assert_eq!(expected_hash, canonical_hash(&bytes));
    assert_eq!(decode_canonical(&bytes).expect("golden decode"), document);
    assert_eq!(canonical_bytes(&document).expect("repeat 1"), bytes);
    assert_eq!(canonical_bytes(&document).expect("repeat 2"), bytes);
}

#[test]
fn semantic_vector_order_is_preserved_and_changes_identity_when_reordered() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let original = canonical_bytes(&document).expect("original");
    let mut reordered = document.clone();
    reordered.content.swap(0, 1);
    let reordered_bytes = canonical_bytes(&reordered).expect("reordered");

    assert_ne!(original, reordered_bytes);
    assert_ne!(canonical_hash(&original), canonical_hash(&reordered_bytes));
    assert_eq!(
        decode_canonical(&reordered_bytes).expect("decode").content,
        reordered.content
    );
}

#[test]
fn asset_bytes_resolve_separately_and_corruption_cannot_mutate_the_document() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let before = canonical_bytes(&document).expect("before");
    let descriptor = &document.assets[0];
    verify_asset_bytes(descriptor, &[]).expect("empty fixture bytes match descriptor");
    let error = verify_asset_bytes(descriptor, b"substituted bytes").expect_err("hash mismatch");
    assert_eq!(error.code(), "FLOW_ASSET_HASH_MISMATCH");
    assert_eq!(canonical_bytes(&document).expect("unchanged"), before);
}

#[test]
fn identical_creation_converges_and_divergent_same_identity_conflicts_atomically() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let existing = canonical_bytes(&document).expect("existing");
    assert_eq!(
        reconcile_canonical_identity(&existing, &existing).expect("idempotent"),
        IdentityResolution::Idempotent
    );

    let mut divergent = document.clone();
    divergent.locale = "en-US".to_owned();
    let divergent_bytes = canonical_bytes(&divergent).expect("divergent");
    let existing_before = existing.clone();
    let divergent_before = divergent_bytes.clone();
    assert_eq!(
        reconcile_canonical_identity(&existing, &divergent_bytes)
            .expect_err("same identity with different bytes")
            .code(),
        "FLOW_IDENTITY_CONFLICT"
    );
    assert_eq!(existing, existing_before);
    assert_eq!(divergent_bytes, divergent_before);

    let mut independent = divergent;
    independent.document_id = DocumentId::new("00000000-0000-4000-8000-000000009990").expect("id");
    assert_eq!(
        reconcile_canonical_identity(
            &existing,
            &canonical_bytes(&independent).expect("independent")
        )
        .expect("independent identity"),
        IdentityResolution::Independent
    );
}

#[test]
fn every_versioned_resource_ceiling_accepts_n_and_rejects_n_plus_one() {
    let limits = DocumentLimits::V1;
    let cases = [
        (LimitKind::CanonicalBytes, limits.canonical_bytes),
        (LimitKind::TreeDepth, limits.tree_depth),
        (LimitKind::SemanticNodes, limits.semantic_nodes),
        (LimitKind::TotalTextBytes, limits.total_text_bytes),
        (LimitKind::TextNodeBytes, limits.text_node_bytes),
        (LimitKind::Styles, limits.styles),
        (LimitKind::Assets, limits.assets),
        (LimitKind::Fields, limits.fields),
        (
            LimitKind::TransactionOperations,
            limits.transaction_operations,
        ),
        (LimitKind::TransactionBytes, limits.transaction_bytes),
        (LimitKind::RecoveryRecords, limits.recovery_records),
        (LimitKind::RecoveryBytes, limits.recovery_bytes),
    ];

    for (kind, maximum) in cases {
        limits.check(kind, maximum).expect("N must pass");
        let error = limits.check(kind, maximum + 1).expect_err("N+1 must fail");
        assert_eq!(error.code(), kind.code());
    }
}

#[test]
fn locked_v1_limits_match_the_reviewed_profile_exactly() {
    assert_eq!(
        DocumentLimits::V1,
        DocumentLimits {
            canonical_bytes: 64 * 1024 * 1024,
            tree_depth: 128,
            semantic_nodes: 200_000,
            total_text_bytes: 32 * 1024 * 1024,
            text_node_bytes: 4 * 1024 * 1024,
            styles: 4_096,
            assets: 10_000,
            fields: 10_000,
            transaction_operations: 10_000,
            transaction_bytes: 8 * 1024 * 1024,
            recovery_records: 10_000,
            recovery_bytes: 64 * 1024 * 1024,
        }
    );
}

#[test]
fn byte_and_depth_preflight_runs_before_typed_json_publication() {
    let at_limit = format!("{}0{}", "[".repeat(128), "]".repeat(128));
    preflight_canonical_bytes(at_limit.as_bytes()).expect("depth N passes preflight");

    let beyond = format!("{}0{}", "[".repeat(129), "]".repeat(129));
    assert_eq!(
        preflight_canonical_bytes(beyond.as_bytes())
            .expect_err("depth N+1")
            .code(),
        "FLOW_LIMIT_TREE_DEPTH"
    );
}

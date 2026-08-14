use flow_core::{
    canonical::{canonical_bytes, canonical_hash},
    model::{MigrationHop, Provenance},
    schema::{MigrationRegistry, MigrationStep, SchemaError},
};

const OLDER_FILE: &[u8] = include_bytes!("../../../fixtures/flowdoc/older.json");
const CURRENT_FILE: &[u8] = include_bytes!("../../../fixtures/flowdoc/current.json");
const MIGRATED_FILE: &[u8] = include_bytes!("../../../fixtures/flowdoc/migrated.json");
const MIGRATED_HASH: &str = include_str!("../../../fixtures/flowdoc/migrated.hash");

fn payload(file: &'static [u8]) -> &'static [u8] {
    file.strip_suffix(b"\n").unwrap_or(file)
}

#[test]
fn older_fixture_migrates_one_pure_hop_to_the_checked_in_current_schema_boundary() {
    let registry = MigrationRegistry::current();
    let first = registry.migrate(payload(OLDER_FILE)).expect("migration");
    let second = registry.migrate(payload(OLDER_FILE)).expect("repeat migration");

    assert_eq!(first.canonical_bytes, payload(MIGRATED_FILE));
    assert_eq!(first.canonical_hash, MIGRATED_HASH.trim());
    assert_eq!(first.canonical_bytes, second.canonical_bytes);
    assert_eq!(first.canonical_hash, second.canonical_hash);
    assert_eq!(first.report.source_schema_version, 0);
    assert_eq!(first.report.current_schema_version, 1);
    assert_eq!(
        first.report.hops,
        vec![MigrationHop {
            from_version: 0,
            to_version: 1,
        }]
    );
    assert!(first.report.requires_new_snapshot);
    assert!(first.report.preserve_source_records);
    assert!(matches!(
        first.document.provenance,
        Provenance::Migrated {
            source_schema_version: 0,
            current_schema_version: 1,
            ..
        }
    ));
}

#[test]
fn applying_the_registry_to_current_canonical_bytes_is_an_exact_no_op() {
    let current = payload(CURRENT_FILE);
    let outcome = MigrationRegistry::current()
        .migrate(current)
        .expect("current no-op");
    assert_eq!(outcome.canonical_bytes, current);
    assert_eq!(outcome.canonical_hash, canonical_hash(current));
    assert_eq!(canonical_bytes(&outcome.document).expect("bytes"), current);
    assert!(outcome.report.hops.is_empty());
    assert!(!outcome.report.requires_new_snapshot);
}

#[test]
fn future_and_missing_hops_fail_with_stable_codes_and_no_partial_output() {
    let future = br#"{"schemaVersion":2}"#;
    assert_eq!(
        MigrationRegistry::current()
            .migrate(future)
            .expect_err("future version")
            .code(),
        "FLOW_MIGRATION_FUTURE_VERSION"
    );
    assert_eq!(
        MigrationRegistry::new(Vec::new())
            .migrate(payload(OLDER_FILE))
            .expect_err("missing hop")
            .code(),
        "FLOW_MIGRATION_HOP_MISSING"
    );
}

fn invalid_intermediate(_: &[u8]) -> Result<Vec<u8>, SchemaError> {
    Ok(br#"{"schemaVersion":1}"#.to_vec())
}

fn interrupted(_: &[u8]) -> Result<Vec<u8>, SchemaError> {
    Err(SchemaError::migration_aborted())
}

#[test]
fn invalid_or_interrupted_hops_never_publish_a_partial_current_document() {
    let input = payload(OLDER_FILE).to_vec();
    let invalid_registry =
        MigrationRegistry::new(vec![MigrationStep::new(0, 1, invalid_intermediate)]);
    assert_eq!(
        invalid_registry
            .migrate(&input)
            .expect_err("invalid intermediate")
            .code(),
        "FLOW_MIGRATION_INTERMEDIATE_INVALID"
    );
    assert_eq!(input, payload(OLDER_FILE));

    let interrupted_registry = MigrationRegistry::new(vec![MigrationStep::new(0, 1, interrupted)]);
    assert_eq!(
        interrupted_registry
            .migrate(&input)
            .expect_err("interrupted")
            .code(),
        "FLOW_MIGRATION_ABORTED"
    );
    assert_eq!(input, payload(OLDER_FILE));
}

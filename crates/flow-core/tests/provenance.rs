use flow_core::{
    canonical::{canonical_bytes, canonical_hash},
    model::{FlowDocument, MigrationHop, Provenance},
    provenance::{
        PreviewExportProvenance, ProvenanceError, RevisionHash, RevisionLineage, RevisionProvenance,
    },
};

#[test]
fn revision_provenance_reports_exact_available_lineage_without_export_invention() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let hash = canonical_hash(&canonical_bytes(&document).expect("canonical bytes"));
    let source_hash = RevisionHash::parse(&hash).expect("versioned hash");
    let provenance = RevisionProvenance::from_document(&document, &hash)
        .expect("revision provenance")
        .with_source_hashes(vec![source_hash])
        .expect("safe source hashes");

    assert_eq!(provenance.document_id(), &document.document_id);
    assert_eq!(provenance.revision(), document.revision);
    assert_eq!(provenance.schema_version(), document.schema_version);
    assert_eq!(provenance.canonical_hash().as_str(), hash);
    assert!(matches!(
        provenance.preview_export_provenance(),
        PreviewExportProvenance::UnavailableInPhaseOne
    ));

    let serialized = serde_json::to_string(&provenance).expect("provenance serializes");
    for forbidden in ["previewBytes", "exportPayload", "SENSITIVE_DOCUMENT_TEXT"] {
        assert!(
            !serialized.contains(forbidden),
            "provenance invented or leaked {forbidden}"
        );
    }
    assert!(serialized.contains("unavailableInPhaseOne"));

    let mut unknown = serde_json::to_value(&provenance).expect("provenance JSON value");
    unknown.as_object_mut().expect("object").insert(
        "previewPayload".to_owned(),
        serde_json::json!("SENSITIVE_DOCUMENT_TEXT"),
    );
    assert!(serde_json::from_value::<RevisionProvenance>(unknown).is_err());
}

#[test]
fn canonical_validation_rejects_forged_or_discontinuous_lineage() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    document.provenance = Provenance::Migrated {
        source_schema_version: 0,
        current_schema_version: 2,
        source_created_at: "2026-08-14T00:00:00Z".to_owned(),
        hops: vec![MigrationHop {
            from_version: 7,
            to_version: 8,
        }],
    };
    assert_eq!(
        canonical_bytes(&document)
            .expect_err("discontinuous lineage")
            .code(),
        "FLOW_INVALID_DOCUMENT"
    );

    document.provenance = Provenance::LocalSample {
        created_at: "not-a-timestamp".to_owned(),
    };
    assert_eq!(
        canonical_bytes(&document)
            .expect_err("unbounded provenance text")
            .code(),
        "FLOW_INVALID_DOCUMENT"
    );
}

#[test]
fn revision_provenance_rejects_a_well_formed_hash_for_different_canonical_bytes() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let different_document = FlowDocument::deterministic_sample("en-US").expect("other sample");
    let unrelated_hash = canonical_hash(
        &canonical_bytes(&different_document).expect("different canonical document bytes"),
    );

    assert_eq!(
        RevisionProvenance::from_document(&document, unrelated_hash)
            .expect_err("a provenance hash must identify the supplied document"),
        ProvenanceError::InvalidHash
    );
}

#[test]
fn provenance_timestamp_rejects_impossible_calendar_values_and_accepts_a_leap_day() {
    for invalid in [
        "0000-01-01T00:00:00Z",
        "2026-00-01T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-02-29T00:00:00Z",
        "2024-04-31T00:00:00Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T23:60:00Z",
        "2026-01-01T23:59:61Z",
    ] {
        assert!(
            serde_json::from_value::<RevisionLineage>(serde_json::json!({
                "kind": "created",
                "createdAt": invalid,
            }))
            .is_err(),
            "accepted impossible provenance timestamp {invalid}"
        );
    }

    assert!(
        serde_json::from_value::<RevisionLineage>(serde_json::json!({
            "kind": "created",
            "createdAt": "2024-02-29T23:59:59Z",
        }))
        .is_ok(),
        "a real leap day must remain valid"
    );
}

#[test]
fn source_hashes_are_never_invented_and_explicit_values_remain_bounded() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let hash = canonical_hash(&canonical_bytes(&document).expect("canonical bytes"));
    let provenance =
        RevisionProvenance::from_document(&document, hash).expect("verified revision provenance");
    let serialized = serde_json::to_value(&provenance).expect("provenance JSON");
    assert_eq!(serialized["sourceHashes"], serde_json::json!([]));
    assert_eq!(
        serde_json::from_value::<RevisionProvenance>(serialized.clone())
            .expect("valid provenance round trip"),
        provenance
    );

    let duplicate =
        RevisionHash::parse(format!("flowpdf:blake3:v1:{:064x}", 1)).expect("explicit source hash");
    assert_eq!(
        provenance
            .clone()
            .with_source_hashes(vec![duplicate.clone(), duplicate])
            .expect_err("duplicate source hashes"),
        ProvenanceError::InvalidSourceHashes
    );

    let over_limit = (0..17)
        .map(|index| {
            RevisionHash::parse(format!("flowpdf:blake3:v1:{index:064x}"))
                .expect("explicit bounded hash syntax")
        })
        .collect();
    assert_eq!(
        provenance
            .with_source_hashes(over_limit)
            .expect_err("source hash limit"),
        ProvenanceError::InvalidSourceHashes
    );

    let mut hostile = serialized;
    hostile["sourceHashes"] = serde_json::json!(
        (0..17)
            .map(|_| format!("flowpdf:blake3:v1:{:064x}", 1))
            .collect::<Vec<_>>()
    );
    assert!(
        serde_json::from_value::<RevisionProvenance>(hostile).is_err(),
        "deserialization must enforce the same source-hash bounds as constructors"
    );
}

#[test]
fn revision_hash_rejects_unversioned_or_malformed_values() {
    assert!(RevisionHash::parse("blake3:abc").is_err());
    assert!(RevisionHash::parse("flowpdf:blake3:v1:not-hex").is_err());
    assert!(RevisionHash::parse(format!("flowpdf:blake3:v1:{:064X}", 0xAB)).is_err());
}

use flow_core::{
    canonical::{canonical_bytes, canonical_hash},
    model::FlowDocument,
    provenance::{PreviewExportProvenance, RevisionHash, RevisionProvenance},
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
        assert!(!serialized.contains(forbidden), "provenance invented or leaked {forbidden}");
    }
    assert!(serialized.contains("unavailableInPhaseOne"));

    let mut unknown = serde_json::to_value(&provenance).expect("provenance JSON value");
    unknown
        .as_object_mut()
        .expect("object")
        .insert("previewPayload".to_owned(), serde_json::json!("SENSITIVE_DOCUMENT_TEXT"));
    assert!(serde_json::from_value::<RevisionProvenance>(unknown).is_err());
}

#[test]
fn revision_hash_rejects_unversioned_or_malformed_values() {
    assert!(RevisionHash::parse("blake3:abc").is_err());
    assert!(RevisionHash::parse("flowpdf:blake3:v1:not-hex").is_err());
}

use flow_core::{
    canonical::{canonical_bytes, canonical_hash, decode_canonical},
    model::{FontFamily, HeaderFooterRun, HeaderFooterStyle, NodeId, SectionId, SectionSettings},
    schema::{MigrationRegistry, validate_document, validate_new_document},
};

const V2_DEFAULT: &[u8] = include_bytes!("../../../fixtures/flowdoc/schema-v2-current.json");
const V2_LAYOUT_DEFAULT: &[u8] =
    include_bytes!("../../../fixtures/flowdoc/schema-v2-layout-default.json");
const V3_SECTIONS: &[u8] = include_bytes!("../../../fixtures/flowdoc/schema-v3-sections.json");
const V3_MIGRATED: &[u8] = include_bytes!("../../../fixtures/flowdoc/schema-v3-migrated.json");
const V3_MIGRATED_HASH: &str = include_str!("../../../fixtures/flowdoc/schema-v3-migrated.hash");

fn payload(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

#[test]
fn explicit_sections_headers_and_footers_are_canonical_and_derived_free() {
    let input = payload(V3_SECTIONS);
    let document = decode_canonical(input).expect("v3 fixture");
    validate_new_document(&document).expect("fixture is authorable");
    assert_eq!(document.sections.len(), 2);
    assert_eq!(document.sections[0].start_node_id, None);
    assert_eq!(
        document.sections[1]
            .start_node_id
            .as_ref()
            .unwrap()
            .as_str(),
        "00000000-0000-4000-8000-000000000101"
    );
    assert_eq!(document.sections[0].header.runs[0].text, "FlowPDF");
    assert_eq!(document.sections[1].footer.runs[0].text, "2");
    assert_eq!(canonical_bytes(&document).expect("canonical bytes"), input);
    assert_eq!(
        canonical_hash(input),
        canonical_hash(&canonical_bytes(&document).unwrap())
    );

    let serialized = serde_json::to_value(&document).expect("document JSON");
    for derived in [
        "glyphs",
        "lineBoxes",
        "fragments",
        "displayList",
        "workerRequestId",
    ] {
        assert!(
            serialized.get(derived).is_none(),
            "derived field leaked: {derived}"
        );
    }
}

#[test]
fn v2_default_migrates_once_without_changing_semantic_payloads() {
    let source: serde_json::Value = serde_json::from_slice(payload(V2_DEFAULT)).unwrap();
    let outcome = MigrationRegistry::current()
        .migrate(payload(V2_DEFAULT))
        .expect("v2 migration");
    assert_eq!(outcome.report.source_schema_version, 2);
    assert_eq!(outcome.report.current_schema_version, 3);
    assert_eq!(
        outcome
            .report
            .hops
            .iter()
            .map(|hop| (hop.from_version, hop.to_version))
            .collect::<Vec<_>>(),
        vec![(2, 3)]
    );
    assert_eq!(outcome.document.sections.len(), 1);
    let migrated: serde_json::Value = serde_json::from_slice(&outcome.canonical_bytes).unwrap();
    for key in [
        "documentId",
        "revision",
        "locale",
        "pageSettings",
        "styles",
        "content",
        "assets",
        "fields",
    ] {
        assert_eq!(migrated[key], source[key], "semantic field changed: {key}");
    }
    let repeated = MigrationRegistry::current()
        .migrate(payload(V2_DEFAULT))
        .expect("repeat migration");
    assert_eq!(outcome, repeated);

    let no_op = MigrationRegistry::current()
        .migrate(&outcome.canonical_bytes)
        .expect("v3 no-op");
    assert_eq!(no_op.canonical_bytes, outcome.canonical_bytes);
    assert_eq!(no_op.canonical_hash, outcome.canonical_hash);
    assert!(no_op.report.hops.is_empty());
    assert!(!no_op.report.requires_new_snapshot);
}

#[test]
fn v2_layout_default_receives_the_same_single_default_section() {
    let outcome = MigrationRegistry::current()
        .migrate(payload(V2_LAYOUT_DEFAULT))
        .expect("v2 layout default migration");
    assert_eq!(outcome.document.sections.len(), 1);
    assert_eq!(outcome.document.sections[0].start_node_id, None);
    assert_eq!(
        outcome.document.sections[0].page_settings,
        outcome.document.page_settings
    );
    assert_eq!(
        outcome
            .report
            .hops
            .iter()
            .map(|hop| (hop.from_version, hop.to_version))
            .collect::<Vec<_>>(),
        vec![(2, 3)]
    );
}

#[test]
fn full_legacy_route_has_a_checked_in_v3_golden() {
    let source = include_bytes!("../../../fixtures/flowdoc/older.json");
    let outcome = MigrationRegistry::current()
        .migrate(payload(source))
        .expect("full legacy migration");
    assert_eq!(outcome.canonical_bytes, payload(V3_MIGRATED));
    assert_eq!(outcome.canonical_hash, V3_MIGRATED_HASH.trim());
    assert_eq!(
        canonical_bytes(&outcome.document).unwrap(),
        payload(V3_MIGRATED)
    );
}

#[test]
fn section_validation_rejects_duplicate_boundaries_geometry_and_oversized_static_content() {
    let mut document = decode_canonical(payload(V3_SECTIONS)).unwrap();

    document.sections[1].id = document.sections[0].id.clone();
    assert_eq!(
        validate_document(&document).unwrap_err().code(),
        "FLOW_DUPLICATE_ID"
    );

    let mut document = decode_canonical(payload(V3_SECTIONS)).unwrap();
    document.sections[1].start_node_id =
        Some(NodeId::new("00000000-0000-4000-8000-000000009999").unwrap());
    assert_eq!(
        validate_document(&document).unwrap_err().code(),
        "FLOW_LAYOUT_SECTION_INVALID"
    );

    let mut document = decode_canonical(payload(V3_SECTIONS)).unwrap();
    document.page_settings.margins_millimetres.left = 200;
    document.sections[0].page_settings.margins_millimetres.left = 200;
    assert_eq!(
        validate_document(&document).unwrap_err().code(),
        "FLOW_LAYOUT_GEOMETRY_INVALID"
    );

    let mut document = decode_canonical(payload(V3_SECTIONS)).unwrap();
    document.sections[0].header.runs[0] = HeaderFooterRun {
        text: "x".repeat(flow_core::schema::MAX_HEADER_FOOTER_BYTES + 1),
        style: HeaderFooterStyle {
            font_family: FontFamily::noto_sans(),
            font_size_millipoints: 9_000,
            bold: false,
            italic: false,
        },
    };
    assert_eq!(
        validate_document(&document).unwrap_err().code(),
        "FLOW_LAYOUT_HEADER_FOOTER_LIMIT"
    );
}

#[test]
fn defaults_have_one_document_start_section_and_stable_identity() {
    let document = flow_core::model::FlowDocument::deterministic_sample("uk-UA").unwrap();
    assert_eq!(document.sections.len(), 1);
    assert_eq!(
        document.sections[0].id,
        SectionId::new(SectionSettings::DEFAULT_ID).unwrap()
    );
    assert_eq!(document.sections[0].start_node_id, None);
    assert_eq!(document.sections[0].page_settings, document.page_settings);
    assert!(!document.sections[0].header.enabled);
    assert!(!document.sections[0].footer.enabled);
}

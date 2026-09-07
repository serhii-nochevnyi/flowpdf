//! Permanent compatibility gate. This module loads the private frozen decoder
//! directly so it can run before the current public schema is changed.
#[path = "../src/schema/legacy.rs"]
mod legacy;

use flow_core::canonical::canonical_hash;
use flow_core::schema::MigrationRegistry;

const OLD: &[u8] = include_bytes!("../../../fixtures/flowdoc/older.json");
const V1: &[u8] = include_bytes!("../../../fixtures/flowdoc/current.json");
const MIGRATED: &[u8] = include_bytes!("../../../fixtures/flowdoc/migrated.json");
const VALID: &[u8] = include_bytes!("../../../fixtures/flowdoc/schema-v1-rich-text-valid.json");
const INVALID: &[u8] =
    include_bytes!("../../../fixtures/flowdoc/schema-v1-rich-text-legacy-invalid.json");

fn payload(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

#[test]
fn legacy_freeze_gate() {
    for (label, bytes) in [
        ("old", payload(OLD)),
        ("valid", payload(VALID)),
        ("invalid", payload(INVALID)),
        (
            "decoder",
            include_bytes!("../src/schema/legacy.rs").as_slice(),
        ),
    ] {
        eprintln!("frozen {label}: {}", canonical_hash(bytes));
    }
    for (label, file, version, expected) in [
        (
            "v0",
            OLD,
            0,
            "flowpdf:blake3:v1:aff752048ed79b6f71876ba8ed419f91e15bcb9770f9a761fbdbbe395fd0619d",
        ),
        (
            "v1",
            V1,
            1,
            "flowpdf:blake3:v1:31b29e879e168b91188efca7c74fabdd45e4d016630427e2bd9c52b10e65a03f",
        ),
        (
            "v0-to-v1",
            MIGRATED,
            1,
            "flowpdf:blake3:v1:76d9db21fbddc216ea9de298a84e12cddc1f025cfa8834be29a30e5eb0669aa0",
        ),
        (
            "valid",
            VALID,
            1,
            "flowpdf:blake3:v1:3aeb83759447715fd893ecd407748fb81feedc07e1dfce125bf30c23c263449c",
        ),
        (
            "invalid",
            INVALID,
            1,
            "flowpdf:blake3:v1:1d2b570c7a903f2ff2ce100b1efadbbb8b61f1cdeb3489967332523f7257f826",
        ),
    ] {
        let bytes = payload(file);
        let decoded = legacy::decode(bytes, version).expect("frozen decoder");
        assert_eq!(decoded.canonical_bytes().unwrap(), bytes, "{label}");
        assert_eq!(canonical_hash(bytes), expected, "{label}");
        assert!(legacy::decode(bytes, 2).is_err());
        let mut changed = bytes.to_vec();
        changed.push(b' ');
        assert!(legacy::decode(&changed, version).is_err());
    }
    let valid = legacy::decode(payload(VALID), 1).unwrap().into_v1();
    let invalid = legacy::decode(payload(INVALID), 1).unwrap().into_v1();
    assert_eq!(valid.document_id.as_str(), invalid.document_id.as_str());
    assert_eq!(valid.content[0].text, "и\u{0306}😀");
    assert_eq!(valid.fields[0].anchor.utf16_offset, 2);
    assert_eq!(invalid.fields[0].anchor.utf16_offset, 1);
    assert_eq!(
        invalid.fields[1].anchor.node_id.as_str(),
        "00000000-0000-4000-8000-000000000999"
    );
    assert!(!valid.assets[0].alt_text.is_empty());
    assert!(invalid.assets[0].alt_text.is_empty());
    assert_eq!(invalid.styles[0].font_family, "Historical Font");
    assert_eq!(
        legacy::decode(payload(OLD), 0)
            .unwrap()
            .into_v1()
            .canonical_bytes()
            .unwrap(),
        payload(MIGRATED)
    );
    let decoder_source = include_str!("../src/schema/legacy.rs");
    assert!(!decoder_source.contains("crate::model"));
    assert_eq!(
        canonical_hash(decoder_source.as_bytes()),
        "flowpdf:blake3:v1:7bf6c1002bca88c5454b017b3facddc4f3555ce0c76ecc20d26e85b8929b4f81"
    );
}

#[test]
fn migration_routes_preserve_semantics_and_execute_current_no_op() {
    for (source, versions) in [(OLD, vec![(0, 1), (1, 2)]), (VALID, vec![(1, 2)]), (INVALID, vec![(1, 2)])] {
        let first = MigrationRegistry::current().migrate(payload(source)).expect("migration");
        assert_eq!(first.document.schema_version, 2);
        assert_eq!(first.report.hops.iter().map(|hop| (hop.from_version, hop.to_version)).collect::<Vec<_>>(), versions);
        let repeated = MigrationRegistry::current().migrate(payload(source)).unwrap();
        assert_eq!(first, repeated);
        let json = serde_json::to_value(&first.document).unwrap();
        assert_eq!(json["content"][0]["body"]["kind"], "paragraph");
        assert!(json["content"][0].get("text").is_none());
        assert!(json["content"][0]["body"]["runs"].is_array());
        let noop = MigrationRegistry::current().migrate(&first.canonical_bytes).unwrap();
        assert_eq!(noop.document, first.document);
        assert_eq!(noop.canonical_bytes, first.canonical_bytes);
        assert_eq!(noop.canonical_hash, first.canonical_hash);
        assert!(noop.report.hops.is_empty());
        assert!(!noop.report.requires_new_snapshot);
        assert!(noop.report.preserve_source_records);
    }
    let valid = MigrationRegistry::current().migrate(payload(VALID)).unwrap();
    let invalid = MigrationRegistry::current().migrate(payload(INVALID)).unwrap();
    let valid = serde_json::to_value(valid.document).unwrap();
    let invalid = serde_json::to_value(invalid.document).unwrap();
    assert_eq!(valid["fields"][0]["anchor"]["status"], "graphemeSafe");
    assert_eq!(valid["fields"][0]["anchor"]["original"]["utf16Offset"], 2);
    assert_eq!(invalid["fields"][0]["anchor"]["status"], "legacyInvalid");
    assert_eq!(invalid["fields"][0]["anchor"]["reason"], "nonGraphemeBoundary");
    assert_eq!(invalid["fields"][0]["anchor"]["original"]["utf16Offset"], 1);
    assert_eq!(invalid["fields"][1]["anchor"]["reason"], "missingNode");
    assert_eq!(invalid["styles"][0]["fontFamily"]["kind"], "legacyUnknown");
    assert_eq!(invalid["styles"][0]["fontFamily"]["original"], "Historical Font");
    assert_eq!(valid["content"][2]["body"]["accessibility"]["kind"], "described");
    assert_eq!(invalid["content"][2]["body"]["accessibility"]["kind"], "missingLegacy");
}

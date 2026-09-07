//! Permanent compatibility gate. This module loads the private frozen decoder
//! directly so it can run before the current public schema is changed.
#[path = "../src/schema/legacy.rs"]
mod legacy;

use flow_core::canonical::canonical_hash;

const OLD: &[u8] = include_bytes!("../../../fixtures/flowdoc/older.json");
const V1: &[u8] = include_bytes!("../../../fixtures/flowdoc/current.json");
const MIGRATED: &[u8] = include_bytes!("../../../fixtures/flowdoc/migrated.json");
const VALID: &[u8] = include_bytes!("../../../fixtures/flowdoc/schema-v1-rich-text-valid.json");
const INVALID: &[u8] = include_bytes!("../../../fixtures/flowdoc/schema-v1-rich-text-legacy-invalid.json");

fn payload(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

#[test]
fn legacy_freeze_gate() {
    for (label, file, version, expected) in [
        ("v0", OLD, 0, "PENDING_OLD"),
        ("v1", V1, 1, "flowpdf:blake3:v1:31b29e879e168b91188efca7c74fabdd45e4d016630427e2bd9c52b10e65a03f"),
        ("v0-to-v1", MIGRATED, 1, "flowpdf:blake3:v1:76d9db21fbddc216ea9de298a84e12cddc1f025cfa8834be29a30e5eb0669aa0"),
        ("valid", VALID, 1, "PENDING_VALID"),
        ("invalid", INVALID, 1, "PENDING_INVALID"),
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
    assert_eq!(invalid.fields[1].anchor.node_id.as_str(), "00000000-0000-4000-8000-000000000999");
    assert!(!valid.assets[0].alt_text.is_empty());
    assert!(invalid.assets[0].alt_text.is_empty());
    assert_eq!(invalid.styles[0].font_family, "Historical Font");
    assert_eq!(legacy::decode(payload(OLD), 0).unwrap().into_v1().canonical_bytes().unwrap(), payload(MIGRATED));
    let decoder_source = include_str!("../src/schema/legacy.rs");
    assert!(!decoder_source.contains("crate::model"));
    assert_eq!(canonical_hash(decoder_source.as_bytes()), "PENDING_DECODER");
}

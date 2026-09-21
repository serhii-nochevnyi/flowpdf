//! Permanent compatibility gate. This module loads the private frozen decoder
//! directly so it can run before the current public schema is changed.
#[path = "../src/schema/legacy.rs"]
mod legacy;

use flow_core::canonical::canonical_hash;
use flow_core::canonical::{canonical_bytes, decode_canonical};
use flow_core::model::*;
use flow_core::schema::MigrationRegistry;
use flow_core::schema::{validate_document, validate_new_document};

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
    for (source, versions) in [
        (OLD, vec![(0, 1), (1, 2), (2, 3)]),
        (VALID, vec![(1, 2), (2, 3)]),
        (INVALID, vec![(1, 2), (2, 3)]),
    ] {
        let first = MigrationRegistry::current()
            .migrate(payload(source))
            .expect("migration");
        assert_eq!(first.document.schema_version, 3);
        assert_eq!(
            first
                .report
                .hops
                .iter()
                .map(|hop| (hop.from_version, hop.to_version))
                .collect::<Vec<_>>(),
            versions
        );
        let repeated = MigrationRegistry::current()
            .migrate(payload(source))
            .unwrap();
        assert_eq!(first, repeated);
        let json = serde_json::to_value(&first.document).unwrap();
        assert_eq!(json["content"][0]["body"]["kind"], "paragraph");
        assert!(json["content"][0].get("text").is_none());
        assert!(json["content"][0]["body"]["runs"].is_array());
        let noop = MigrationRegistry::current()
            .migrate(&first.canonical_bytes)
            .unwrap();
        assert_eq!(noop.document, first.document);
        assert_eq!(noop.canonical_bytes, first.canonical_bytes);
        assert_eq!(noop.canonical_hash, first.canonical_hash);
        assert!(noop.report.hops.is_empty());
        assert!(!noop.report.requires_new_snapshot);
        assert!(noop.report.preserve_source_records);
    }
    let valid = MigrationRegistry::current()
        .migrate(payload(VALID))
        .unwrap();
    let invalid = MigrationRegistry::current()
        .migrate(payload(INVALID))
        .unwrap();
    let valid = serde_json::to_value(valid.document).unwrap();
    let invalid = serde_json::to_value(invalid.document).unwrap();
    assert_eq!(valid["fields"][0]["anchor"]["status"], "graphemeSafe");
    assert_eq!(valid["fields"][0]["anchor"]["original"]["utf16Offset"], 2);
    assert_eq!(invalid["fields"][0]["anchor"]["status"], "legacyInvalid");
    assert_eq!(
        invalid["fields"][0]["anchor"]["reason"],
        "nonGraphemeBoundary"
    );
    assert_eq!(invalid["fields"][0]["anchor"]["original"]["utf16Offset"], 1);
    assert_eq!(invalid["fields"][1]["anchor"]["reason"], "missingNode");
    assert_eq!(invalid["styles"][0]["fontFamily"]["kind"], "legacyUnknown");
    assert_eq!(
        invalid["styles"][0]["fontFamily"]["original"],
        "Historical Font"
    );
    assert_eq!(
        valid["content"][2]["body"]["accessibility"]["kind"],
        "described"
    );
    assert_eq!(
        invalid["content"][2]["body"]["accessibility"]["kind"],
        "missingLegacy"
    );
}

#[test]
fn migration_routes_golden_bytes() {
    let sample = flow_core::canonical::canonical_bytes(
        &flow_core::model::FlowDocument::deterministic_sample("uk-UA").unwrap(),
    )
    .unwrap();
    assert_eq!(
        flow_core::canonical::decode_canonical(&sample).unwrap(),
        flow_core::model::FlowDocument::deterministic_sample("uk-UA").unwrap()
    );
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/flowdoc");
    let v3_golden = std::fs::read(directory.join("schema-v3-sections.json")).unwrap();
    let v3_bytes = payload(&v3_golden);
    let v3_document = flow_core::canonical::decode_canonical(v3_bytes).unwrap();
    assert_eq!(canonical_bytes(&v3_document).unwrap(), v3_bytes);

    for source in [OLD, VALID, INVALID] {
        let outcome = MigrationRegistry::current()
            .migrate(payload(source))
            .unwrap();
        assert_eq!(
            canonical_bytes(&outcome.document).unwrap(),
            outcome.canonical_bytes
        );
        assert_eq!(outcome.document.schema_version, 3);
        assert!(!outcome.document.sections.is_empty());
    }
}

fn node(sequence: u64, body: BlockKind) -> ContentNode {
    ContentNode {
        id: NodeId::new(format!("00000000-0000-4000-8000-{sequence:012}")).unwrap(),
        style_id: None,
        body,
    }
}

fn paragraph(sequence: u64) -> ContentNode {
    ContentNode::paragraph(
        NodeId::new(format!("00000000-0000-4000-8000-{sequence:012}")).unwrap(),
        None,
        "и\u{0306}😀".to_owned(),
    )
}

fn table(rows: usize, columns: usize) -> ContentNode {
    let mut id = 10_000;
    let mut next = || {
        id += 1;
        id
    };
    let rows = (0..rows)
        .map(|_| {
            let cells = (0..columns)
                .map(|_| {
                    node(
                        next(),
                        BlockKind::TableCell {
                            children: vec![paragraph(next())],
                        },
                    )
                })
                .collect();
            node(next(), BlockKind::TableRow { cells })
        })
        .collect();
    node(
        next(),
        BlockKind::Table {
            header_rows: 1,
            rows,
        },
    )
}

#[test]
fn migration_routes_preserve_all_noncontent_records_and_source_bytes() {
    for source in [OLD, V1, VALID, INVALID] {
        let source_before = source.to_vec();
        let old = legacy::decode(payload(source), if source == OLD { 0 } else { 1 })
            .unwrap()
            .into_v1();
        let out = MigrationRegistry::current()
            .migrate(payload(source))
            .unwrap();
        let original = serde_json::to_value(&old).unwrap();
        let current = serde_json::to_value(&out.document).unwrap();
        for key in ["documentId", "revision", "locale", "pageSettings", "assets"] {
            assert_eq!(original[key], current[key], "{key}");
        }
        for (old, new) in old.content.iter().zip(&out.document.content) {
            assert_eq!(old.id.as_str(), new.id.as_str());
            assert_eq!(
                old.style_id.as_ref().map(|id| id.as_str()),
                new.style_id.as_ref().map(|id| id.as_str())
            );
            assert_eq!(old.text, new.text());
        }
        for (old, new) in original["fields"]
            .as_array()
            .unwrap()
            .iter()
            .zip(current["fields"].as_array().unwrap())
        {
            for key in [
                "id",
                "name",
                "label",
                "kind",
                "required",
                "readOnly",
                "defaultValue",
                "options",
            ] {
                assert_eq!(old[key], new[key], "{key}");
            }
            assert_eq!(old["anchor"], new["anchor"]["original"]);
        }
        assert_eq!(source, source_before);
    }
}

#[test]
fn migration_routes_reject_malformed_legacy_semantics_before_publication() {
    type LegacyMutation = Box<dyn Fn(&mut legacy::LegacyFlowDocumentV1)>;
    let cases: Vec<LegacyMutation> = vec![
        Box::new(|d| d.content[1].id = d.content[0].id.clone()),
        Box::new(|d| d.styles.clear()),
        Box::new(|d| d.assets.clear()),
        Box::new(|d| d.content[2].text = "hidden legacy image text".to_owned()),
        Box::new(|d| d.fields[0].default_value = legacy::FieldValue::Checked { value: true }),
        Box::new(|d| d.fields[1].options = d.fields[2].options.clone()),
        Box::new(|d| d.styles[0].font_family = " ".to_owned()),
        Box::new(|d| {
            d.provenance = legacy::Provenance::Migrated {
                source_schema_version: 0,
                current_schema_version: 1,
                source_created_at: "2026-08-14T00:00:00Z".to_owned(),
                hops: vec![legacy::MigrationHop {
                    from_version: 7,
                    to_version: 8,
                }],
            }
        }),
        Box::new(|d| {
            d.content[0].text =
                "x".repeat(flow_core::schema::DocumentLimits::V1.text_node_bytes + 1)
        }),
    ];
    for mutate in cases {
        let mut source = legacy::decode(payload(V1), 1).unwrap().into_v1();
        mutate(&mut source);
        let bytes = source.canonical_bytes().unwrap();
        assert!(MigrationRegistry::current().migrate(&bytes).is_err());
    }
    let mut value: serde_json::Value = serde_json::from_slice(payload(V1)).unwrap();
    value["content"][0]["unexpected"] = serde_json::json!(true);
    assert!(
        MigrationRegistry::current()
            .migrate(&serde_json::to_vec(&value).unwrap())
            .is_err()
    );
}

#[test]
fn schema_v2_round_trips_every_semantic_kind_and_inline_attribute() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").unwrap();
    let marks = MarkSet {
        bold: true,
        italic: true,
        underline: true,
        font_family: Some(FontFamily::Known {
            id: FontFamilyId::NotoSerif,
        }),
        font_size_millipoints: Some(18_000),
        color: Some([255, 0, 64]),
        language: Some(RunLanguage::English),
    };
    document.content.push(node(
        1_000,
        BlockKind::Heading {
            level: 2,
            attrs: ParagraphAttrs {
                alignment: Alignment::Center,
                spacing_before_millipoints: 6_000,
                spacing_after_millipoints: 12_000,
            },
            runs: vec![InlineRun {
                text: "Heading".to_owned(),
                marks,
            }],
        },
    ));
    document.content.push(node(
        1_001,
        BlockKind::OrderedList {
            items: vec![node(
                1_002,
                BlockKind::ListItem {
                    children: vec![
                        paragraph(1_003),
                        node(
                            1_004,
                            BlockKind::UnorderedList {
                                items: vec![node(
                                    1_005,
                                    BlockKind::ListItem {
                                        children: vec![paragraph(1_006)],
                                    },
                                )],
                            },
                        ),
                    ],
                },
            )],
        },
    ));
    document.content.push(table(2, 2));
    document.content.push(node(1_007, BlockKind::PageBreak));
    validate_new_document(&document).unwrap();
    let bytes = canonical_bytes(&document).unwrap();
    assert_eq!(decode_canonical(&bytes).unwrap(), document);
    assert_eq!(
        MigrationRegistry::current()
            .migrate(&bytes)
            .unwrap()
            .canonical_bytes,
        bytes
    );
    let mut nested_duplicate = document.clone();
    if let BlockKind::OrderedList { items } = &mut nested_duplicate.content[4].body {
        items[0].id = document.content[0].id.clone();
    }
    assert_eq!(
        validate_document(&nested_duplicate).unwrap_err().code(),
        "FLOW_DUPLICATE_ID"
    );
}

#[test]
fn schema_v2_rejects_malformed_nesting_and_closed_values() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").unwrap();
    for body in [
        BlockKind::ListItem {
            children: vec![paragraph(3_001)],
        },
        BlockKind::OrderedList {
            items: vec![paragraph(3_001)],
        },
        BlockKind::UnorderedList { items: vec![] },
        BlockKind::TableCell {
            children: vec![paragraph(3_001)],
        },
        BlockKind::TableRow { cells: vec![] },
        BlockKind::Table {
            header_rows: 2,
            rows: vec![],
        },
        BlockKind::Heading {
            level: 7,
            attrs: ParagraphAttrs::default(),
            runs: vec![],
        },
    ] {
        document.content.push(node(3_000, body));
        assert!(validate_document(&document).is_err());
        document.content.pop();
    }
    let mut value = serde_json::to_value(&document).unwrap();
    for (pointer, invalid) in [
        ("/content/0/body/kind", serde_json::json!("html")),
        ("/content/0/body/attrs/alignment", serde_json::json!("left")),
        (
            "/content/0/body/runs/0/marks/language",
            serde_json::json!("fr-FR"),
        ),
        (
            "/content/0/body/runs/0/marks/color",
            serde_json::json!("red"),
        ),
        (
            "/content/0/body/runs/0/marks/color",
            serde_json::json!([256, 0, 0]),
        ),
        (
            "/content/0/body/runs/0/marks/fontFamily",
            serde_json::json!({"kind":"known", "id":"Arial"}),
        ),
        (
            "/content/0/body/runs/0/marks/fontSizeMillipoints",
            serde_json::json!(12.5),
        ),
    ] {
        let previous = value.pointer(pointer).unwrap().clone();
        *value.pointer_mut(pointer).unwrap() = invalid;
        assert!(
            serde_json::from_value::<FlowDocument>(value.clone()).is_err(),
            "{pointer}"
        );
        *value.pointer_mut(pointer).unwrap() = previous;
    }
    value["content"][0]["body"]["runs"][0]["marks"]["css"] = serde_json::json!("display:none");
    assert!(serde_json::from_value::<FlowDocument>(value).is_err());
}

#[test]
fn schema_v2_new_authoring_rejects_legacy_states_and_preserves_them_on_load() {
    let invalid = MigrationRegistry::current()
        .migrate(payload(INVALID))
        .unwrap()
        .document;
    validate_document(&invalid).unwrap();
    assert!(validate_new_document(&invalid).is_err());
    let sample = FlowDocument::deterministic_sample("uk-UA").unwrap();
    let mut cases = Vec::new();
    let mut font = sample.clone();
    font.styles[0].font_family = FontFamily::LegacyUnknown {
        original: "Historical Font".to_owned(),
    };
    cases.push(font);
    let mut image = sample.clone();
    if let BlockKind::Image { accessibility, .. } = &mut image.content[2].body {
        *accessibility = ImageAccessibility::MissingLegacy;
    }
    cases.push(image);
    let mut field = sample.clone();
    field.fields[0].anchor = FieldAnchorState::LegacyInvalid {
        original: field.fields[0].anchor.original().clone(),
        reason: LegacyAnchorReason::MissingNode,
    };
    cases.push(field);
    for document in cases {
        validate_document(&document).unwrap();
        assert!(validate_new_document(&document).is_err());
    }
    let mut decorative = sample.clone();
    if let BlockKind::Image { accessibility, .. } = &mut decorative.content[2].body {
        *accessibility = ImageAccessibility::Decorative;
    }
    validate_new_document(&decorative).unwrap();
    let mut empty = sample;
    empty.content.clear();
    empty.fields.clear();
    validate_document(&empty).unwrap();
    assert!(validate_new_document(&empty).is_err());
}

#[test]
fn schema_v2_bounds_and_normalized_runs_reject_n_plus_one() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").unwrap();
    document.content.push(table(50, 20));
    validate_document(&document).unwrap();
    document.content.pop();
    for (rows, columns) in [(51, 20), (50, 21)] {
        document.content.push(table(rows, columns));
        assert_eq!(
            validate_document(&document).unwrap_err().code(),
            "FLOW_LIMIT_TABLE"
        );
        document.content.pop();
    }
    let make_list = |depth| {
        let mut child = paragraph(5_000);
        for index in 0..depth {
            child = node(
                5_001 + index * 3,
                BlockKind::UnorderedList {
                    items: vec![node(
                        5_002 + index * 3,
                        BlockKind::ListItem {
                            children: vec![paragraph(5_003 + index * 3), child],
                        },
                    )],
                },
            );
        }
        child
    };
    document.content.push(make_list(8));
    validate_document(&document).unwrap();
    document.content.pop();
    document.content.push(make_list(9));
    assert_eq!(
        validate_document(&document).unwrap_err().code(),
        "FLOW_LIMIT_LIST_DEPTH"
    );
    document.content.pop();
    let runs = (0..4_096)
        .map(|index| InlineRun {
            text: "x".to_owned(),
            marks: MarkSet {
                bold: index % 2 == 0,
                ..MarkSet::default()
            },
        })
        .collect();
    document.content.push(node(
        6_000,
        BlockKind::Paragraph {
            attrs: ParagraphAttrs::default(),
            runs,
        },
    ));
    validate_document(&document).unwrap();
    if let BlockKind::Paragraph { runs, .. } = &mut document.content.last_mut().unwrap().body {
        runs.push(InlineRun {
            text: "x".to_owned(),
            marks: MarkSet {
                bold: true,
                ..MarkSet::default()
            },
        });
    }
    assert_eq!(
        validate_document(&document).unwrap_err().code(),
        "FLOW_LIMIT_INLINE_RUNS"
    );
    document.content.pop();
    for (size, valid) in [
        (5_999, false),
        (6_000, true),
        (288_000, true),
        (288_001, false),
    ] {
        if let BlockKind::Paragraph { runs, .. } = &mut document.content[0].body {
            runs[0].marks.font_size_millipoints = Some(size);
        }
        assert_eq!(validate_document(&document).is_ok(), valid);
    }
    if let BlockKind::Paragraph { runs, .. } = &mut document.content[0].body {
        runs[0].marks.font_size_millipoints = None;
        let same = runs[0].clone();
        runs.push(same);
    }
    assert!(validate_document(&document).is_err());
}

#[test]
fn schema_v2_legacy_commands_reject_rich_targets_and_review_state_authoring_atomically() {
    use flow_core::anchor::Utf16Offset;
    use flow_core::transaction::{
        Command, CommandKind, EditorState, SourceModality, TransactionService,
    };
    let sample = FlowDocument::deterministic_sample("uk-UA").unwrap();
    let mut rich = sample.clone();
    if let BlockKind::Paragraph { runs, .. } = &mut rich.content[0].body {
        runs[0].marks.bold = true;
    }
    let state = EditorState::new(rich).unwrap();
    let before = state.clone();
    let command = Command {
        command_id: CommandId::new("00000000-0000-4000-8000-000000099990").unwrap(),
        base_revision: 1,
        modality: SourceModality::Api,
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
        kind: CommandKind::InsertText {
            target: LogicalPosition {
                node_id: sample.content[0].id.clone(),
                utf16_offset: Utf16Offset::new(0),
                affinity: Affinity::Forward,
            },
            text: "X".to_owned(),
        },
    };
    assert_eq!(
        TransactionService::apply(&state, command.clone())
            .unwrap_err()
            .code(),
        "FLOW_INVALID_TARGET"
    );
    assert_eq!(state, before);
    let state = EditorState::new(sample.clone()).unwrap();
    let invalid_image = node(
        99_991,
        BlockKind::Image {
            asset_id: sample.assets[0].id.clone(),
            accessibility: ImageAccessibility::MissingLegacy,
        },
    );
    let insert = Command {
        kind: CommandKind::InsertNode {
            index: 3,
            node: invalid_image,
        },
        ..command.clone()
    };
    assert!(TransactionService::apply(&state, insert).is_err());
    let mut field = sample.fields[0].clone();
    field.anchor = FieldAnchorState::LegacyInvalid {
        original: field.anchor.original().clone(),
        reason: LegacyAnchorReason::MissingNode,
    };
    let forged = Command {
        kind: CommandKind::SetField {
            field_id: field.id.clone(),
            field,
        },
        ..command
    };
    assert!(TransactionService::apply(&state, forged).is_err());
    assert_eq!(state.document(), &sample);
    assert_eq!(state.history(), &Default::default());

    let mut last_paragraph = sample;
    last_paragraph.fields.clear();
    last_paragraph.content.truncate(1);
    let delete = Command {
        command_id: CommandId::new("00000000-0000-4000-8000-000000099992").unwrap(),
        base_revision: 1,
        modality: SourceModality::Api,
        issued_at: "2026-08-14T00:00:00Z".to_owned(),
        kind: CommandKind::DeleteNode {
            node_id: last_paragraph.content[0].id.clone(),
        },
    };
    let state = EditorState::new(last_paragraph.clone()).unwrap();
    assert!(TransactionService::apply(&state, delete).is_err());
    assert_eq!(state.document(), &last_paragraph);
}

#[test]
fn schema_v2_nested_tables_empty_runs_and_spacing_limits_fail_closed() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").unwrap();
    let mut outer = table(1, 1);
    if let BlockKind::Table { rows, .. } = &mut outer.body
        && let BlockKind::TableRow { cells } = &mut rows[0].body
        && let BlockKind::TableCell { children } = &mut cells[0].body
    {
        children.push(node(
            90_000,
            BlockKind::Table {
                header_rows: 0,
                rows: vec![node(
                    90_001,
                    BlockKind::TableRow {
                        cells: vec![node(
                            90_002,
                            BlockKind::TableCell {
                                children: vec![paragraph(90_003)],
                            },
                        )],
                    },
                )],
            },
        ));
    }
    document.content.push(outer);
    assert_eq!(
        validate_document(&document).unwrap_err().code(),
        "FLOW_INVALID_DOCUMENT"
    );
    document.content.pop();
    for (spacing, valid) in [(144_000, true), (144_001, false)] {
        if let BlockKind::Paragraph { attrs, .. } = &mut document.content[0].body {
            attrs.spacing_after_millipoints = spacing;
        }
        assert_eq!(validate_document(&document).is_ok(), valid);
    }
    if let BlockKind::Paragraph { attrs, runs } = &mut document.content[0].body {
        attrs.spacing_after_millipoints = 0;
        runs[0].text.clear();
    }
    assert!(validate_document(&document).is_err());
    let mut image = FlowDocument::deterministic_sample("uk-UA").unwrap();
    for (length, valid) in [(4_096, true), (4_097, false)] {
        if let BlockKind::Image { accessibility, .. } = &mut image.content[2].body {
            *accessibility = ImageAccessibility::Described {
                text: "a".repeat(length),
            };
        }
        assert_eq!(validate_new_document(&image).is_ok(), valid);
    }
}

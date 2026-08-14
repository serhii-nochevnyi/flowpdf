use flow_core::{
    canonical::{canonical_bytes, canonical_hash, decode_canonical},
    model::{
        Affinity, FieldKind, FieldValue, FlowDocument, LogicalPosition, NodeId, TextInputHint,
    },
    schema::{SchemaError, validate_document},
};

fn code(error: SchemaError) -> &'static str {
    error.code()
}

#[test]
fn representative_document_validates_and_round_trips_through_one_canonical_utf8_path() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("valid sample");
    validate_document(&document).expect("schema v1 sample validates");

    let bytes = canonical_bytes(&document).expect("canonical bytes");
    assert!(std::str::from_utf8(&bytes).is_ok());
    assert_eq!(bytes, canonical_bytes(&document).expect("repeatable bytes"));
    assert_eq!(canonical_hash(&bytes), canonical_hash(&bytes));
    assert!(canonical_hash(&bytes).starts_with("flowpdf:blake3:v1:"));
    assert_eq!(decode_canonical(&bytes).expect("decode"), document);

    let exact_text = "Український текст: и\u{0306}, апостроф ’, emoji 😀, non-BMP 𝄞.";
    assert!(document.content.iter().any(|node| node.text == exact_text));
    assert!(
        bytes
            .windows(exact_text.len())
            .any(|window| window == exact_text.as_bytes())
    );
}

#[test]
fn equal_and_adjacent_semantic_nodes_remain_distinct_and_ordered() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").expect("valid sample");
    document.fields.clear();
    document.content.retain(|node| node.asset_id.is_none());
    document.assets.clear();
    let mut duplicate_text = document.content[0].clone();
    duplicate_text.id = NodeId::new("00000000-0000-4000-8000-000000009901").expect("id");
    document.content.insert(1, duplicate_text);

    let decoded = decode_canonical(&canonical_bytes(&document).expect("encode")).expect("decode");
    assert_eq!(decoded.content.len(), document.content.len());
    assert_eq!(decoded.content[0].text, decoded.content[1].text);
    assert_ne!(decoded.content[0].id, decoded.content[1].id);
    assert_eq!(decoded.content, document.content);
}

#[test]
fn required_collections_preserve_empty_and_singleton_but_reject_null_or_omission() {
    let mut empty = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    empty.styles.clear();
    empty.content.clear();
    empty.assets.clear();
    empty.fields.clear();
    let bytes = canonical_bytes(&empty).expect("empty collections are valid");
    assert_eq!(decode_canonical(&bytes).expect("empty round trip"), empty);

    let null_content = String::from_utf8(bytes.clone())
        .expect("utf8")
        .replace("\"content\":[]", "\"content\":null");
    assert_eq!(
        code(decode_canonical(null_content.as_bytes()).expect_err("null is invalid")),
        "FLOW_SCHEMA_DECODE"
    );
    let omitted_content = String::from_utf8(bytes)
        .expect("utf8")
        .replace("\"content\":[],", "");
    assert_eq!(
        code(decode_canonical(omitted_content.as_bytes()).expect_err("omission is invalid")),
        "FLOW_SCHEMA_DECODE"
    );
}

#[test]
fn schema_rejects_unknown_coordinates_floats_invalid_ids_duplicates_and_dangling_references() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let mut json = String::from_utf8(canonical_bytes(&document).expect("bytes")).expect("utf8");
    json.pop();
    json.push_str(",\"pdfRectangle\":[0,0,10,10]}");
    assert_eq!(
        code(decode_canonical(json.as_bytes()).expect_err("unknown coordinate")),
        "FLOW_SCHEMA_DECODE"
    );

    let float_revision = String::from_utf8(canonical_bytes(&document).expect("bytes"))
        .expect("utf8")
        .replacen("\"revision\":1", "\"revision\":1.5", 1);
    assert_eq!(
        code(decode_canonical(float_revision.as_bytes()).expect_err("float")),
        "FLOW_SCHEMA_DECODE"
    );

    let invalid_id = String::from_utf8(canonical_bytes(&document).expect("bytes"))
        .expect("utf8")
        .replacen("00000000-0000-4000-8000-000000000001", "not-a-uuid", 1);
    assert_eq!(
        code(decode_canonical(invalid_id.as_bytes()).expect_err("invalid id")),
        "FLOW_INVALID_ID"
    );

    let mut duplicate = document.clone();
    duplicate.content[1].id = duplicate.content[0].id.clone();
    assert_eq!(
        code(validate_document(&duplicate).expect_err("duplicate id")),
        "FLOW_DUPLICATE_ID"
    );

    let mut dangling_style = document.clone();
    dangling_style.content[0].style_id =
        Some(flow_core::model::StyleId::new("00000000-0000-4000-8000-000000009902").expect("id"));
    assert_eq!(
        code(validate_document(&dangling_style).expect_err("dangling style")),
        "FLOW_DANGLING_REFERENCE"
    );

    let mut dangling_asset = document.clone();
    let image = dangling_asset
        .content
        .iter_mut()
        .find(|node| node.asset_id.is_some())
        .expect("image node");
    image.asset_id =
        Some(flow_core::model::AssetId::new("00000000-0000-4000-8000-000000009903").expect("id"));
    assert_eq!(
        code(validate_document(&dangling_asset).expect_err("dangling asset")),
        "FLOW_DANGLING_REFERENCE"
    );
}

#[test]
fn field_vocabulary_is_closed_and_kind_value_option_constraints_are_exhaustive() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("valid sample");
    let kinds = document
        .fields
        .iter()
        .map(|field| &field.kind)
        .collect::<Vec<_>>();
    assert!(kinds.iter().any(|kind| matches!(
        kind,
        FieldKind::Text {
            multiline: false,
            input_hint: TextInputHint::Plain
        }
    )));
    assert!(kinds.iter().any(|kind| matches!(kind, FieldKind::Checkbox)));
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, FieldKind::RadioGroup))
    );
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, FieldKind::Select { multiple: true }))
    );
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, FieldKind::Signature))
    );
    assert!(kinds.iter().any(|kind| matches!(kind, FieldKind::Button)));

    for input_hint in [
        TextInputHint::Plain,
        TextInputHint::Date,
        TextInputHint::Number,
        TextInputHint::Email,
    ] {
        for multiline in [false, true] {
            for value in [FieldValue::Empty, FieldValue::text("exact text")] {
                let mut valid = document.clone();
                valid.fields[0].kind = FieldKind::Text {
                    multiline,
                    input_hint: input_hint.clone(),
                };
                valid.fields[0].default_value = value;
                validate_document(&valid).expect("every text hint/value form is valid");
            }
        }
    }

    for value in [FieldValue::Empty, FieldValue::checked(true)] {
        let mut valid = document.clone();
        valid.fields[1].default_value = value;
        validate_document(&valid).expect("checkbox empty/checked values are valid");
    }

    let mut valid_single_select = document.clone();
    valid_single_select.fields[3].kind = FieldKind::Select { multiple: false };
    valid_single_select.fields[3].default_value =
        FieldValue::selected(vec![valid_single_select.fields[3].options[0].id.clone()]);
    validate_document(&valid_single_select).expect("single select may select one option");

    let mut invalid_text = document.clone();
    invalid_text.fields[0].default_value = FieldValue::checked(true);
    assert_eq!(
        code(validate_document(&invalid_text).expect_err("text/checked mismatch")),
        "FLOW_FIELD_VALUE_INVALID"
    );

    let mut invalid_checkbox = document.clone();
    invalid_checkbox.fields[1].options = document.fields[2].options.clone();
    assert_eq!(
        code(validate_document(&invalid_checkbox).expect_err("checkbox options")),
        "FLOW_FIELD_OPTIONS_INVALID"
    );

    let mut invalid_radio = document.clone();
    let radio_options = invalid_radio.fields[2].options.clone();
    invalid_radio.fields[2].default_value = FieldValue::selected(vec![
        radio_options[0].id.clone(),
        radio_options[1].id.clone(),
    ]);
    assert_eq!(
        code(validate_document(&invalid_radio).expect_err("radio cardinality")),
        "FLOW_FIELD_VALUE_INVALID"
    );

    let mut invalid_single_select = document.clone();
    invalid_single_select.fields[3].kind = FieldKind::Select { multiple: false };
    assert_eq!(
        code(validate_document(&invalid_single_select).expect_err("select cardinality")),
        "FLOW_FIELD_VALUE_INVALID"
    );

    let mut invalid_option_reference = document.clone();
    invalid_option_reference.fields[2].default_value = FieldValue::selected(vec![
        flow_core::model::FieldOptionId::new("00000000-0000-4000-8000-000000009904").expect("id"),
    ]);
    assert_eq!(
        code(validate_document(&invalid_option_reference).expect_err("unknown option")),
        "FLOW_FIELD_VALUE_INVALID"
    );

    let mut duplicate_export = document.clone();
    duplicate_export.fields[2].options[1].export_value =
        duplicate_export.fields[2].options[0].export_value.clone();
    assert_eq!(
        code(validate_document(&duplicate_export).expect_err("duplicate export value")),
        "FLOW_FIELD_OPTIONS_INVALID"
    );

    for index in [4_usize, 5] {
        let mut invalid = document.clone();
        invalid.fields[index].default_value = FieldValue::text("forbidden");
        assert_eq!(
            code(validate_document(&invalid).expect_err("signature/button text")),
            "FLOW_FIELD_VALUE_INVALID"
        );
    }
}

#[test]
fn utf16_positions_accept_boundaries_and_reject_surrogate_interiors() {
    let mut document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let node = &document.content[0];
    let emoji_byte = node.text.find('😀').expect("emoji");
    let before_emoji = u32::try_from(node.text[..emoji_byte].encode_utf16().count()).expect("size");

    document.fields[0].anchor = LogicalPosition {
        node_id: node.id.clone(),
        utf16_offset: before_emoji,
        affinity: Affinity::Forward,
    };
    validate_document(&document).expect("boundary before emoji");

    document.fields[0].anchor.utf16_offset = before_emoji + 1;
    assert_eq!(
        code(validate_document(&document).expect_err("surrogate interior")),
        "FLOW_INVALID_UTF16_POSITION"
    );
}

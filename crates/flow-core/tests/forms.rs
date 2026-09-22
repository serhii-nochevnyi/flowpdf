use flow_core::{
    FormProjectionError, FormSessionAction, FormSessionError, FormSessionRequest, FormSessionState,
    FormValueErrorCode, FormWidgetReviewReason, PdfDisplayList, apply_form_session,
    build_display_list,
    canonical::canonical_bytes,
    layout::{FontCatalog, FontFace, LayoutUnit, PaginationRequest},
    model::{
        Affinity, CommandId, ContentNode, DocumentId, FieldAnchorState, FieldDescriptor, FieldId,
        FieldKind, FieldOption, FieldOptionId, FieldValue, FlowDocument, LogicalPosition, NodeId,
        TextInputHint, TombstoneToken,
    },
    paginate_document, resolve_form_widgets, resolve_form_widgets_with_session,
    validate_field_value,
};

const NOTO_SANS: &[u8] = include_bytes!("../data/NotoSans-Regular.ttf");

fn node_id(value: u32) -> NodeId {
    NodeId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("valid node id")
}

fn field_id(value: u32) -> FieldId {
    FieldId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("valid field id")
}

fn option_id(value: u32) -> FieldOptionId {
    FieldOptionId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("valid option id")
}

fn position(node_id: NodeId, offset: u32) -> LogicalPosition {
    LogicalPosition {
        node_id,
        utf16_offset: offset.into(),
        affinity: Affinity::Forward,
    }
}

fn text_field(id: u32, node_id: NodeId, offset: u32) -> FieldDescriptor {
    FieldDescriptor {
        id: field_id(id),
        name: format!("field-{id}"),
        label: Some(format!("Field {id}")),
        anchor: FieldAnchorState::GraphemeSafe {
            original: position(node_id, offset),
        },
        kind: FieldKind::Text {
            multiline: false,
            input_hint: TextInputHint::Plain,
        },
        required: false,
        read_only: false,
        default_value: FieldValue::Empty,
        options: Vec::new(),
    }
}

fn document_with_field() -> FlowDocument {
    let mut document = FlowDocument::deterministic_sample("en-US").expect("sample document");
    let node = node_id(9_901);
    let style_id = document.styles.first().map(|style| style.id.clone());
    document.content = vec![ContentNode::paragraph(
        node.clone(),
        style_id,
        "Hello form field anchor.".to_owned(),
    )];
    document.assets.clear();
    document.fields = vec![text_field(9_951, node, 6)];
    document
}

fn catalog() -> FontCatalog {
    FontCatalog::new(vec![
        FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).expect("font face"),
    ])
    .expect("font catalog")
}

fn display_list(document: &FlowDocument) -> PdfDisplayList {
    let request = PaginationRequest::for_document(document).expect("pagination request");
    let pagination = paginate_document(document, &request, &catalog(), None).expect("pagination");
    build_display_list(document, &pagination, &catalog(), None).expect("display list")
}

#[test]
fn field_values_are_validated_by_kind_and_constraints() {
    let node = node_id(9_902);
    let mut field = text_field(9_952, node.clone(), 0);
    field.required = true;
    assert_eq!(
        validate_field_value(&field, &FieldValue::Empty),
        Err(FormValueErrorCode::Required)
    );

    field.required = false;
    field.kind = FieldKind::Text {
        multiline: false,
        input_hint: TextInputHint::Date,
    };
    assert!(validate_field_value(&field, &FieldValue::text("2028-02-29")).is_ok());
    assert_eq!(
        validate_field_value(&field, &FieldValue::text("2027-02-29")),
        Err(FormValueErrorCode::InvalidDate)
    );

    field.kind = FieldKind::Text {
        multiline: false,
        input_hint: TextInputHint::Plain,
    };
    assert_eq!(
        validate_field_value(&field, &FieldValue::text("line one\nline two")),
        Err(FormValueErrorCode::MultilineNotAllowed)
    );

    let first = option_id(9_961);
    let second = option_id(9_962);
    field.kind = FieldKind::RadioGroup;
    field.options = vec![
        FieldOption {
            id: first.clone(),
            label: "First".to_owned(),
            export_value: "first".to_owned(),
        },
        FieldOption {
            id: second.clone(),
            label: "Second".to_owned(),
            export_value: "second".to_owned(),
        },
    ];
    assert_eq!(
        validate_field_value(
            &field,
            &FieldValue::selected(vec![first.clone(), second.clone()])
        ),
        Err(FormValueErrorCode::SelectionLimit)
    );
    assert_eq!(
        validate_field_value(&field, &FieldValue::selected(vec![option_id(9_963)])),
        Err(FormValueErrorCode::OptionSetInvalid)
    );
    assert!(validate_field_value(&field, &FieldValue::selected(vec![first])).is_ok());

    field.kind = FieldKind::Text {
        multiline: false,
        input_hint: TextInputHint::Number,
    };
    assert!(validate_field_value(&field, &FieldValue::text("-12.50")).is_ok());
    assert_eq!(
        validate_field_value(&field, &FieldValue::text("twelve")),
        Err(FormValueErrorCode::InvalidNumber)
    );
    field.kind = FieldKind::Text {
        multiline: false,
        input_hint: TextInputHint::Email,
    };
    assert!(validate_field_value(&field, &FieldValue::text("person@example.test")).is_ok());
    assert_eq!(
        validate_field_value(&field, &FieldValue::text("not-an-email")),
        Err(FormValueErrorCode::InvalidEmail)
    );
    assert_eq!(
        validate_field_value(&field, &FieldValue::text("x".repeat(64 * 1024 + 1)),),
        Err(FormValueErrorCode::TextSizeLimit)
    );

    let mut checkbox = text_field(9_953, node.clone(), 0);
    checkbox.kind = FieldKind::Checkbox;
    checkbox.required = true;
    assert!(validate_field_value(&checkbox, &FieldValue::checked(true)).is_ok());
    assert_eq!(
        validate_field_value(&checkbox, &FieldValue::checked(false)),
        Err(FormValueErrorCode::Required)
    );
    assert_eq!(
        validate_field_value(&checkbox, &FieldValue::text("wrong type")),
        Err(FormValueErrorCode::TypeMismatch)
    );

    let mut select = text_field(9_954, node.clone(), 0);
    select.kind = FieldKind::Select { multiple: false };
    select.options = vec![
        FieldOption {
            id: option_id(9_964),
            label: "First".to_owned(),
            export_value: "first".to_owned(),
        },
        FieldOption {
            id: option_id(9_965),
            label: "Second".to_owned(),
            export_value: "second".to_owned(),
        },
    ];
    assert_eq!(
        validate_field_value(
            &select,
            &FieldValue::selected(vec![option_id(9_964), option_id(9_965)]),
        ),
        Err(FormValueErrorCode::SelectionLimit)
    );
    assert!(validate_field_value(&select, &FieldValue::selected(vec![option_id(9_964)])).is_ok());

    for kind in [FieldKind::Signature, FieldKind::Button] {
        let mut unsupported = text_field(9_966, node.clone(), 0);
        unsupported.kind = kind;
        assert!(validate_field_value(&unsupported, &FieldValue::Empty).is_ok());
        assert_eq!(
            validate_field_value(&unsupported, &FieldValue::text("not supported")),
            Err(FormValueErrorCode::UnsupportedValue)
        );
    }
}

#[test]
fn projection_is_deterministic_and_revision_bound() {
    let document = document_with_field();
    let display = display_list(&document);
    let first = resolve_form_widgets(&document, &display).expect("form projection");
    let second = resolve_form_widgets(&document, &display).expect("repeat projection");

    assert_eq!(first, second);
    assert_eq!(first.widgets.len(), 1);
    assert_eq!(
        first.widgets[0].widget_id,
        "flow-form-widget-00000000-0000-4000-8000-000000009951"
    );
    assert_eq!(first.widgets[0].page_index, 0);
    assert_eq!(first.widgets[0].tab_order, 0);
    assert!(first.widgets[0].rect.width.raw() > 0);
    assert!(!first.result_hash.is_empty());

    let original_rect = first.widgets[0].rect;
    let mut reflowed = display.clone();
    let page = &mut reflowed.pages[0];
    page.page_index = 1;
    for item in &mut page.items {
        for line in &mut item.lines {
            line.rect.x = LayoutUnit::from_raw(line.rect.x.raw() + 128);
            line.rect.y = LayoutUnit::from_raw(line.rect.y.raw() + 128);
            for glyph in &mut line.glyphs {
                glyph.x = LayoutUnit::from_raw(glyph.x.raw() + 128);
                glyph.y = LayoutUnit::from_raw(glyph.y.raw() + 128);
            }
        }
    }
    reflowed.result_hash = "reflowed-display".to_owned();
    let moved = resolve_form_widgets(&document, &reflowed).expect("reflowed projection");
    assert_eq!(moved.widgets[0].page_index, 1);
    assert!(moved.widgets[0].rect.x > original_rect.x);
    assert!(moved.widgets[0].rect.y > original_rect.y);
    assert_ne!(moved.display_list_hash, first.display_list_hash);

    let mut stale = document.clone();
    stale.revision += 1;
    assert_eq!(
        resolve_form_widgets(&stale, &display),
        Err(FormProjectionError::StaleRevision)
    );

    let mut forged = display;
    forged.source_hash = "forged-source-hash".to_owned();
    assert_eq!(
        resolve_form_widgets(&document, &forged),
        Err(FormProjectionError::SourceHashMismatch)
    );
}

#[test]
fn projection_tab_order_follows_canonical_field_order_not_geometry() {
    let mut document = document_with_field();
    let node = document.content[0].id.clone();
    let second = text_field(9_956, node, 0);
    document.fields.push(second.clone());
    let display = display_list(&document);
    let projection = resolve_form_widgets(&document, &display).expect("ordered projection");
    assert_eq!(
        projection
            .widgets
            .iter()
            .find(|widget| widget.field_id == document.fields[0].id)
            .expect("first widget")
            .tab_order,
        0
    );
    assert_eq!(
        projection
            .widgets
            .iter()
            .find(|widget| widget.field_id == second.id)
            .expect("second widget")
            .tab_order,
        1
    );

    document.fields.swap(0, 1);
    let swapped_display = display_list(&document);
    let swapped = resolve_form_widgets(&document, &swapped_display).expect("swapped projection");
    assert_eq!(
        swapped
            .widgets
            .iter()
            .find(|widget| widget.field_id == document.fields[0].id)
            .expect("swapped first widget")
            .tab_order,
        0
    );
    assert_eq!(
        swapped
            .widgets
            .iter()
            .find(|widget| widget.field_id == document.fields[1].id)
            .expect("swapped second widget")
            .tab_order,
        1
    );
}

#[test]
fn required_empty_defaults_are_projectable_but_empty_user_values_are_rejected() {
    let mut document = document_with_field();
    document.fields[0].required = true;
    assert_eq!(
        validate_field_value(&document.fields[0], &FieldValue::Empty),
        Err(FormValueErrorCode::Required)
    );
    let display = display_list(&document);
    let projection = resolve_form_widgets(&document, &display).expect("empty default projection");
    assert_eq!(projection.widgets.len(), 1);
    assert_eq!(projection.widgets[0].default_value, FieldValue::Empty);
}

#[test]
fn form_session_separates_defaults_from_immutable_fill_overrides() {
    let mut document = document_with_field();
    document.fields[0].default_value = FieldValue::text("Template");
    let original_document = document.clone();
    let field = field_id(9_951);
    let session = FormSessionState::from_document(&document).expect("form session");
    assert_eq!(session.generation, 0);
    assert!(session.overrides().is_empty());
    assert_eq!(
        session.value_for(&document, &field).expect("default value"),
        FieldValue::text("Template")
    );

    let filled = session
        .set_value(&document, &field, FieldValue::text("Alice"))
        .expect("fill value");
    assert_eq!(session.generation, 0);
    assert!(session.overrides().is_empty());
    assert_eq!(filled.generation, 1);
    assert_eq!(filled.overrides().len(), 1);
    assert_eq!(
        filled.value_for(&document, &field).expect("current value"),
        FieldValue::text("Alice")
    );
    assert_eq!(document, original_document);
    assert_eq!(
        document.fields[0].default_value,
        FieldValue::text("Template")
    );

    let display = display_list(&document);
    let default_projection = resolve_form_widgets(&document, &display).expect("default projection");
    assert_eq!(default_projection.schema_version, 2);
    assert_eq!(
        default_projection.widgets[0].default_value,
        FieldValue::text("Template")
    );
    assert_eq!(
        default_projection.widgets[0].value,
        FieldValue::text("Template")
    );
    let filled_projection =
        resolve_form_widgets_with_session(&document, &display, &filled).expect("filled projection");
    assert_eq!(
        filled_projection.widgets[0].default_value,
        FieldValue::text("Template")
    );
    assert_eq!(
        filled_projection.widgets[0].value,
        FieldValue::text("Alice")
    );
    assert_eq!(
        filled_projection.widgets[0].rect,
        default_projection.widgets[0].rect
    );
    assert_eq!(
        filled_projection.source_hash,
        default_projection.source_hash
    );
    assert_eq!(
        filled_projection.display_list_hash,
        default_projection.display_list_hash
    );
    assert_ne!(
        filled_projection.result_hash,
        default_projection.result_hash
    );

    let cleared = filled.clear_value(&document, &field).expect("clear value");
    assert_eq!(cleared.generation, 2);
    assert!(cleared.overrides().is_empty());
    assert_eq!(
        cleared
            .value_for(&document, &field)
            .expect("restored default"),
        FieldValue::text("Template")
    );
    let cleared_projection = resolve_form_widgets_with_session(&document, &display, &cleared)
        .expect("cleared projection");
    assert_eq!(cleared_projection, default_projection);

    let encoded = serde_json::to_vec(&filled).expect("session JSON");
    let decoded: FormSessionState = serde_json::from_slice(&encoded).expect("session round trip");
    assert_eq!(decoded, filled);
}

#[test]
fn form_session_reuses_validation_and_rejects_forged_or_stale_state() {
    let mut document = document_with_field();
    document.fields[0].required = true;
    let field = field_id(9_951);
    let session = FormSessionState::from_document(&document).expect("session");

    let error = session
        .set_value(&document, &field, FieldValue::Empty)
        .expect_err("required empty value");
    assert_eq!(error.code(), "FLOW_FORM_SESSION_VALUE_INVALID");
    assert_eq!(
        error,
        FormSessionError::InvalidValue {
            field_id: field.clone(),
            code: FormValueErrorCode::Required,
        }
    );
    assert!(!error.to_string().contains("secret"));

    let unknown = field_id(9_999);
    assert_eq!(
        session.value_for(&document, &unknown),
        Err(FormSessionError::UnknownField { field_id: unknown })
    );

    let mut stale_document = document.clone();
    stale_document.revision += 1;
    assert_eq!(
        session.value_for(&stale_document, &field),
        Err(FormSessionError::StaleRevision)
    );

    let mut other_document = FlowDocument::deterministic_sample("uk-UA").expect("other document");
    other_document.document_id =
        DocumentId::new("00000000-0000-4000-8000-000000009998").expect("other document id");
    other_document.fields.clear();
    assert_eq!(
        session.validate_against(&other_document),
        Err(FormSessionError::DocumentMismatch)
    );

    let mut forged = session.clone();
    forged.source_hash = "forged".to_owned();
    assert_eq!(
        forged.validate_against(&document),
        Err(FormSessionError::SourceHashMismatch)
    );
    let display = display_list(&document);
    assert_eq!(
        resolve_form_widgets_with_session(&document, &display, &forged),
        Err(FormProjectionError::Session(
            FormSessionError::SourceHashMismatch
        ))
    );
    forged = session.clone();
    forged.schema_version += 1;
    assert_eq!(
        forged.validate_against(&document),
        Err(FormSessionError::SchemaVersion)
    );
    forged = session.clone();
    forged
        .overrides
        .insert(field_id(9_999), FieldValue::text("unknown"));
    assert_eq!(
        forged.validate_against(&document),
        Err(FormSessionError::UnknownField {
            field_id: field_id(9_999),
        })
    );

    let mut read_only_document = document.clone();
    read_only_document.fields[0].required = false;
    read_only_document.fields[0].read_only = true;
    let read_only_session =
        FormSessionState::from_document(&read_only_document).expect("read-only session");
    assert_eq!(
        read_only_session.set_value(&read_only_document, &field, FieldValue::text("Alice")),
        Err(FormSessionError::ReadOnlyField {
            field_id: field.clone(),
        })
    );
    let mut forged_read_only = read_only_session.clone();
    forged_read_only
        .overrides
        .insert(field.clone(), FieldValue::text("Alice"));
    assert_eq!(
        forged_read_only.validate_against(&read_only_document),
        Err(FormSessionError::ReadOnlyField { field_id: field })
    );

    let mut overflow = session;
    overflow.generation = u64::MAX;
    assert_eq!(
        overflow.set_value(&document, &field_id(9_951), FieldValue::text("Alice")),
        Err(FormSessionError::GenerationOverflow)
    );
}

#[test]
fn invalid_deleted_and_unmapped_anchors_are_explicitly_reviewed() {
    let mut document = document_with_field();
    let node = document.content[0].id.clone();
    document.fields.push(text_field(9_953, node.clone(), 6));
    document.fields.push(text_field(9_954, node.clone(), 6));
    document.fields[1].anchor = FieldAnchorState::LegacyInvalid {
        original: position(node.clone(), 6),
        reason: flow_core::model::LegacyAnchorReason::NonGraphemeBoundary,
    };
    let deleted_node = node_id(9_999);
    document.fields[2].anchor = FieldAnchorState::TargetDeleted {
        original: position(deleted_node, 6),
        tombstone: TombstoneToken {
            command_id: CommandId::new("00000000-0000-4000-8000-000000009970").expect("command id"),
            slot: 1,
        },
    };

    let display = display_list(&document);
    let projection = resolve_form_widgets(&document, &display).expect("review projection");
    assert_eq!(projection.widgets.len(), 1);
    assert_eq!(projection.review.len(), 2);
    assert!(
        projection
            .review
            .iter()
            .any(|review| matches!(review.reason, FormWidgetReviewReason::LegacyInvalid { .. }))
    );
    assert!(
        projection
            .review
            .iter()
            .any(|review| matches!(review.reason, FormWidgetReviewReason::TargetDeleted))
    );

    let unmapped_document = document_with_field();
    let mut unmapped = display_list(&unmapped_document);
    for page in &mut unmapped.pages {
        page.items.clear();
    }
    let projection = resolve_form_widgets(&unmapped_document, &unmapped).expect("unmapped review");
    assert!(projection.widgets.is_empty());
    assert_eq!(projection.review.len(), 1);
    assert_eq!(projection.review[0].field_id, field_id(9_951));
    assert_eq!(projection.review[0].source_node_id, node_id(9_901));
    assert_eq!(
        projection.review[0].reason,
        FormWidgetReviewReason::SourceMappingMissing
    );
}

#[test]
fn page_geometry_uses_fixed_point_display_bounds() {
    let document = document_with_field();
    let display = display_list(&document);
    let projection = resolve_form_widgets(&document, &display).expect("projection");
    let page = &display.pages[0];
    let widget = &projection.widgets[0];
    assert!(widget.rect.x >= page.bounds.x);
    assert!(widget.rect.y >= page.bounds.y);
    assert!(
        widget.rect.x.raw() + widget.rect.width.raw()
            <= page.bounds.x.raw() + page.bounds.width.raw()
    );
    assert!(
        widget.rect.y.raw() + widget.rect.height.raw()
            <= page.bounds.y.raw() + page.bounds.height.raw()
    );
}

#[test]
fn form_session_protocol_is_immutable_and_source_bound() {
    let document = document_with_field();
    let canonical_json = String::from_utf8(canonical_bytes(&document).expect("canonical bytes"))
        .expect("canonical UTF-8");
    let start = apply_form_session(FormSessionRequest {
        protocol_version: flow_core::FORM_SESSION_PROTOCOL_VERSION,
        canonical_json: canonical_json.clone(),
        session: None,
        action: FormSessionAction::Start,
    });
    assert!(start.ok);
    let session = start.value.expect("started session").session;
    assert_eq!(session.generation, 0);

    let field = document.fields[0].id.clone();
    let filled = apply_form_session(FormSessionRequest {
        protocol_version: flow_core::FORM_SESSION_PROTOCOL_VERSION,
        canonical_json: canonical_json.clone(),
        session: Some(session.clone()),
        action: FormSessionAction::SetValue {
            field_id: field.clone(),
            value: FieldValue::text("Alice"),
        },
    });
    assert!(filled.ok);
    let filled = filled.value.expect("filled session").session;
    assert_eq!(filled.generation, 1);
    assert_eq!(
        filled.value_for(&document, &field).expect("value"),
        FieldValue::text("Alice")
    );
    assert_eq!(document.revision, 1);

    let cleared = apply_form_session(FormSessionRequest {
        protocol_version: flow_core::FORM_SESSION_PROTOCOL_VERSION,
        canonical_json: canonical_json.clone(),
        session: Some(filled.clone()),
        action: FormSessionAction::ClearValue {
            field_id: field.clone(),
        },
    });
    assert!(cleared.ok);
    let cleared = cleared.value.expect("cleared session").session;
    assert_eq!(cleared.generation, 2);
    assert!(cleared.overrides().is_empty());

    let mut stale = filled;
    stale.source_revision += 1;
    let rejected = apply_form_session(FormSessionRequest {
        protocol_version: flow_core::FORM_SESSION_PROTOCOL_VERSION,
        canonical_json,
        session: Some(stale),
        action: FormSessionAction::Validate,
    });
    assert!(!rejected.ok);
    assert_eq!(
        rejected.error.expect("stale error").code,
        "FLOW_FORM_SESSION_REVISION_STALE"
    );

    let unexpected = apply_form_session(FormSessionRequest {
        protocol_version: flow_core::FORM_SESSION_PROTOCOL_VERSION,
        canonical_json: String::from_utf8(canonical_bytes(&document).expect("canonical bytes"))
            .expect("canonical UTF-8"),
        session: Some(cleared),
        action: FormSessionAction::Start,
    });
    assert!(!unexpected.ok);
    assert_eq!(
        unexpected.error.expect("unexpected session error").code,
        "FLOW_FORM_SESSION_UNEXPECTED"
    );
}

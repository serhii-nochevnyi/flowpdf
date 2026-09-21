use flow_core::{
    FormProjectionError, FormValueErrorCode, FormWidgetReviewReason, PdfDisplayList,
    build_display_list,
    layout::{FontCatalog, FontFace, LayoutUnit, PaginationRequest},
    model::{
        Affinity, CommandId, ContentNode, FieldAnchorState, FieldDescriptor, FieldId, FieldKind,
        FieldOption, FieldOptionId, FieldValue, FlowDocument, LogicalPosition, NodeId,
        TextInputHint, TombstoneToken,
    },
    paginate_document, resolve_form_widgets, validate_field_value,
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

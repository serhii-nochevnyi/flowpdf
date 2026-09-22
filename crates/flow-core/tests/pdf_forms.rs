use flow_core::{
    FormSessionState, FormValueErrorCode, PdfError, PdfFormError, PdfFormPlan, PdfPagePlan,
    PdfRecoveryExpectation, PdfSupportReport, build_display_list, build_pdf_form_plan,
    canonical::{canonical_bytes, canonical_hash},
    export_pdf,
    layout::{FontCatalog, FontFace, LayoutUnit, PaginationRequest},
    model::{Affinity, ContentNode, FieldAnchorState, FlowDocument, LogicalPosition, NodeId},
    paginate_document,
    pdf::{PdfExportOptions, PdfExportRequest, PdfSupportedFeature, PdfUnsupportedFeature},
    recover_owned_source, resolve_form_widgets, resolve_form_widgets_with_session,
};

const NOTO_SANS: &[u8] = include_bytes!("../data/NotoSans-Regular.ttf");

fn node_id(value: u32) -> NodeId {
    NodeId::new(format!("00000000-0000-4000-8000-{value:012}")).expect("valid node id")
}

fn catalog() -> FontCatalog {
    FontCatalog::new(vec![
        FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).expect("font face"),
    ])
    .expect("font catalog")
}

fn document() -> FlowDocument {
    let mut document = FlowDocument::deterministic_sample("en-US").expect("sample document");
    let node = node_id(9_901);
    let style_id = document.styles.first().map(|style| style.id.clone());
    document.content = vec![ContentNode::paragraph(
        node.clone(),
        style_id,
        "Hello form fields for the owned PDF envelope.".to_owned(),
    )];
    document.assets.clear();
    for field in &mut document.fields {
        field.anchor = FieldAnchorState::GraphemeSafe {
            original: LogicalPosition {
                node_id: node.clone(),
                utf16_offset: 6.into(),
                affinity: Affinity::Forward,
            },
        };
    }
    document
}

fn projection(document: &FlowDocument) -> flow_core::FormWidgetProjection {
    let request = PaginationRequest::for_document(document).expect("pagination request");
    let pagination = paginate_document(document, &request, &catalog(), None).expect("pagination");
    let display =
        build_display_list(document, &pagination, &catalog(), None).expect("display list");
    resolve_form_widgets(document, &display).expect("form projection")
}

fn session_projection(
    document: &FlowDocument,
) -> (
    flow_core::PdfDisplayList,
    FormSessionState,
    flow_core::FormWidgetProjection,
) {
    let request = PaginationRequest::for_document(document).expect("pagination request");
    let pagination = paginate_document(document, &request, &catalog(), None).expect("pagination");
    let display =
        build_display_list(document, &pagination, &catalog(), None).expect("display list");
    let session = FormSessionState::from_document(document).expect("form session");
    let field_id = document.fields[0].id.clone();
    let session = session
        .set_value(
            document,
            &field_id,
            flow_core::model::FieldValue::text("Alice"),
        )
        .expect("fill value");
    let projection = resolve_form_widgets_with_session(document, &display, &session)
        .expect("session projection");
    (display, session, projection)
}

fn bare_request(document: &FlowDocument, page_count: usize) -> PdfExportRequest {
    let pages = (0..page_count)
        .map(|_| PdfPagePlan::empty(LayoutUnit::from_raw(100_000), LayoutUnit::from_raw(100_000)))
        .collect::<Result<Vec<_>, _>>()
        .expect("page plan");
    let source_hash = canonical_hash(&canonical_bytes(document).expect("canonical bytes"));
    PdfExportRequest::new(
        document.revision,
        source_hash,
        "layout-identity",
        pages,
        PdfExportOptions::default(),
    )
    .expect("export request")
}

fn request(document: &FlowDocument, plan: PdfFormPlan, page_count: usize) -> PdfExportRequest {
    bare_request(document, page_count)
        .with_form_plan(plan)
        .expect("form plan identity")
}

fn rehash_plan(mut plan: PdfFormPlan) -> PdfFormPlan {
    plan.result_hash.clear();
    let bytes = serde_json::to_vec(&plan).expect("form plan bytes");
    plan.result_hash = blake3::hash(&bytes).to_hex().to_string();
    plan
}

#[test]
fn form_plan_maps_all_current_field_kinds_and_export_is_deterministic() {
    let document = document();
    let projection = projection(&document);
    assert_eq!(projection.widgets.len(), 6);
    let plan = build_pdf_form_plan(&document, &projection).expect("form plan");
    let repeat = build_pdf_form_plan(&document, &projection).expect("repeat form plan");
    assert_eq!(plan, repeat);
    assert_eq!(plan.fields.len(), 6);
    assert!(
        plan.fields
            .iter()
            .any(|field| { matches!(field.field_type, flow_core::PdfFormFieldType::Text) })
    );
    assert!(plan.fields.iter().any(|field| {
        matches!(field.field_type, flow_core::PdfFormFieldType::Button) && !field.options.is_empty()
    }));
    assert!(
        plan.fields
            .iter()
            .any(|field| matches!(field.field_type, flow_core::PdfFormFieldType::Choice))
    );
    assert!(
        plan.fields
            .iter()
            .any(|field| matches!(field.field_type, flow_core::PdfFormFieldType::Signature))
    );
    let mut tab_orders = plan
        .fields
        .iter()
        .map(|field| field.tab_order)
        .collect::<Vec<_>>();
    tab_orders.sort_unstable();
    assert_eq!(tab_orders, (0..6).collect::<Vec<_>>());
    assert!(!plan.result_hash.is_empty());

    let request = request(&document, plan, 1)
        .with_source_document(&document)
        .expect("source");
    let first = export_pdf(&request).expect("form export");
    let second = export_pdf(&request).expect("repeat form export");
    assert_eq!(first, second);
    assert!(
        first
            .support_report
            .supported
            .contains(&PdfSupportedFeature::FormFields)
    );
    assert!(
        !first
            .support_report
            .unsupported
            .contains(&PdfUnsupportedFeature::FormFields)
    );
    let pdf = String::from_utf8_lossy(&first.bytes);
    assert!(pdf.contains("/AcroForm"));
    assert!(pdf.contains("/Widget"));
    assert!(pdf.contains("/Kids"));
    assert!(pdf.contains("/FT"));
    assert!(pdf.contains("/T"));
    assert!(pdf.contains("sample-name"));
    assert_eq!(pdf.matches("/AP").count(), 6);
    assert!(pdf.contains("/NeedAppearances false"));
    assert!(pdf.contains("/DA"));
    assert!(pdf.contains("/DR"));
    assert!(pdf.contains("/Subtype /Form"));
    assert!(pdf.contains("/BaseFont /Helvetica"));
    assert!(pdf.contains("/Off"));
    assert!(pdf.contains("/Yes"));
    assert!(pdf.contains("/flow-option-"));
    assert!(pdf.contains("<FEFF"));
}

#[test]
fn session_values_change_form_export_identity_without_moving_widgets() {
    let document = document();
    let default_projection = projection(&document);
    let (display, session, filled_projection) = session_projection(&document);
    let default_plan = build_pdf_form_plan(&document, &default_projection).expect("default plan");
    let filled_plan = build_pdf_form_plan(&document, &filled_projection).expect("filled plan");
    assert_ne!(default_plan.result_hash, filled_plan.result_hash);
    assert_eq!(
        default_projection.widgets[0].rect,
        filled_projection.widgets[0].rect
    );
    assert_eq!(
        default_projection.widgets[0].default_value,
        filled_projection.widgets[0].default_value
    );
    assert_ne!(
        default_projection.widgets[0].value,
        filled_projection.widgets[0].value
    );

    let default_request = request(&document, default_plan, display.pages.len())
        .with_source_document(&document)
        .expect("default request");
    let filled_request = request(&document, filled_plan, display.pages.len())
        .with_source_document(&document)
        .expect("filled request");
    let default_export = export_pdf(&default_request).expect("default export");
    let filled_export = export_pdf(&filled_request).expect("filled export");
    assert_ne!(default_export.byte_hash, filled_export.byte_hash);
    assert_eq!(default_export.source_hash, filled_export.source_hash);
    assert!(!session.overrides().is_empty());
}

#[test]
fn explicit_flattening_is_deterministic_partial_and_source_bound() {
    let document = document();
    let plan = build_pdf_form_plan(&document, &projection(&document)).expect("plan");
    let selected = plan.fields[0].field_id.as_str().to_owned();

    let mut partial_request = request(&document, plan.clone(), 1)
        .with_source_document(&document)
        .expect("source");
    partial_request.options.flattened_field_ids = vec![selected.clone()];
    let partial = export_pdf(&partial_request).expect("partial flatten export");
    let repeat = export_pdf(&partial_request).expect("repeat partial flatten export");
    let default_request = request(&document, plan.clone(), 1)
        .with_source_document(&document)
        .expect("default source");
    let default = export_pdf(&default_request).expect("default export");
    assert_eq!(partial, repeat);
    assert_ne!(partial.byte_hash, default.byte_hash);
    let partial_pdf = String::from_utf8_lossy(&partial.bytes);
    assert!(partial_pdf.contains("/AcroForm"));
    assert_eq!(partial_pdf.matches("/AP").count(), 5);
    assert!(partial_pdf.contains("1 0 0 1"));
    assert!(partial_pdf.contains("/Helv"));
    assert_eq!(partial.manifest.options.flattened_field_ids, vec![selected]);

    let recovered = recover_owned_source(
        Some(&partial.private_source_stream),
        &PdfRecoveryExpectation::default(),
    )
    .expect("recover flattened source");
    assert!(recovered.exact);
    assert_eq!(recovered.canonical_hash, partial.source_hash);
    assert_eq!(recovered.document, document);

    let mut all_request = request(&document, plan.clone(), 1)
        .with_source_document(&document)
        .expect("source");
    all_request.options.flattened_field_ids = plan
        .fields
        .iter()
        .map(|field| field.field_id.as_str().to_owned())
        .rev()
        .collect();
    let all = export_pdf(&all_request).expect("all flatten export");
    let all_pdf = String::from_utf8_lossy(&all.bytes);
    assert!(!all_pdf.contains("/AcroForm"));
    assert!(!all_pdf.contains("/Widget"));
    assert!(!all_pdf.contains("/AP"));
    assert!(all_pdf.contains("1 0 0 1"));
    assert!(all_pdf.contains("/Helv"));
}

#[test]
fn flattening_rejects_ambiguous_or_unbound_selections_before_export() {
    let document = document();
    let plan = build_pdf_form_plan(&document, &projection(&document)).expect("plan");
    let selected = plan.fields[0].field_id.as_str().to_owned();

    let mut no_plan = bare_request(&document, 1);
    no_plan.options.flattened_field_ids = vec![selected.clone()];
    assert_eq!(
        export_pdf(&no_plan),
        Err(PdfError::Forms(PdfFormError::FlatteningRequiresPlan))
    );

    let mut duplicate = request(&document, plan.clone(), 1);
    duplicate.options.flattened_field_ids = vec![selected.clone(), selected];
    assert_eq!(
        export_pdf(&duplicate),
        Err(PdfError::Forms(PdfFormError::InvalidFlatteningSelection))
    );

    let mut unknown = request(&document, plan.clone(), 1);
    unknown.options.flattened_field_ids = vec!["missing-field".to_owned()];
    assert_eq!(
        export_pdf(&unknown),
        Err(PdfError::Forms(PdfFormError::InvalidFlatteningSelection))
    );

    let mut oversized = request(&document, plan, 1);
    oversized.options.flattened_field_ids = vec!["field".to_owned(); 2_049];
    assert_eq!(
        export_pdf(&oversized),
        Err(PdfError::Forms(PdfFormError::FieldLimit))
    );
}

#[test]
fn form_export_rejects_review_mismatch_and_page_geometry_before_publish() {
    let document = document();
    let mut review_projection = projection(&document);
    review_projection.review.push(flow_core::FormWidgetReview {
        field_id: document.fields[0].id.clone(),
        source_node_id: document.fields[0].anchor.original().node_id.clone(),
        reason: flow_core::FormWidgetReviewReason::SourceMappingMissing,
    });
    assert_eq!(
        build_pdf_form_plan(&document, &review_projection),
        Err(PdfFormError::ReviewRequired)
    );

    let mut forged_projection = projection(&document);
    forged_projection.source_hash = "forged".to_owned();
    assert_eq!(
        build_pdf_form_plan(&document, &forged_projection),
        Err(PdfFormError::SourceHashMismatch)
    );

    let valid_projection = projection(&document);
    let mut plan = build_pdf_form_plan(&document, &valid_projection).expect("plan");
    plan.fields[0].page_index = 1;
    let export = request(&document, rehash_plan(plan), 1);
    assert_eq!(
        export_pdf(&export),
        Err(PdfError::Forms(PdfFormError::PageOutOfRange))
    );

    let mut plan = build_pdf_form_plan(&document, &valid_projection).expect("plan");
    plan.fields[0].rect.x = LayoutUnit::from_raw(100_000);
    let export = request(&document, rehash_plan(plan), 1);
    assert_eq!(
        export_pdf(&export),
        Err(PdfError::Forms(PdfFormError::InvalidGeometry))
    );

    let valid_plan = build_pdf_form_plan(&document, &valid_projection).expect("plan");
    let mut forged = valid_plan.clone();
    forged.result_hash = "forged".to_owned();
    assert_eq!(
        bare_request(&document, 1).with_form_plan(forged),
        Err(PdfError::Forms(PdfFormError::Serialization))
    );

    let mut oversized = valid_plan.clone();
    oversized.fields[0].value = flow_core::PdfFormValue::Text {
        value: "x".repeat(64 * 1024 + 1),
    };
    assert_eq!(
        bare_request(&document, 1).with_form_plan(rehash_plan(oversized)),
        Err(PdfError::Forms(PdfFormError::InvalidValue(
            FormValueErrorCode::TextSizeLimit,
        )))
    );

    let mut too_many_fields = valid_plan.clone();
    too_many_fields
        .fields
        .resize(2_049, valid_plan.fields[0].clone());
    assert_eq!(
        bare_request(&document, 1).with_form_plan(too_many_fields),
        Err(PdfError::Forms(PdfFormError::FieldLimit))
    );

    let mut too_many_options = valid_plan;
    let option_field = too_many_options
        .fields
        .iter_mut()
        .find(|field| !field.options.is_empty())
        .expect("option field");
    let option = option_field.options[0].clone();
    option_field.options.resize(4_097, option);
    assert_eq!(
        bare_request(&document, 1).with_form_plan(too_many_options),
        Err(PdfError::Forms(PdfFormError::OptionLimit))
    );
}

#[test]
fn support_report_without_forms_remains_unchanged() {
    let report = PdfSupportReport::current();
    assert!(!report.supported.contains(&PdfSupportedFeature::FormFields));
    assert!(
        report
            .unsupported
            .contains(&PdfUnsupportedFeature::FormFields)
    );
}

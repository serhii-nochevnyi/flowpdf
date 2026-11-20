use flow_core::pdf::native::{
    NATIVE_MAX_FIELD_NAME_BYTES, NativeEditLimits, NativeFieldCommand, NativeFieldRefusal,
    NativePageCommand, NativePdfSession, NativeRequestLookup, NativeSessionError,
};
use flow_core::pdf::{PdfReaderLimits, PdfSceneElement, PdfSceneRequest, open_pdf, read_pdf_scene};
use serde_json::json;

fn build_pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n", index + 1).bytes());
        pdf.extend_from_slice(object);
        pdf.extend(b"\nendobj\n");
    }
    let xref = pdf.len();
    let size = offsets.len() + 1;
    pdf.extend(format!("xref\n0 {size}\n0000000000 65535 f \n").bytes());
    for offset in offsets {
        pdf.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    pdf.extend(
        format!("trailer\n<< /Root 1 0 R /Size {size} >>\nstartxref\n{xref}\n%%EOF\n").bytes(),
    );
    pdf
}

fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut object = format!("<< /Length {} {entries} >>\nstream\n", data.len()).into_bytes();
    object.extend_from_slice(data);
    object.extend(b"\nendstream");
    object
}

fn session(bytes: &[u8]) -> NativePdfSession {
    NativePdfSession::open_scoped(bytes, NativeEditLimits::default(), [7; 16]).unwrap()
}

fn apply(session: &mut NativePdfSession, command: serde_json::Value) -> Result<(), &'static str> {
    let command: NativeFieldCommand = serde_json::from_value(command).unwrap();
    let snapshot = session.snapshot();
    session
        .apply_field(snapshot.binding, snapshot.consumed_request + 1, &command)
        .map(|_| ())
        .map_err(|error| error.code())
}

fn assert_saved_value(bytes: &[u8], name: &str, kind: &str, expected: &str) {
    let reader = open_pdf(bytes, Default::default()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    let field = scene.pages[0]
        .elements
        .iter()
        .find_map(|element| match element {
            PdfSceneElement::Form { value } if value.name.as_deref() == Some(name) => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing field {name}"));
    assert_eq!(field.field_type.as_deref(), Some(kind));
    assert_eq!(field.value.as_deref(), Some(expected));
}

fn imported_checkbox(geometry: &str, extra_page_entries: &str, state: &str) -> Vec<u8> {
    imported_field(geometry, extra_page_entries, state, "Btn", "", "")
}

fn imported_field(
    geometry: &str,
    extra_page_entries: &str,
    state: &str,
    kind: &str,
    extra_form_entries: &str,
    extra_owner_entries: &str,
) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm 5 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] >>",
        &format!("<< /Type /Page /Parent 2 0 R /Annots [7 0 R] {extra_owner_entries} >>"),
        &format!("<< /Type /Page /Parent 2 0 R {extra_page_entries} >>"),
        &format!("<< /Fields [6 0 R] {extra_form_entries} >>"),
        &format!("<< /FT /{kind} /T (agreement) /Kids [7 0 R] /V /Off /DV /Off >>"),
        &format!("<< /Type /Annot /Subtype /Widget /P 3 0 R /Parent 6 0 R /Rect {geometry} /AP << /N << /Off 8 0 R /Agreed 9 0 R >> >> /AS /{state} >>"),
    ].into_iter().map(|s| s.as_bytes().to_vec()).collect();
    let form = "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources <<>>";
    objects.push(stream(
        form,
        b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\nQ\n",
    ));
    objects.push(stream(
        form,
        b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\n2 w 2 2 m 18 18 l 2 18 m 18 2 l S\nQ\n",
    ));
    build_pdf(&objects)
}

#[test]
fn imported_checkbox_uses_parent_value_and_keeps_its_on_state_and_default() {
    let original = imported_checkbox("[10 10 30 30]", "", "Off");
    let mut pdf = session(&original);
    assert_eq!(pdf.snapshot().fields[0].checked, Some(false));
    assert_saved_value(&original, "agreement", "Btn", "Off");
    let field = pdf.snapshot().fields[0].id;
    apply(
        &mut pdf,
        json!({"kind":"setFieldChecked","field":field,"checked":true}),
    )
    .unwrap();

    let bytes = pdf.output_bytes();
    let serialized = String::from_utf8_lossy(bytes);
    assert!(serialized.contains("/AS /Agreed"));
    assert!(serialized.contains("/V /Agreed"));
    assert!(serialized.contains("/DV /Off"));
    assert!(serialized.contains("/Kids [7 0 R"));
    assert!(serialized.contains("/Parent 6 0 R"));
    let reopened = session(bytes);
    assert_eq!(reopened.snapshot().fields[0].checked, Some(true));
    assert_saved_value(bytes, "agreement", "Btn", "Agreed");

    let id = pdf.snapshot().fields[0].id;
    apply(
        &mut pdf,
        json!({"kind":"moveField","field":id,"page":1,"rect":[45,60,75,90]}),
    )
    .unwrap();
    assert_eq!(pdf.snapshot().fields[0].page, 1);
    assert_eq!(pdf.snapshot().fields[0].rect, Some([45, 60, 75, 90]));
    let id = pdf.snapshot().fields[0].id;
    apply(&mut pdf, json!({"kind":"removeField","field":id})).unwrap();
    assert!(pdf.snapshot().fields.is_empty());
    assert!(!String::from_utf8_lossy(pdf.output_bytes()).contains("/Subtype /Widget"));
    assert_eq!(pdf.origin_bytes(), original);
}

#[test]
fn checkbox_toggle_and_create_accept_inactive_acroform_settings() {
    for settings in [
        "",
        "/SigFlags 0 /CO [] /NeedAppearances false",
        "/SigFlags null /CO null /NeedAppearances null",
    ] {
        let bytes = imported_field("[10 10 30 30]", "", "Off", "Btn", settings, "");
        let mut pdf = session(&bytes);
        let field = pdf.snapshot().fields[0].id;
        apply(
            &mut pdf,
            json!({"kind":"setFieldChecked","field":field,"checked":true}),
        )
        .unwrap();
        apply(
            &mut pdf,
            json!({"kind":"addCheckboxField","page":0,"rect":[40,40,60,60],"name":"approval","checked":false}),
        )
        .unwrap();

        let reopened = session(pdf.output_bytes());
        let fields = &reopened.snapshot().fields;
        assert_eq!(fields.len(), 2);
        assert_saved_value(pdf.output_bytes(), "agreement", "Btn", "Agreed");
        assert_saved_value(pdf.output_bytes(), "approval", "Btn", "Off");
        for (name, checked) in [("agreement", true), ("approval", false)] {
            let field = fields
                .iter()
                .find(|field| field.name.as_deref() == Some(name))
                .unwrap();
            assert_eq!(field.checked, Some(checked));
        }
    }
}

#[test]
fn forged_geometry_appearance_and_inherited_ownership_refuse_atomically() {
    for (bounds, extra, state, reason) in [
        ("[10 10 400 30]", "", "Off", NativeFieldRefusal::Geometry),
        (
            "[10 10 30 30]",
            "/Annots [7 0 R]",
            "Off",
            NativeFieldRefusal::Reference,
        ),
        (
            "[10 10 30 30]",
            "",
            "Agreed",
            NativeFieldRefusal::Appearance,
        ),
    ] {
        let bytes = imported_checkbox(bounds, extra, state);
        let mut pdf = session(&bytes);
        let entry = &pdf.snapshot().fields[0];
        assert_eq!(entry.refusal, Some(reason));
        assert!(!entry.can_edit_appearance);
        let id = entry.id;
        assert_eq!(
            apply(&mut pdf, json!({"kind":"removeField","field":id})),
            Err(reason.code())
        );
        assert_eq!(pdf.output_bytes(), bytes);
        assert_eq!(pdf.snapshot().binding.revision, 0);
    }
}

#[test]
fn read_only_appearance_retains_verified_name_without_a_setter() {
    let bytes = imported_field("[10 10 40 40]", "", "Off", "Btn", "", "");
    let mut pdf = session(&bytes);
    let view = &pdf.snapshot().fields[0];
    assert_eq!(view.name.as_deref(), Some("agreement"));
    assert_eq!(view.refusal, Some(NativeFieldRefusal::Appearance));
    assert!(!view.can_edit_appearance);
    assert_eq!(view.checked, None);
    let id = view.id;
    assert_eq!(
        apply(
            &mut pdf,
            json!({"kind":"setFieldChecked","field":id,"checked":true})
        ),
        Err(NativeFieldRefusal::Appearance.code())
    );
    assert_eq!(pdf.output_bytes(), bytes);
}

#[test]
fn public_widget_metadata_does_not_inherit_labels_from_rejected_split_fields() {
    let bytes = build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 500] /Annots [4 0 R 6 0 R] >>".to_vec(),
        b"<< /Subtype /Widget /Parent 5 0 R /Rect [10 10 30 30] >>".to_vec(),
        b"<< /FT /Tx /T (recipient) /V (saved) /Kids 7 0 R >>".to_vec(),
        b"<< /Subtype /Widget /Parent 5 0 R /Rect [40 10 60 30] /FT /Btn /V /Yes >>".to_vec(),
        b"[4 0 R 6 0 R]".to_vec(),
    ]);
    let reader = open_pdf(&bytes, Default::default()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    let [
        PdfSceneElement::Form { value: first },
        PdfSceneElement::Form { value: second },
    ] = scene.pages[0].elements.as_slice()
    else {
        panic!("expected two form widgets");
    };
    assert_eq!(first.name, None);
    assert_eq!(first.field_type, None);
    assert_eq!(first.value, None);
    assert_eq!(second.name, None);
    assert_eq!(second.field_type.as_deref(), Some("Btn"));
    assert_eq!(second.value.as_deref(), Some("Yes"));
}

#[test]
fn named_read_only_widgets_keep_their_labels_after_a_page_rewrite() {
    let form = "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources <<>>";
    let max_label = format!("<{}>", "E9".repeat(32));
    let oversized_label = format!("<{}>", "E9".repeat(33));
    let expected_max_label = "é".repeat(32);
    for (merged_name, parent_name, widget_label_entry, expected_names, refusal) in [
        (
            "(consent)",
            "(approval)",
            "",
            [Some("consent"), Some("approval")],
            NativeFieldRefusal::Profile,
        ),
        (
            "(Consent form)",
            "<FEFF04170433043E04340430>",
            "",
            [Some("Consent form"), Some("Згода")],
            NativeFieldRefusal::Profile,
        ),
        (
            "<436166E9>",
            "<FEFF04170433043E04340430>",
            "/T null",
            [Some("Café"), Some("Згода")],
            NativeFieldRefusal::Profile,
        ),
        (
            "10 0 R",
            "11 0 R",
            "/T 12 0 R",
            [Some("Café"), Some("Згода")],
            NativeFieldRefusal::Profile,
        ),
        (
            "11 0 R",
            "(approval)",
            "/T 10 0 R",
            [Some("Згода"), Some("Café")],
            NativeFieldRefusal::Profile,
        ),
        (
            "11 0 R",
            "(approval)",
            "/T 14 0 R",
            [Some("Згода"), None],
            NativeFieldRefusal::Profile,
        ),
        (
            "<EFBBBF436166C3A9>",
            "<18A080>",
            "",
            [Some("ï»¿CafÃ©"), Some("˘€•")],
            NativeFieldRefusal::Profile,
        ),
        (
            "<417F>",
            "<FEFFD800>",
            "",
            [None, None],
            NativeFieldRefusal::Reference,
        ),
        (
            max_label.as_str(),
            oversized_label.as_str(),
            "",
            [Some(expected_max_label.as_str()), None],
            NativeFieldRefusal::Profile,
        ),
    ] {
        let bytes = build_pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 500] /Annots [5 0 R 7 0 R] >>".to_vec(),
            b"<< /Fields [5 0 R 6 0 R] >>".to_vec(),
            format!("<< /Type /Annot /Subtype /Widget /FT /Btn /T {merged_name} /Ff 1 /V /Off /P 3 0 R /Rect [10 10 30 30] /AP << /N << /Off 8 0 R /Yes 9 0 R >> >> /AS /Off >>").into_bytes(),
            format!("<< /FT /Btn /T {parent_name} /Ff 1 /V /Off /Kids 13 0 R >>").into_bytes(),
            format!("<< /Type /Annot /Subtype /Widget /Parent 6 0 R /P 3 0 R /Rect [40 10 60 30] /AP << /N << /Off 8 0 R /Yes 9 0 R >> >> /AS /Off {widget_label_entry} >>").into_bytes(),
            stream(form, b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\nQ\n"),
            stream(form, b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\n2 w 2 2 m 18 18 l 2 18 m 18 2 l S\nQ\n"),
            b"<436166E9>".to_vec(),
            b"<FEFF04170433043E04340430>".to_vec(),
            b"null".to_vec(),
            b"[7 0 R]".to_vec(),
            b"true".to_vec(),
        ]);
        let mut pdf = session(&bytes);
        let assert_fields = |pdf: &NativePdfSession| {
            let fields = &pdf.snapshot().fields;
            assert_eq!(fields.len(), expected_names.len());
            for (field, name) in fields.iter().zip(expected_names) {
                assert_eq!(field.name.as_deref(), name);
                assert_eq!(field.refusal, Some(refusal));
                assert!(!field.can_edit_appearance);
                assert_eq!(field.kind, None);
                assert_eq!(field.value, None);
                assert_eq!(field.checked, None);
            }

            let reader = open_pdf(
                pdf.output_bytes(),
                PdfReaderLimits {
                    max_string_bytes: NATIVE_MAX_FIELD_NAME_BYTES,
                    ..Default::default()
                },
            )
            .unwrap();
            let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
            let widgets: Vec<_> = scene.pages[0]
                .elements
                .iter()
                .filter_map(|element| match element {
                    PdfSceneElement::Form { value } => Some(value),
                    _ => None,
                })
                .collect();
            assert_eq!(widgets.len(), fields.len());
            for (widget, field) in widgets.into_iter().zip(fields) {
                assert_eq!(widget.provenance.object_ref, field.object);
                assert_eq!(widget.name, field.name);
            }
        };
        assert_fields(&pdf);
        for index in 0..expected_names.len() {
            let field = pdf.snapshot().fields[index].id;
            assert_eq!(
                apply(
                    &mut pdf,
                    json!({"kind":"setFieldChecked","field":field,"checked":true})
                ),
                Err(refusal.code())
            );
        }
        assert_eq!(pdf.output_bytes(), bytes);

        let snapshot = pdf.snapshot();
        let command: NativePageCommand = serde_json::from_value(json!({
            "kind": "rotatePage", "degrees": 90,
            "target": {"page": 0, "stableId": snapshot.page_state.order[0]}
        }))
        .unwrap();
        pdf.apply_page(snapshot.binding, snapshot.consumed_request + 1, &command)
            .unwrap();
        assert!(pdf.output_bytes().starts_with(b"%PDF-1.7\n"));
        assert_fields(&pdf);
        assert_fields(&session(pdf.output_bytes()));
    }
}

#[test]
fn supported_checkbox_stays_editable_alongside_read_only_field_names() {
    let form = "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources <<>>";
    let long_name = format!("({})", "a".repeat(65));
    for (read_only_name, label, collision) in [
        ("(Consent form)", Some("Consent form"), false),
        ("<436166E9>", Some("Café"), false),
        ("<EFBBBF617070726F76616C>", Some("ï»¿approval"), false),
        ("<FEFFD83DDE00>", Some("😀"), false),
        (long_name.as_str(), None, false),
        ("<FEFF0041000A0042>", None, false),
        (
            "<FEFF0061007000700072006F00760061006C>",
            Some("approval"),
            true,
        ),
    ] {
        let bytes = build_pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 500] /Annots [5 0 R 6 0 R] >>".to_vec(),
            b"<< /Fields [5 0 R 6 0 R] >>".to_vec(),
            b"<< /Type /Annot /Subtype /Widget /FT /Btn /T (approval) /V /Off /P 3 0 R /Rect [10 10 30 30] /AP << /N << /Off 7 0 R /Yes 8 0 R >> >> /AS /Off >>".to_vec(),
            format!("<< /Type /Annot /Subtype /Widget /FT /Btn /T {read_only_name} /Ff 1 /V /Off /P 3 0 R /Rect [40 10 60 30] /AP << /N << /Off 7 0 R /Yes 8 0 R >> >> /AS /Off >>").into_bytes(),
            stream(form, b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\nQ\n"),
            stream(form, b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\n2 w 2 2 m 18 18 l 2 18 m 18 2 l S\nQ\n"),
        ]);
        let mut pdf = session(&bytes);
        let fields = &pdf.snapshot().fields;
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[1].name.as_deref(), label);
        assert!(!fields[1].can_edit_appearance);
        let command = json!({"kind":"setFieldChecked","field":fields[0].id,"checked":true});
        if collision {
            assert_eq!(fields[0].refusal, Some(NativeFieldRefusal::Reference));
            assert_eq!(
                apply(&mut pdf, command),
                Err(NativeFieldRefusal::Reference.code())
            );
            assert_eq!(pdf.output_bytes(), bytes);
            continue;
        }

        assert_eq!(fields[0].checked, Some(false));
        assert_eq!(fields[1].refusal, Some(NativeFieldRefusal::Profile));
        let read_only = fields[1].id;
        assert_eq!(
            apply(
                &mut pdf,
                json!({"kind":"setFieldChecked","field":read_only,"checked":true}),
            ),
            Err(NativeFieldRefusal::Profile.code())
        );
        assert_eq!(pdf.output_bytes(), bytes);
        apply(&mut pdf, command).unwrap();
        let reopened = session(pdf.output_bytes());
        let fields = &reopened.snapshot().fields;
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name.as_deref(), Some("approval"));
        assert_eq!(fields[0].checked, Some(true));
        assert_eq!(fields[1].name.as_deref(), label);
        assert_eq!(fields[1].refusal, Some(NativeFieldRefusal::Profile));
    }
}

#[test]
fn form_name_inventory_uses_the_live_projection_and_command_budgets() {
    let long_name = "a".repeat(24 * 1024);
    let bytes = build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 500] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [5 0 R 6 0 R] >>".to_vec(),
        b"<< /Fields [5 0 R 6 0 R] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 1 /T (reserved) /P 3 0 R /Rect [10 10 30 30] >>".to_vec(),
        format!("<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 1 /T ({long_name}) /P 3 0 R /Rect [40 10 60 30] >>").into_bytes(),
    ]);
    let add: NativeFieldCommand = serde_json::from_value(json!({
        "kind": "addCheckboxField", "page": 0, "rect": [70,10,90,30],
        "name": "reserved", "checked": false,
    }))
    .unwrap();
    let mut roomy = session(&bytes);
    assert_eq!(
        roomy.apply_field(roomy.snapshot().binding, 1, &add),
        Err(NativeSessionError::Field(NativeFieldRefusal::Reference))
    );
    assert!(matches!(
        NativePdfSession::open_scoped(
            &bytes,
            NativeEditLimits {
                max_retained_bytes: 128 * 1024,
                ..NativeEditLimits::default()
            },
            [7; 16],
        ),
        Err(NativeSessionError::Budget)
    ));

    let limits = NativeEditLimits {
        max_retained_bytes: 256 * 1024,
        ..NativeEditLimits::default()
    };
    let mut pdf = NativePdfSession::open_scoped(&bytes, limits, [7; 16]).unwrap();
    let accepted = pdf.snapshot().clone();
    assert_eq!(accepted.fields[0].name.as_deref(), Some("reserved"));
    assert!(!accepted.fields[0].can_edit_appearance);
    assert_eq!(accepted.fields[1].name, None);
    assert_eq!(
        pdf.apply_field(accepted.binding, 1, &add),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(pdf.snapshot().binding, accepted.binding);
    assert_eq!(pdf.snapshot().consumed_request, accepted.consumed_request);
    assert_eq!(pdf.snapshot().fields, accepted.fields);
    assert_eq!(
        pdf.lookup(accepted.binding, 1),
        Ok(NativeRequestLookup::Unseen)
    );
    assert_eq!(pdf.output_bytes(), bytes);
}

#[test]
fn read_only_label_does_not_shadow_an_editable_field_name() {
    let form = "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources <<>>";
    let bytes = build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 500] /Annots [5 0 R 7 0 R] >>".to_vec(),
        b"<< /Fields [5 0 R 6 0 R] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /FT /Btn /T (consent) /V /Off /P 3 0 R /Rect [10 10 30 30] /AP << /N << /Off 8 0 R /Yes 9 0 R >> >> /AS /Off >>".to_vec(),
        b"<< /FT /Btn /T (preferences) /Kids [7 0 R] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /Parent 6 0 R /T (consent) /Ff 1 /V /Off /P 3 0 R /Rect [40 10 60 30] /AP << /N << /Off 8 0 R /Yes 9 0 R >> >> /AS /Off >>".to_vec(),
        stream(form, b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\nQ\n"),
        stream(form, b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\n2 w 2 2 m 18 18 l 2 18 m 18 2 l S\nQ\n"),
    ]);
    let mut pdf = session(&bytes);
    let fields = &pdf.snapshot().fields;
    assert_eq!(fields.len(), 2);
    assert!(
        fields
            .iter()
            .all(|field| field.name.as_deref() == Some("consent"))
    );
    assert!(fields[0].can_edit_appearance);
    assert_eq!(fields[1].refusal, Some(NativeFieldRefusal::Profile));
    let field = fields[0].id;
    assert_eq!(
        apply(
            &mut pdf,
            json!({"kind":"addCheckboxField","page":0,"rect":[70,10,90,30],"name":"preferences","checked":false}),
        ),
        Err(NativeFieldRefusal::Reference.code())
    );
    assert_eq!(pdf.output_bytes(), bytes);
    apply(
        &mut pdf,
        json!({"kind":"setFieldChecked","field":field,"checked":true}),
    )
    .unwrap();
    assert_eq!(pdf.snapshot().fields[0].checked, Some(true));

    let field = pdf.snapshot().fields[0].id;
    apply(&mut pdf, json!({"kind":"removeField","field":field})).unwrap();
    let reopened = session(pdf.output_bytes());
    let fields = &reopened.snapshot().fields;
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name.as_deref(), Some("consent"));
    assert_eq!(fields[0].refusal, Some(NativeFieldRefusal::Profile));
}

fn embedded_appearance(field_widths: &str) -> Vec<u8> {
    let font = include_bytes!("../data/NotoSans-Regular.ttf");
    let face = ttf_parser::Face::parse(font, 0).unwrap();
    let gid = face.glyph_index('Ї').unwrap().0.to_be_bytes();
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapType 2 def\n/CMapName /Example def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n1 beginbfchar\n<0001> <0407>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend end";
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 500] /Annots [11 0 R] >>".to_vec(),
        b"<< /Fields [11 0 R] /DR << /Font << /FlowField 5 0 R >> >> >>".to_vec(),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /NotoSans-Regular /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>".to_vec(),
        format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NotoSans-Regular /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /W {field_widths} /FontDescriptor 8 0 R /CIDToGIDMap 10 0 R >>").into_bytes(),
        stream("", cmap),
        b"<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 /FontFile2 9 0 R >>".to_vec(),
        stream(&format!("/Length1 {}", font.len()), font),
        stream("", &[0, 0, gid[0], gid[1]]),
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (recipient) /V () /DV () /P 3 0 R /Rect [10 10 150 40] /AP << /N 12 0 R >> >>".to_vec(),
        stream("/Type /XObject /Subtype /Form /BBox [0 0 140 30] /Resources << /Font << /FlowField 5 0 R >> >>",
            b"q 1 g 0 0 140 30 re f 0 G 1 w 1 1 138 28 re S\nBT /FlowField 10 Tf 0 g 2 4 Td <> Tj ET\nQ\n"),
    ];
    build_pdf(&objects)
}

#[test]
fn embedded_appearance_round_trips_ukrainian_and_refuses_unmapped_glyphs() {
    let bytes = embedded_appearance("[1 [339]]");
    let mut pdf = session(&bytes);
    assert_eq!(pdf.snapshot().fields[0].value.as_deref(), Some(""));
    let id = pdf.snapshot().fields[0].id;
    apply(
        &mut pdf,
        json!({"kind":"setFieldText","field":id,"value":"Ї"}),
    )
    .unwrap();
    assert_eq!(pdf.snapshot().fields[0].value.as_deref(), Some("Ї"));
    assert_saved_value(pdf.output_bytes(), "recipient", "Tx", "Ї");
    assert!(
        pdf.output_bytes()
            .windows(b"<0001> Tj".len())
            .any(|window| window == b"<0001> Tj")
    );
    let accepted = pdf.output_bytes().to_vec();
    let id = pdf.snapshot().fields[0].id;
    assert_eq!(
        apply(
            &mut pdf,
            json!({"kind":"setFieldText","field":id,"value":"Ж"})
        ),
        Err(NativeFieldRefusal::Glyph.code())
    );
    assert_eq!(pdf.output_bytes(), accepted);
    assert_eq!(pdf.snapshot().binding.revision, 1);
}

#[test]
fn embedded_font_uses_the_live_projection_and_command_budgets() {
    let bytes = embedded_appearance("[1 [339] 2 4096 339]");
    let open = |max_retained_bytes| {
        NativePdfSession::open_scoped(
            &bytes,
            NativeEditLimits {
                max_retained_bytes,
                ..NativeEditLimits::default()
            },
            [7; 16],
        )
    };
    let set_text = |pdf: &NativePdfSession| NativeFieldCommand::SetFieldText {
        field: pdf.snapshot().fields[0].id,
        value: "Ї".into(),
    };
    let mut roomy = open(7 * 1024 * 1024).unwrap();
    let command = set_text(&roomy);
    roomy
        .apply_field(roomy.snapshot().binding, 1, &command)
        .unwrap();
    assert_saved_value(roomy.output_bytes(), "recipient", "Tx", "Ї");
    let reopened = session(roomy.output_bytes());
    assert!(reopened.snapshot().fields[0].can_edit_appearance);
    assert_eq!(reopened.snapshot().fields[0].value.as_deref(), Some("Ї"));

    assert!(matches!(
        open(2 * 1024 * 1024),
        Err(NativeSessionError::Budget)
    ));
    let mut pdf = open(5 * 1024 * 1024).unwrap();
    let accepted = pdf.snapshot().clone();
    assert!(accepted.fields[0].can_edit_appearance);
    let command = set_text(&pdf);
    assert_eq!(
        pdf.apply_field(accepted.binding, 1, &command),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(pdf.snapshot().binding, accepted.binding);
    assert_eq!(pdf.snapshot().output_hash, accepted.output_hash);
    assert_eq!(pdf.snapshot().consumed_request, accepted.consumed_request);
    assert_eq!(pdf.snapshot().fields, accepted.fields);
    assert_eq!(
        pdf.lookup(accepted.binding, 1),
        Ok(NativeRequestLookup::Unseen)
    );
    assert_eq!(pdf.output_bytes(), bytes);
}

#[test]
fn unsupported_embedded_font_keeps_the_appearance_refusal() {
    let bytes = embedded_appearance("[1 [0]]");
    let mut pdf = session(&bytes);
    let field = &pdf.snapshot().fields[0];
    assert!(!field.can_edit_appearance);
    assert_eq!(field.refusal, Some(NativeFieldRefusal::Appearance));
    let command = NativeFieldCommand::SetFieldText {
        field: field.id,
        value: "Ї".into(),
    };
    assert_eq!(
        pdf.apply_field(pdf.snapshot().binding, 1, &command),
        Err(NativeSessionError::Field(NativeFieldRefusal::Appearance))
    );
    assert_eq!(pdf.output_bytes(), bytes);
}

#[test]
fn ordinary_text_field_edit_round_trips_in_the_public_scene() {
    let mut pdf = session(&imported_checkbox("[10 10 30 30]", "", "Off"));
    apply(
        &mut pdf,
        json!({"kind":"addTextField","page":0,"rect":[40,40,160,60],"name":"recipient","value":"Original"}),
    )
    .unwrap();
    assert_saved_value(pdf.output_bytes(), "recipient", "Tx", "Original");
    let id = pdf
        .snapshot()
        .fields
        .iter()
        .find(|field| field.name.as_deref() == Some("recipient"))
        .unwrap()
        .id;
    apply(
        &mut pdf,
        json!({"kind":"setFieldText","field":id,"value":"Updated"}),
    )
    .unwrap();
    assert_saved_value(pdf.output_bytes(), "recipient", "Tx", "Updated");
}

#[test]
fn unsupported_controls_and_same_owner_aliases_remain_read_only() {
    for (kind, private_data, reason) in [
        ("Ch", "", NativeFieldRefusal::Profile),
        ("Btn", "/PrivateData 7 0 R", NativeFieldRefusal::Reference),
    ] {
        let bytes = imported_field("[10 10 30 30]", "", "Off", kind, "", private_data);
        let mut pdf = session(&bytes);
        let field = &pdf.snapshot().fields[0];
        assert_eq!(field.refusal, Some(reason));
        assert_eq!(field.name.as_deref(), Some("agreement"));
        let id = field.id;
        assert_eq!(
            apply(&mut pdf, json!({"kind":"removeField","field":id})),
            Err(reason.code())
        );
        assert_eq!(pdf.output_bytes(), bytes);
    }
}

#[test]
fn xfa_and_signature_controls_refuse_at_admission() {
    for (kind, form) in [("Btn", "/XFA (packet)"), ("Sig", "")] {
        let bytes = imported_field("[10 10 30 30]", "", "Off", kind, form, "");
        let error = NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [7; 16])
            .err()
            .expect("unsupported form must refuse");
        assert_eq!(error.code(), "FLOW_PDF_NATIVE_UNSUPPORTED");
    }
}

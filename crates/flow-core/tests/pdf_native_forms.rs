use flow_core::pdf::native::{
    NATIVE_MAX_FIELD_NAME_BYTES, NativeEditError, NativeEditLimits, NativeFieldCommand,
    NativeFieldRefusal, NativeNoteCommand, NativePageCommand, NativePdfSession,
    NativeRequestLookup, NativeSessionError,
};
use flow_core::pdf::{
    PdfReadError, PdfReaderLimits, PdfSceneElement, PdfSceneRequest, open_pdf, read_pdf_scene,
};
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
    build_pdf(&imported_field_objects(
        geometry,
        extra_page_entries,
        state,
        kind,
        extra_form_entries,
        extra_owner_entries,
    ))
}

fn imported_field_objects(
    geometry: &str,
    extra_page_entries: &str,
    state: &str,
    kind: &str,
    extra_form_entries: &str,
    extra_owner_entries: &str,
) -> Vec<Vec<u8>> {
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
    objects
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
fn checkbox_add_refuses_atomically_below_the_widget_work_boundary() {
    let mut objects = imported_field_objects("[10 10 30 30]", "", "Off", "Btn", "", "");
    objects[4] = b"<< /Fields 10 0 R >>".to_vec();
    objects.push(b"[6 0 R]".to_vec());
    let source = build_pdf(&objects);
    let fitting = apply_at_work_boundary(&source, |_| NativeFieldCommand::AddCheckboxField {
        page: 0,
        rect: [40, 40, 60, 60],
        name: "approval".into(),
        checked: true,
    });
    assert_saved_value(fitting.output_bytes(), "approval", "Btn", "Yes");
}

#[test]
fn multi_field_lookup_preserves_projection_and_refuses_atomically_at_the_work_boundary() {
    let mut objects =
        imported_field_objects("[10 10 30 30]", "/Annots [10 0 R]", "Off", "Btn", "", "");
    objects[4] = b"<< /Fields [6 0 R 10 0 R] >>".to_vec();
    objects.push(b"<< /Type /Annot /Subtype /Widget /P 4 0 R /FT /Btn /T (acceptance) /V /Off /Rect [10 10 30 30] /AP << /N << /Off 8 0 R /Agreed 9 0 R >> >> /AS /Off >>".to_vec());
    let source = build_pdf(&objects);
    let labels = ["agreement", "acceptance"];
    let fitting = apply_at_work_boundary(&source, |pdf| {
        let fields = &pdf.snapshot().fields;
        assert_eq!(fields.len(), labels.len());
        for (field, label) in fields.iter().zip(labels) {
            assert_eq!(field.name.as_deref(), Some(label));
            assert!(field.can_edit_appearance);
            assert_eq!(field.checked, Some(false));
        }
        NativeFieldCommand::SetFieldChecked {
            field: fields[0].id,
            checked: true,
        }
    });
    assert_saved_value(fitting.output_bytes(), "agreement", "Btn", "Agreed");
    let fields = &fitting.snapshot().fields;
    assert_eq!(fields.len(), labels.len());
    for ((field, label), checked) in fields.iter().zip(labels).zip([true, false]) {
        assert_eq!(field.name.as_deref(), Some(label));
        assert!(field.can_edit_appearance);
        assert_eq!(field.checked, Some(checked));
    }
    assert_eq!(fitting.origin_bytes(), source);
}

fn apply_at_work_boundary(
    source: &[u8],
    command: impl Fn(&NativePdfSession) -> NativeFieldCommand,
) -> NativePdfSession {
    let limits = NativeEditLimits::default();
    let open = |max_work| {
        NativePdfSession::open_scoped(source, NativeEditLimits { max_work, ..limits }, [7; 16])
    };
    let apply = |pdf: &mut NativePdfSession| {
        let command = command(pdf);
        pdf.apply_field(pdf.snapshot().binding, 1, &command)
    };
    let (mut low, mut high) = (1, limits.max_work);
    while low < high {
        let mid = low + (high - low) / 2;
        match open(mid).and_then(|mut pdf| apply(&mut pdf)) {
            Ok(_) => high = mid,
            Err(
                NativeSessionError::Budget
                | NativeSessionError::Graph(NativeEditError::Reader(PdfReadError::TokenLimit)),
            ) => low = mid + 1,
            Err(error) => panic!("unexpected refusal at {mid} work units: {error:?}"),
        }
    }

    let mut constrained = open(low - 1).unwrap();
    let accepted = constrained.snapshot().clone();
    assert_eq!(apply(&mut constrained), Err(NativeSessionError::Budget));
    assert_eq!(constrained.output_bytes(), source);
    assert_eq!(constrained.origin_bytes(), source);
    assert_eq!(constrained.snapshot().binding, accepted.binding);
    assert_eq!(constrained.snapshot().output_hash, accepted.output_hash);
    assert_eq!(
        constrained.snapshot().scene.scene_hash,
        accepted.scene.scene_hash
    );
    assert_eq!(constrained.snapshot().fields, accepted.fields);
    assert_eq!(
        constrained.snapshot().consumed_request,
        accepted.consumed_request
    );
    assert_eq!(
        constrained.lookup(accepted.binding, 1),
        Ok(NativeRequestLookup::Unseen)
    );

    let mut fitting = open(low).unwrap();
    let binding = fitting.snapshot().binding;
    let receipt = apply(&mut fitting).unwrap();
    assert_eq!(receipt.binding.revision, binding.revision + 1);
    assert_eq!(
        fitting.lookup(binding, 1),
        Ok(NativeRequestLookup::Accepted(receipt))
    );
    fitting
}

#[test]
fn passive_checkbox_state_names_survive_reopen_and_note_rewrite() {
    for (on, kids) in [
        ("Launch", Some("[7 0 R]")),
        ("Launch", Some("10 0 R")),
        ("Sig", None),
    ] {
        let mut objects = imported_field_objects("[10 10 30 30]", "", "Off", "Btn", "", "");
        let field = format!("/FT /Btn /T (agreement) /V /Off /DV /{on}");
        let owner = if let Some(kids) = kids {
            if kids == "10 0 R" {
                objects.push(b"[7 0 R]".to_vec());
            }
            objects[5] = format!("<< {field} /Kids {kids} >>").into_bytes();
            "/Parent 6 0 R".to_owned()
        } else {
            objects[4] = b"<< /Fields [7 0 R] >>".to_vec();
            field
        };
        objects[6] = format!(
            "<< /Type /Annot /Subtype /Widget /P 3 0 R {owner} /Rect [10 10 30 30] /AP << /N << /Off 8 0 R /{on} 9 0 R >> >> /AS /Off >>"
        ).into_bytes();
        let source = build_pdf(&objects);
        let mut pdf = session(&source);
        assert_eq!(pdf.snapshot().fields[0].checked, Some(false));
        assert_saved_value(&source, "agreement", "Btn", "Off");

        for (checked, request, state) in [(true, 1, on), (false, 3, "Off")] {
            let snapshot = pdf.snapshot();
            pdf.apply_field(
                snapshot.binding,
                request,
                &NativeFieldCommand::SetFieldChecked {
                    field: snapshot.fields[0].id,
                    checked,
                },
            )
            .unwrap();
            let saved = pdf.output_bytes();
            let serialized = String::from_utf8_lossy(saved);
            assert!(serialized.contains(&format!("/AS /{state}")));
            assert!(serialized.contains(&format!("/DV /{on}")));
            assert_eq!(session(saved).snapshot().fields[0].checked, Some(checked));
            assert_saved_value(saved, "agreement", "Btn", state);
            if checked {
                let snapshot = pdf.snapshot();
                pdf.apply(
                    snapshot.binding,
                    request + 1,
                    &NativeNoteCommand::Add {
                        page: snapshot.scene.pages[0].id,
                        rect: [50, 50, 90, 75],
                        contents: "Reviewed checkbox".into(),
                    },
                )
                .unwrap();
                let saved = pdf.output_bytes();
                assert_eq!(session(saved).snapshot().fields[0].checked, Some(true));
                assert_saved_value(saved, "agreement", "Btn", on);
            }
        }
        assert_eq!(pdf.origin_bytes(), source);
    }
}

#[test]
fn passive_checkbox_exception_requires_reciprocal_mapped_state() {
    for (owner_type, kids, field_value, appearance_state) in [
        ("Btn", "[7 0 R 11 0 R]", "Launch", "Off"),
        ("Btn", "[7 0 R]", "Launch", "Off"),
        ("Tx", "[7 0 R]", "Off", "Launch"),
    ] {
        let mut objects = imported_field_objects("[10 10 30 30]", "", "Off", "Btn", "", "");
        objects[5] = format!(
            "<< /FT /{owner_type} /T (agreement) /Kids 10 0 R /V /{field_value} /DV /Off >>"
        )
        .into_bytes();
        let on = if field_value == "Off" || kids.contains("11") {
            "Launch"
        } else {
            "Agreed"
        };
        objects[6] = format!(
            "<< /Type /Annot /Subtype /Widget /P 3 0 R /Parent 6 0 R /Rect [10 10 30 30] /AP << /N << /Off 8 0 R /{on} 9 0 R >> >> /AS /{appearance_state} >>"
        )
        .into_bytes();
        objects.push(kids.as_bytes().to_vec());
        if kids.contains("11") {
            objects.push(objects[6].clone());
        }
        assert!(matches!(
            NativePdfSession::open_scoped(
                &build_pdf(&objects),
                NativeEditLimits::default(),
                [7; 16]
            ),
            Err(NativeSessionError::Graph(NativeEditError::Unsupported))
        ));
    }
}

#[test]
fn stream_dictionary_cannot_use_the_checkbox_state_exception() {
    let mut objects = imported_field_objects("[10 10 30 30]", "", "Off", "Btn", "", "");
    objects[2] = b"<< /Type /Page /Parent 2 0 R /Annots [7 0 R] /Contents 10 0 R >>".to_vec();
    objects.push(stream("/FT /Btn /Kids [7 0 R] /V /Launch", b"q Q"));
    assert!(matches!(
        NativePdfSession::open_scoped(&build_pdf(&objects), NativeEditLimits::default(), [7; 16]),
        Err(NativeSessionError::Graph(NativeEditError::Unsupported))
    ));
}

#[test]
fn duplicate_page_keeps_indirect_form_fields_independently_editable() {
    let form = "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources <<>>";
    let source = build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 500] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [6 0 R] >>".to_vec(),
        b"<< /Fields 5 0 R >>".to_vec(),
        b"[6 0 R]".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /P 3 0 R /Rect [10 10 30 30] /FT /Btn /T (agreement) /V /Off /AS /Off /AP << /N << /Off 7 0 R /Yes 8 0 R >> >> >>".to_vec(),
        stream(form, b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\nQ\n"),
        stream(form, b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\n2 w 2 2 m 18 18 l 2 18 m 18 2 l S\nQ\n"),
    ]);
    let mut pdf = session(&source);
    let initial = pdf.snapshot();
    let target = flow_core::pdf::native::NativePageTarget {
        page: 0,
        stable_id: initial.page_state.order[0],
    };
    pdf.apply_page(
        initial.binding,
        1,
        &NativePageCommand::DuplicatePage { target },
    )
    .unwrap();
    let fields = &pdf.snapshot().fields;
    assert_eq!(fields.len(), 2);
    assert!(fields.iter().all(|field| field.can_edit_appearance));
    assert_eq!(fields[0].name.as_deref(), Some("agreement"));
    assert_ne!(fields[1].name, fields[0].name);
    assert_eq!(fields[1].page, 1);
    let copied = fields[1].id;
    apply(
        &mut pdf,
        json!({"kind":"setFieldChecked","field":copied,"checked":true}),
    )
    .unwrap();
    let reopened = session(pdf.output_bytes());
    assert_eq!(reopened.snapshot().fields[0].checked, Some(false));
    assert_eq!(reopened.snapshot().fields[1].checked, Some(true));
    assert_eq!(pdf.origin_bytes(), source);
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
fn native_and_public_widget_labels_require_consistent_split_field_children() {
    for (second_entries, expected_label, second_type, second_value) in [
        ("", Some("recipient"), None, None),
        ("/FT /Btn /V /Yes", None, Some("Btn"), Some("Yes")),
    ] {
        let bytes = build_pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] >> >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 500] /Annots [4 0 R 6 0 R] >>".to_vec(),
            b"<< /Type /Annot /Subtype /Widget /P 3 0 R /Parent 5 0 R /Rect [10 10 30 30] >>".to_vec(),
            b"<< /FT /Tx /T 8 0 R /V (saved) /Kids 7 0 R >>".to_vec(),
            format!("<< /Type /Annot /Subtype /Widget /P 3 0 R /Parent 5 0 R /Rect [40 10 60 30] {second_entries} >>").into_bytes(),
            b"[4 0 R 6 0 R]".to_vec(),
            b"(recipient)".to_vec(),
        ]);
        let native = session(&bytes);
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_string_bytes: 16,
                ..Default::default()
            },
        )
        .unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let [
            PdfSceneElement::Form { value: first },
            PdfSceneElement::Form { value: second },
        ] = scene.pages[0].elements.as_slice()
        else {
            panic!("expected two form widgets");
        };
        let fields = &native.snapshot().fields;
        assert_eq!(fields.len(), 2);
        for (field, widget) in fields.iter().zip([first, second]) {
            assert_eq!(field.object, widget.provenance.object_ref);
            assert_eq!(field.name, widget.name);
            assert_eq!(widget.name.as_deref(), expected_label);
            assert!(!field.can_edit_appearance);
            assert_eq!(field.value, None);
        }
        assert_eq!(first.field_type, None);
        assert_eq!(first.value, None);
        assert_eq!(second.field_type.as_deref(), second_type);
        assert_eq!(second.value.as_deref(), second_value);
    }
}

#[test]
fn public_split_widget_labels_share_the_ownership_work_allowance() {
    let count = 10;
    let widgets: String = (7..7 + count)
        .map(|index| format!("{index} 0 R "))
        .collect();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 500] /Annots [{widgets}] >>")
            .into_bytes(),
        b"<< /FT /Tx /T 6 0 R /V (saved) /Kids 5 0 R >>".to_vec(),
        format!("[{widgets}]").into_bytes(),
        b"(recipient)".to_vec(),
    ];
    for row in 0..count {
        let y = 10 + row * 22;
        objects.push(
            format!(
                "<< /Type /Annot /Subtype /Widget /P 3 0 R /Parent 4 0 R /Rect [10 {y} 30 {}] >>",
                y + 20
            )
            .into_bytes(),
        );
    }
    let bytes = build_pdf(&objects);
    let scene = |max_content_tokens| {
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_content_tokens,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(&reader, &PdfSceneRequest::default())
    };
    let fit = count * (count + 5);
    let exact = scene(fit).unwrap();
    assert_eq!(exact.pages[0].elements.len(), count);
    assert!(exact.pages[0].elements.iter().all(|element| {
        matches!(element, PdfSceneElement::Form { value } if
            value.name.as_deref() == Some("recipient") && value.value.is_none())
    }));
    assert_eq!(scene(fit - 1), Err(PdfReadError::TokenLimit));
}

#[test]
fn read_only_nested_widget_labels_agree_after_reopen() {
    for (roots, children, widget_entries, expected_label) in [
        ("5 0 R", "6 0 R", "", Some("recipient")),
        ("", "6 0 R", "", None),
        ("5 0 R 5 0 R", "6 0 R", "", None),
        ("5 0 R 6 0 R", "6 0 R", "", None),
        ("5 0 R 4 0 R", "6 0 R", "", None),
        ("5 0 R", "6 0 R 6 0 R", "", None),
        ("5 0 R", "", "", None),
        ("", "6 0 R", "/T (direct)", Some("direct")),
    ] {
        let bytes = build_pdf(&[
            format!("<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [{roots}] >> >>")
                .into_bytes(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 500] /Annots [4 0 R] >>".to_vec(),
            format!("<< /Type /Annot /Subtype /Widget /P 3 0 R /Parent 6 0 R /Rect [10 10 30 30] {widget_entries} >>").into_bytes(),
            format!("<< /T (section) /Kids [{children}] >>").into_bytes(),
            b"<< /Parent 5 0 R /FT /Tx /T 7 0 R /V (saved) /Kids [4 0 R] >>".to_vec(),
            b"(recipient)".to_vec(),
        ]);
        let mut native = session(&bytes);
        let assert_views = |native: &NativePdfSession| {
            let [widget] = native.snapshot().fields.as_slice() else {
                panic!("expected one native widget");
            };
            let reader = open_pdf(native.output_bytes(), Default::default()).unwrap();
            let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
            let [PdfSceneElement::Form { value }] = scene.pages[0].elements.as_slice() else {
                panic!("expected one public widget");
            };
            assert_eq!(widget.name.as_deref(), expected_label);
            assert_eq!(value.name, widget.name);
            assert_eq!(value.provenance.object_ref, widget.object);
            assert!(!widget.can_edit_appearance);
            assert_eq!(widget.value, None);
            assert_eq!(value.field_type, None);
            assert_eq!(value.value, None);
        };
        assert_views(&native);
        assert_eq!(native.output_bytes(), bytes);

        let snapshot = native.snapshot();
        let command = serde_json::from_value(json!({
            "kind": "rotatePage", "degrees": 90,
            "target": {"page": 0, "stableId": snapshot.page_state.order[0]}
        }))
        .unwrap();
        native.apply_page(snapshot.binding, 1, &command).unwrap();
        assert_views(&session(native.output_bytes()));
    }
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
        b"<< /Fields 7 0 R >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 1 /T (reserved) /P 3 0 R /Rect [10 10 30 30] >>".to_vec(),
        format!("<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 1 /T ({long_name}) /P 3 0 R /Rect [40 10 60 30] >>").into_bytes(),
        b"[5 0 R 6 0 R]".to_vec(),
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
    let add_distinct = NativeFieldCommand::AddCheckboxField {
        page: 0,
        rect: [70, 10, 90, 30],
        name: "approval".into(),
        checked: true,
    };
    assert_eq!(
        pdf.apply_field(accepted.binding, 1, &add),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(
        pdf.apply_field(accepted.binding, 1, &add_distinct),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(pdf.snapshot().binding, accepted.binding);
    assert_eq!(pdf.snapshot().output_hash, accepted.output_hash);
    assert_eq!(pdf.snapshot().scene.scene_hash, accepted.scene.scene_hash);
    assert_eq!(pdf.snapshot().consumed_request, accepted.consumed_request);
    assert_eq!(pdf.snapshot().fields, accepted.fields);
    assert_eq!(
        pdf.lookup(accepted.binding, 1),
        Ok(NativeRequestLookup::Unseen)
    );
    assert_eq!(pdf.output_bytes(), bytes);
    assert_eq!(pdf.origin_bytes(), bytes);

    let mut near_limit = NativePdfSession::open_scoped(
        &bytes,
        NativeEditLimits {
            max_retained_bytes: 512 * 1024,
            ..limits
        },
        [7; 16],
    )
    .unwrap();
    let receipt = near_limit
        .apply_field(near_limit.snapshot().binding, 1, &add_distinct)
        .unwrap();
    assert_eq!(receipt.binding.revision, 1);
    assert_eq!(near_limit.snapshot().fields.len(), 3);
    assert_saved_value(near_limit.output_bytes(), "approval", "Btn", "Yes");
    assert_eq!(near_limit.origin_bytes(), bytes);
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

fn embedded_form_objects(field_widths: &str) -> Vec<Vec<u8>> {
    let font = include_bytes!("../data/NotoSans-Regular.ttf");
    let face = ttf_parser::Face::parse(font, 0).unwrap();
    let gid = face.glyph_index('Ї').unwrap().0.to_be_bytes();
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapType 2 def\n/CMapName /Example def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n1 beginbfchar\n<0001> <0407>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend end";
    vec![
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
    ]
}

fn embedded_appearance(field_widths: &str) -> Vec<u8> {
    build_pdf(&embedded_form_objects(field_widths))
}

#[test]
fn checkbox_edit_skips_unused_embedded_default_font() {
    let mut objects = embedded_form_objects("[1 [339] 2 4096 339]");
    objects[10] = b"<< /Type /Annot /Subtype /Widget /FT /Btn /T (agreement) /V /Off /DV /Off /AS /Off /P 3 0 R /Rect [10 10 30 30] /AP << /N << /Off 12 0 R /Yes 13 0 R >> >> >>".to_vec();
    let appearance = "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources <<>>";
    objects[11] = stream(
        appearance,
        b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\nQ\n",
    );
    objects.push(stream(
        appearance,
        b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\n2 w 2 2 m 18 18 l 2 18 m 18 2 l S\nQ\n",
    ));
    let valid_text_appearance =
        &b"q 1 g 0 0 140 30 re f 0 G 1 w 1 1 138 28 re S\nBT /FlowField 10 Tf 0 g 2 4 Td <> Tj ET\nQ\n"[..];
    for read_only in [
        None,
        Some((
            "/Q 1",
            "",
            valid_text_appearance,
            NativeFieldRefusal::Profile,
        )),
        Some((
            "",
            "/Matrix [1 0 0 1 0 0]",
            valid_text_appearance,
            NativeFieldRefusal::Appearance,
        )),
        Some((
            "",
            "",
            &b"q 0 g 0 0 140 30 re f Q\n"[..],
            NativeFieldRefusal::Appearance,
        )),
    ] {
        let mut objects = objects.clone();
        if let Some((field_options, stream_options, data, _)) = read_only {
            objects[2] =
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 500] /Annots [11 0 R 14 0 R] >>"
                    .to_vec();
            objects[3] =
                b"<< /Fields [11 0 R 14 0 R] /DR << /Font << /FlowField 5 0 R >> >> >>".to_vec();
            objects.push(format!("<< /Type /Annot /Subtype /Widget /FT /Tx /T (recipient) /V () /DV () /P 3 0 R /Rect [50 50 190 80] /AP << /N 15 0 R >> {field_options} >>").into_bytes());
            objects.push(stream(&format!("/Type /XObject /Subtype /Form /BBox [0 0 140 30] /Resources << /Font << /FlowField 5 0 R >> >> {stream_options}"), data));
        }
        let bytes = build_pdf(&objects);
        let limits = NativeEditLimits {
            max_retained_bytes: 6 * 1024 * 1024,
            ..NativeEditLimits::default()
        };
        let mut pdf = NativePdfSession::open_scoped(&bytes, limits, [7; 16]).unwrap();
        let field = &pdf.snapshot().fields[0];
        assert!(field.can_edit_appearance);
        assert_eq!(field.checked, Some(false));
        let command = NativeFieldCommand::SetFieldChecked {
            field: field.id,
            checked: true,
        };
        let receipt = pdf
            .apply_field(pdf.snapshot().binding, 1, &command)
            .unwrap();
        assert_eq!(receipt.binding.revision, 1);
        let reopened = NativePdfSession::open_scoped(pdf.output_bytes(), limits, [8; 16]).unwrap();
        assert!(reopened.snapshot().fields[0].can_edit_appearance);
        assert_eq!(reopened.snapshot().fields[0].checked, Some(true));
        if let Some((_, _, _, refusal)) = read_only {
            let other = &reopened.snapshot().fields[1];
            assert_eq!(other.name.as_deref(), Some("recipient"));
            assert_eq!(other.refusal, Some(refusal));
        }
    }
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
fn saved_pdf_17_field_text_and_defaults_survive_native_rewrites() {
    let font = include_bytes!("../data/NotoSans-Regular.ttf");
    let face = ttf_parser::Face::parse(font, 0).unwrap();
    let bullet_gid = face.glyph_index('•').unwrap();
    let upem = u32::from(face.units_per_em());
    let advance = u32::from(face.glyph_hor_advance(bullet_gid).unwrap());
    let bullet_width = (advance * 1000 + upem / 2) / upem;
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapType 2 def\n/CMapName /Example def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n2 beginbfchar\n<0001> <0407>\n<0002> <2022>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend end";
    for (saved, default, operand, expected) in [
        ("<80>", "<FEFF0407>", "0002", "•"),
        ("<FEFF0407>", "<80>", "0001", "Ї"),
    ] {
        let mut objects = embedded_form_objects(&format!("[1 [339 {bullet_width}]]"));
        objects[6] = stream("", cmap);
        let mut glyphs = vec![0, 0];
        for gid in [face.glyph_index('Ї').unwrap(), bullet_gid] {
            glyphs.extend(gid.0.to_be_bytes());
        }
        objects[9] = stream("", &glyphs);
        objects[10] = format!("<< /Type /Annot /Subtype /Widget /FT /Tx /T (recipient) /V {saved} /DV {default} /P 3 0 R /Rect [10 10 150 40] /AP << /N 12 0 R >> >>").into_bytes();
        let paint = format!(
            "q 1 g 0 0 140 30 re f 0 G 1 w 1 1 138 28 re S\nBT /FlowField 10 Tf 0 g 2 4 Td <{operand}> Tj ET\nQ\n"
        );
        objects[11] = stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 140 30] /Resources << /Font << /FlowField 5 0 R >> >>",
            paint.as_bytes(),
        );
        let bytes = build_pdf(&objects);
        assert_saved_value(&bytes, "recipient", "Tx", expected);
        let assert_field = |pdf: &NativePdfSession| {
            let field = &pdf.snapshot().fields[0];
            assert!(field.can_edit_appearance, "{:?}", field.refusal);
            assert_eq!(field.value.as_deref(), Some(expected));
        };
        let mut pdf = session(&bytes);
        assert_field(&pdf);
        let snapshot = pdf.snapshot();
        pdf.apply(
            snapshot.binding,
            1,
            &NativeNoteCommand::Add {
                page: snapshot.scene.pages[0].id,
                rect: [170, 60, 190, 80],
                contents: "Review recipient".into(),
            },
        )
        .unwrap();
        assert_saved_value(pdf.output_bytes(), "recipient", "Tx", expected);

        let mut reopened = session(pdf.output_bytes());
        assert_field(&reopened);
        let field = reopened.snapshot().fields[0].id;
        apply(
            &mut reopened,
            json!({"kind":"moveField","field":field,"page":0,"rect":[15,15,155,45]}),
        )
        .unwrap();
        assert_field(&session(reopened.output_bytes()));
        assert_saved_value(reopened.output_bytes(), "recipient", "Tx", expected);
        let default_token = format!("/DV {default}");
        assert!(
            reopened
                .output_bytes()
                .windows(default_token.len())
                .any(|bytes| bytes == default_token.as_bytes())
        );
        assert_eq!(pdf.origin_bytes(), bytes);
    }
}

#[test]
fn embedded_winansi_form_text_remains_editable_after_reopening() {
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapType 2 def\n/CMapName /Example def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<41> <0041>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend end";
    for (initial, operand, replacement) in [("", "", "A"), ("A", "41", "")] {
        let mut objects = embedded_form_objects("[1 [339]]");
        objects[4] = b"<< /Type /Font /Subtype /TrueType /BaseFont /NotoSans-Regular /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 65 /Widths [639] /FontDescriptor 8 0 R /ToUnicode 7 0 R >>".to_vec();
        objects[6] = stream("", cmap);
        objects[10] = format!("<< /Type /Annot /Subtype /Widget /FT /Tx /T (recipient) /V ({initial}) /DV () /P 3 0 R /Rect [10 10 150 40] /AP << /N 12 0 R >> >>").into_bytes();
        let paint = format!(
            "q 1 g 0 0 140 30 re f 0 G 1 w 1 1 138 28 re S\nBT /FlowField 10 Tf 0 g 2 4 Td <{operand}> Tj ET\nQ\n"
        );
        objects[11] = stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 140 30] /Resources << /Font << /FlowField 5 0 R >> >>",
            paint.as_bytes(),
        );
        let bytes = build_pdf(&objects);
        let mut pdf = session(&bytes);
        let field = &pdf.snapshot().fields[0];
        assert!(field.can_edit_appearance);
        assert_eq!(field.value.as_deref(), Some(initial));
        let command = NativeFieldCommand::SetFieldText {
            field: field.id,
            value: replacement.into(),
        };
        let receipt = pdf
            .apply_field(pdf.snapshot().binding, 1, &command)
            .unwrap();
        assert_eq!(receipt.binding.revision, 1);
        let reopened = session(pdf.output_bytes());
        assert!(reopened.snapshot().fields[0].can_edit_appearance);
        assert_eq!(
            reopened.snapshot().fields[0].value.as_deref(),
            Some(replacement)
        );
        assert_eq!(pdf.origin_bytes(), bytes);
    }
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
fn ordinary_field_additions_respect_the_live_retained_allowance() {
    let source = imported_checkbox("[10 10 30 30]", "", "Off");
    let open = |max_retained_bytes| {
        NativePdfSession::open_scoped(
            &source,
            NativeEditLimits {
                max_retained_bytes,
                ..NativeEditLimits::default()
            },
            [7; 16],
        )
        .unwrap()
    };
    let text = "Approved for delivery";
    let commands = [
        (
            NativeFieldCommand::AddTextField {
                page: 0,
                rect: [40, 40, 290, 65],
                name: "decision".into(),
                value: text.into(),
            },
            "decision",
            "Tx",
            text,
            "Approved for pickup",
        ),
        (
            NativeFieldCommand::AddCheckboxField {
                page: 0,
                rect: [40, 40, 60, 60],
                name: "approval".into(),
                checked: true,
            },
            "approval",
            "Btn",
            "Yes",
            "Off",
        ),
    ];
    for (command, name, kind, value, updated) in commands {
        let mut constrained = open(64 * 1024);
        let accepted = constrained.snapshot().clone();
        assert_eq!(
            constrained.apply_field(accepted.binding, 1, &command),
            Err(NativeSessionError::Budget)
        );
        assert_eq!(constrained.snapshot().binding, accepted.binding);
        assert_eq!(constrained.snapshot().output_hash, accepted.output_hash);
        assert_eq!(
            constrained.snapshot().scene.scene_hash,
            accepted.scene.scene_hash
        );
        assert_eq!(
            constrained.snapshot().consumed_request,
            accepted.consumed_request
        );
        assert_eq!(constrained.snapshot().fields, accepted.fields);
        assert_eq!(
            constrained.lookup(accepted.binding, 1),
            Ok(NativeRequestLookup::Unseen)
        );
        assert_eq!(constrained.output_bytes(), source);
        assert_eq!(constrained.origin_bytes(), source);

        let mut fitting = open(192 * 1024);
        let receipt = fitting
            .apply_field(fitting.snapshot().binding, 1, &command)
            .unwrap();
        assert_eq!(receipt.binding.revision, 1);
        assert_saved_value(fitting.output_bytes(), name, kind, value);
        assert_eq!(fitting.origin_bytes(), source);

        let field = fitting
            .snapshot()
            .fields
            .iter()
            .find(|field| field.name.as_deref() == Some(name))
            .unwrap()
            .id;
        let update = match command {
            NativeFieldCommand::AddTextField { .. } => NativeFieldCommand::SetFieldText {
                field,
                value: updated.into(),
            },
            _ => NativeFieldCommand::SetFieldChecked {
                field,
                checked: false,
            },
        };
        let receipt = fitting.apply_field(receipt.binding, 2, &update).unwrap();
        assert_eq!(receipt.binding.revision, 2);
        assert_saved_value(fitting.output_bytes(), name, kind, updated);
        assert_eq!(fitting.origin_bytes(), source);
    }
}

fn apply_at_retained_boundary(
    source: &[u8],
    command: impl Fn(&NativePdfSession) -> NativeFieldCommand,
) -> NativePdfSession {
    let open = |max_retained_bytes| {
        NativePdfSession::open_scoped(
            source,
            NativeEditLimits {
                max_retained_bytes,
                ..NativeEditLimits::default()
            },
            [7; 16],
        )
    };
    let apply = |pdf: &mut NativePdfSession| {
        let command = command(pdf);
        pdf.apply_field(pdf.snapshot().binding, 1, &command)
    };
    let allowance_refusal = |error: &NativeSessionError| {
        matches!(
            error,
            NativeSessionError::Budget
                | NativeSessionError::Graph(NativeEditError::Reader(PdfReadError::InputSizeLimit))
        )
    };
    let (mut low, mut high) = (1, NativeEditLimits::default().max_retained_bytes);
    while low < high {
        let mid = low + (high - low) / 2;
        match open(mid).and_then(|mut pdf| apply(&mut pdf)) {
            Ok(_) => high = mid,
            Err(error) if allowance_refusal(&error) => low = mid + 1,
            Err(error) => panic!("unexpected refusal at {mid} bytes: {error:?}"),
        }
    }

    let mut constrained = open(low - 1).unwrap();
    let accepted = constrained.snapshot().clone();
    let error = apply(&mut constrained).unwrap_err();
    assert!(
        allowance_refusal(&error),
        "unexpected refusal one byte below the fitting allowance: {error:?}"
    );
    assert_eq!(constrained.output_bytes(), source);
    assert_eq!(constrained.snapshot().binding, accepted.binding);
    assert_eq!(constrained.snapshot().output_hash, accepted.output_hash);
    assert_eq!(constrained.snapshot().fields, accepted.fields);
    assert_eq!(
        constrained.snapshot().consumed_request,
        accepted.consumed_request
    );
    assert_eq!(
        constrained.lookup(accepted.binding, 1),
        Ok(NativeRequestLookup::Unseen)
    );

    let mut fitting = open(low).unwrap();
    let binding = fitting.snapshot().binding;
    let receipt = apply(&mut fitting).unwrap();
    assert_eq!(receipt.binding.revision, binding.revision + 1);
    assert_eq!(
        fitting.lookup(binding, 1),
        Ok(NativeRequestLookup::Accepted(receipt))
    );
    fitting
}

#[test]
fn checkbox_add_to_indirect_annots_preserves_existing_note_and_widget() {
    let source = build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 500] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots 4 0 R >>".to_vec(),
        b"[5 0 R 6 0 R]".to_vec(),
        b"<< /Type /Annot /Subtype /Text /Rect [10 10 30 30] /Contents (Delivery note) /P 3 0 R >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /Rect [40 40 60 60] /P 3 0 R /FT /Btn /T (agreement) /V /Off /AS /Off >>".to_vec(),
    ]);
    let pdf = apply_at_retained_boundary(&source, |_| NativeFieldCommand::AddCheckboxField {
        page: 0,
        rect: [80, 40, 100, 60],
        name: "approval".into(),
        checked: true,
    });
    let reopened = session(pdf.output_bytes());
    assert_eq!(reopened.snapshot().fields.len(), 2);
    assert_saved_value(pdf.output_bytes(), "agreement", "Btn", "Off");
    assert_saved_value(pdf.output_bytes(), "approval", "Btn", "Yes");
    assert!(reopened.snapshot().scene.pages[0].annotations.iter().any(|annotation| {
        matches!(annotation, flow_core::pdf::NativePdfAnnotation::PlainNote(note) if note.contents == "Delivery note")
    }));
}

#[test]
fn ordinary_text_field_creation_and_update_respect_the_retained_allowance() {
    let source = imported_checkbox("[10 10 30 30]", "", "Off");
    let pdf = apply_at_retained_boundary(&source, |_| NativeFieldCommand::AddTextField {
        page: 0,
        rect: [40, 40, 160, 60],
        name: "recipient".into(),
        value: "Original".into(),
    });
    assert_saved_value(pdf.output_bytes(), "recipient", "Tx", "Original");
    let updated = apply_at_retained_boundary(pdf.output_bytes(), |reopened| {
        let field = reopened
            .snapshot()
            .fields
            .iter()
            .find(|field| field.name.as_deref() == Some("recipient"))
            .unwrap();
        NativeFieldCommand::SetFieldText {
            field: field.id,
            value: "Updated".into(),
        }
    });
    assert_saved_value(updated.output_bytes(), "recipient", "Tx", "Updated");
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

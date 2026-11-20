use flow_core::pdf::{
    PdfReadDiagnosticCode, PdfReadError, PdfReaderLimits, PdfSceneElement, PdfSceneRequest,
    PdfTextMapping, open_pdf, read_pdf_scene,
};

fn stream(id: u32, dictionary: &str, data: &[u8]) -> (u32, Vec<u8>) {
    let mut body = format!("<< {dictionary} /Length {} >>\nstream\n", data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    (id, body)
}

fn pdf(objects: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let max_id = objects.iter().map(|(id, _)| *id).max().unwrap();
    let mut output = b"%PDF-1.7\n%\xFF\xFF\xFF\xFF\n".to_vec();
    let mut offsets = vec![None; usize::try_from(max_id).unwrap() + 1];
    for (id, body) in objects {
        offsets[usize::try_from(id).unwrap()] = Some(output.len());
        output.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        output.extend_from_slice(&body);
        output.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = output.len();
    output.extend_from_slice(format!("xref\n0 {}\n", max_id + 1).as_bytes());
    output.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        output.extend_from_slice(
            format!(
                "{:010} 00000 {} \n",
                offset.unwrap_or(0),
                if offset.is_some() { 'n' } else { 'f' }
            )
            .as_bytes(),
        );
    }
    output.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            max_id + 1
        )
        .as_bytes(),
    );
    output
}

fn fixture() -> Vec<u8> {
    let cmap = b"/CIDInit /ProcSet findresource begin\nbegincmap\n2 beginbfchar\n<48> <0048>\n<69> <0069>\nendbfchar\nendcmap\nend\nend\n";
    let content = b"q\n0 0 50 50 re W n\nBT /F1 12 Tf 10 150 Td (Hi) Tj ET\n0 0 m 50 0 l 50 50 l h S\n/Im1 Do\nQ\n";
    pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R /OpenAction 11 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources 4 0 R /Contents 5 0 R /Annots [9 0 R 10 0 R] >>".to_vec()),
        (4, b"<< /Font << /F1 6 0 R >> /XObject << /Im1 7 0 R >> >>".to_vec()),
        stream(5, "", content),
        (6, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 8 0 R /MissingWidth 500 >>".to_vec()),
        stream(7, "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode", &[0xFF, 0xD8, 0xFF, 0xD9]),
        stream(8, "", cmap),
        (9, b"<< /Type /Annot /Subtype /Link /Rect [10 10 30 30] /Dest [3 0 R /Fit] >>".to_vec()),
        (10, b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (field) /V (value) /Rect [20 20 50 40] >>".to_vec()),
        (11, b"<< /S /JavaScript /JS (blocked) >>".to_vec()),
    ])
}

#[test]
fn scene_preserves_text_glyphs_paths_clips_images_forms_links_and_provenance() {
    let reader = open_pdf(&fixture(), Default::default()).expect("reader");
    let scene = read_pdf_scene(
        &reader,
        &PdfSceneRequest {
            first_page: 0,
            page_count: 1,
        },
    )
    .expect("scene");
    assert_eq!(scene.pages.len(), 1);
    let page = &scene.pages[0];
    assert!(page.elements.iter().any(|element| matches!(element, PdfSceneElement::Text { value } if value.text == "Hi" && value.mapping == PdfTextMapping::ToUnicode && value.glyphs.len() == 2)));
    assert!(
        page.elements
            .iter()
            .any(|element| matches!(element, PdfSceneElement::Path { value } if value.stroke))
    );
    assert!(
        page.elements
            .iter()
            .any(|element| matches!(element, PdfSceneElement::Clip { .. }))
    );
    assert!(page.elements.iter().any(|element| matches!(element, PdfSceneElement::Image { value } if value.media_type == "image/jpeg" && value.data_hex == "ffd8ffd9")));
    assert!(page.elements.iter().any(|element| matches!(element, PdfSceneElement::Form { value } if value.name.as_deref() == Some("field") && value.value.as_deref() == Some("value"))));
    assert!(page.elements.iter().any(|element| matches!(element, PdfSceneElement::Link { value } if value.internal && value.destination_page == Some(0))));
    let text = page
        .elements
        .iter()
        .find_map(|element| match element {
            PdfSceneElement::Text { value } => Some(value),
            _ => None,
        })
        .expect("text element");
    assert_eq!(text.glyphs[0].provenance.object_ref.object_number, 5);
    assert_eq!(text.glyphs[0].provenance.operator.as_deref(), Some("Tj"));
    assert!(
        scene
            .report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == PdfReadDiagnosticCode::ActiveContent)
    );
    assert!(!scene.result_hash.is_empty());
}

#[test]
fn unsupported_operator_and_missing_mapping_are_reported_without_authored_payloads() {
    let mut bytes = fixture();
    let needle = b"/Im1 Do";
    let offset = bytes
        .windows(needle.len())
        .position(|window| window == needle)
        .unwrap();
    bytes.splice(offset..offset + needle.len(), b"Unknown".iter().copied());
    let reader = open_pdf(&bytes, Default::default()).expect("reader");
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).expect("scene");
    assert!(
        scene
            .report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == PdfReadDiagnosticCode::UnsupportedOperator)
    );
    let serialized = serde_json::to_string(&scene.report).unwrap();
    assert!(!serialized.contains("JavaScript"));
    assert!(!serialized.contains("alert"));
}

#[test]
fn single_content_stream_reads_at_low_nesting_limit() {
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>".to_vec(),
        ),
        stream(4, "", b"10 10 20 20 re f"),
    ]);
    let reader = open_pdf(
        &bytes,
        PdfReaderLimits {
            max_nesting: 2,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reader.summary().unwrap().page_count, 1);
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    assert!(scene.pages[0].elements.iter().any(|element| matches!(
        element,
        PdfSceneElement::Path { value } if value.fill && value.provenance.object_ref.object_number == 4
    )));
}

#[test]
fn form_text_at_lower_clip_edge_is_retained_as_partial() {
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Form1 5 0 R >> >> /Contents 4 0 R >>".to_vec()),
        stream(4, "", b"/Form1 Do"),
        stream(5, "/Type /XObject /Subtype /Form /BBox [10 20 40 40] /Resources << /Font << /F1 6 0 R >> >>", b"BT /F1 10 Tf 15 20 Td (A) Tj ET"),
        (6, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /MissingWidth 500 /ToUnicode 7 0 R >>".to_vec()),
        stream(7, "", b"begincmap\n1 beginbfchar\n<41> <0041>\nendbfchar\nendcmap\n"),
    ]);
    let reader = open_pdf(&bytes, Default::default()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    let page = &scene.pages[0];
    assert!(page.partial);
    assert!(page.elements.iter().any(|element| matches!(
        element,
        PdfSceneElement::Text { value } if value.text == "A" && value.provenance.object_ref.object_number == 5
    )));
    assert!(
        page.report
            .diagnostics
            .iter()
            .any(
                |diagnostic| diagnostic.code == PdfReadDiagnosticCode::PartialPage
                    && diagnostic.object_number == Some(5)
            )
    );
}

#[test]
fn nested_forms_resolve_equivalent_names_and_report_uncertain_clipping() {
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Outer 5 0 R /Later 9 0 R >> >> /Contents 4 0 R >>".to_vec()),
        stream(4, "", b"q 0 0 65 100 re W n /#4Futer Do Q /Later Do 90 5 5 5 re f"),
        stream(5, "/Type /XObject /Subtype /Form /BBox [10 10 40 40] /Matrix [2 0 0 1 5 0] /Resources << /XObject << /In#6Eer 6 0 R >> >>", b"/Inner Do"),
        stream(6, "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /Font << /F#E9 7 0 R >> >>", b"BT /#46#e9 10 Tf 15 25 Td (A) Tj ET 28 20 8 5 re f 35 30 2 2 re f 2 20 3 3 re f"),
        (7, b"<< /Type /Font /Subtype /Type1 /BaseFont /#D0#A8#D1#80#D0#B8#D1#84#D1#82 /MissingWidth 500 /ToUnicode 8 0 R >>".to_vec()),
        stream(8, "", b"begincmap\n1 beginbfchar\n<41> <0041>\nendbfchar\nendcmap\n"),
        stream(9, "/Type /XObject /Subtype /Form /BBox [70 0 90 20]", b"75 5 5 5 re f"),
    ]);
    let reader = open_pdf(&bytes, Default::default()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    let page = &scene.pages[0];
    assert!(page.partial);
    assert!(
        page.report
            .diagnostics
            .iter()
            .any(
                |diagnostic| diagnostic.code == PdfReadDiagnosticCode::PartialPage
                    && diagnostic.object_number == Some(6)
            )
    );
    assert!(page.elements.iter().any(|element| matches!(
        element,
        PdfSceneElement::Annotation { value } if value.provenance.object_ref.object_number == 5
            && value.rect.x.raw() == 25 * 64 && value.rect.y.raw() == 10 * 64
            && value.rect.width.raw() == 60 * 64 && value.rect.height.raw() == 30 * 64
    )));
    assert!(page.elements.iter().any(|element| matches!(
        element,
        PdfSceneElement::Text { value } if value.text == "A"
            && value.mapping == PdfTextMapping::ToUnicode && value.font_name.as_deref() == Some("Шрифт")
    )));
    let painted_x: Vec<_> = page
        .elements
        .iter()
        .filter_map(|element| match element {
            PdfSceneElement::Path { value } => Some(value.rect.x.raw()),
            _ => None,
        })
        .collect();
    assert_eq!(painted_x, [61 * 64, 75 * 64, 90 * 64]);
}

#[test]
fn nested_form_paint_respects_direct_and_inherited_page_crop() {
    for (parent_crop, page_crop) in [
        ("/CropBox [100 100 150 150]", "/CropBox [90 70 20 -10]"),
        ("/CropBox 8 0 R", ""),
    ] {
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, format!("<< /Type /Pages /Kids [3 0 R] /Count 1 {parent_crop} >>").into_bytes()),
            (3, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] {page_crop} /Resources << /XObject << /Outer 5 0 R /Later 7 0 R /Image 9 0 R >> >> /Contents 4 0 R >>").into_bytes()),
            stream(4, "", b"q 1 0 0 1 3 0 cm /Outer Do 70 5 5 5 re f /Later Do Q 85 5 2 2 re f q 4 0 0 4 82 10 cm /Image Do Q q 4 0 0 4 95 10 cm /Image Do Q q 4 0 0 4 88 10 cm /Image Do Q"),
            stream(5, "/Type /XObject /Subtype /Form /BBox [-20 -20 60 100] /Matrix [1 0 0 1 5 0] /Resources << /XObject << /Inner 6 0 R >> >>", b"/Inner Do 0 0 2 2 re W n 1 0 0 1 100 0 cm"),
            stream(6, "/Type /XObject /Subtype /Form /BBox [-20 -20 60 100] /Matrix [1 0 0 1 2 0]", b"2 20 3 3 re f 20 -8 3 3 re f 8 25 6 4 re f 20 65 4 10 re f 25 35 4 4 re f"),
            stream(7, "/Type /XObject /Subtype /Form /BBox [70 0 110 18]", b"75 10 4 4 re f 95 10 4 4 re f"),
            (8, b"[20 70 90 -10]".to_vec()),
            stream(9, "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8", &[0x12, 0x34, 0x56]),
        ]);
        let reader = open_pdf(&bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        assert!(page.partial && scene.report.partial);
        assert!(matches!(
            page.report.diagnostics.as_slice(),
            [nested, direct] if nested.code == PdfReadDiagnosticCode::PartialPage
                && nested.object_number == Some(6)
                && direct.code == PdfReadDiagnosticCode::PartialPage
                && direct.object_number == Some(4)
        ));
        let painted: Vec<_> = page
            .elements
            .iter()
            .filter_map(|element| match element {
                PdfSceneElement::Path { value } => Some((
                    value.provenance.object_ref.object_number,
                    [
                        value.rect.x,
                        value.rect.y,
                        value.rect.width,
                        value.rect.height,
                    ]
                    .map(|coordinate| coordinate.raw()),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            painted,
            [
                (6, [18, 25, 6, 4].map(|points| points * 64)),
                (6, [30, 65, 4, 10].map(|points| points * 64)),
                (6, [35, 35, 4, 4].map(|points| points * 64)),
                (4, [73, 5, 5, 5].map(|points| points * 64)),
                (7, [78, 10, 4, 4].map(|points| points * 64)),
                (4, [85, 5, 2, 2].map(|points| points * 64)),
            ]
        );
        let image_x: Vec<_> = page
            .elements
            .iter()
            .filter_map(|element| match element {
                PdfSceneElement::Image { value } => Some(value.rect.x.raw()),
                _ => None,
            })
            .collect();
        assert_eq!(image_x, [82 * 64, 88 * 64]);
    }
}

#[test]
fn form_path_readback_accounts_for_fractional_clip_edges_and_transform() {
    for (right, skew, adjustment, partial) in [
        ("10.121", "0", "0", true),
        ("10.125", "0.004", "-0.004", true),
        ("10.125", "0", "0", false),
    ] {
        let paint = format!("1 {adjustment} 0 1 0 0 cm 5 0 5.125 5 re f");
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /F 5 0 R >> >> /Contents 4 0 R >>".to_vec()),
            stream(4, "", b"/F Do"),
            stream(5, &format!("/Type /XObject /Subtype /Form /BBox [0 0 {right} 20] /Matrix [1 {skew} 0 1 20.125 20]"), paint.as_bytes()),
        ]);
        let reader = open_pdf(&bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        let [
            PdfSceneElement::Annotation { .. },
            PdfSceneElement::Path { value },
        ] = page.elements.as_slice()
        else {
            panic!("expected a Form with a filled path");
        };
        assert!(value.fill);
        assert_eq!(value.rect.x.raw(), 25 * 64 + 8);
        assert_eq!(value.rect.width.raw(), 5 * 64 + 8);
        assert_eq!(page.partial, partial, "{right} {skew}");
        assert_eq!(scene.report.partial, partial);
        if partial {
            assert!(matches!(page.report.diagnostics.as_slice(), [diagnostic]
                if diagnostic.code == PdfReadDiagnosticCode::PartialPage
                    && diagnostic.object_number == Some(5)));
        } else {
            assert!(page.report.diagnostics.is_empty());
        }
    }
}

#[test]
fn equivalent_default_crop_keeps_content_clip_visibility_and_diagnostics() {
    for crop in ["", "/CropBox [0 0 200 200]", "/CropBox [-10 -10 210 210]"] {
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (3, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] {crop} /Resources << /XObject << /Im 5 0 R >> >> /Contents [4 0 R 6 0 R] >>").into_bytes()),
            stream(4, "", b"210 10 5 5 re f q 10 10 30 30 re W n 45 10 5 5 re f /Im Do Q 10 10 5 5 re f"),
            stream(5, "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[0x12]),
            stream(6, "", b"q 0 0 30 30 re W n 25 10 10 5 re f Q"),
        ]);
        let reader = open_pdf(&bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        assert!(matches!(
            page.elements.as_slice(),
            [PdfSceneElement::Clip { .. }, PdfSceneElement::Path { value: visible },
             PdfSceneElement::Clip { .. }, PdfSceneElement::Path { value: clipped }]
                if visible.rect.x.raw() == 10 * 64 && clipped.rect.x.raw() == 25 * 64
        ));
        assert!(page.partial && scene.report.partial);
        assert!(matches!(
            page.report.diagnostics.as_slice(),
            [diagnostic] if diagnostic.code == PdfReadDiagnosticCode::PartialPage
                && diagnostic.object_number == Some(6)
        ));
    }
}

#[test]
fn wide_stroke_is_partial_with_or_without_materialized_default_crop() {
    for crop in ["", "/CropBox [0 0 200 200]"] {
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (3, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] {crop} /Contents [4 0 R 5 0 R] >>").into_bytes()),
            stream(4, "", b"q 10 w 20 3 m 80 3 l S Q"),
            stream(5, "", b"20 20 m 80 20 l S"),
        ]);
        let reader = open_pdf(&bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        assert!(matches!(
            page.elements.as_slice(),
            [PdfSceneElement::Path { value: wide }, PdfSceneElement::Path { value: narrow }]
                if wide.stroke && wide.rect.y.raw() == 3 * 64 && narrow.stroke
        ));
        assert!(page.partial && scene.report.partial);
        assert!(matches!(
            page.report.diagnostics.as_slice(),
            [diagnostic] if diagnostic.code == PdfReadDiagnosticCode::PartialPage
                && diagnostic.object_number == Some(4)
        ));
    }
}

#[test]
fn text_near_upper_edge_is_retained_as_partial_under_equivalent_crops() {
    for crop in ["", "/CropBox [0 0 200 200]"] {
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (3, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] {crop} /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>").into_bytes()),
            stream(4, "", b"BT /F1 10 Tf 20 199 Td (A) Tj ET"),
            (5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /MissingWidth 500 >>".to_vec()),
        ]);
        let reader = open_pdf(&bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        assert!(matches!(
            page.elements.as_slice(),
            [PdfSceneElement::Text { value }] if value.text == "A"
        ));
        assert!(page.partial && scene.report.partial);
        assert!(matches!(
            page.report.diagnostics.as_slice(),
            [diagnostic] if diagnostic.code == PdfReadDiagnosticCode::PartialPage
                && diagnostic.object_number == Some(4)
        ));
    }
}

#[test]
fn widget_readback_requires_a_registered_split_field_with_a_single_matching_child() {
    for (fields, children, widget_entries, field_entries, expected) in [
        ("[5 0 R]", "[4 0 R]", "", "", Some("Ї")),
        ("[5 0 R]", "[4 0 R]", "/Kids []", "", None),
        ("[5 0 R]", "[4 0 R]", "", "/Parent 7 0 R", None),
        ("[5 0 R]", "[4 0 R 4 0 R]", "", "", None),
        ("[5 0 R]", "[]", "", "", None),
        ("[]", "[4 0 R]", "", "", None),
        ("[5 0 R 5 0 R]", "[4 0 R]", "", "", None),
    ] {
        let bytes = pdf(vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /AcroForm 7 0 R >>".to_vec(),
            ),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>".to_vec(),
            ),
            (
                4,
                format!(
                    "<< /Subtype /Widget /Parent 5 0 R /Rect [10 10 40 30] {widget_entries} >>"
                )
                .into_bytes(),
            ),
            (
                5,
                format!("<< /FT /Tx /T (recipient) /V <FEFF0407> /Kids 6 0 R {field_entries} >>")
                    .into_bytes(),
            ),
            (6, children.as_bytes().to_vec()),
            (7, b"<< /Fields 8 0 R >>".to_vec()),
            (8, fields.as_bytes().to_vec()),
        ]);
        let reader = open_pdf(&bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let [PdfSceneElement::Form { value }] = scene.pages[0].elements.as_slice() else {
            panic!("expected a widget");
        };
        assert_eq!(value.field_type.as_deref(), expected.map(|_| "Tx"));
        assert_eq!(value.value.as_deref(), expected);
        assert_eq!(value.name.as_deref(), expected.map(|_| "recipient"));
        assert_eq!(value.provenance.object_ref.object_number, 4);
    }
}

#[test]
fn widget_readback_decodes_saved_pdf_17_text_and_requires_a_saved_value() {
    for (saved, expected) in [
        ("/V (Caf\\351 \\200)", Some("Café •")),
        ("/V <FEFF0407>", Some("Ї")),
        ("/V <9F>", None),
        ("/V <FEFFD800>", None),
        ("", None),
    ] {
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>".to_vec()),
            (4, format!("<< /Subtype /Widget /FT /Tx /T (recipient) /Rect [10 10 40 30] /DV (Default) {saved} >>").into_bytes()),
        ]);
        let reader = open_pdf(&bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let [PdfSceneElement::Form { value }] = scene.pages[0].elements.as_slice() else {
            panic!("expected a widget");
        };
        assert_eq!(value.value.as_deref(), expected, "{saved}");
    }
}

#[test]
fn link_destinations_resolve_within_reader_limits() {
    for (entries, destination, max_nesting, expected) in [
        ("/Dest 5 0 R", "6 0 R", 8, Some(0)),
        ("/Dest 5 0 R", "6 0 R", 4, None),
        ("/A << /S /GoTo /D 8 0 R >>", "null", 4, Some(0)),
        ("/A << /S /GoToR /D 8 0 R >>", "null", 4, None),
        ("/Dest 5 0 R", "5 0 R", 4, None),
        ("/Dest 5 0 R", "[3 0 R /FitR null 0 20 30]", 4, None),
    ] {
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>".to_vec(),
            ),
            (
                4,
                format!("<< /Subtype /Link /Rect [10 10 40 30] {entries} >>").into_bytes(),
            ),
            (5, destination.as_bytes().to_vec()),
            (6, b"7 0 R".to_vec()),
            (7, b"8 0 R".to_vec()),
            (8, b"9 0 R".to_vec()),
            (9, b"[3 0 R /XYZ null 20 null]".to_vec()),
        ]);
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_nesting,
                ..Default::default()
            },
        )
        .unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let [PdfSceneElement::Link { value }] = scene.pages[0].elements.as_slice() else {
            panic!("expected a link");
        };
        assert_eq!(value.destination_page, expected, "{entries} {destination}");
        assert_eq!(value.internal, expected.is_some());
    }
}

#[test]
fn indirect_page_crop_obeys_the_reader_nesting_limit() {
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /CropBox 4 0 R >>".to_vec(),
        ),
        (4, b"[20 0 90 70]".to_vec()),
    ]);
    let reader = open_pdf(
        &bytes,
        PdfReaderLimits {
            max_nesting: 2,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reader.summary(), Err(PdfReadError::NestingLimit));
}

#[test]
fn link_resolution_does_not_consume_widget_readback_budget() {
    for widget_first in [true, false] {
        let mut annotations = (9..=20).collect::<Vec<_>>();
        annotations.insert(if widget_first { 0 } else { annotations.len() }, 4);
        let references = annotations
            .into_iter()
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut objects = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] >> >>".to_vec(),
            ),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [{references}] >>"
                )
                .into_bytes(),
            ),
            (
                4,
                b"<< /Subtype /Widget /Parent 5 0 R /Rect [10 10 40 30] >>".to_vec(),
            ),
            (
                5,
                b"<< /FT /Tx /T (recipient) /V (saved) /Kids 6 0 R >>".to_vec(),
            ),
            (6, b"[4 0 R]".to_vec()),
        ];
        objects.extend((9..=20).map(|id| {
            (
                id,
                b"<< /Subtype /Link /Rect [50 10 70 30] /Dest 21 0 R >>".to_vec(),
            )
        }));
        objects.extend((21..=35).map(|id| (id, format!("{} 0 R", id + 1).into_bytes())));
        objects.push((36, b"[3 0 R /Fit]".to_vec()));
        let reader = open_pdf(
            &pdf(objects),
            PdfReaderLimits {
                max_content_tokens: 192,
                ..PdfReaderLimits::default()
            },
        )
        .unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        assert!(scene.pages[0].elements.iter().any(|element| {
            matches!(element, PdfSceneElement::Form { value }
                if value.field_type.as_deref() == Some("Tx")
                && value.name.as_deref() == Some("recipient")
                && value.value.as_deref() == Some("saved"))
        }));
    }
}

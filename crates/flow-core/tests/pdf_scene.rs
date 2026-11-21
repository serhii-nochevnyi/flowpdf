use flow_core::pdf::{
    PdfPathCommand, PdfReadDiagnosticCode, PdfReadError, PdfReaderLimits, PdfSceneElement,
    PdfSceneRequest, PdfTextMapping, open_pdf, read_pdf_scene,
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
fn page_and_form_text_report_only_potential_paint_in_a_collapsed_clip() {
    for (additional_clip, partial) in [("", true), ("80 20 10 12 re W n", false)] {
        let text =
            format!("22.125 20 0.004 12 re W n {additional_clip} BT /F1 10 Tf 20 20 Td (A) Tj ET");
        let page_content = format!("q {text} Q /Form1 Do");
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 6 0 R >> /XObject << /Form1 5 0 R >> >> /Contents 4 0 R >>".to_vec()),
            stream(4, "", page_content.as_bytes()),
            stream(5, "/Type /XObject /Subtype /Form /BBox [0 0 100 100]", text.as_bytes()),
            (6, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /MissingWidth 500 /ToUnicode 7 0 R >>".to_vec()),
            stream(7, "", b"begincmap
1 beginbfchar
<41> <0041>
endbfchar
endcmap
"),
        ]);
        let reader = open_pdf(&bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        let collapsed_clips = page.elements.iter().filter(|element| {
            matches!(element, PdfSceneElement::Clip { value } if value.rect.width.raw() == 0)
        });
        assert_eq!(collapsed_clips.count(), 2);
        assert!(
            !page
                .elements
                .iter()
                .any(|element| matches!(element, PdfSceneElement::Text { .. }))
        );
        assert_eq!(page.partial, partial);
        assert_eq!(scene.report.partial, partial);
        let diagnostic_objects: Vec<_> = page
            .report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                assert_eq!(diagnostic.code, PdfReadDiagnosticCode::PartialPage);
                diagnostic.object_number.unwrap()
            })
            .collect();
        assert_eq!(
            diagnostic_objects.as_slice(),
            if partial { &[4, 5][..] } else { &[] }
        );
    }
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

fn read_content_scene(content: &str) -> flow_core::pdf::PdfScene {
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>".to_vec(),
        ),
        stream(4, "", content.as_bytes()),
    ]);
    let reader = open_pdf(&bytes, Default::default()).unwrap();
    read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap()
}

#[test]
fn explicit_clip_readback_proves_fractional_edges_and_rectangularity() {
    for (clip, paint, partial) in [
        (
            "20.125 20.125 40 40 re W n",
            "50.125 50.125 10 10 re f",
            false,
        ),
        (
            "20.125 20.125 39.996 40 re W n",
            "50.125 50.125 10 10 re f",
            true,
        ),
        (
            "20.125 20.125 m 60.125 20.125 l 60.125 60.125 l 20.125 60.125 l W* n",
            "20.125 40.125 10 10 re f",
            false,
        ),
        (
            "20.125 20.125 m 60.125 20.125 l 60.125 60.125 l 20.125 60.125 l 20.125 20.125 l W n",
            "20.125 40.125 10 10 re f",
            false,
        ),
        (
            "20.125 20.125 m 60.125 20.125 l 60.125 60.125 l 20.125 60.125 l 20.129 20.125 l W n",
            "20.125 40.125 10 10 re f",
            true,
        ),
        (
            "20.125 20.125 m 60.125 20.125 l 60.125 60.125 l 20.129 60.125 l h W* n",
            "20.125 40.125 10 10 re f",
            true,
        ),
        (
            "1 .004 0 1 0 0 cm 20.125 20.125 40 40 re W n 1 -.004 0 1 0 0 cm",
            "30.125 20.125 10 10 re f",
            true,
        ),
    ] {
        let scene = read_content_scene(&format!("{clip} {paint}"));
        let page = &scene.pages[0];
        assert!(
            matches!(
                page.elements.as_slice(),
                [PdfSceneElement::Clip { value: clip }, PdfSceneElement::Path { value: path }]
                    if clip.rect.width.raw() == 40 * 64 && path.fill && !path.stroke
            ),
            "{clip} {paint}"
        );
        assert_eq!(page.partial, partial, "{clip}");
        assert_eq!(scene.report.partial, partial);
        assert_eq!(page.report.diagnostics.len(), usize::from(partial));
        if partial {
            assert_eq!(
                page.report.diagnostics[0].code,
                PdfReadDiagnosticCode::PartialPage
            );
            assert_eq!(page.report.diagnostics[0].object_number, Some(4));
        }
    }
}

#[test]
fn stroke_readback_proves_containment_against_the_active_clip() {
    for (clip, paint, retained, partial) in [
        (
            "20.125 20.125 80 80 re W n",
            "1.125 w 35.125 40.125 m 80.125 40.125 l S",
            true,
            false,
        ),
        (
            "20.125 20.125 80 80 re W n",
            "1.125 w 35.125 20.625 m 80.125 20.625 l S",
            true,
            true,
        ),
        (
            "20.125 20.125 .004 80 re W n",
            "1 w 20.125 40 m 25 40 l S",
            false,
            true,
        ),
        ("", "1 M 150 1.25 m 190 1.25 l S", true, false),
        (
            "",
            "1 M 1 -.004 0 1 0 0 cm 150 1.25 m 190 1.25 l S",
            true,
            true,
        ),
    ] {
        let scene = read_content_scene(&format!("{clip} {paint}"));
        let page = &scene.pages[0];
        let paint_elements = if clip.is_empty() {
            page.elements.as_slice()
        } else {
            assert!(matches!(page.elements[0], PdfSceneElement::Clip { .. }));
            &page.elements[1..]
        };
        if retained {
            assert!(matches!(
                paint_elements,
                [PdfSceneElement::Path { value }] if value.stroke && !value.fill
            ));
        } else {
            assert!(paint_elements.is_empty());
        }
        assert_eq!(page.partial, partial, "{clip} {paint}");
        assert_eq!(scene.report.partial, partial);
        assert_eq!(page.report.diagnostics.len(), usize::from(partial));
        if partial {
            assert_eq!(
                page.report.diagnostics[0].code,
                PdfReadDiagnosticCode::PartialPage
            );
            assert_eq!(page.report.diagnostics[0].object_number, Some(4));
        }
    }
}

#[test]
fn stroke_readback_reports_collapsed_fixed_point_segments() {
    for (path, partial, has_extent) in [
        ("50 50 m 50.004 50 l", true, false),
        ("50 50 m 50.004 50.004 50.004 50 50.004 50 c", true, false),
        ("50 50 m 55 50 l 55.004 50 l", true, true),
        ("50 50 m 51 50 l", false, true),
        ("50 50 m 50 51 l", false, true),
    ] {
        let scene = read_content_scene(&format!("1 w {path} S"));
        let page = &scene.pages[0];
        let [PdfSceneElement::Path { value }] = page.elements.as_slice() else {
            panic!("missing stroke for {path}");
        };
        assert!(value.stroke && !value.fill);
        assert_eq!(
            value.rect.width.raw() > 0 || value.rect.height.raw() > 0,
            has_extent
        );
        assert_eq!(page.partial, partial, "{path}");
        assert_eq!(scene.report.partial, partial, "{path}");
        assert_eq!(page.report.diagnostics.len(), usize::from(partial));
        if partial {
            let diagnostic = &page.report.diagnostics[0];
            assert_eq!(diagnostic.code, PdfReadDiagnosticCode::PartialPage);
            assert_eq!(diagnostic.object_number, Some(4));
        }
    }
}

#[test]
fn stroke_readback_reports_collapsed_closing_segments() {
    for (closing, partial) in [
        ("50.004 50 l h S", true),
        ("50.004 50 l s", true),
        ("70 50 60 50 50.004 50 c h S", true),
        ("50 50.004 l h S", false),
        ("50 50.004 l s", false),
    ] {
        let scene = read_content_scene(&format!(
            "1 w 50 50.004 m 50 80 l 80 80 l 80 50 l {closing}"
        ));
        let page = &scene.pages[0];
        let [PdfSceneElement::Path { value }] = page.elements.as_slice() else {
            panic!("missing stroke for {closing}");
        };
        assert!(value.stroke && !value.fill);
        assert_eq!(value.commands.last(), Some(&PdfPathCommand::ClosePath));
        assert_eq!(page.partial, partial, "{closing}");
        assert_eq!(scene.report.partial, partial, "{closing}");
        let diagnostics = &page.report.diagnostics;
        assert_eq!(diagnostics.len(), usize::from(partial), "{closing}");
        if partial {
            assert_eq!(diagnostics[0].code, PdfReadDiagnosticCode::PartialPage);
        }
    }
}

#[test]
fn shorthand_curve_uses_the_closed_subpath_start_across_streams() {
    for (curve, partial) in [
        ("60 70 65 64 v S", false),
        ("50.004 50 50.004 50 v S", true),
    ] {
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents [4 0 R 5 0 R] >>"
                    .to_vec(),
            ),
            stream(4, "", b"1 w 50 50 m 80 50 l 80 80 l h\n"),
            stream(5, "", curve.as_bytes()),
        ]);
        let reader = open_pdf(&bytes, PdfReaderLimits::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        let [PdfSceneElement::Path { value }] = page.elements.as_slice() else {
            panic!("missing stroke");
        };
        assert!(value.stroke && !value.fill);
        assert!(matches!(
            value.commands.get(4),
            Some(PdfPathCommand::CurveTo { x1, y1, .. }) if x1.raw() == 50 * 64 && y1.raw() == 50 * 64
        ));
        assert_eq!(value.provenance.object_ref.object_number, 5);
        assert_eq!(value.provenance.invocation_path, [1]);
        assert_eq!(page.partial, partial, "{curve}");
        assert_eq!(scene.report.partial, partial, "{curve}");
        if partial {
            assert!(matches!(
                page.report.diagnostics.as_slice(),
                [diagnostic] if diagnostic.code == PdfReadDiagnosticCode::PartialPage
                    && diagnostic.page_index == Some(0)
                    && diagnostic.object_number == Some(5)
            ));
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
fn page_crop_readback_reports_only_the_stroke_that_can_cross_an_edge() {
    for crop in ["", "/CropBox [0 0 200 200]", "/CropBox [10 0 100 100]"] {
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

#[test]
fn unfinished_text_array_across_page_streams_preserves_committed_paint() {
    let first = b"10 20 5 5 re f\nBT /F1 12 Tf 20 40 Td [(Hello) 0 0 0\n";
    let second = b"] TJ ET\n";
    let fixture = |contents, streams: Vec<_>| {
        let mut objects = vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (3, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 6 0 R >> >> /Contents {contents} >>").into_bytes()),
            (6, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /MissingWidth 500 >>".to_vec()),
        ];
        objects.extend(streams);
        pdf(objects)
    };
    let limits = PdfReaderLimits {
        max_array_items: 4,
        ..Default::default()
    };
    let split = fixture(
        "[4 0 R 5 0 R]",
        vec![stream(4, "", first), stream(5, "", second)],
    );
    let reader = open_pdf(&split, limits.clone()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    let page = &scene.pages[0];
    assert!(matches!(
        page.elements.as_slice(),
        [PdfSceneElement::Path { value }] if value.fill && value.provenance.object_ref.object_number == 4
            && value.provenance.invocation_path == [0]
    ));
    assert!(page.partial && scene.report.partial);
    assert!(matches!(
        page.report.diagnostics.as_slice(),
        [diagnostic] if diagnostic.code == PdfReadDiagnosticCode::PartialPage
            && diagnostic.page_index == Some(0)
            && diagnostic.object_number == Some(4)
    ));

    let joined = fixture(
        "4 0 R",
        vec![stream(4, "", &[first.as_slice(), second].concat())],
    );
    let reader = open_pdf(&joined, limits.clone()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    let page = &scene.pages[0];
    assert!(matches!(
        page.elements.as_slice(),
        [PdfSceneElement::Path { .. }, PdfSceneElement::Text { value }] if value.text == "Hello"
            && value.provenance.object_ref.object_number == 4
            && value.provenance.operator.as_deref() == Some("TJ")
            && value.provenance.invocation_path == [0]
    ));
    // @invariant: Glyph advance rectangles do not bound the painted glyphs.
    assert!(page.partial && scene.report.partial);
    assert!(matches!(
        page.report.diagnostics.as_slice(),
        [diagnostic] if diagnostic.code == PdfReadDiagnosticCode::PartialPage
            && diagnostic.page_index == Some(0)
            && diagnostic.object_number == Some(4)
    ));

    let truncated = fixture("4 0 R", vec![stream(4, "", first)]);
    let reader = open_pdf(&truncated, limits.clone()).unwrap();
    assert!(matches!(
        read_pdf_scene(&reader, &PdfSceneRequest::default()),
        Err(PdfReadError::Syntax)
    ));
    let over_limit = fixture(
        "[4 0 R 5 0 R]",
        vec![
            stream(4, "", &[first.as_slice(), b"0\n"].concat()),
            stream(5, "", second),
        ],
    );
    let reader = open_pdf(&over_limit, limits).unwrap();
    assert!(matches!(
        read_pdf_scene(&reader, &PdfSceneRequest::default()),
        Err(PdfReadError::TokenLimit)
    ));
}

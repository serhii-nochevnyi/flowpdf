use flow_core::pdf::{
    PdfReadDiagnosticCode, PdfSceneElement, PdfSceneRequest, PdfTextMapping, open_pdf,
    read_pdf_scene,
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
    assert!(page.elements.iter().any(|element| matches!(element, PdfSceneElement::Form { value } if value.name.as_deref() == Some("field"))));
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

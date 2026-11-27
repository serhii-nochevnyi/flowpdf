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
    let mut scene = scene;
    let result_hash = std::mem::take(&mut scene.result_hash);
    let serialized = serde_json::to_vec(&scene).unwrap();
    assert_eq!(result_hash, blake3::hash(&serialized).to_hex().as_str());
}

#[test]
fn text_only_scene_window_preserves_output_with_live_stream_workspace() {
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 100 100] /Resources << /Font << /F1 6 0 R >> >> >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_vec()),
        (4, b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_vec()),
        stream(5, "", b"BT /F1 10 Tf 5 20 Td (Ordinary text) Tj ET"),
        (6, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()),
    ]);
    let request = PdfSceneRequest {
        first_page: 0,
        page_count: 2,
    };
    let read = |max_scene_bytes| {
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(&reader, &request)
    };
    let mut expected = read(PdfReaderLimits::default().max_scene_bytes).unwrap();
    assert_eq!(expected.pages.len(), 2);
    assert!(expected.pages.iter().all(|page| {
        page.elements
            .iter()
            .any(|item| matches!(item, PdfSceneElement::Text { .. }))
    }));
    let result_hash = std::mem::take(&mut expected.result_hash);
    let serialized = serde_json::to_vec(&expected).unwrap();
    assert_eq!(result_hash, blake3::hash(&serialized).to_hex().as_str());
    expected.result_hash = result_hash;
    assert_eq!(read(serialized.len() + 8192).unwrap(), expected);
    assert!(matches!(
        read(serialized.len() - 1),
        Err(PdfReadError::StreamLimit)
    ));
}

#[test]
fn mapped_font_scene_reserves_decode_workspace_at_the_allowance_boundary() {
    let cmap = b"begincmap\n2 beginbfchar\n<41> <0410>\n<42> <00660069>\nendbfchar\nendcmap\n";
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(cmap, 6);
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font 7 0 R >> /Contents 4 0 R >>".to_vec()),
        stream(4, "", b"BT /F1 10 Tf 5 20 Td [(AAAAAAAAAAAA) 15 (B)] TJ ET"),
        (5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /Widths [600 500] /ToUnicode 6 0 R >>".to_vec()),
        stream(6, "/Filter /FlateDecode", &compressed),
        (7, b"<< /F1 5 0 R >>".to_vec()),
    ]);
    let read = |max_scene_bytes, max_decoded_stream_bytes| {
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes,
                max_decoded_stream_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(&reader, &PdfSceneRequest::default())
    };
    let limits = PdfReaderLimits::default();
    let read_with_allowance = |allowance| read(allowance, limits.max_decoded_stream_bytes);
    let expected = read_with_allowance(limits.max_scene_bytes).unwrap();
    assert!(matches!(
        expected.pages[0].elements.as_slice(),
        [PdfSceneElement::Text { value: first }, PdfSceneElement::Text { value: second }]
        if first.text == "А".repeat(12) && second.text == "fi"
            && first.glyphs.len() == 12
            && first.rect.x == first.glyphs[0].rect.x
            && first.rect.width.raw() == {
                let last = &first.glyphs[11].rect;
                last.x.raw() + last.width.raw() - first.rect.x.raw()
            }
            && [first, second].into_iter().all(|value| {
                value.mapping == PdfTextMapping::ToUnicode
                    && value.font_name.as_deref() == Some("Helvetica")
                    && value.glyphs[0].provenance == value.provenance
                    && value.provenance.operator.as_deref() == Some("TJ")
            })
    ));
    let mut serialized = expected.clone();
    serialized.result_hash.clear();
    let mut rejected = size_of::<miniz_oxide::inflate::core::DecompressorOxide>();
    let mut admitted = 64 * 1024;
    assert!(serde_json::to_vec(&serialized).unwrap().len() < rejected);
    assert_eq!(
        read_with_allowance(rejected),
        Err(PdfReadError::StreamLimit)
    );
    assert_eq!(read_with_allowance(admitted).unwrap(), expected);
    while admitted - rejected > 1 {
        let middle = rejected + (admitted - rejected) / 2;
        match read_with_allowance(middle) {
            Err(PdfReadError::StreamLimit) => rejected = middle,
            Ok(scene) => {
                assert_eq!(scene, expected);
                admitted = middle;
            }
            result => panic!("unexpected boundary result: {result:?}"),
        }
    }
    assert_eq!(
        read_with_allowance(rejected),
        Err(PdfReadError::StreamLimit)
    );
    assert_eq!(read_with_allowance(admitted).unwrap(), expected);
    assert_eq!(
        read(limits.max_scene_bytes, cmap.len() - 1),
        Err(PdfReadError::DecodedStreamLimit)
    );
}

#[test]
fn mapped_tj_applies_real_and_integer_spacing_at_the_active_horizontal_scale() {
    let content = b"BT /F1 20 Tf 50 Tz 10 50 Td [(A) -125.5 (B) 100 (A)] TJ ET";
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec()),
        stream(4, "", content),
        (5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /Widths [600 600] /ToUnicode 6 0 R >>".to_vec()),
        stream(6, "", b"begincmap\n2 beginbfchar\n<41> <0410>\n<42> <0411>\nendbfchar\nendcmap\n"),
    ]);
    let reader = open_pdf(&bytes, Default::default()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    let page = &scene.pages[0];
    assert!(page.partial);
    assert_eq!(page.elements.len(), 3);
    let mut shared_provenance = None;
    for (element, (text, x)) in page
        .elements
        .iter()
        .zip([("А", 640), ("Б", 1104), ("А", 1424)])
    {
        let PdfSceneElement::Text { value } = element else {
            panic!("expected mapped text");
        };
        assert_eq!(value.text, text);
        assert_eq!(value.mapping, PdfTextMapping::ToUnicode);
        assert_eq!(value.rect.x.raw(), x);
        assert_eq!(value.glyphs[0].advance.raw(), 384);
        assert_eq!(value.provenance.object_ref.object_number, 4);
        assert_eq!(value.provenance.operator.as_deref(), Some("TJ"));
        assert_eq!(value.glyphs[0].provenance, value.provenance);
        assert_eq!(
            *shared_provenance.get_or_insert(&value.provenance),
            &value.provenance
        );
    }
}

#[test]
fn mapped_text_runs_require_allowance_for_element_storage_growth() {
    let mut content = String::from("BT /F1 10 Tf 5 20 Td (A) Tj\n");
    content.push_str(&"0 12 Td (A) Tj\n".repeat(63));
    content.push_str("ET");
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 900] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec()),
        stream(4, "", content.as_bytes()),
        (5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>".to_vec()),
        stream(6, "", b"begincmap\n1 beginbfchar\n<41> <0410>\nendbfchar\nendcmap\n"),
    ]);
    let read = |max_scene_bytes| {
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(&reader, &PdfSceneRequest::default())
    };
    let expected = read(PdfReaderLimits::default().max_scene_bytes).unwrap();
    assert_eq!(expected.pages[0].elements.len(), 64);
    assert!(expected.pages[0].elements.iter().all(|element| matches!(
        element,
        PdfSceneElement::Text { value }
            if value.text == "А" && value.mapping == PdfTextMapping::ToUnicode
    )));
    let mut serialized = expected.clone();
    serialized.result_hash.clear();
    let serialized_bytes = serde_json::to_vec(&serialized).unwrap().len();
    let slot_bytes = size_of::<PdfSceneElement>();
    // @invariant: The last run grows 63 slots to 94, overlapping both buffers.
    assert_eq!(
        read(serialized_bytes + 64 * slot_bytes),
        Err(PdfReadError::StreamLimit)
    );
    assert_eq!(read(serialized_bytes + 160 * slot_bytes).unwrap(), expected);
}

#[test]
fn clipped_mapped_text_reserves_glyph_workspace_even_without_a_text_element() {
    let content = format!(
        "110 110 5 5 re W n BT /F1 10 Tf 5 20 Td ({}) Tj ET",
        "A".repeat(96),
    );
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec()),
        stream(4, "", content.as_bytes()),
        (5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>".to_vec()),
        stream(6, "", b"begincmap\n1 beginbfchar\n<41> <0410>\nendbfchar\nendcmap\n"),
    ]);
    let read = |max_scene_bytes, max_scene_elements| {
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes,
                max_scene_elements,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(&reader, &PdfSceneRequest::default())
    };
    let limits = PdfReaderLimits::default();
    let expected = read(limits.max_scene_bytes, limits.max_scene_elements).unwrap();
    assert!(matches!(
        expected.pages[0].elements.as_slice(),
        [PdfSceneElement::Clip { .. }]
    ));
    assert_eq!(
        read(8 * 1024, limits.max_scene_elements),
        Err(PdfReadError::StreamLimit)
    );
    assert_eq!(
        read(32 * 1024, limits.max_scene_elements).unwrap(),
        expected
    );
    assert_eq!(read(32 * 1024, 95), Err(PdfReadError::TokenLimit));
}

#[test]
fn text_dispatch_shares_workspace_with_its_live_operand_array() {
    let read = |adjustments: &str, text: &str, max_scene_bytes| {
        let content = format!(
            "q 110 110 5 5 re W n BT /F1 10 Tf 5 20 Td [{adjustments}({text})] TJ ET Q 10 10 5 5 re f"
        );
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources 6 0 R /Contents 4 0 R >>".to_vec()),
            stream(4, "", content.as_bytes()),
            (5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()),
            (6, b"<< /Font << /F1 5 0 R >> >>".to_vec()),
        ]);
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(&reader, &PdfSceneRequest::default())
    };
    let adjustments = "0 ".repeat(200);
    let text = "A".repeat(96);
    let small_allowance = 23 * 1024;
    assert!(read(&adjustments, "A", small_allowance).is_ok());
    assert!(read("", &text, small_allowance).is_ok());
    assert_eq!(
        read(&adjustments, &text, small_allowance),
        Err(PdfReadError::StreamLimit)
    );

    let expected = read(
        &adjustments,
        &text,
        PdfReaderLimits::default().max_scene_bytes,
    )
    .unwrap();
    assert!(matches!(
        expected.pages[0].elements.as_slice(),
        [PdfSceneElement::Clip { .. }, PdfSceneElement::Path { value }] if value.fill
    ));
    assert_eq!(read(&adjustments, &text, 40 * 1024).unwrap(), expected);
}

#[test]
fn image_payload_allowance_is_shared_across_pages_and_nested_forms() {
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 100 100] /Resources << /XObject << /Im 8 0 R /Outer 6 0 R /Inner 7 0 R >> /Font << /F1 10 0 R >> >> >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_vec()),
        (4, b"<< /Type /Page /Parent 2 0 R /Contents 9 0 R >>".to_vec()),
        stream(5, "", b"/Im Do /Outer Do"),
        stream(6, "/Type /XObject /Subtype /Form /BBox [0 0 100 100]", b"/Inner Do"),
        stream(7, "/Type /XObject /Subtype /Form /BBox [0 0 100 100]", b"/Im Do /Im Do"),
        stream(8, "/Type /XObject /Subtype /Image /Width 4 /Height 4 /ColorSpace /DeviceGray /BitsPerComponent 8", &[0x7f; 16]),
        stream(9, "", b"/Im Do BT /F1 10 Tf 5 20 Td (Second) Tj ET"),
        (10, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()),
    ]);
    let reader = open_pdf(
        &bytes,
        PdfReaderLimits {
            max_stream_bytes: 64,
            ..Default::default()
        },
    )
    .unwrap();
    let scene = read_pdf_scene(
        &reader,
        &PdfSceneRequest {
            first_page: 0,
            page_count: 2,
        },
    )
    .unwrap();
    let [first, second] = scene.pages.as_slice() else {
        panic!("expected both pages");
    };
    let [
        PdfSceneElement::Image { value: direct },
        PdfSceneElement::Annotation { value: outer },
        PdfSceneElement::Annotation { value: inner },
        PdfSceneElement::Image { value: nested },
    ] = first.elements.as_slice()
    else {
        panic!("expected two Form markers, one direct image, and one nested image");
    };
    assert_eq!(outer.provenance.object_ref.object_number, 6);
    assert_eq!(inner.provenance.object_ref.object_number, 7);
    for (value, invocation_depth) in [(direct, 1), (nested, 3)] {
        assert_eq!(value.data_hex, "7f".repeat(16));
        assert_eq!(value.provenance.object_ref.object_number, 8);
        assert_eq!(value.provenance.invocation_path.len(), invocation_depth);
    }
    assert!(scene.report.partial);
    for page in [first, second] {
        assert!(page.partial);
        assert!(page.report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == PdfReadDiagnosticCode::ImageLimit
                && diagnostic.page_index == Some(page.page_index)
                && diagnostic.object_number == Some(8)
        }));
    }
    assert!(matches!(
        second.elements.as_slice(),
        [PdfSceneElement::Text { value }] if value.text == "Second"
    ));

    let second_only = read_pdf_scene(
        &reader,
        &PdfSceneRequest {
            first_page: 1,
            page_count: 1,
        },
    )
    .unwrap();
    assert!(
        second_only
            .report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != PdfReadDiagnosticCode::ImageLimit)
    );
    assert!(matches!(
        second_only.pages[0].elements.as_slice(),
        [PdfSceneElement::Image { .. }, PdfSceneElement::Text { value }] if value.text == "Second"
    ));
}

#[test]
fn ordinary_image_payloads_spend_the_remaining_scene_bytes() {
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 100 100] /Resources << /XObject << /Im 9 0 R /Outer 7 0 R /Inner 8 0 R >> >> >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_vec()),
        (4, b"<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>".to_vec()),
        stream(5, "", b"/Im Do"),
        stream(6, "", b"/Outer Do"),
        stream(7, "/Type /XObject /Subtype /Form /BBox [0 0 100 100]", b"/Inner Do"),
        stream(8, "/Type /XObject /Subtype /Form /BBox [0 0 100 100]", b"/Im Do"),
        stream(9, "/Type /XObject /Subtype /Image /Width 32 /Height 32 /ColorSpace /DeviceGray /BitsPerComponent 8", &[0x7f; 1024]),
    ]);
    let read = |first_page, page_count, max_scene_bytes| {
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(
            &reader,
            &PdfSceneRequest {
                first_page,
                page_count,
            },
        )
    };
    for (page_index, invocation_depth) in [(0, 1), (1, 3)] {
        let single = read(page_index, 1, 24 * 1024).unwrap();
        assert!(matches!(
            single.pages[0].elements.last(),
            Some(PdfSceneElement::Image { value })
                if value.data_hex == "7f".repeat(1024)
                    && value.provenance.object_ref.object_number == 9
                    && value.provenance.invocation_path.len() == invocation_depth
        ));
    }
    assert_eq!(read(0, 1, 2047), Err(PdfReadError::StreamLimit));
    assert_eq!(read(0, 2, 4096), Err(PdfReadError::StreamLimit));

    let mut expected = read(0, 2, PdfReaderLimits::default().max_scene_bytes).unwrap();
    let result_hash = std::mem::take(&mut expected.result_hash);
    let serialized = serde_json::to_vec(&expected).unwrap();
    assert_eq!(result_hash, blake3::hash(&serialized).to_hex().as_str());
    expected.result_hash = result_hash;
    assert_eq!(read(0, 2, serialized.len()), Err(PdfReadError::StreamLimit));
    assert_eq!(read(0, 2, 24 * 1024).unwrap(), expected);
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
fn terminal_form_syntax_is_independent_of_a_page_stream_boundary() {
    for form_content in [b"BT [(unfinished".as_slice(), b"BT [<61"] {
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /F 6 0 R >> >> /Contents [4 0 R 5 0 R] >>".to_vec()),
            stream(4, "", b"10 20 5 5 re f"),
            stream(5, "", b"/F Do"),
            stream(6, "/Type /XObject /Subtype /Form /BBox [0 0 100 100]", form_content),
        ]);
        let reader = open_pdf(&bytes, Default::default()).unwrap();
        assert!(matches!(
            read_pdf_scene(&reader, &PdfSceneRequest::default()),
            Err(PdfReadError::Syntax)
        ));
    }
}

fn scene_with_nested_forms(
    page: [&str; 2],
    forms: [&str; 2],
    limits: PdfReaderLimits,
) -> Result<flow_core::pdf::PdfScene, PdfReadError> {
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Outer 6 0 R /Inner 7 0 R >> >> /Contents [4 0 R 5 0 R] >>".to_vec()),
        stream(4, "", page[0].as_bytes()),
        stream(5, "", page[1].as_bytes()),
        stream(6, "/Type /XObject /Subtype /Form /BBox [0 0 150 150]", forms[0].as_bytes()),
        stream(7, "/Type /XObject /Subtype /Form /BBox [0 0 100 100]", forms[1].as_bytes()),
    ]);
    let reader = open_pdf(&bytes, limits).unwrap();
    read_pdf_scene(&reader, &PdfSceneRequest::default())
}

#[test]
fn page_streams_share_the_token_limit_and_preserve_graphics_state() {
    let first = format!("q 1 0 0 1 10 0 cm {}\n", "q Q ".repeat(22));
    let page = [first.as_str(), "10 20 5 5 re f Q 10 10 5 5 re f"];
    let limits = PdfReaderLimits {
        max_content_tokens: 65,
        ..Default::default()
    };
    let scene = scene_with_nested_forms(page, ["", ""], limits.clone()).unwrap();
    let page_scene = &scene.pages[0];
    let [
        PdfSceneElement::Path { value: translated },
        PdfSceneElement::Path { value: restored },
    ] = page_scene.elements.as_slice()
    else {
        panic!("expected both paths from the second stream");
    };
    assert_eq!(translated.rect.x.raw(), 20 * 64);
    assert_eq!(restored.rect.x.raw(), 10 * 64);
    for path in [translated, restored] {
        assert_eq!(path.provenance.object_ref.object_number, 5);
        assert_eq!(path.provenance.invocation_path, [1]);
        assert_eq!(path.provenance.operator.as_deref(), Some("f"));
    }
    assert!(!page_scene.partial);
    assert_eq!(
        scene_with_nested_forms(
            page,
            ["", ""],
            PdfReaderLimits {
                max_content_tokens: 64,
                ..limits
            },
        ),
        Err(PdfReadError::TokenLimit)
    );
}

#[test]
fn nested_non_text_dispatch_shares_workspace_with_operator_storage() {
    let read = |operator: &str, max_scene_bytes| {
        let paint = format!("q {operator} 10 10 5 5 re f Q");
        scene_with_nested_forms(
            ["/Outer Do", ""],
            ["/Inner Do", &paint],
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
    };
    let operator = "VendorPaint".repeat(768);
    let limited = 32 * 1024;
    assert!(read("VendorPaint", limited).is_ok());
    assert!(matches!(
        read(&operator, limited),
        Err(PdfReadError::StreamLimit)
    ));
    let expected = read(&operator, PdfReaderLimits::default().max_scene_bytes).unwrap();
    assert_eq!(read(&operator, 96 * 1024).unwrap(), expected);
    assert!(
        expected.pages[0]
            .report
            .diagnostics
            .iter()
            .any(
                |diagnostic| diagnostic.code == PdfReadDiagnosticCode::UnsupportedOperator
                    && diagnostic.operator.as_deref() == Some(operator.as_str())
            )
    );
    assert!(matches!(
        expected.pages[0].elements.last(),
        Some(PdfSceneElement::Path { value })
            if value.provenance.object_ref.object_number == 7
                && value.provenance.invocation_path.len() == 3
    ));
}

#[test]
fn aggregate_report_retains_operator_storage_across_two_pages() {
    let later = format!("%{}\nVendorStroke 10 10 5 5 re f", " ".repeat(20 * 1024));
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 100 100] >>".to_vec(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_vec(),
        ),
        (
            4,
            b"<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>".to_vec(),
        ),
        stream(5, "", b"VendorPaint VendorPaint 10 10 5 5 re f"),
        stream(6, "", later.as_bytes()),
    ]);
    let read = |max_scene_bytes| {
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(
            &reader,
            &PdfSceneRequest {
                first_page: 0,
                page_count: 2,
            },
        )
    };
    let expected = read(PdfReaderLimits::default().max_scene_bytes).unwrap();
    assert_eq!(expected.pages[0].report.diagnostics.len(), 1);
    assert_eq!(expected.pages[1].report.diagnostics.len(), 1);
    assert_eq!(
        expected.report.diagnostics,
        [
            expected.pages[0].report.diagnostics[0].clone(),
            expected.pages[1].report.diagnostics[0].clone()
        ]
    );
    assert_eq!(
        expected.report.diagnostics[0].operator.as_deref(),
        Some("VendorPaint")
    );
    assert_eq!(
        expected.report.diagnostics[1].operator.as_deref(),
        Some("VendorStroke")
    );
    assert!(!expected.report.partial);
    let mut hashed = expected.clone();
    hashed.result_hash.clear();
    assert_eq!(
        expected.result_hash,
        blake3::hash(&serde_json::to_vec(&hashed).unwrap())
            .to_hex()
            .as_str()
    );
    let exact = 24_534 + 2 * size_of::<flow_core::pdf::PdfScenePage>();
    assert_eq!(read(exact).unwrap(), expected);
    assert!(matches!(read(exact - 1), Err(PdfReadError::StreamLimit)));
}

#[test]
fn nested_form_marker_transfers_its_provenance_at_the_workspace_boundary() {
    let inner = format!("%{}\n10 10 5 5 re f", " ".repeat(20 * 1024));
    let read = |max_scene_bytes| {
        scene_with_nested_forms(
            ["/Outer Do", ""],
            ["1 0 0 1 300 0 cm /Inner Do", &inner],
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
    };
    let expected = read(PdfReaderLimits::default().max_scene_bytes).unwrap();
    assert!(matches!(
        expected.pages[0].elements.as_slice(),
        [PdfSceneElement::Annotation { value: outer }, PdfSceneElement::Annotation { value: inner }]
            if outer.provenance.object_ref.object_number == 6
                && inner.provenance.object_ref.object_number == 7
                && inner.provenance.invocation_path == [0, outer.provenance.stream_offset]
    ));
    let exact = 32_217 + size_of::<flow_core::pdf::PdfScenePage>();
    assert_eq!(read(exact).unwrap(), expected);
    assert!(matches!(read(exact - 1), Err(PdfReadError::StreamLimit)));
}

#[test]
fn repeated_nested_forms_spend_the_page_token_limit_including_array_entries() {
    let page = ["/Outer Do\n", "/Outer Do 10 10 5 5 re f"];
    let inner = format!("{}[1 2] 0 d 15 20 5 5 re f", "q Q ".repeat(10));
    let forms = ["q /Inner Do Q", inner.as_str()];
    let limits = PdfReaderLimits {
        max_content_tokens: 80,
        ..Default::default()
    };
    let scene = scene_with_nested_forms(page, forms, limits.clone()).unwrap();
    let paths: Vec<_> = scene.pages[0]
        .elements
        .iter()
        .filter_map(|element| match element {
            PdfSceneElement::Path { value } => Some(&value.provenance),
            _ => None,
        })
        .collect();
    assert!(matches!(paths.as_slice(), [first, second, last]
        if first.object_ref.object_number == 7 && first.invocation_path.len() == 3
            && first.invocation_path[0] == 0 && second.object_ref == first.object_ref
            && second.invocation_path.len() == 3 && second.invocation_path[0] == 1
            && second.stream_offset == first.stream_offset
            && last.object_ref.object_number == 5 && last.invocation_path == [1]));
    assert!(!scene.pages[0].partial);
    for max_content_tokens in [70, 79] {
        assert_eq!(
            scene_with_nested_forms(
                page,
                forms,
                PdfReaderLimits {
                    max_content_tokens,
                    ..limits.clone()
                },
            ),
            Err(PdfReadError::TokenLimit)
        );
    }
}

#[test]
fn nested_forms_charge_live_streams_and_indirect_resources_together() {
    let padded =
        |content, kibibytes: usize| format!("%{}\n{content}\n", " ".repeat(kibibytes * 1024));
    let document = |property_bytes| {
        let resources = format!(
            "<< /XObject 9 0 R /Properties << /MC0 << /ActualText ({}) >> >> >>",
            "a".repeat(property_bytes)
        );
        pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Outer 6 0 R >> >> /Contents [4 0 R 5 0 R] >>".to_vec()),
            stream(4, "", padded("/Outer Do", 12).as_bytes()),
            stream(5, "", padded("20 20 4 4 re f", 40).as_bytes()),
            stream(6, "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources 8 0 R", padded("q /Inner Do Q", 12).as_bytes()),
            stream(7, "/Type /XObject /Subtype /Form /BBox [0 0 100 100]", padded("10 10 4 4 re f", 12).as_bytes()),
            (8, resources.into_bytes()),
            (9, b"<< /Inner 7 0 R >>".to_vec()),
        ])
    };
    let read = |bytes: &[u8], max_scene_bytes| {
        let reader = open_pdf(
            bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(&reader, &PdfSceneRequest::default())
    };
    let constrained = 55 * 1024;
    assert!(read(&document(0), constrained).is_ok());

    let bytes = document(7 * 1024);
    assert_eq!(read(&bytes, constrained), Err(PdfReadError::StreamLimit));
    let expected = read(&bytes, PdfReaderLimits::default().max_scene_bytes).unwrap();
    assert_eq!(read(&bytes, 64 * 1024).unwrap(), expected);
    let page = &expected.pages[0];
    assert!(!page.partial);
    assert!(matches!(
        page.elements.as_slice(),
        [PdfSceneElement::Annotation { value: outer }, PdfSceneElement::Annotation { value: inner },
         PdfSceneElement::Path { value: nested }, PdfSceneElement::Path { value: following }]
        if outer.provenance.object_ref.object_number == 6
            && inner.provenance.object_ref.object_number == 7
            && nested.provenance.object_ref.object_number == 7
            && nested.provenance.invocation_path ==
                [0, outer.provenance.stream_offset, inner.provenance.stream_offset]
            && following.provenance.object_ref.object_number == 5
            && following.provenance.invocation_path == [1]
    ));
}

#[test]
fn nested_forms_share_the_inherited_page_resource_allowance() {
    let document = |property_bytes| {
        let resources = format!(
            "<< /XObject << /Outer 5 0 R /Inner 6 0 R >> /Properties << /MC0 << /ActualText ({}) >> >> >>",
            "a".repeat(property_bytes)
        );
        let paint = format!("%{}\n10 10 4 4 re f", " ".repeat(12 * 1024));
        pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources 7 0 R >>".to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".to_vec(),
            ),
            stream(4, "", b"/Outer Do"),
            stream(
                5,
                "/Type /XObject /Subtype /Form /BBox [0 0 100 100]",
                b"/Inner Do",
            ),
            stream(
                6,
                "/Type /XObject /Subtype /Form /BBox [0 0 100 100]",
                paint.as_bytes(),
            ),
            (7, resources.into_bytes()),
        ])
    };
    let read = |bytes: &[u8], max_scene_bytes| {
        let reader = open_pdf(
            bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(&reader, &PdfSceneRequest::default())
    };
    let constrained = 27 * 1024;
    assert!(read(&document(0), constrained).is_ok());
    let bytes = document(12 * 1024);
    assert_eq!(read(&bytes, constrained), Err(PdfReadError::StreamLimit));
    let expected = read(&bytes, PdfReaderLimits::default().max_scene_bytes).unwrap();
    assert_eq!(read(&bytes, 48 * 1024).unwrap(), expected);
    assert!(matches!(
        expected.pages[0].elements.last(),
        Some(PdfSceneElement::Path { value })
            if value.provenance.object_ref.object_number == 6
                && value.provenance.invocation_path.len() == 3
    ));
}

#[test]
fn scene_page_window_defers_unused_direct_resources() {
    let inherited = format!(
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 100 100] /Resources << /XObj#65ct << /Ou#74er 7 0 R /In#6Eer 8 0 R >> /Propert#69es << /MC#30 << /Subtype /Lay#65r /ActualText ({}) >> >> >> >>",
        "a".repeat(32 * 1024)
    );
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, inherited.into_bytes()),
        (3, b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_vec()),
        (4, b"<< /Type /Page /Parent 2 0 R /Resources << /XObj#65ct << /Ou#74er 7 0 R /In#6Eer 8 0 R >> >> /Contents 6 0 R >>".to_vec()),
        stream(5, "", b"/Outer Do"),
        stream(6, "", b"/Outer Do"),
        stream(7, "/Type /XObject /Subtype /Form /BBox [0 0 100 100]", b"/Inner Do"),
        stream(8, "/Type /XObject /Subtype /Form /BBox [0 0 100 100]", b"10 10 4 4 re f"),
    ]);
    let read = |first_page, max_scene_bytes| {
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(
            &reader,
            &PdfSceneRequest {
                first_page,
                page_count: 1,
            },
        )
    };
    let constrained = 24 * 1024;
    let roomy = PdfReaderLimits::default().max_scene_bytes;
    assert_eq!(read(0, constrained), Err(PdfReadError::StreamLimit));
    assert!(read(0, roomy).is_ok());
    let expected = read(1, roomy).unwrap();
    assert_eq!(read(1, constrained).unwrap(), expected);
    assert_eq!(expected.page_count, 2);
    assert_eq!(expected.pages[0].page_index, 1);
    assert!(matches!(
        expected.pages[0].elements.last(),
        Some(PdfSceneElement::Path { value })
            if value.provenance.object_ref.object_number == 8
                && value.provenance.invocation_path.len() == 3
    ));
}

#[test]
fn one_page_scene_charges_nested_discovery_and_resolves_links_outside_the_window() {
    let kids = |ids: Vec<u32>| {
        format!(
            "[{}]",
            ids.into_iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" ")
        )
        .into_bytes()
    };
    let mut objects = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids 5 0 R /Count 96 /MediaBox 6 0 R /CropBox [10 10 230 230] /Resources 9 0 R >>".to_vec()),
        (3, b"<< /Type /Pages /Parent 2 0 R /Kids 106 0 R /Count 48 >>".to_vec()),
        (4, b"<< /Type /Pages /Parent 2 0 R /Kids 107 0 R /Count 48 >>".to_vec()),
        (5, b"[3 0 R 4 0 R]".to_vec()),
        (6, b"[0 0 240 240]".to_vec()),
        (7, b"[108 0 R]".to_vec()),
        (8, b"[109 0 R]".to_vec()),
        (9, b"<< >>".to_vec()),
        (106, kids((10..58).collect())),
        (107, kids((59..106).chain([58]).collect())),
        stream(108, "", b"20 20 30 30 re f"),
        (109, b"<< /Type /Annot /Subtype /Link /Rect [15 15 50 50] /Dest 110 0 R >>".to_vec()),
        (110, b"[58 0 R /Fit]".to_vec()),
    ];
    objects.extend((10..106).map(|id| {
        let parent = if id < 58 { 3 } else { 4 };
        let entries = if id == 11 {
            "/Contents 7 0 R /Annots 8 0 R"
        } else {
            ""
        };
        (
            id,
            format!("<< /Type /Page /Parent {parent} 0 R {entries} >>").into_bytes(),
        )
    }));
    let bytes = pdf(objects);
    let request = PdfSceneRequest {
        first_page: 1,
        page_count: 1,
    };
    let read = |max_scene_bytes| {
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(reader.summary().unwrap().page_count, 96);
        read_pdf_scene(&reader, &request)
    };
    let expected = read(PdfReaderLimits::default().max_scene_bytes).unwrap();
    assert_eq!(expected.page_count, 96);
    let [page] = expected.pages.as_slice() else {
        panic!("expected one scene page");
    };
    assert_eq!(page.page_index, 1);
    assert!(!page.partial);
    let [
        PdfSceneElement::Link { value: link },
        PdfSceneElement::Path { value: path },
    ] = page.elements.as_slice()
    else {
        panic!("expected an internal link and a filled path");
    };
    assert!(link.internal);
    assert_eq!(link.destination_page, Some(95));
    assert!(path.fill);
    assert_eq!(path.provenance.object_ref.object_number, 108);
    assert_eq!(read(64 * 1024).unwrap(), expected);
    assert_eq!(read(12 * 1024), Err(PdfReadError::StreamLimit));
}

#[test]
fn scene_page_slots_share_the_live_window_allowance() {
    let mut objects = vec![(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec())];
    let kids = (3..35)
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push((
        2,
        format!("<< /Type /Pages /Kids [{kids}] /Count 32 /MediaBox [0 0 200 200] >>").into_bytes(),
    ));
    objects.extend((3..35).map(|id| {
        (
            id,
            b"<< /Type /Page /Parent 2 0 R /Contents 35 0 R >>".to_vec(),
        )
    }));
    objects.push(stream(35, "", b"20 20 40 10 re f"));
    let bytes = pdf(objects);
    let read = |max_scene_bytes| {
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(
            &reader,
            &PdfSceneRequest {
                first_page: 4,
                page_count: 24,
            },
        )
    };
    let expected = read(PdfReaderLimits::default().max_scene_bytes).unwrap();
    assert_eq!(expected.page_count, 32);
    assert_eq!(expected.pages.len(), 24);
    for (page, index) in expected.pages.iter().zip(4..28) {
        assert_eq!(page.page_index, index);
        assert_eq!(page.source_object.object_number, index + 3);
        assert!(
            matches!(page.elements.as_slice(), [PdfSceneElement::Path { value }] if value.fill)
        );
    }
    assert_eq!(read(32 * 1024).unwrap(), expected);
    // @invariant: This allowance fits the read only when output page slots are omitted.
    assert_eq!(read(29 * 1024), Err(PdfReadError::StreamLimit));
}

#[test]
fn nested_form_paths_reserve_transformed_paint_and_clip_storage() {
    let mut outline = String::from("10 10 m 5 30 85 3 10 15 c ");
    for x in 11..=75 {
        outline.push_str(&format!("{x} 15 l {x} 20 l "));
    }
    for operator in ["s", "W n"] {
        let paint = format!("{outline}{operator}");
        let read = |max_scene_bytes| {
            scene_with_nested_forms(
                ["/Outer Do", ""],
                ["/Inner Do", &paint],
                PdfReaderLimits {
                    max_scene_bytes,
                    ..Default::default()
                },
            )
        };
        let expected = read(PdfReaderLimits::default().max_scene_bytes).unwrap();
        assert_eq!(read(128 * 1024).unwrap(), expected);
        let element = expected.pages[0].elements.last().unwrap();
        let (commands, provenance, rect) = match element {
            PdfSceneElement::Path { value } if operator == "s" => {
                (&value.commands, &value.provenance, value.rect)
            }
            PdfSceneElement::Clip { value } if operator == "W n" => {
                (&value.commands, &value.provenance, value.rect)
            }
            other => panic!("unexpected scene element {other:?}"),
        };
        assert_eq!(commands.len(), if operator == "s" { 133 } else { 132 });
        assert_eq!(
            [
                rect.x.raw(),
                rect.y.raw(),
                rect.width.raw(),
                rect.height.raw()
            ],
            [5 * 64, 3 * 64, 80 * 64, 27 * 64]
        );
        assert_eq!(provenance.object_ref.object_number, 7);
        assert_eq!(provenance.invocation_path.len(), 3);
        assert_eq!(
            read(if operator == "s" {
                44 * 1024
            } else {
                55 * 1024
            }),
            Err(PdfReadError::StreamLimit),
            "{operator}"
        );
    }
}

#[test]
fn form_graphics_saves_count_toward_the_enclosing_stack_limit() {
    let page = ["q q\n", "/Outer Do Q Q 10 10 5 5 re f"];
    let forms = ["1 0 0 1 20 0 cm /Inner Do", "q Q 15 20 5 5 re f"];
    let scene = scene_with_nested_forms(
        page,
        forms,
        PdfReaderLimits {
            max_nesting: 5,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!scene.pages[0].partial);
    assert!(
        matches!(scene.pages[0].elements.last(), Some(PdfSceneElement::Path { value })
        if value.rect.x.raw() == 10 * 64 && value.provenance.object_ref.object_number == 5
            && value.provenance.invocation_path == [1])
    );

    let limits = PdfReaderLimits {
        max_nesting: 4,
        ..Default::default()
    };
    for (page, forms) in [
        (page, forms),
        (["q q q\n", "/Outer Do Q Q Q"], ["/Inner Do", ""]),
        (["q q q q\n", "q Q Q Q Q Q"], ["", ""]),
    ] {
        assert_eq!(
            scene_with_nested_forms(page, forms, limits.clone()),
            Err(PdfReadError::TokenLimit)
        );
    }
    assert_eq!(
        scene_with_nested_forms(page, ["/Inner Do", "Q"], limits),
        Err(PdfReadError::Syntax)
    );
}

#[test]
fn graphics_state_allowance_covers_stack_slots_and_restored_paths() {
    let read = |first, second, max_scene_bytes| {
        scene_with_nested_forms(
            [first, second],
            ["", ""],
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
    };
    let saves = format!("{}{}\n", "q ".repeat(48), "Q ".repeat(48));
    assert_eq!(read(&saves, "", 8 * 1024), Err(PdfReadError::StreamLimit));
    assert!(read(&saves, "", 96 * 1024).is_ok());

    let path = "10 10 5 5 re ".repeat(16);
    let restored = format!("{path}q Q\n");
    let discarded = format!("q {path}Q\n");
    let following = format!("%{}\nf", " ".repeat(26 * 1024));
    let expected = read(
        &restored,
        &following,
        PdfReaderLimits::default().max_scene_bytes,
    )
    .unwrap();
    assert!(matches!(
        expected.pages[0].elements.as_slice(),
        [PdfSceneElement::Path { value }] if value.commands.len() == 80
            && value.provenance.object_ref.object_number == 5
            && value.provenance.invocation_path == [1]
    ));
    assert_eq!(read(&restored, &following, 54 * 1024).unwrap(), expected);
    assert_eq!(
        read(&restored, &following, 37 * 1024),
        Err(PdfReadError::StreamLimit)
    );
    assert!(read(&restored, "", 40 * 1024).is_ok());
    assert!(read(&discarded, &following, 37 * 1024).is_ok());
}

#[test]
fn nested_forms_share_and_release_saved_path_and_clip_bytes() {
    let path = "10 10 5 5 re ".repeat(8);
    let first = format!("{path}W q\n");
    let second = "/Outer Do /Outer Do Q n";
    let outer = format!("1 0 0 1 20 0 cm {path}q /Inner Do Q n");
    let inner = format!("{path}q Q n 15 20 5 5 re f");
    let forms = [outer.as_str(), inner.as_str()];
    let read = |first, max_scene_bytes| {
        scene_with_nested_forms(
            [first, second],
            forms,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
    };
    let expected = read(&first, PdfReaderLimits::default().max_scene_bytes).unwrap();
    let admitted = read(&first, 70 * 1024).unwrap();
    assert_eq!(admitted, expected);
    let elements = &admitted.pages[0].elements;
    let paths: Vec<_> = elements
        .iter()
        .filter_map(|element| match element {
            PdfSceneElement::Path { value } => Some(value),
            _ => None,
        })
        .collect();
    assert!(matches!(paths.as_slice(), [first, second]
        if [first, second].into_iter().all(|value| value.rect.x.raw() == 35 * 64
            && value.provenance.object_ref.object_number == 7
            && value.provenance.invocation_path.len() == 3)
            && first.provenance.invocation_path != second.provenance.invocation_path));
    assert!(matches!(
        elements.last(),
        Some(PdfSceneElement::Clip { value }) if value.commands.len() == 40
            && value.rect.x.raw() == 10 * 64
            && value.provenance.object_ref.object_number == 4
            && value.provenance.invocation_path == [0]
            && value.provenance.operator.as_deref() == Some("W")
    ));
    let constrained = 57 * 1024;
    assert_eq!(read(&first, constrained), Err(PdfReadError::StreamLimit));
    let without_pending_clip = format!("{path}q\n");
    assert!(read(&without_pending_clip, constrained).is_ok());
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
fn page_and_form_clips_omit_only_excluded_ordinary_paint() {
    for in_form in [false, true] {
        for (kind, paint, retained, partial) in [
            ("S", "1 w 35 50 m 55 50 l S", true, false),
            ("S", "8 w 30 103 m 70 103 l S", true, true),
            ("S", "1 w 30 120 m 70 120 l S", false, false),
            ("B", "1 M 8 w 30 103 10 10 re B", true, true),
            ("B", "1 M 8 w 30 120 10 10 re B", false, false),
            ("f", "40 40 8 4 re f", true, false),
            ("f", "80 98 8 4 re f", true, true),
            ("f", "1 0 0 1.004 0 0 cm 80 99.875 8 4 re f", false, false),
            ("f", "40 40 0 4 re f", false, false),
            ("f", "40 40 .004 4 re f", false, true),
            ("f", "40 40 .0000000000000001 4 re f", false, true),
            ("f", "120 40 .0000000000000001 4 re f", false, false),
            ("Do", "8 0 0 4 40 40 cm /Im Do", true, false),
            ("Do", "8 0 0 4 80 98 cm /Im Do", true, true),
            (
                "Do",
                "1 0 0 1.004 0 0 cm 8 0 0 4 80 99.875 cm /Im Do",
                false,
                false,
            ),
            ("Do", "8 .004 0 4 40 40 cm /Im Do", false, true),
        ] {
            let (crop, page_content, paint_stream) = if in_form {
                ("", "/F Do", 5)
            } else {
                ("/CropBox [20 20 100 100]", paint, 4)
            };
            let bytes = pdf(vec![
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
                (3, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] {crop} /Resources << /XObject << /F 5 0 R /Im 6 0 R >> >> /Contents 4 0 R >>").into_bytes()),
                stream(4, "", page_content.as_bytes()),
                stream(5, "/Type /XObject /Subtype /Form /BBox [20 20 100 100]", paint.as_bytes()),
                stream(6, "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[0x12]),
            ]);
            let reader = open_pdf(&bytes, Default::default()).unwrap();
            let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
            let page = &scene.pages[0];
            let artwork: Vec<_> = page
                .elements
                .iter()
                .filter_map(|element| match element {
                    PdfSceneElement::Path { value } => Some((&value.provenance, paint_stream)),
                    PdfSceneElement::Image { value } => Some((&value.provenance, 6)),
                    _ => None,
                })
                .collect();
            assert_eq!(artwork.len(), usize::from(retained), "{in_form} {paint}");
            if let Some((source, object_number)) = artwork.first() {
                assert_eq!(source.object_ref.object_number, *object_number);
                assert_eq!(source.operator.as_deref(), Some(kind));
                assert_eq!(source.invocation_path.len(), if in_form { 2 } else { 1 });
            }
            assert_eq!(page.partial, partial, "{in_form} {paint}");
            assert_eq!(scene.report.partial, partial);
            assert_eq!(page.report.diagnostics.len(), usize::from(partial));
            if partial {
                let diagnostic = &page.report.diagnostics[0];
                assert_eq!(diagnostic.code, PdfReadDiagnosticCode::PartialPage);
                assert_eq!(diagnostic.object_number, Some(paint_stream));
            }
        }
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
        stream(5, "/Type /XObject /Subtype /Form /BBox [10 10 40 40] /Matrix [2 0 0 1 5 0] /Resources << /XObject << /In#6Eer 6 0 R >> >>", b"/#49#6E#6E#65#72 Do"),
        stream(6, "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /Font << /F#E9 7 0 R >> >>", b"BT /#46#e9 10 Tf 15 25 Td (A) Tj ET 28 20 8 5 re f 35 30 2 2 re f 2 20 3 3 re f"),
        (7, b"<< /Type /Font /Subtype /Type1 /BaseFont /#D0#A8#D1#80#D0#B8#D1#84#D1#82 /MissingWidth 500 /ToUnicode 8 0 R >>".to_vec()),
        stream(8, "", b"begincmap\n1 beginbfchar\n<41> <0041>\nendbfchar\nendcmap\n"),
        stream(9, "/Type /XObject /Subtype /Form /BBox [70 0 90 20]", b"75 5 5 5 re f"),
    ]);
    let reader = open_pdf(&bytes, Default::default()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    let constrained = open_pdf(
        &bytes,
        PdfReaderLimits {
            max_scene_bytes: 48 * 1024,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        read_pdf_scene(&constrained, &PdfSceneRequest::default()).unwrap(),
        scene
    );
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
fn recovered_font_syntax_releases_scene_allowance_for_the_next_page() {
    let mut interrupted =
        b"% caf\xe9\nbegincmap\n1 beginbfrange\n<20> <83> <0020>\nendbfrange\n".to_vec();
    interrupted.extend_from_slice(b"1 beginbfchar\n<41> <D800>\nendbfchar\nendcmap\n");
    let complete = b"begincmap\n1 beginbfrange\n<20> <5F> <0020>\nendbfrange\nendcmap\n";
    let bytes = pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 100 100] >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 7 0 R >> >> /Contents [11 0 R 5 0 R] >>".to_vec()),
        (4, b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 8 0 R >> >> /Contents 6 0 R >>".to_vec()),
        stream(5, "", b"BT /F1 10 Tf 15 25 Td (A) Tj ET"),
        stream(6, "", b"BT /F1 10 Tf 15 25 Td (NEXT PAGE) Tj ET"),
        (7, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 9 0 R >>".to_vec()),
        (8, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 10 0 R >>".to_vec()),
        stream(9, "", &interrupted),
        stream(10, "", complete),
        stream(11, "", b"10 10 10 10 re f"),
    ]);
    let read = |max_scene_bytes| {
        let reader = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(
            &reader,
            &PdfSceneRequest {
                first_page: 0,
                page_count: 2,
            },
        )
    };
    let expected = read(PdfReaderLimits::default().max_scene_bytes).unwrap();
    assert!(expected.pages[0].partial);
    assert!(matches!(
        expected.pages[0].elements.as_slice(),
        [PdfSceneElement::Path { .. }]
    ));
    assert!(
        matches!(expected.pages[1].elements.as_slice(), [PdfSceneElement::Text { value }]
        if value.text == "NEXT PAGE" && value.mapping == PdfTextMapping::ToUnicode)
    );
    assert_eq!(read(96 * 1024).unwrap(), expected);
    assert_eq!(read(16 * 1024), Err(PdfReadError::StreamLimit));
}

#[test]
fn nested_form_font_aliases_share_maps_and_the_scene_allowance() {
    let document = |inner_font, cmap_prefix: &[u8]| {
        let widths = "600 ".repeat(64);
        let font = format!(
            "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 600 700] \
             /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /A 11 0 R >> \
             /Encoding << /Type /Encoding /Differences [65 /A] >> /Resources << >> \
             /FirstChar 32 /LastChar 95 /Widths [{widths}] /ToUnicode 10 0 R >>"
        );
        let mut cmap = b"begincmap\n".to_vec();
        cmap.extend_from_slice(cmap_prefix);
        cmap.extend_from_slice(
            b"1 begincodespacerange\n<00> <FF>\nendcodespacerange\n\
              1 beginbfrange\n<20> <5F> <0020>\nendbfrange\n\
              1 beginbfchar\n<41> <0410>\nendbfchar\nendcmap\n",
        );
        let inner_resources = format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 100 100] \
             /Resources << /Font << /InnerAlias {inner_font} 0 R >> >>"
        );
        pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 100 100] /Resources << /Font << /PageAlias 8 0 R >> /XObject << /Outer 6 0 R >> >> >>".to_vec()),
            (3, b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_vec()),
            (4, b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_vec()),
            stream(5, "", b"BT /PageAlias 10 Tf 15 25 Td (A) Tj ET /Outer Do BT 15 65 Td (A) Tj ET"),
            stream(6, "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /Font << /OuterAlias 8 0 R >> /XObject << /Inner 7 0 R >> >>", b"BT /OuterAlias 10 Tf 15 35 Td (A) Tj ET /Inner Do BT 15 55 Td (A) Tj ET"),
            stream(7, &inner_resources, b"BT /InnerAlias 10 Tf 15 45 Td (A) Tj ET"),
            (8, font.as_bytes().to_vec()),
            (9, font.into_bytes()),
            stream(10, "", &cmap),
            stream(11, "", b"600 0 d0 0 0 600 700 re f"),
        ])
    };
    let read = |bytes: &[u8], max_scene_bytes| {
        let reader = open_pdf(
            bytes,
            PdfReaderLimits {
                max_scene_bytes,
                ..Default::default()
            },
        )
        .unwrap();
        read_pdf_scene(
            &reader,
            &PdfSceneRequest {
                first_page: 0,
                page_count: 2,
            },
        )
    };

    let shared = document(8, b"");
    let default_limit = PdfReaderLimits::default().max_scene_bytes;
    let constrained = 94 * 1024;
    let scene = read(&shared, constrained).unwrap();
    assert_eq!(scene, read(&shared, default_limit).unwrap());
    assert_eq!(scene.pages.len(), 2);
    for page in &scene.pages {
        let labels: Vec<_> = page
            .elements
            .iter()
            .filter_map(|element| match element {
                PdfSceneElement::Text { value } => {
                    assert_eq!(value.text, "А");
                    assert_eq!(value.mapping, PdfTextMapping::ToUnicode);
                    assert_eq!(value.glyphs[0].advance.raw(), 6 * 64);
                    value.font_name.as_deref()
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            labels,
            [
                "PageAlias",
                "OuterAlias",
                "InnerAlias",
                "OuterAlias",
                "PageAlias"
            ]
        );
    }
    assert_eq!(read(&shared, 24 * 1024), Err(PdfReadError::StreamLimit));

    let distinct = document(9, b"");
    assert!(read(&distinct, default_limit).is_ok());
    assert_eq!(read(&distinct, constrained), Err(PdfReadError::StreamLimit));

    let comment_allowance = 128 * 1024;
    let ascii_comments = document(8, &b"% cafe\n".repeat(7_000));
    assert!(read(&ascii_comments, comment_allowance).is_ok());
    let legacy_comments = document(8, &b"% caf\xe9\n".repeat(7_000));
    assert!(read(&legacy_comments, default_limit).is_ok());
    assert_eq!(
        read(&legacy_comments, comment_allowance),
        Err(PdfReadError::StreamLimit)
    );
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
fn decimal_transform_with_nonzero_determinant_can_round_to_collinear_fill() {
    let scene = read_content_scene("1 1 1 1.0000000000000001 0 0 cm 10 10 20 20 re f");
    let page = &scene.pages[0];
    let [PdfSceneElement::Path { value }] = page.elements.as_slice() else {
        panic!("expected the quantized path");
    };
    assert!(value.fill && value.rect.width.raw() > 0);
    assert!(value.commands.iter().all(|command| match command {
        PdfPathCommand::MoveTo { x, y } | PdfPathCommand::LineTo { x, y } => x == y,
        PdfPathCommand::ClosePath => true,
        PdfPathCommand::CurveTo { .. } => false,
    }));
    assert!(page.partial && scene.report.partial);
    assert!(matches!(page.report.diagnostics.as_slice(), [diagnostic]
        if diagnostic.code == PdfReadDiagnosticCode::PartialPage
            && diagnostic.object_number == Some(4)));
}

#[test]
fn fill_reports_an_implicit_contour_that_starts_at_the_closed_origin() {
    for closed in ["50 20 20 20 re", "50 20 m 70 20 l 70 40 l 50 40 l h"] {
        let scene = read_content_scene(&format!("{closed} 50.004 25 l 50 30 l h f"));
        let page = &scene.pages[0];
        assert!(page.partial && scene.report.partial);
        assert!(
            matches!(page.elements.as_slice(), [PdfSceneElement::Path { value }]
            if value.fill && value.provenance.operator.as_deref() == Some("f"))
        );
        assert!(matches!(page.report.diagnostics.as_slice(), [diagnostic]
            if diagnostic.code == PdfReadDiagnosticCode::PartialPage
                && diagnostic.object_number == Some(4)));
    }
}

#[test]
fn compound_fill_reports_visible_contours_lost_to_quantization() {
    for (clip, small, retained, partial) in [
        ("", "50.125 20 0.004 10 re", true, true),
        ("", "50.125 20 0.0000000000000001 10 re", true, true),
        ("", "50 20 m 50.004 22 50.004 28 50 30 c", true, true),
        ("", "50 20 m 50.004 28 50 30 v", true, true),
        ("", "50 20 m 50.004 22 50 30 y", true, true),
        ("", "50 20 m 55 25.004 l 60 30 l h", true, true),
        ("", "50 20 0 10 re", true, false),
        ("", "50 20 m 55 25 60 30 65 35 c", true, false),
        (
            "",
            "50.1 20.1 m 55.2 25.2 60.3 30.3 65.4 35.4 c",
            true,
            false,
        ),
        (
            "100 100 20 20 re W n",
            "50.125 20 0.004 10 re",
            false,
            false,
        ),
    ] {
        let scene = read_content_scene(&format!("{clip} 10 10 20 20 re {small} f"));
        let page = &scene.pages[0];
        let paths: Vec<_> = page
            .elements
            .iter()
            .filter_map(|element| match element {
                PdfSceneElement::Path { value } => Some(value),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), usize::from(retained), "{small}");
        if let Some(path) = paths.first() {
            assert!(path.fill && !path.stroke);
            assert_eq!(path.provenance.object_ref.object_number, 4);
            assert_eq!(path.provenance.operator.as_deref(), Some("f"));
            assert_eq!(
                path.commands
                    .iter()
                    .filter(|command| matches!(command, PdfPathCommand::MoveTo { .. }))
                    .count(),
                2
            );
        }
        assert_eq!(page.partial, partial, "{small}");
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
        (
            "20 20 100 100 re W n",
            "[3 2] 0 d 40 60 m 80 60 l S",
            true,
            true,
        ),
        (
            "20 20 100 100 re W n",
            "q [3 2] 0 d Q 40 60 m 80 60 l S",
            true,
            false,
        ),
        (
            "20 20 100 100 re W n",
            "[3 2] 0 d q [] 0 d Q 40 60 m 80 60 l S",
            true,
            true,
        ),
        (
            "20 20 100 100 re W n",
            "[3 2] 0 d [] 0 d 40 60 m 80 60 l S",
            true,
            false,
        ),
        (
            "20 20 100 100 re W n",
            "[3 2] 0 d 140 160 m 180 160 l S",
            false,
            false,
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
fn fractional_page_crops_report_only_potentially_visible_text() {
    for inherited in [false, true] {
        for (crop, partial) in [
            ("/CropBox [20.001 20 20.002 80]", true),
            ("/CropBox [200.001 20 200.002 80]", false),
        ] {
            let (parent_crop, page_crop) = if inherited { (crop, "") } else { ("", crop) };
            let bytes = pdf(vec![
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (2, format!("<< /Type /Pages /Kids [3 0 R] /Count 1 {parent_crop} >>").into_bytes()),
                (3, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] {page_crop} /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>").into_bytes()),
                stream(4, "", b"BT /F1 10 Tf 20 40 Td (A) Tj ET"),
                (5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /MissingWidth 500 >>".to_vec()),
            ]);
            let reader = open_pdf(&bytes, Default::default()).unwrap();
            assert_eq!(reader.summary().unwrap().page_count, 1);
            let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
            let page = &scene.pages[0];
            assert!(page.elements.is_empty(), "{crop}, inherited={inherited}");
            assert_eq!(page.partial, partial, "{crop}, inherited={inherited}");
            assert_eq!(scene.report.partial, partial);
            if partial {
                assert!(matches!(
                    page.report.diagnostics.as_slice(),
                    [diagnostic] if diagnostic.code == PdfReadDiagnosticCode::PartialPage
                        && diagnostic.page_index == Some(0)
                        && diagnostic.object_number == Some(4)
                ));
            } else {
                assert!(page.report.diagnostics.is_empty());
            }
        }
    }
}

#[test]
fn page_discovery_requires_layout_area_for_media_and_positive_area_for_crop() {
    for boxes in [
        "/MediaBox [20.001 20 20.002 80]",
        "/MediaBox [0 0 200 200] /CropBox [20.001 20 20.001 80]",
    ] {
        let bytes = pdf(vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                format!("<< /Type /Page /Parent 2 0 R {boxes} >>").into_bytes(),
            ),
        ]);
        let reader = open_pdf(&bytes, Default::default()).unwrap();
        assert_eq!(reader.summary(), Err(PdfReadError::PageTreeInvalid));
    }
}

#[test]
fn scene_loads_field_roots_only_for_windows_with_widgets() {
    for indirect_fields in [false, true] {
        let mut objects = vec![
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [7 0 R] >>".to_vec(),
            ),
            (
                4,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [5 0 R] >>".to_vec(),
            ),
            (
                5,
                b"<< /Type /Annot /Subtype /Widget /Parent 6 0 R /Rect [10 10 40 30] >>".to_vec(),
            ),
            (
                6,
                b"<< /FT /Tx /T (recipient) /V (Olena) /Kids [5 0 R] >>".to_vec(),
            ),
            (
                7,
                b"<< /Type /Annot /Subtype /Link /Rect [10 10 40 30] /Dest [4 0 R /Fit] >>"
                    .to_vec(),
            ),
        ];
        let mut fields = String::from("[6 0 R");
        for id in 9..89 {
            use std::fmt::Write as _;
            write!(fields, " {id} 0 R").unwrap();
            objects.push((
                id,
                format!("<< /FT /Tx /T (entry-{id}) /V (pending) >>").into_bytes(),
            ));
        }
        fields.push(']');
        if indirect_fields {
            objects.push((8, fields.into_bytes()));
            fields = "8 0 R".to_owned();
        }
        objects.push((
            1,
            format!("<< /Type /Catalog /Pages 2 0 R /OpenAction [3 0 R /Fit] /AcroForm << /Fields {fields} >> >>").into_bytes(),
        ));
        let bytes = pdf(objects);
        let roomy = open_pdf(&bytes, Default::default()).unwrap();
        let constrained = open_pdf(
            &bytes,
            PdfReaderLimits {
                max_scene_bytes: 6 * 1024,
                ..Default::default()
            },
        )
        .unwrap();
        let read = |reader, first_page| {
            read_pdf_scene(
                reader,
                &PdfSceneRequest {
                    first_page,
                    page_count: 1,
                },
            )
        };
        let summary = constrained.summary().unwrap();
        let widget_free = read(&constrained, 0).unwrap();
        assert_eq!(summary, roomy.summary().unwrap());
        assert_eq!(summary.page_count, 2);
        assert_eq!(widget_free.page_count, summary.page_count);
        assert_eq!(widget_free.source_hash, summary.source_hash);
        assert_eq!(widget_free, read(&roomy, 0).unwrap());
        assert!(widget_free.report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == PdfReadDiagnosticCode::ActiveContent
                && diagnostic.object_number == Some(1)
        }));
        let [PdfSceneElement::Link { value }] = widget_free.pages[0].elements.as_slice() else {
            panic!("expected a link");
        };
        assert_eq!(value.destination_page, Some(1));

        let with_widget = read(&roomy, 1).unwrap();
        let [PdfSceneElement::Form { value }] = with_widget.pages[0].elements.as_slice() else {
            panic!("expected a widget");
        };
        assert_eq!(value.field_type.as_deref(), Some("Tx"));
        assert_eq!(value.name.as_deref(), Some("recipient"));
        assert_eq!(value.value.as_deref(), Some("Olena"));
        assert_eq!(read(&constrained, 1), Err(PdfReadError::StreamLimit));
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
    use PdfReadDiagnosticCode::{ActiveContent, ContentLimit, UnsupportedAnnotation};

    for (entries, destination, max_nesting, max_array_items, expected, diagnostic) in [
        ("/Dest 5 0 R", "6 0 R", 8, 6, Some(0), None),
        ("/Dest 5 0 R", "6 0 R", 4, 6, None, Some(ContentLimit)),
        ("/Dest 5 0 R", "6 0 R", 8, 4, None, Some(ContentLimit)),
        ("/A << /S /GoTo /D 8 0 R >>", "null", 4, 6, Some(0), None),
        (
            "/A << /S /GoToR /D 8 0 R >>",
            "null",
            4,
            6,
            None,
            Some(ActiveContent),
        ),
        ("/Dest 5 0 R", "5 0 R", 4, 6, None, Some(ContentLimit)),
        (
            "/Dest 5 0 R",
            "[3 0 R /FitR null 0 20 30]",
            4,
            6,
            None,
            Some(UnsupportedAnnotation),
        ),
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
                max_array_items,
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
        assert_eq!(scene.pages[0].partial, diagnostic.is_some());
        assert_eq!(scene.report.partial, diagnostic.is_some());
        if let Some(code) = diagnostic {
            assert!(scene.report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.page_index == Some(0)
                    && diagnostic.object_number == Some(4)
            }));
        }
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
        let mut annotations = (8..=20).collect::<Vec<_>>();
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
        objects.extend((8..=20).map(|id| {
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
        let page = &scene.pages[0];
        assert!(page.partial);
        assert!(scene.report.partial);
        let mut links = page.elements.iter().filter_map(|element| match element {
            PdfSceneElement::Link { value } => Some(value),
            _ => None,
        });
        for object_number in 8..=20 {
            let link = links.next().unwrap();
            assert_eq!(link.provenance.object_ref.object_number, object_number);
            assert_eq!(link.destination_page, (object_number < 20).then_some(0));
            assert_eq!(link.internal, object_number < 20);
        }
        assert!(links.next().is_none());
        assert!(page.report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == PdfReadDiagnosticCode::ContentLimit
                && diagnostic.page_index == Some(0)
                && diagnostic.object_number == Some(20)
        }));
        assert!(scene.pages[0].elements.iter().any(|element| {
            matches!(element, PdfSceneElement::Form { value }
                if value.field_type.as_deref() == Some("Tx")
                && value.name.as_deref() == Some("recipient")
                && value.value.as_deref() == Some("saved"))
        }));
    }
}

#[test]
fn unfinished_text_operands_across_page_streams_preserve_committed_paint() {
    let prefix = b"10 20 5 5 re f\nBT /F1 12 Tf 20 40 Td ";
    let contents = "[4 0 R 5 0 R]";
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
    for (start, end, operator) in [
        (
            b"[(Hello) 0 0 0\n".as_slice(),
            b"] TJ ET\n".as_slice(),
            "TJ",
        ),
        (b"[(Hel", b"lo) 0 0 0] TJ ET\n", "TJ"),
        (b"[(Hel\\", b"\nlo) 0 0 0] TJ ET\n", "TJ"),
        (b"[<4865", b"6c6c6f> 0 0 0] TJ ET\n", "TJ"),
        (b"(Hel", b"lo) Tj ET\n", "Tj"),
    ] {
        let first = [prefix.as_slice(), start].concat();
        let split = fixture(contents, vec![stream(4, "", &first), stream(5, "", end)]);
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
            vec![stream(4, "", &[first.as_slice(), end].concat())],
        );
        let reader = open_pdf(&joined, limits.clone()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        assert!(matches!(
            page.elements.as_slice(),
            [PdfSceneElement::Path { .. }, PdfSceneElement::Text { value }] if value.text == "Hello"
                && value.provenance.object_ref.object_number == 4
                && value.provenance.operator.as_deref() == Some(operator)
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

        let truncated = fixture("4 0 R", vec![stream(4, "", &first)]);
        let reader = open_pdf(&truncated, limits.clone()).unwrap();
        assert!(matches!(
            read_pdf_scene(&reader, &PdfSceneRequest::default()),
            Err(PdfReadError::Syntax)
        ));
    }

    for operand in [b"[(Hel".as_slice(), b"[<4Z"] {
        let invalid_final = fixture(
            contents,
            vec![stream(4, "", b"10 20 5 5 re f "), stream(5, "", operand)],
        );
        let reader = open_pdf(&invalid_final, limits.clone()).unwrap();
        assert!(matches!(
            read_pdf_scene(&reader, &PdfSceneRequest::default()),
            Err(PdfReadError::Syntax)
        ));
    }

    for (operand, operand_limits, expected) in [
        (b"[<4Z".to_vec(), limits.clone(), PdfReadError::Syntax),
        (b"[<<".to_vec(), limits.clone(), PdfReadError::Syntax),
        (
            b"[(Hello) 0 0 0 0\n".to_vec(),
            limits,
            PdfReadError::TokenLimit,
        ),
        (
            [b"[[".as_slice(), &b"0 ".repeat(64)].concat(),
            PdfReaderLimits {
                max_content_tokens: 64,
                ..Default::default()
            },
            PdfReadError::TokenLimit,
        ),
    ] {
        let bytes = fixture(
            contents,
            vec![
                stream(4, "", &[prefix.as_slice(), &operand].concat()),
                stream(5, "", b"] TJ ET\n"),
            ],
        );
        let reader = open_pdf(&bytes, operand_limits).unwrap();
        assert_eq!(
            read_pdf_scene(&reader, &PdfSceneRequest::default()),
            Err(expected)
        );
    }
}

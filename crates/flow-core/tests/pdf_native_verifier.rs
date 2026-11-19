use flow_core::pdf::native::{
    NativeBinding, NativeRedactionContext, NativeRedactionGeometry, NativeRedactionMark,
    NativeRedactionPolicy, NativeVerificationFailure, NativeVerificationStatus,
    verify_native_redaction,
};

fn stream(dict: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} {dict} >>\nstream\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\nendstream");
    out
}

fn pdf(pages: &[(&str, &str)], extra: Vec<Vec<u8>>, trailer: &str) -> Vec<u8> {
    let kids = (0..pages.len())
        .map(|page| format!("{} 0 R", page * 2 + 3))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [{kids}] /Count {} /MediaBox [0 0 100 100] >>",
            pages.len()
        )
        .into_bytes(),
    ];
    for (page, (content, entries)) in pages.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /Contents {} 0 R {entries} >>",
                page * 2 + 4
            )
            .into_bytes(),
        );
        objects.push(stream("", content.as_bytes()));
    }
    objects.extend(extra);
    pdf_objects(objects, trailer)
}

fn pdf_objects(objects: Vec<Vec<u8>>, trailer: &str) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    let size = offsets.len() + 1;
    bytes.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R {trailer} >>\nstartxref\n{xref}\n%%EOF\n")
            .as_bytes(),
    );
    bytes
}

const SECRET: &str = "1 0 0 rg 10 10 10 10 re f ";
const PUBLIC: &str = "0 g 55 55 10 10 re f";
const GEOMETRY: NativeRedactionGeometry = NativeRedactionGeometry {
    media_box: [0, 0, 100_000, 100_000],
    crop_box: [0, 0, 100_000, 100_000],
    rotation: 0,
};

fn mark(page_id: u64, rect: [i64; 4]) -> NativeRedactionMark {
    NativeRedactionMark {
        page_id,
        geometry: GEOMETRY,
        rect,
    }
}

fn verify(
    accepted: &[u8],
    candidate: &[u8],
    order: &[u64],
    marks: &[NativeRedactionMark],
) -> flow_core::pdf::native::VersionedNativeRedactionEvidence {
    let binding = NativeBinding {
        epoch: [7; 16],
        session_id: 9,
        origin_hash: *blake3::hash(accepted).as_bytes(),
        revision: 4,
    };
    let context = NativeRedactionContext {
        binding,
        request_id: 8,
        accepted_hash: *blake3::hash(accepted).as_bytes(),
        page_order: order,
    };
    let policy = NativeRedactionPolicy {
        binding,
        request_id: 8,
        candidate_hash: *blake3::hash(candidate).as_bytes(),
        marks,
    };
    verify_native_redaction(accepted, accepted, candidate, context, policy)
}

#[test]
fn fresh_structure_and_full_page_raster_pass_a_scrubbed_pair() {
    let accepted = pdf(
        &[(&format!("{SECRET}{PUBLIC}"), "")],
        vec![b"<< /Title (Private) >>".to_vec()],
        "/Info 5 0 R",
    );
    let candidate = pdf(&[(PUBLIC, "")], vec![], "");
    let evidence = verify(
        &accepted,
        &candidate,
        &[91],
        &[mark(91, [9_000, 9_000, 12_000, 12_000])],
    );
    assert!(evidence.checks_passed(), "{evidence:?}");
    assert_eq!(
        evidence.structure().status(),
        NativeVerificationStatus::Passed
    );
    assert_eq!(evidence.text().status(), NativeVerificationStatus::Passed);
    assert_eq!(evidence.raster().status(), NativeVerificationStatus::Passed);
    assert_eq!(evidence.raster().covered_pages()[0].page_id, 91);
}

fn independent_page_streams(
    first: &str,
    second: &str,
    resources: &str,
    extra: Vec<Vec<u8>>,
) -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 /MediaBox [0 0 100 100] >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R {resources} >>").into_bytes(),
        stream("", first.as_bytes()),
        format!("<< /Type /Page /Parent 2 0 R /Contents [6 0 R 7 0 R] {resources} >>").into_bytes(),
        stream("", b"0 0 1 rg 1 0 0 1 45 45 cm"),
        stream("", second.as_bytes()),
    ];
    objects.extend(extra);
    pdf_objects(objects, "")
}

#[test]
fn identical_vector_streams_can_have_independent_page_owners() {
    let shape = "10 10 10 10 re f";
    let accepted = independent_page_streams(shape, shape, "", vec![]);
    let candidate = independent_page_streams("", shape, "", vec![]);
    let marks = [mark(91, [9_000, 9_000, 12_000, 12_000])];
    let evidence = verify(&accepted, &candidate, &[91, 92], &marks);
    assert!(evidence.checks_passed(), "{evidence:?}");
}

#[test]
fn nearby_stroke_is_preserved_when_its_ink_misses_the_mark() {
    let stroke = "0 0 1 RG 2 w 30 50 m 50 30 l 65 30 l S";
    let square = "1 0 0 rg 35 35 5 5 re f ";
    let marks = [mark(91, [34_000, 34_000, 37_000, 37_000])];
    let accepted = pdf(&[(&format!("{square}{stroke}"), "")], vec![], "");
    let candidate = pdf(&[(stroke, "")], vec![], "");
    let evidence = verify(&accepted, &candidate, &[91], &marks);
    assert!(evidence.checks_passed(), "{evidence:?}");

    let blank = pdf(&[("", "")], vec![], "");
    let evidence = verify(&accepted, &blank, &[91], &marks);
    assert_eq!(
        evidence.raster().failure(),
        Some(NativeVerificationFailure::RasterMismatch)
    );
}

#[test]
fn near_crop_edge_stroke_is_admitted_only_when_its_paint_fits() {
    let geometry = NativeRedactionGeometry {
        crop_box: [5_000, 5_000, 95_000, 95_000],
        ..GEOMETRY
    };
    let marks = [NativeRedactionMark {
        page_id: 91,
        geometry,
        rect: [9_000, 9_000, 12_000, 12_000],
    }];
    let cropped = "/CropBox [5 5 95 95]";
    let stroke = |x, endpoint| format!("q 0 1 -1 0 {x} 0 cm 2 w 10 6 m 26 6 l 26 {endpoint} l S Q");
    let retained = stroke(98, 25);
    let accepted = pdf(&[(&format!("{SECRET}{retained}"), cropped)], vec![], "");
    let candidate = pdf(&[(&retained, cropped)], vec![], "");
    let evidence = verify(&accepted, &candidate, &[91], &marks);
    assert!(evidence.checks_passed(), "{evidence:?}");

    let changed = pdf(&[(&stroke(98, 24), cropped)], vec![], "");
    let clipped = pdf(&[(&stroke(101, 25), cropped)], vec![], "");
    for (candidate, failure) in [
        (changed, NativeVerificationFailure::RasterMismatch),
        (clipped, NativeVerificationFailure::UnsupportedProfile),
    ] {
        let evidence = verify(&accepted, &candidate, &[91], &marks);
        assert_eq!(evidence.raster().failure(), Some(failure));
    }
}

#[test]
fn transformed_fill_is_preserved_when_only_its_bounds_meet_the_mark() {
    let square = "1 0 0 rg 42 46 4 4 re f ";
    let empty_corner = [mark(91, [41_000, 45_000, 44_000, 48_000])];
    let crossing = [mark(91, [46_000, 38_000, 50_000, 42_000])];
    for op in ["f", "f*"] {
        let triangle =
            |endpoint| format!("q 0 1 -1 0 90 0 cm 0 g 30 30 m 50 30 l 30 {endpoint} l {op} Q");
        let retained = triangle(50);
        let accepted = pdf(&[(&format!("{square}{retained}"), "")], vec![], "");
        let candidate = pdf(&[(&retained, "")], vec![], "");
        let evidence = verify(&accepted, &candidate, &[91], &empty_corner);
        assert!(evidence.checks_passed(), "{evidence:?}");

        let changed = pdf(&[(&triangle(49), "")], vec![], "");
        let evidence = verify(&accepted, &changed, &[91], &empty_corner);
        assert_eq!(
            evidence.raster().failure(),
            Some(NativeVerificationFailure::RasterMismatch)
        );

        let expected = pdf(&[(square, "")], vec![], "");
        let evidence = verify(&accepted, &expected, &[91], &crossing);
        assert!(evidence.checks_passed(), "{evidence:?}");
        let evidence = verify(&accepted, &accepted, &[91], &crossing);
        assert_eq!(
            evidence.raster().failure(),
            Some(NativeVerificationFailure::RasterMismatch)
        );
    }
}

#[test]
fn blank_page_accepts_direct_and_indirect_null_contents() {
    let document = |paint: &str, contents: &str, extra: Option<Vec<u8>>| {
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 /MediaBox [0 0 100 100] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
            stream("", paint.as_bytes()),
            format!("<< /Type /Page /Parent 2 0 R {contents} >>").into_bytes(),
        ];
        objects.extend(extra);
        pdf_objects(objects, "")
    };
    let marks = [mark(91, [9_000, 9_000, 12_000, 12_000])];
    for (contents, extra) in [
        ("/Contents null", None),
        ("/Contents 6 0 R", Some(b"null".to_vec())),
    ] {
        let accepted = document(&format!("{SECRET}{PUBLIC}"), contents, extra.clone());
        let candidate = document(PUBLIC, contents, extra);
        let evidence = verify(&accepted, &candidate, &[91, 92], &marks);
        assert!(evidence.checks_passed(), "{evidence:?}");
    }
    let invalid = document(PUBLIC, "/Contents 42", None);
    let accepted = document(&format!("{SECRET}{PUBLIC}"), "", None);
    let evidence = verify(&accepted, &invalid, &[91, 92], &marks);
    assert_eq!(
        evidence.structure().failure(),
        Some(NativeVerificationFailure::Structure)
    );
}

#[test]
fn retained_metadata_or_unreachable_object_fails_structure() {
    let accepted = pdf(&[(&format!("{SECRET}{PUBLIC}"), "")], vec![], "");
    for trailer in ["/Info 5 0 R", ""] {
        let candidate = pdf(
            &[(PUBLIC, "")],
            vec![b"<< /Title (Private) >>".to_vec()],
            trailer,
        );
        let evidence = verify(
            &accepted,
            &candidate,
            &[91],
            &[mark(91, [9_000, 9_000, 12_000, 12_000])],
        );
        assert!(!evidence.checks_passed());
        assert_eq!(
            evidence.structure().status(),
            NativeVerificationStatus::Failed
        );
    }
}

#[test]
fn unrelated_geometry_and_missing_or_covered_objects_fail_raster() {
    let accepted = pdf(
        &[(&format!("{SECRET}{PUBLIC}"), ""), (PUBLIC, "")],
        vec![],
        "",
    );
    for pages in [
        [(PUBLIC, ""), ("0 g 56 55 10 10 re f", "")],
        [("", ""), (PUBLIC, "")],
        [("0 g 55 55 10 10 re f 10 10 10 10 re f", ""), (PUBLIC, "")],
    ] {
        let candidate = pdf(&pages, vec![], "");
        let evidence = verify(
            &accepted,
            &candidate,
            &[91, 52],
            &[mark(91, [9_000, 9_000, 12_000, 12_000])],
        );
        assert_eq!(
            evidence.structure().status(),
            NativeVerificationStatus::Passed
        );
        assert_eq!(evidence.text().status(), NativeVerificationStatus::Passed);
        assert_eq!(
            evidence.raster().failure(),
            Some(NativeVerificationFailure::RasterMismatch)
        );
        assert!(!evidence.checks_passed());
    }
}

#[test]
fn a_scrubbed_image_requires_exact_retained_samples() {
    let content = "q 20 0 0 20 10 10 cm /Im Do Q";
    let dict = "/Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB";
    let entries = "/Resources << /XObject << /Im 5 0 R >> >>";
    let source = pdf(
        &[(content, entries)],
        vec![stream(dict, &[200, 100, 50, 3, 5, 8])],
        "",
    );
    for (samples, pass) in [
        ([0, 0, 0, 3, 5, 8], true),
        ([0, 0, 0, 3, 5, 9], false),
        ([200, 100, 50, 3, 5, 8], false),
    ] {
        let candidate = pdf(&[(content, entries)], vec![stream(dict, &samples)], "");
        let evidence = verify(
            &source,
            &candidate,
            &[91],
            &[mark(91, [10_000, 10_000, 11_000, 11_000])],
        );
        assert_eq!(evidence.checks_passed(), pass, "{evidence:?}");
    }
}

fn test_font_objects(first: usize) -> Vec<Vec<u8>> {
    let font = include_bytes!("../data/NotoSans-Regular.ttf");
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapType 2 def\n/CMapName /Example def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<41> <0041>\n<42> <0042>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend end";
    let face = ttf_parser::Face::parse(font, 0).unwrap();
    let width = |ch| {
        u32::from(
            face.glyph_hor_advance(face.glyph_index(ch).unwrap())
                .unwrap(),
        )
    };
    vec![
        format!("<< /Type /Font /Subtype /TrueType /BaseFont /NotoSans-Regular /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 66 /Widths [{} {}] /FontDescriptor {} 0 R /ToUnicode {} 0 R >>", width('A'), width('B'), first + 1, first + 3).into_bytes(),
        format!("<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 /FontFile2 {} 0 R >>", first + 2).into_bytes(),
        stream(&format!("/Length1 {}", font.len()), font),
        stream("", cmap),
    ]
}

fn text_pdf(content: &str) -> Vec<u8> {
    pdf(
        &[(content, "/Resources << /Font << /F 5 0 R >> >>")],
        test_font_objects(5),
        "",
    )
}

#[test]
fn identical_text_streams_can_have_independent_page_owners() {
    let text = "BT /F 12 Tf 10 15 Td (AB) Tj ET";
    let resources = "/Resources << /Font << /F 8 0 R >> >>";
    let accepted = independent_page_streams(text, text, resources, test_font_objects(8));
    let candidate = independent_page_streams("", text, resources, test_font_objects(8));
    let marks = [mark(91, [9_000, 16_000, 15_000, 20_000])];
    let evidence = verify(&accepted, &candidate, &[91, 92], &marks);
    assert!(evidence.checks_passed(), "{evidence:?}");
}

#[test]
fn extraction_requires_removed_show_to_disappear_and_same_text_elsewhere_to_survive() {
    let target = "BT /F 12 Tf 10 15 Td (AB) Tj ET";
    let retained = "BT /F 12 Tf 55 55 Td [(AB) 0] TJ ET";
    let source = text_pdf(&format!("{target}\n{retained}"));
    let marks = [mark(91, [9_000, 16_000, 15_000, 20_000])];
    let candidate = text_pdf(retained);
    let evidence = verify(&source, &candidate, &[91], &marks);
    assert!(evidence.checks_passed(), "{evidence:?}");
    for content in [
        format!("{target}\n{retained}"),
        "BT /F 12 Tf 55 55 Td (BA) Tj ET".to_string(),
    ] {
        let evidence = verify(&source, &text_pdf(&content), &[91], &marks);
        assert_eq!(
            evidence.structure().status(),
            NativeVerificationStatus::Passed
        );
        assert_eq!(
            evidence.text().failure(),
            Some(NativeVerificationFailure::TextMismatch)
        );
        assert!(!evidence.checks_passed());
    }
}

#[test]
fn live_context_and_bytes_must_match_the_evidence() {
    let origin = pdf(&[(SECRET, "")], vec![], "");
    let accepted = pdf(&[(&format!("{SECRET}{PUBLIC}"), "")], vec![], "");
    let candidate = pdf(&[(PUBLIC, "")], vec![], "");
    let marks = [mark(91, [9_000, 9_000, 12_000, 12_000])];
    let binding = NativeBinding {
        epoch: [7; 16],
        session_id: 9,
        revision: 4,
        origin_hash: *blake3::hash(&origin).as_bytes(),
    };
    let context = NativeRedactionContext {
        binding,
        request_id: 8,
        accepted_hash: *blake3::hash(&accepted).as_bytes(),
        page_order: &[91],
    };
    let policy = NativeRedactionPolicy {
        binding,
        request_id: 8,
        candidate_hash: *blake3::hash(&candidate).as_bytes(),
        marks: &marks,
    };
    let evidence = verify_native_redaction(&origin, &accepted, &candidate, context, policy);
    assert!(
        evidence.eligible_for(&candidate, context, policy),
        "{evidence:?}"
    );
    let mut changed_bytes = candidate.clone();
    changed_bytes.push(b'\n');
    assert!(!evidence.eligible_for(&changed_bytes, context, policy));

    for wrong in [
        NativeRedactionPolicy {
            candidate_hash: [0; 32],
            ..policy
        },
        NativeRedactionPolicy {
            request_id: 9,
            ..policy
        },
        NativeRedactionPolicy {
            binding: NativeBinding {
                revision: 5,
                ..binding
            },
            ..policy
        },
        NativeRedactionPolicy {
            binding: NativeBinding {
                session_id: 10,
                ..binding
            },
            ..policy
        },
    ] {
        let result = verify_native_redaction(&origin, &accepted, &candidate, context, wrong);
        assert_eq!(
            result.identity_failure(),
            Some(NativeVerificationFailure::Identity)
        );
        assert_eq!(
            result.structure().status(),
            NativeVerificationStatus::Pending
        );
        assert!(!evidence.eligible_for(&candidate, context, wrong));
    }
    let wrong_origin = verify_native_redaction(&candidate, &accepted, &candidate, context, policy);
    assert_eq!(
        wrong_origin.identity_failure(),
        Some(NativeVerificationFailure::Identity)
    );
    let different_marks = [mark(91, [9_000, 9_000, 13_000, 12_000])];
    assert!(!evidence.eligible_for(
        &candidate,
        context,
        NativeRedactionPolicy {
            marks: &different_marks,
            ..policy
        }
    ));
}

#[test]
fn cropped_rotated_pages_and_notes_have_complete_coverage() {
    let geometry = NativeRedactionGeometry {
        crop_box: [5_000, 6_000, 80_000, 90_000],
        rotation: 90,
        ..GEOMETRY
    };
    let notes = [
        b"<< /Type /Annot /Subtype /Text /Rect [10 10 20 20] /Contents (remove) /P 3 0 R >>"
            .to_vec(),
        b"<< /Type /Annot /Subtype /Text /Rect [30 30 40 40] /Contents (keep) /P 3 0 R >>".to_vec(),
    ];
    let cropped = "/CropBox [5 6 80 90] /Rotate 90";
    let before = pdf(
        &[
            (SECRET, &format!("{cropped} /Annots [7 0 R 8 0 R]")),
            (SECRET, ""),
        ],
        notes.to_vec(),
        "",
    );
    let after = pdf(
        &[("", &format!("{cropped} /Annots [7 0 R]")), ("", "")],
        vec![notes[1].clone()],
        "",
    );
    let marks = [
        mark(12, [10_000, 10_000, 12_000, 12_000]),
        NativeRedactionMark {
            page_id: 77,
            geometry,
            rect: [9_000, 9_000, 12_000, 12_000],
        },
    ];
    let evidence = verify(&before, &after, &[77, 12], &marks);
    assert!(evidence.checks_passed(), "{evidence:?}");
    let covered = evidence.identity().unwrap().affected_pages();
    assert_eq!(
        covered.iter().map(|page| page.page_id).collect::<Vec<_>>(),
        [77, 12]
    );
    assert_eq!(evidence.structure().covered_pages(), covered);
    assert_eq!(evidence.text().covered_pages(), covered);
    assert_eq!(evidence.raster().covered_pages(), covered);

    let absent = pdf(&[("", cropped)], vec![], "");
    for wrong in [
        verify(&before, &absent, &[77, 12], &marks),
        verify(&before, &after, &[77], &marks[1..]),
        verify(
            &before,
            &after,
            &[77, 12],
            &[mark(77, marks[1].rect), marks[0]],
        ),
    ] {
        assert_eq!(
            wrong.structure().failure(),
            Some(NativeVerificationFailure::Coverage)
        );
        assert!(!wrong.checks_passed());
    }
}

#[test]
fn original_shared_image_samples_cannot_survive_in_another_use() {
    let dict = "/Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray";
    let content = "q 20 0 0 20 10 10 cm /Im Do Q";
    let old_samples = [200, 8];
    let new_samples = [0, 8];
    let shared = "/Resources << /XObject << /Im 7 0 R >> >>";
    let unsanitized = "/Resources << /XObject << /Im 8 0 R >> >>";
    let source = pdf(
        &[(content, shared), (content, shared)],
        vec![stream(dict, &old_samples)],
        "",
    );
    let candidate = pdf(
        &[(content, shared), (content, unsanitized)],
        vec![stream(dict, &new_samples), stream(dict, &old_samples)],
        "",
    );
    let first = mark(77, [10_000, 10_000, 12_000, 12_000]);
    let evidence = verify(&source, &candidate, &[77, 12], &[first]);
    assert_eq!(
        evidence.raster().failure(),
        Some(NativeVerificationFailure::RasterMismatch)
    );
    let partially_scrubbed_twice = pdf(
        &[(content, shared), (content, unsanitized)],
        vec![stream(dict, &new_samples), stream(dict, &[200, 0])],
        "",
    );
    let evidence = verify(
        &source,
        &partially_scrubbed_twice,
        &[77, 12],
        &[first, mark(12, [20_000, 10_000, 22_000, 12_000])],
    );
    assert_eq!(
        evidence.raster().failure(),
        Some(NativeVerificationFailure::RasterMismatch)
    );
    let sanitized = pdf(
        &[(content, shared), (content, shared)],
        vec![stream(dict, &new_samples)],
        "",
    );
    let evidence = verify(
        &source,
        &sanitized,
        &[77, 12],
        &[first, mark(12, first.rect)],
    );
    assert!(evidence.checks_passed(), "{evidence:?}");
}

#[test]
fn invisible_text_and_interpolated_images_refuse() {
    let text = text_pdf("BT /F 12 Tf 10 15 Td 3 Tr (AB) Tj ET");
    let blank = pdf(&[("", "")], vec![], "");
    let marks = [mark(91, [9_000, 16_000, 15_000, 20_000])];
    let evidence = verify(&text, &blank, &[91], &marks);
    assert_eq!(
        evidence.structure().status(),
        NativeVerificationStatus::Passed
    );
    assert_eq!(
        evidence.text().status(),
        NativeVerificationStatus::Inconclusive
    );
    assert_eq!(
        evidence.raster().status(),
        NativeVerificationStatus::Inconclusive
    );
    assert!(!evidence.checks_passed());

    let dict = "/Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Interpolate true";
    let image = pdf(
        &[(
            "q 20 0 0 20 10 10 cm /Im Do Q",
            "/Resources << /XObject << /Im 5 0 R >> >>",
        )],
        vec![stream(dict, &[200])],
        "",
    );
    let evidence = verify(&image, &blank, &[91], &marks);
    assert_eq!(
        evidence.raster().failure(),
        Some(NativeVerificationFailure::UnsupportedProfile)
    );
    assert!(!evidence.checks_passed());
}

#[test]
fn redaction_refuses_a_shared_form_with_conflicting_placements() {
    let resources = "/Resources << /XObject << /Fx 7 0 R >> >>";
    let dict = "/Type /XObject /Subtype /Form /BBox [0 0 50 50]";
    let source = pdf(
        &[("/Fx Do", resources), ("/Fx Do", resources)],
        vec![stream(dict, SECRET.as_bytes())],
        "",
    );
    let mark_first = mark(77, [9_000, 9_000, 12_000, 12_000]);
    let candidate = pdf(
        &[("", ""), ("/Fx Do", resources)],
        vec![stream(dict, b"1.0 0 0 rg 10 10 10 10 re f")],
        "",
    );
    let evidence = verify(&source, &candidate, &[77, 12], &[mark_first]);
    assert_eq!(
        evidence.raster().failure(),
        Some(NativeVerificationFailure::RasterMismatch)
    );
    let candidate = pdf(&[("", ""), ("", "")], vec![], "");
    let evidence = verify(
        &source,
        &candidate,
        &[77, 12],
        &[mark_first, mark(12, mark_first.rect)],
    );
    assert!(evidence.checks_passed(), "{evidence:?}");
}

#[test]
fn session_redacts_embedded_font_text_and_publishes_only_the_verified_hash() {
    use flow_core::pdf::native::{NativeEditLimits, NativePdfSession, NativeRedactionStage};

    let source =
        text_pdf("BT /F 10 Tf 1 0 0 1 10 10 Tm (AB) Tj ET BT /F 10 Tf 60 60 Td (BA) Tj ET");
    let mut session =
        NativePdfSession::open_scoped(&source, NativeEditLimits::default(), [3; 16]).unwrap();
    let base = session.snapshot().binding;
    let order = session.snapshot().page_state.order.clone();
    let mark = mark(order[0], [11_000, 11_000, 13_000, 13_000]);
    session.add_redaction_mark(base, mark).unwrap();
    let NativeRedactionStage::Pending(candidate) = session.stage_redaction(base, 1).unwrap() else {
        panic!("expected pending status");
    };
    assert!(session.verify_redaction(base, 1).unwrap().checks_passed());
    let receipt = session.publish_redaction(base, 1).unwrap();
    let bytes = session.redacted_output_bytes().unwrap();
    assert_eq!(receipt.output_hash, candidate.candidate_hash);
    assert_eq!(blake3::hash(bytes).as_bytes(), &candidate.candidate_hash);
    assert!(verify(&source, bytes, &order, &[mark]).checks_passed());

    let different = text_pdf("BT /F 10 Tf 60 60 Td (AB) Tj ET");
    let context = NativeRedactionContext {
        binding: base,
        accepted_hash: base.origin_hash,
        request_id: 1,
        page_order: &order,
    };
    let policy = NativeRedactionPolicy {
        binding: base,
        candidate_hash: candidate.candidate_hash,
        request_id: 1,
        marks: &[mark],
    };
    let evidence = session.accepted_redaction_evidence().unwrap();
    assert!(evidence.eligible_for(bytes, context, policy));
    assert!(!evidence.eligible_for(&different, context, policy));
}

#[test]
fn verifier_accepts_standard_cid_metadata_and_retained_font_paint() {
    let [_, descriptor, bundled_font, _]: [Vec<u8>; 4] = test_font_objects(6).try_into().unwrap();
    let face = ttf_parser::Face::parse(include_bytes!("../data/NotoSans-Regular.ttf"), 0).unwrap();
    let glyph_a = face.glyph_index('A').unwrap();
    let glyph_b = face.glyph_index('B').unwrap();
    let widths = [glyph_a, glyph_b].map(|glyph| face.glyph_hor_advance(glyph).unwrap());
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapType 2 def\n/CMapName /Adobe-Identity-UCS def\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n2 beginbfchar\n<0001> <0041>\n<0002> <0042>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend end";
    let glyph_map = [0, glyph_a.0, glyph_b.0]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect::<Vec<_>>();
    let cmap = std::str::from_utf8(cmap).unwrap();
    for name in ["Adobe-Identity-UCS", "FixtureMap", "Fixture-Latin"] {
        let cmap = cmap.replace("Adobe-Identity-UCS", name);
        let fonts = vec![
            b"<< /Type /Font /Subtype /Type0 /BaseFont /NotoSans-Regular /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 9 0 R >>".to_vec(),
            format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NotoSans-Regular /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /W [1 [{} {}]] /FontDescriptor 7 0 R /CIDToGIDMap 10 0 R >>", widths[0], widths[1]).into_bytes(),
            descriptor.clone(),
            bundled_font.clone(),
            stream("", cmap.as_bytes()),
            stream("", &glyph_map),
        ];
        let resources = "/Resources << /Font << /F1 5 0 R >> >>";
        let retained = "BT /F1 12 Tf 55 55 Td <00010002> Tj ET";
        let accepted = pdf(
            &[(&format!("10 10 5 5 re f {retained}"), resources)],
            fonts.clone(),
            "",
        );
        let candidate = pdf(&[(retained, resources)], fonts, "");
        let evidence = verify(
            &accepted,
            &candidate,
            &[91],
            &[mark(91, [11_000, 11_000, 14_000, 14_000])],
        );
        assert!(evidence.checks_passed(), "{evidence:?}");
    }
}

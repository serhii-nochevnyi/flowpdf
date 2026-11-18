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

fn text_pdf(content: &str) -> Vec<u8> {
    let font = include_bytes!("../data/NotoSans-Regular.ttf");
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapType 2 def\n/CMapName /Example def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<41> <0041>\n<42> <0042>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend end";
    let face = ttf_parser::Face::parse(font, 0).unwrap();
    let width = |ch| {
        u32::from(
            face.glyph_hor_advance(face.glyph_index(ch).unwrap())
                .unwrap(),
        )
    };
    pdf(
        &[(content, "/Resources << /Font << /F 5 0 R >> >>")],
        vec![
            format!("<< /Type /Font /Subtype /TrueType /BaseFont /NotoSans-Regular /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 66 /Widths [{} {}] /FontDescriptor 6 0 R /ToUnicode 8 0 R >>", width('A'), width('B')).into_bytes(),
            b"<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 /FontFile2 7 0 R >>".to_vec(),
            stream(&format!("/Length1 {}", font.len()), font),
            stream("", cmap),
        ],
        "",
    )
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

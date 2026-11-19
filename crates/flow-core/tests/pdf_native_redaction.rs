use flow_core::pdf::native::{
    NativeEditLimits, NativeNoteCommand, NativePdfSession, NativeRedactionGeometry,
    NativeRedactionMark, NativeRedactionStage, NativeRequestLookup, NativeVerificationStatus,
};
use miniz_oxide::deflate::compress_to_vec_zlib;

const GEOMETRY: NativeRedactionGeometry = NativeRedactionGeometry {
    media_box: [0, 0, 100_000, 100_000],
    crop_box: [0, 0, 100_000, 100_000],
    rotation: 0,
};

fn pdf(objects: Vec<Vec<u8>>, trailer: &str) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.into_iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(&object);
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

fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} {entries} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

fn flate(entries: &str, data: &[u8]) -> Vec<u8> {
    stream(
        &format!("/Filter /FlateDecode {entries}"),
        &compress_to_vec_zlib(data, 6),
    )
}

fn fixture() -> Vec<u8> {
    pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R /Metadata 9 0 R /FlowPDFSource 10 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Annots [5 0 R 6 0 R] /Resources << /XObject << /Im 7 0 R >> >> /Metadata 11 0 R >>".to_vec(),
            flate("", b"1 0 0 rg 10 10 10 10 re f 0 g 55 55 10 10 re f q 20 0 0 10 10 30 cm /Im Do Q"),
            b"<< /Type /Annot /Subtype /Text /Rect [12 12 15 15] /Contents (private note) /P 3 0 R >>".to_vec(),
            b"<< /Type /Annot /Subtype /Text /Rect [60 60 65 65] /Contents (public note) /P 3 0 R >>".to_vec(),
            flate("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[217, 83]),
            b"<< /Title (private title) /Author (private author) >>".to_vec(),
            stream("/Type /Metadata /Subtype /XML", b"<xmp>catalog metadata</xmp>"),
            stream("/Type /Metadata /Subtype /FlowPDF.Source", b"original source envelope"),
            stream("/Type /Metadata /Subtype /XML", b"<xmp>page metadata</xmp>"),
        ],
        "/Info 8 0 R /ID [<0123> <4567>]",
    )
}

fn open(bytes: &[u8]) -> NativePdfSession {
    NativePdfSession::open_scoped(bytes, NativeEditLimits::default(), [5; 16]).unwrap()
}

fn mark(session: &NativePdfSession, page: usize, rect: [i64; 4]) -> NativeRedactionMark {
    NativeRedactionMark {
        page_id: session.snapshot().page_state.order[page],
        geometry: GEOMETRY,
        rect,
    }
}

fn add(session: &mut NativePdfSession, page: usize, rect: [i64; 4]) {
    session
        .add_redaction_mark(session.snapshot().binding, mark(session, page, rect))
        .unwrap();
}

fn assert_accepted(session: &NativePdfSession, bytes: &[u8], revision: u64, origin: &[u8]) {
    assert_eq!(session.output_bytes(), bytes);
    assert_eq!(session.origin_bytes(), origin);
    assert_eq!(session.snapshot().binding.revision, revision);
    assert_eq!(
        session.snapshot().output_hash,
        *blake3::hash(bytes).as_bytes()
    );
    assert!(session.redacted_output_bytes().is_none());
}

#[test]
fn marks_and_quarantine_keep_accepted_bytes_and_scene_until_exact_publication() {
    let source = fixture();
    let mut session = open(&source);
    let base = session.snapshot().clone();
    let first = mark(&session, 0, [11_000, 11_000, 13_000, 13_000]);
    let second = mark(&session, 0, [14_000, 32_000, 15_000, 33_000]);
    assert!(
        session
            .add_redaction_mark(
                base.binding,
                NativeRedactionMark {
                    geometry: NativeRedactionGeometry {
                        rotation: 90,
                        ..GEOMETRY
                    },
                    ..first
                }
            )
            .is_err()
    );
    session.add_redaction_mark(base.binding, first).unwrap();
    session.remove_redaction_mark(base.binding, first).unwrap();
    session.add_redaction_mark(base.binding, first).unwrap();
    session.clear_redaction_marks(base.binding).unwrap();
    assert!(session.redaction_marks().is_empty());
    session.add_redaction_mark(base.binding, first).unwrap();
    session.add_redaction_mark(base.binding, second).unwrap();
    assert_accepted(&session, &source, 0, &source);

    let staged = session.stage_redaction(base.binding, 1).unwrap();
    let NativeRedactionStage::Pending(identity) = staged else {
        panic!("expected pending status")
    };
    assert_eq!(session.stage_redaction(base.binding, 1), Ok(staged));
    assert_eq!(
        session.redaction_candidate_status().unwrap().status(),
        NativeVerificationStatus::Pending
    );
    assert!(session.publish_redaction(base.binding, 1).is_err());
    assert_eq!(session.snapshot().scene.scene_hash, base.scene.scene_hash);
    assert_accepted(&session, &source, 0, &source);
    assert_eq!(session.redaction_marks(), &[first, second]);

    let evidence = session.verify_redaction(base.binding, 1).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    assert_eq!(
        evidence.identity().unwrap().candidate_hash(),
        identity.candidate_hash
    );
    assert_accepted(&session, &source, 0, &source);
    let receipt = session.publish_redaction(base.binding, 1).unwrap();
    assert_eq!(receipt.output_hash, identity.candidate_hash);
    assert_eq!(receipt.binding.revision, 1);
    assert_eq!(session.publish_redaction(base.binding, 1), Ok(receipt));
    assert_eq!(
        session.stage_redaction(base.binding, 1),
        Ok(NativeRedactionStage::Accepted(receipt))
    );
    assert_eq!(
        session.lookup(base.binding, 1),
        Ok(NativeRequestLookup::Accepted(receipt))
    );
    assert!(session.redaction_marks().is_empty());
    assert!(session.redaction_candidate_status().is_none());
    assert!(
        session
            .accepted_redaction_evidence()
            .unwrap()
            .checks_passed()
    );
    let out = session.redacted_output_bytes().unwrap();
    assert_eq!(out, session.output_bytes());
    assert_eq!(blake3::hash(out).as_bytes(), &identity.candidate_hash);
    for removed in [
        "/Info",
        "/ID",
        "/Metadata",
        "/FlowPDFSource",
        "/Prev",
        "10 10 10 10 re",
        "70726976617465206E6F7465",
    ] {
        assert!(
            !out.windows(removed.len())
                .any(|window| window == removed.as_bytes()),
            "{removed}"
        );
    }
    assert!(out.windows(2).any(|window| window == [0, 83]));
    assert_eq!(session.snapshot().scene.pages[0].annotations.len(), 1);
    assert_eq!(session.origin_bytes(), source);
}

#[test]
fn shared_image_requires_equal_scrubbing_for_every_use() {
    for target in [7, 8] {
        let image = flate(
            "/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[217, 83],
        );
        let source = pdf(
            vec![
                b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
                b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 /MediaBox [0 0 100 100] >>".to_vec(),
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Im 7 0 R >> >> >>".to_vec(),
                flate("", b"q 20 0 0 10 10 30 cm /Im Do Q"),
                format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Im {target} 0 R >> >> >>").into_bytes(),
                b"(unused document object)".to_vec(),
                image.clone(),
                image,
            ],
            "",
        );
        let mut session = open(&source);
        let binding = session.snapshot().binding;
        let rect = [14_000, 32_000, 15_000, 33_000];
        add(&mut session, 0, rect);
        assert!(session.stage_redaction(binding, 1).is_err());
        assert_accepted(&session, &source, 0, &source);
        assert_eq!(session.redaction_marks().len(), 1);
        add(&mut session, 1, rect);
        session.stage_redaction(binding, 2).unwrap();
        assert!(
            session
                .verify_redaction(binding, 2)
                .unwrap()
                .checks_passed()
        );
        session.publish_redaction(binding, 2).unwrap();
        assert!(
            session
                .output_bytes()
                .windows(2)
                .any(|window| window == [0, 83])
        );
        assert!(
            !session
                .output_bytes()
                .windows(2)
                .any(|window| window == [217, 83])
        );
    }
}

#[test]
fn cancellation_and_ordinary_edits_invalidate_candidate_and_stale_marks() {
    let source = fixture();
    let mut session = open(&source);
    let base = session.snapshot().binding;
    add(&mut session, 0, [11_000, 11_000, 13_000, 13_000]);
    session.stage_redaction(base, 1).unwrap();
    let other = open(&source);
    assert!(
        session
            .verify_redaction(other.snapshot().binding, 1)
            .is_err()
    );
    assert!(session.publish_redaction(base, 2).is_err());
    assert!(session.stage_redaction(base, 2).is_err());
    session.cancel_redaction(base, 1).unwrap();
    assert!(session.stage_redaction(base, 1).is_err());
    assert!(session.publish_redaction(base, 1).is_err());
    assert_accepted(&session, &source, 0, &source);
    session.stage_redaction(base, 2).unwrap();
    assert!(session.verify_redaction(base, 2).unwrap().checks_passed());
    let receipt = session
        .apply(
            base,
            3,
            &NativeNoteCommand::Add {
                page: session.snapshot().scene.pages[0].id,
                rect: [70, 70, 75, 75],
                contents: "new note".into(),
            },
        )
        .unwrap();
    let prior = session.output_bytes().to_vec();
    assert!(session.redaction_candidate_status().is_none());
    assert!(session.publish_redaction(base, 2).is_err());
    assert!(session.stage_redaction(receipt.binding, 4).is_err());
    assert_eq!(session.redaction_marks().len(), 1);
    assert_accepted(&session, &prior, 1, &source);
    session.clear_redaction_marks(receipt.binding).unwrap();
    add(&mut session, 0, [11_000, 11_000, 13_000, 13_000]);
    session.stage_redaction(receipt.binding, 5).unwrap();
    assert!(
        session
            .verify_redaction(receipt.binding, 5)
            .unwrap()
            .checks_passed()
    );
    session.publish_redaction(receipt.binding, 5).unwrap();
    assert_eq!(session.origin_bytes(), source);
}

#[test]
fn valid_form_storage_is_outside_the_scrub_profile() {
    let source = pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R /AcroForm 5 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
            flate("", b"10 10 10 10 re f"),
            b"<< /Fields [] >>".to_vec(),
        ],
        "",
    );
    let mut session = open(&source);
    let binding = session.snapshot().binding;
    add(&mut session, 0, [11_000, 11_000, 13_000, 13_000]);
    assert!(session.stage_redaction(binding, 1).is_err());
    assert_accepted(&session, &source, 0, &source);
    assert_eq!(session.redaction_marks().len(), 1);
    assert!(session.redaction_candidate_status().is_none());
}

#[test]
fn an_inconclusive_raster_check_preserves_the_previous_accepted_revision() {
    let source = pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 3000 3000] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
            flate("", b"10 10 10 10 re f"),
        ],
        "",
    );
    let mut session = open(&source);
    let prior = session
        .apply(
            session.snapshot().binding,
            1,
            &NativeNoteCommand::Add {
                page: session.snapshot().scene.pages[0].id,
                rect: [20, 20, 30, 30],
                contents: "accepted note".into(),
            },
        )
        .unwrap();
    let before = session.output_bytes().to_vec();
    let page_id = session.snapshot().page_state.order[0];
    let mark = NativeRedactionMark {
        page_id,
        geometry: session.redaction_mark_geometry(page_id).unwrap(),
        rect: [11_000, 11_000, 13_000, 13_000],
    };
    session.add_redaction_mark(prior.binding, mark).unwrap();
    session.stage_redaction(prior.binding, 2).unwrap();
    let evidence = session.verify_redaction(prior.binding, 2).unwrap();
    assert_eq!(
        evidence.structure().status(),
        NativeVerificationStatus::Passed
    );
    assert_eq!(evidence.text().status(), NativeVerificationStatus::Passed);
    assert_eq!(
        evidence.raster().status(),
        NativeVerificationStatus::Inconclusive
    );
    assert_eq!(
        session.redaction_candidate_status().unwrap().status(),
        NativeVerificationStatus::Inconclusive
    );
    assert!(session.publish_redaction(prior.binding, 2).is_err());
    assert_accepted(&session, &before, 1, &source);
    assert_eq!(session.snapshot().scene.scene_hash, prior.scene_hash);
    assert_eq!(session.redaction_marks(), &[mark]);
    assert!(session.accepted_redaction_evidence().is_none());
}

#[test]
fn rotated_crop_marks_replace_only_positively_intersecting_raster_cells() {
    let source = pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] /Resources << /XObject << /Im 5 0 R >> >> >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /CropBox [5.125 6 95 90] /Rotate 90 >>".to_vec(),
            flate("", b"q 0 20 -20 0 50 20 cm /Im Do Q"),
            flate("/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8",
                &[101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112]),
        ],
        "",
    );
    let mut session = open(&source);
    let binding = session.snapshot().binding;
    let page_id = session.snapshot().page_state.order[0];
    let geometry = session.redaction_mark_geometry(page_id).unwrap();
    assert_eq!(geometry.crop_box, [5125, 6000, 95000, 90000]);
    assert_eq!(geometry.rotation, 90);
    assert_eq!(geometry.media_box, GEOMETRY.media_box);
    session
        .add_redaction_mark(
            binding,
            NativeRedactionMark {
                page_id,
                geometry,
                rect: [30_000, 20_000, 40_000, 30_000],
            },
        )
        .unwrap();
    session.stage_redaction(binding, 1).unwrap();
    let evidence = session.verify_redaction(binding, 1).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    session.publish_redaction(binding, 1).unwrap();
    assert!(
        session
            .output_bytes()
            .windows(12)
            .any(|window| { window == [0, 0, 0, 104, 105, 106, 107, 108, 109, 110, 111, 112] })
    );
    assert_eq!(session.redaction_mark_geometry(page_id).unwrap(), geometry);
}

#[test]
fn shared_form_content_is_removed_from_every_retained_placement() {
    let source = pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Fm 5 0 R >> >> >>".to_vec(),
            flate("", b"q 1 0 0 1 12 12 cm /Fm Do Q q 1 0 0 1 55 42 cm /Fm Do Q"),
            flate("/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << >>", b"2 2 5 5 re f"),
        ],
        "",
    );
    let mut session = open(&source);
    let binding = session.snapshot().binding;
    add(&mut session, 0, [14_000, 14_000, 15_000, 15_000]);
    assert!(session.stage_redaction(binding, 1).is_err());
    assert_accepted(&session, &source, 0, &source);
    add(&mut session, 0, [57_000, 44_000, 58_000, 45_000]);
    session.stage_redaction(binding, 2).unwrap();
    let evidence = session.verify_redaction(binding, 2).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    session.publish_redaction(binding, 2).unwrap();
    let removed = b"2 2 5 5 re";
    assert!(
        !session
            .output_bytes()
            .windows(removed.len())
            .any(|window| window == removed)
    );
}

#[test]
fn output_budget_refusal_creates_no_candidate() {
    let source = fixture();
    let mut session = NativePdfSession::open_scoped(
        &source,
        NativeEditLimits {
            max_output_bytes: 32,
            ..NativeEditLimits::default()
        },
        [5; 16],
    )
    .unwrap();
    let binding = session.snapshot().binding;
    add(&mut session, 0, [11_000, 11_000, 13_000, 13_000]);
    assert!(session.stage_redaction(binding, 1).is_err());
    assert_accepted(&session, &source, 0, &source);
    assert!(session.redaction_candidate_status().is_none());
    assert_eq!(session.redaction_marks().len(), 1);
}

#[test]
fn redaction_keeps_disjoint_paint_inside_overlapping_path_bounds() {
    let diagonal = b"10 10 m\n80 80 l\nS";
    let triangle = b"10 10 m\n80 10 l\n80 80 l\nh\nf*";
    let square = b"18 63 6 6 re\nf";
    let content = [
        b"0 0 1 RG ".as_slice(),
        diagonal,
        b" 0 1 0 rg ",
        triangle,
        b" 1 0 0 rg ",
        square,
    ]
    .concat();
    let source = pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
            flate("", &content),
        ],
        "",
    );
    let mut session = open(&source);
    let binding = session.snapshot().binding;
    add(&mut session, 0, [19_000, 64_000, 23_000, 68_000]);
    session.stage_redaction(binding, 1).unwrap();
    let evidence = session.verify_redaction(binding, 1).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    session.publish_redaction(binding, 1).unwrap();

    let reopened = open(session.output_bytes());
    let output = reopened.output_bytes();
    for retained in [diagonal.as_slice(), triangle.as_slice()] {
        assert!(
            output
                .windows(retained.len())
                .any(|window| window == retained)
        );
    }
    assert!(!output.windows(square.len()).any(|window| window == square));
}

#[test]
fn published_redaction_preserves_strokes_inside_a_fractional_form_clip() {
    let stroke = b"1 0.5 0 RG 3 w 12 18 m 16 18 l 16 23 l S";
    let source = pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Art 5 0 R >> >> >>".to_vec(),
            stream("", b"0 0 1 rg 10 10 5 5 re f /Art Do"),
            stream(
                "/Type /XObject /Subtype /Form /BBox [0.001 0.001 40.001 40.001] /Matrix [0.5 0 0 0.5 25 25]",
                stroke,
            ),
        ],
        "",
    );
    let mut session = open(&source);
    let binding = session.snapshot().binding;
    add(&mut session, 0, [11_000, 11_000, 12_000, 12_000]);
    session.stage_redaction(binding, 1).unwrap();
    let evidence = session.verify_redaction(binding, 1).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    let receipt = session.publish_redaction(binding, 1).unwrap();
    let output = session.redacted_output_bytes().unwrap();

    assert_eq!(receipt.output_hash, *blake3::hash(output).as_bytes());
    let retained = b"12 18 m\n16 18 l\n16 23 l\nS";
    assert!(
        output
            .windows(retained.len())
            .any(|bytes| bytes == retained)
    );
}

#[test]
fn reference_redaction_pdf_keeps_its_supported_cid_mapping() {
    let source = include_bytes!("../../../fixtures/pdf-native/redaction.pdf");
    let mut session = open(source);
    assert!(session.redaction_admitted());
    let binding = session.snapshot().binding;
    let page_id = session.snapshot().page_state.order[0];
    let geometry = session.redaction_mark_geometry(page_id).unwrap();
    let rect = [12_000, 70_000, 40_000, 92_000];
    session
        .add_redaction_mark(
            binding,
            NativeRedactionMark {
                page_id,
                geometry,
                rect,
            },
        )
        .unwrap();
    session.stage_redaction(binding, 1).unwrap();
    let evidence = session.verify_redaction(binding, 1).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    session.publish_redaction(binding, 1).unwrap();
    assert_eq!(session.origin_bytes(), source);
    assert_ne!(session.output_bytes(), source);
}

use std::fmt::Write as _;

use flow_core::pdf::PdfReadError;
use flow_core::pdf::native::{
    NativeEditError, NativeEditLimits, NativeNoteCommand, NativePageCommand, NativePageSide,
    NativePageTarget, NativePdfSession, NativeRedactionContext, NativeRedactionGeometry,
    NativeRedactionMark, NativeRedactionPolicy, NativeRedactionStage, NativeRequestLookup,
    NativeSessionError, NativeVerificationFailure, NativeVerificationStatus,
    verify_native_redaction,
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

fn add(session: &mut NativePdfSession, request_id: u64, page: usize, rect: [i64; 4]) {
    session
        .add_redaction_mark(
            session.snapshot().binding,
            request_id,
            mark(session, page, rect),
        )
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

fn grayscale_jpeg() -> Vec<u8> {
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode(&[208, 80], 2, 1, image::ExtendedColorType::L8)
        .unwrap();
    jpeg
}

fn jpeg_pdf(filter: &str, data: &[u8]) -> Vec<u8> {
    pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Resources << /XObject << /Photo 5 0 R >> >> >>".to_vec(),
            stream("", b"q 20 0 0 10 10 30 cm /Photo Do Q"),
            stream(
                &format!("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter {filter}"),
                data,
            ),
        ],
        "",
    )
}

#[test]
fn ordinary_rgb_scan_readback() {
    let samples = [208, 140, 80].repeat(1_600 * 1_200);
    let source = pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Resources << /XObject << /Photo 5 0 R >> >> >>".to_vec(),
            stream("", b"q 80 0 0 60 10 20 cm /Photo Do Q"),
            flate(
                "/Type /XObject /Subtype /Image /Width 1600 /Height 1200 /ColorSpace /DeviceRGB /BitsPerComponent 8",
                &samples,
            ),
        ],
        "",
    );
    let mut session = open(&source);
    let binding = session.snapshot().binding;
    add(&mut session, 1, 0, [14_000, 32_000, 15_000, 33_000]);
    let NativeRedactionStage::Pending(candidate) = session.stage_redaction(binding, 2).unwrap()
    else {
        panic!("expected pending redaction");
    };
    let evidence = session.verify_redaction(binding, 2).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    assert_accepted(&session, &source, 0, &source);

    let receipt = session.publish_redaction(binding, 2).unwrap();
    assert_eq!(receipt.output_hash, candidate.candidate_hash);
    assert_eq!(
        session.redacted_output_bytes(),
        Some(session.output_bytes())
    );
}

#[test]
fn jpeg_filter_forms_publish_the_verified_scrubbed_samples() {
    let jpeg = grayscale_jpeg();
    let samples = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
        .unwrap()
        .into_luma8()
        .into_raw();
    for filter in ["/DCTDecode", "[/DCTDecode]"] {
        let source = jpeg_pdf(filter, &jpeg);
        let mut session = open(&source);
        let binding = session.snapshot().binding;
        assert!(session.redaction_admitted(), "{filter}");
        add(&mut session, 1, 0, [14_000, 32_000, 15_000, 33_000]);
        let NativeRedactionStage::Pending(candidate) = session.stage_redaction(binding, 2).unwrap()
        else {
            panic!("expected pending redaction for {filter}");
        };
        let evidence = session.verify_redaction(binding, 2).unwrap();
        assert!(evidence.checks_passed(), "{filter}: {evidence:?}");
        assert_accepted(&session, &source, 0, &source);

        let receipt = session.publish_redaction(binding, 2).unwrap();
        let published = session.redacted_output_bytes().unwrap();
        assert_eq!(receipt.output_hash, candidate.candidate_hash);
        assert_eq!(published, session.output_bytes());
        assert_eq!(
            blake3::hash(published).as_bytes(),
            &candidate.candidate_hash
        );
        let scrubbed_stream = [b"stream\n\0".as_slice(), &[samples[1]], b"\nendstream"].concat();
        assert!(
            published
                .windows(scrubbed_stream.len())
                .any(|bytes| bytes == scrubbed_stream)
        );
        assert!(!published.windows(jpeg.len()).any(|bytes| bytes == jpeg));
    }
}

#[test]
fn jpeg_filter_chains_and_jpeg_candidates_are_refused() {
    let jpeg = grayscale_jpeg();
    let mut ascii_hex = jpeg
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    ascii_hex.push('>');
    let chained = jpeg_pdf("[/ASCIIHexDecode /DCTDecode]", ascii_hex.as_bytes());
    assert!(matches!(
        NativePdfSession::open_scoped(&chained, NativeEditLimits::default(), [5; 16]),
        Err(NativeSessionError::Graph(NativeEditError::Reader(
            PdfReadError::FilterUnsupported
        )))
    ));

    let source = jpeg_pdf("/DCTDecode", &jpeg);
    let session = open(&source);
    let base = session.snapshot();
    let marks = [mark(&session, 0, [14_000, 32_000, 15_000, 33_000])];
    let context = NativeRedactionContext {
        binding: base.binding,
        request_id: 1,
        accepted_hash: base.output_hash,
        page_order: &base.page_state.order,
    };
    for filter in ["/DCTDecode", "[/DCTDecode]"] {
        let candidate = jpeg_pdf(filter, &jpeg);
        let policy = NativeRedactionPolicy {
            binding: base.binding,
            request_id: 1,
            candidate_hash: *blake3::hash(&candidate).as_bytes(),
            marks: &marks,
        };
        let evidence = verify_native_redaction(&source, &source, &candidate, context, policy);
        assert_eq!(
            evidence.structure().failure(),
            Some(NativeVerificationFailure::UnsupportedProfile),
            "{filter}"
        );
    }
}

#[test]
fn mark_requests_order_mutations_without_consuming_document_receipts() {
    let source = fixture();
    let mut session = open(&source);
    let base = session.snapshot().binding;
    let first = mark(&session, 0, [11_000, 11_000, 13_000, 13_000]);
    let other = mark(&session, 0, [14_000, 32_000, 15_000, 33_000]);
    assert_eq!(
        session.add_redaction_mark(base, 0, first),
        Err(NativeSessionError::InvalidRequest)
    );
    session.add_redaction_mark(base, 2, first).unwrap();
    session.add_redaction_mark(base, 2, first).unwrap();
    for (id, mark) in [(1, first), (2, other)] {
        assert_eq!(
            session.add_redaction_mark(base, id, mark),
            Err(NativeSessionError::InvalidRequest)
        );
    }
    assert_eq!(session.lookup(base, 2), Ok(NativeRequestLookup::Unseen));
    let before = session.last_redaction_mark().unwrap();
    assert_eq!(before.request_id, 2);
    assert_eq!(before.binding, base);
    assert_eq!(before.accepted_hash, session.snapshot().output_hash);
    assert_eq!(session.redaction_marks(), &[first]);
    assert_eq!(
        session.remove_redaction_mark(base, 3, other),
        Err(NativeSessionError::InvalidRedactionMark)
    );
    assert_eq!(session.last_redaction_mark(), Some(before));
    session.remove_redaction_mark(base, 3, first).unwrap();
    session.remove_redaction_mark(base, 3, first).unwrap();
    session.clear_redaction_marks(base, 4).unwrap();
    session.clear_redaction_marks(base, 4).unwrap();
    assert!(session.redaction_marks().is_empty());

    session.add_redaction_mark(base, 5, first).unwrap();
    assert_eq!(
        session.stage_redaction(base, 5),
        Err(NativeSessionError::InvalidRequest)
    );
    assert_eq!(
        session.check_close(base, 5),
        Err(NativeSessionError::InvalidRequest)
    );
    let candidate = session.stage_redaction(base, 6).unwrap();
    session.add_redaction_mark(base, 5, first).unwrap();
    assert_eq!(session.stage_redaction(base, 6), Ok(candidate));
    session.cancel_redaction(base, 6).unwrap();
    assert_eq!(
        session.clear_redaction_marks(base, 6),
        Err(NativeSessionError::ReconciliationRequired)
    );
    session.clear_redaction_marks(base, 7).unwrap();
    assert_eq!(session.lookup(base, 7), Ok(NativeRequestLookup::Unseen));
    assert_eq!(session.snapshot().consumed_request, 0);
    assert_accepted(&session, &source, 0, &source);
    let edit = NativeNoteCommand::Add {
        page: session.snapshot().scene.pages[0].id,
        rect: [70, 70, 75, 75],
        contents: "Review note".into(),
    };
    assert_eq!(
        session.apply(base, 7, &edit),
        Err(NativeSessionError::ReconciliationRequired)
    );
    let receipt = session.apply(base, 8, &edit).unwrap();
    assert_eq!(
        session.clear_redaction_marks(base, 9),
        Err(NativeSessionError::RevisionMismatch)
    );
    assert_eq!(
        session.clear_redaction_marks(receipt.binding, 8),
        Err(NativeSessionError::InvalidRequest)
    );
    assert_eq!(session.apply(base, 8, &edit), Ok(receipt));
}

#[test]
fn removed_redaction_mark_replays_after_page_rotation() {
    let source = fixture();
    let mut session = open(&source);
    let binding = session.snapshot().binding;
    let original = mark(&session, 0, [10_000, 10_000, 20_000, 20_000]);
    session.add_redaction_mark(binding, 1, original).unwrap();
    let stable_id = session.snapshot().page_state.order[0];
    let rotate = NativePageCommand::RotatePage {
        target: NativePageTarget { page: 0, stable_id },
        degrees: 90,
    };
    session.apply_page(binding, 2, &rotate).unwrap();

    let current = session.snapshot().binding;
    let current_geometry = session.redaction_mark_geometry(original.page_id).unwrap();
    assert_ne!(current_geometry, original.geometry);
    assert_eq!(session.redaction_marks(), &[original]);
    session.remove_redaction_mark(current, 3, original).unwrap();
    let receipt = session.last_redaction_mark();
    session.remove_redaction_mark(current, 3, original).unwrap();
    assert_eq!(session.last_redaction_mark(), receipt);
    assert_eq!(session.lookup(current, 3), Ok(NativeRequestLookup::Unseen));
    assert_eq!(
        session.remove_redaction_mark(
            current,
            3,
            NativeRedactionMark {
                geometry: current_geometry,
                ..original
            }
        ),
        Err(NativeSessionError::InvalidRedactionMark)
    );
    assert_eq!(
        session.remove_redaction_mark(binding, 3, original),
        Err(NativeSessionError::RevisionMismatch)
    );
    assert!(session.redaction_marks().is_empty());
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
                1,
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
    session.add_redaction_mark(base.binding, 1, first).unwrap();
    session
        .remove_redaction_mark(base.binding, 2, first)
        .unwrap();
    session.add_redaction_mark(base.binding, 3, first).unwrap();
    session.clear_redaction_marks(base.binding, 4).unwrap();
    assert!(session.redaction_marks().is_empty());
    session.add_redaction_mark(base.binding, 5, first).unwrap();
    session.add_redaction_mark(base.binding, 6, second).unwrap();
    assert_accepted(&session, &source, 0, &source);

    let staged = session.stage_redaction(base.binding, 8).unwrap();
    let NativeRedactionStage::Pending(identity) = staged else {
        panic!("expected pending status")
    };
    assert_eq!(
        session.lookup(base.binding, 8),
        Ok(NativeRequestLookup::ReconciliationRequired)
    );
    session.add_redaction_mark(base.binding, 9, first).unwrap();
    assert_eq!(session.stage_redaction(base.binding, 8), Ok(staged));
    assert_eq!(
        session.redaction_candidate_status().unwrap().status(),
        NativeVerificationStatus::Pending
    );
    assert!(session.publish_redaction(base.binding, 8).is_err());
    assert_eq!(session.snapshot().scene.scene_hash, base.scene.scene_hash);
    assert_accepted(&session, &source, 0, &source);
    assert_eq!(session.redaction_marks(), &[first, second]);

    let evidence = session.verify_redaction(base.binding, 8).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    assert_eq!(
        evidence.identity().unwrap().candidate_hash(),
        identity.candidate_hash
    );
    assert_accepted(&session, &source, 0, &source);
    let receipt = session.publish_redaction(base.binding, 8).unwrap();
    assert_eq!(receipt.output_hash, identity.candidate_hash);
    assert_eq!(receipt.binding.revision, 1);
    assert_eq!(session.publish_redaction(base.binding, 8), Ok(receipt));
    assert_eq!(
        session.stage_redaction(base.binding, 8),
        Ok(NativeRedactionStage::Accepted(receipt))
    );
    assert_eq!(
        session.lookup(base.binding, 8),
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

    let next = session
        .apply(
            receipt.binding,
            10,
            &NativeNoteCommand::Add {
                page: session.snapshot().scene.pages[0].id,
                rect: [70, 70, 75, 75],
                contents: "new note".into(),
            },
        )
        .unwrap();
    let edited = session.output_bytes().to_vec();
    assert_ne!(next.output_hash, receipt.output_hash);
    assert_eq!(
        session.publish_redaction(base.binding, 8),
        Err(NativeSessionError::RevisionMismatch)
    );
    assert_eq!(
        session.stage_redaction(base.binding, 8),
        Err(NativeSessionError::RevisionMismatch)
    );
    assert_eq!(
        session.lookup(base.binding, 8),
        Ok(NativeRequestLookup::Accepted(receipt))
    );
    assert_accepted(&session, &edited, next.binding.revision, &source);
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
        add(&mut session, 1, 0, rect);
        assert!(session.stage_redaction(binding, 4).is_err());
        assert_accepted(&session, &source, 0, &source);
        assert_eq!(session.redaction_marks().len(), 1);
        add(&mut session, 3, 1, rect);
        session.stage_redaction(binding, 4).unwrap();
        assert!(
            session
                .verify_redaction(binding, 4)
                .unwrap()
                .checks_passed()
        );
        session.publish_redaction(binding, 4).unwrap();
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
    add(&mut session, 1, 0, [11_000, 11_000, 13_000, 13_000]);
    session.stage_redaction(base, 2).unwrap();
    let other = open(&source);
    assert!(
        session
            .verify_redaction(other.snapshot().binding, 2)
            .is_err()
    );
    assert!(session.publish_redaction(base, 3).is_err());
    assert!(session.stage_redaction(base, 3).is_err());
    session.cancel_redaction(base, 2).unwrap();
    assert!(session.stage_redaction(base, 2).is_err());
    assert!(session.publish_redaction(base, 2).is_err());
    assert_accepted(&session, &source, 0, &source);
    session.stage_redaction(base, 3).unwrap();
    assert!(session.verify_redaction(base, 3).unwrap().checks_passed());
    let receipt = session
        .apply(
            base,
            4,
            &NativeNoteCommand::Add {
                page: session.snapshot().scene.pages[0].id,
                rect: [70, 70, 75, 75],
                contents: "new note".into(),
            },
        )
        .unwrap();
    let prior = session.output_bytes().to_vec();
    assert!(session.redaction_candidate_status().is_none());
    assert!(session.publish_redaction(base, 3).is_err());
    assert!(session.stage_redaction(receipt.binding, 5).is_err());
    assert_eq!(session.redaction_marks().len(), 1);
    assert_accepted(&session, &prior, 1, &source);
    session.clear_redaction_marks(receipt.binding, 6).unwrap();
    add(&mut session, 7, 0, [11_000, 11_000, 13_000, 13_000]);
    session.stage_redaction(receipt.binding, 8).unwrap();
    assert!(
        session
            .verify_redaction(receipt.binding, 8)
            .unwrap()
            .checks_passed()
    );
    session.publish_redaction(receipt.binding, 8).unwrap();
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
    let mark = mark(&session, 0, [11_000, 11_000, 13_000, 13_000]);
    let refusal = Err(NativeSessionError::Graph(NativeEditError::Unsupported));
    assert_eq!(session.check_redaction_admission(), refusal);
    assert_eq!(session.add_redaction_mark(binding, 1, mark), refusal);
    assert_eq!(
        session.stage_redaction(binding, 1),
        Err(NativeSessionError::InvalidRedactionMark)
    );
    assert_eq!(session.lookup(binding, 1), Ok(NativeRequestLookup::Unseen));
    assert_accepted(&session, &source, 0, &source);
    assert_eq!(session.snapshot().binding, binding);
    assert!(session.redaction_marks().is_empty());
    assert!(session.redaction_marks_binding().is_none());
    assert!(session.redaction_candidate_status().is_none());
    session.clear_redaction_marks(binding, 1).unwrap();
}

fn raster_fixture(size: u32, paint: &str) -> Vec<u8> {
    pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            format!("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 {size} {size}] >>")
                .into_bytes(),
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
            flate("", paint.as_bytes()),
        ],
        "",
    )
}

#[test]
fn admission_tracks_the_workspace_left_by_the_live_session() {
    // @contract: An uncompressed stream isolates the scan allowance from decoder scratch space.
    let source = pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
            stream("", b"10 10 10 10 re f 40 40 10 10 re f"),
        ],
        "",
    );
    assert!(open(&source).redaction_admitted());

    let open_with_budget = |max_retained_bytes| {
        NativePdfSession::open_scoped(
            &source,
            NativeEditLimits {
                max_retained_bytes,
                ..NativeEditLimits::default()
            },
            [5; 16],
        )
    };
    let (mut lower, mut upper) = (1, 64 * 1024);
    while lower < upper {
        let mid = lower + (upper - lower) / 2;
        if open_with_budget(mid).is_ok_and(|session| session.redaction_admitted()) {
            upper = mid;
        } else {
            lower = mid + 1;
        }
    }

    let verify_projection = |session: &NativePdfSession, admitted, use_live: bool| {
        let snapshot = session.snapshot();
        NativePdfSession::verify_page_and_field_projection(
            &source,
            snapshot.limits,
            snapshot.binding,
            snapshot.scene.scene_hash,
            &snapshot.page_state,
            &snapshot.fields,
            &[],
            admitted,
            use_live.then_some(session),
        )
    };
    let below = open_with_budget(lower - 1).unwrap();
    assert!(!below.redaction_admitted());
    assert_eq!(verify_projection(&below, false, false), Ok(true));
    assert_eq!(verify_projection(&below, true, false), Ok(false));
    drop(below);

    let mut session = open_with_budget(lower).unwrap();
    let binding = session.snapshot().binding;
    let consumed_request = session.snapshot().consumed_request;
    let mark = mark(&session, 0, [12_000, 12_000, 13_000, 13_000]);
    assert!(session.redaction_admitted());
    assert_eq!(verify_projection(&session, true, true), Ok(true));
    assert_eq!(verify_projection(&session, true, false), Ok(true));

    session.add_redaction_mark(binding, 1, mark).unwrap();
    assert_eq!(
        session.check_redaction_admission(),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(verify_projection(&session, false, true), Ok(true));
    assert_eq!(verify_projection(&session, true, true), Ok(false));
    assert_eq!(verify_projection(&session, false, false), Ok(true));
    assert_eq!(verify_projection(&session, true, false), Ok(true));
    assert_eq!(
        session.add_redaction_mark(
            binding,
            2,
            NativeRedactionMark {
                rect: [42_000, 42_000, 43_000, 43_000],
                ..mark
            },
        ),
        Err(NativeSessionError::Budget)
    );
    session.add_redaction_mark(binding, 2, mark).unwrap();
    assert_eq!(
        session.stage_redaction(binding, 3),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(session.lookup(binding, 3), Ok(NativeRequestLookup::Unseen));
    assert!(session.redaction_candidate_status().is_none());
    assert_eq!(session.redaction_marks(), &[mark]);
    assert_eq!(session.redaction_marks_binding(), Some(binding));
    assert_eq!(session.snapshot().binding, binding);
    assert_eq!(session.snapshot().consumed_request, consumed_request);
    assert_accepted(&session, &source, 0, &source);

    session.remove_redaction_mark(binding, 3, mark).unwrap();
    assert!(session.redaction_admitted());
    session.add_redaction_mark(binding, 4, mark).unwrap();
    assert!(!session.redaction_admitted());
    session.clear_redaction_marks(binding, 5).unwrap();
    assert!(session.redaction_admitted());
}

#[test]
fn origin_over_the_redaction_byte_ceiling_refuses_marks_and_staging() {
    let mut unused_object = b"null\n%".to_vec();
    unused_object.resize(32 * 1024 * 1024, b' ');
    let source = pdf(
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
            flate("", b"10 10 10 10 re f"),
            unused_object,
        ],
        "",
    );
    let mut session = open(&source);
    let binding = session.snapshot().binding;
    let mark = mark(&session, 0, [12_000, 12_000, 13_000, 13_000]);

    assert!(!session.redaction_admitted());
    assert_eq!(
        session.add_redaction_mark(binding, 1, mark),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(
        session.stage_redaction(binding, 1),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(session.lookup(binding, 1), Ok(NativeRequestLookup::Unseen));
    assert!(session.redaction_marks().is_empty());
    assert!(session.redaction_marks_binding().is_none());
    assert!(session.redaction_candidate_status().is_none());
    assert_accepted(&session, &source, 0, &source);
}

#[test]
fn redaction_accepts_a_rewrite_larger_than_the_import_limit() {
    let source = raster_fixture(100, "10 10 10 10 re f 40 40 10 10 re f");
    let limits = NativeEditLimits {
        max_source_bytes: source.len(),
        max_output_bytes: 4096,
        ..NativeEditLimits::default()
    };
    let mut session = NativePdfSession::open_scoped(&source, limits, [5; 16]).unwrap();
    let base = session.snapshot().binding;
    let page_id = session.snapshot().page_state.order[0];
    assert!(session.redaction_admitted());
    let edited = session
        .apply_page(
            base,
            1,
            &NativePageCommand::InsertBlankPage {
                target: NativePageTarget {
                    page: 0,
                    stable_id: page_id,
                },
                side: NativePageSide::After,
            },
        )
        .unwrap();
    let snapshot = session.snapshot();
    let accepted = session.output_bytes();
    assert!(accepted.len() > limits.max_source_bytes);
    assert!(accepted.len() <= limits.max_output_bytes);
    assert_ne!(snapshot.output_hash, edited.binding.origin_hash);
    assert!(session.redaction_admitted());
    assert_eq!(
        NativePdfSession::verify_page_projection(
            accepted,
            limits,
            edited.binding,
            snapshot.scene.scene_hash,
            &snapshot.page_state,
        ),
        Ok(true)
    );
    assert_eq!(
        NativePdfSession::verify_text_projection(
            accepted,
            limits,
            edited.binding,
            &snapshot.scene.text_pages,
        ),
        Ok(true)
    );
    assert_eq!(
        NativePdfSession::verify_edit_projection(
            accepted,
            limits,
            edited.binding,
            &snapshot.scene.text_pages,
            &snapshot.scene.object_pages,
        ),
        Ok(true)
    );
    for live in [Some(&session), None] {
        for admitted in [true, false] {
            assert_eq!(
                NativePdfSession::verify_page_and_field_projection(
                    accepted,
                    limits,
                    edited.binding,
                    snapshot.scene.scene_hash,
                    &snapshot.page_state,
                    &snapshot.fields,
                    &[],
                    admitted,
                    live,
                ),
                Ok(admitted || live.is_none())
            );
        }
    }

    add(&mut session, 2, 0, [12_000, 12_000, 13_000, 13_000]);
    let staged = session.stage_redaction(edited.binding, 3).unwrap();
    assert_eq!(session.stage_redaction(edited.binding, 3), Ok(staged));
    assert_eq!(
        session.stage_redaction(base, 3),
        Err(NativeSessionError::RevisionMismatch)
    );
    let evidence = session.verify_redaction(edited.binding, 3).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    session.publish_redaction(edited.binding, 3).unwrap();
    assert_eq!(session.origin_bytes(), source);
}

#[test]
fn accepted_page_preview_refuses_a_constrained_session_without_mutation() {
    let source = raster_fixture(150, "10 10 10 10 re f 40 40 10 10 re f");
    let default_session = open(&source);
    let snapshot = default_session.snapshot();
    let raster = default_session
        .accepted_page_raster(
            snapshot.binding,
            snapshot.output_hash,
            snapshot.page_state.order[0],
        )
        .unwrap();
    assert_eq!((raster.width, raster.height), (300, 300));
    assert_eq!(raster.crop_box, [0, 0, 150_000, 150_000]);
    assert_eq!(raster.rotation, GEOMETRY.rotation);
    assert_eq!(raster.rgb.len(), 300 * 300 * 3);

    // @contract: The 300 × 300 RGB output alone exceeds the constrained session cap.
    let mut session = NativePdfSession::open_scoped(
        &source,
        NativeEditLimits {
            max_retained_bytes: 192 * 1024,
            ..NativeEditLimits::default()
        },
        [5; 16],
    )
    .unwrap();
    let binding = session.snapshot().binding;
    let output_hash = session.snapshot().output_hash;
    let scene_hash = session.snapshot().scene.scene_hash;
    let page_id = session.snapshot().page_state.order[0];
    let mark = NativeRedactionMark {
        page_id,
        geometry: session.redaction_mark_geometry(page_id).unwrap(),
        rect: [12_000, 12_000, 13_000, 13_000],
    };
    session.add_redaction_mark(binding, 1, mark).unwrap();
    let staged = session.stage_redaction(binding, 2).unwrap();

    assert_eq!(
        session
            .accepted_page_raster(binding, output_hash, mark.page_id)
            .err(),
        Some(NativeSessionError::Budget)
    );
    assert_accepted(&session, &source, binding.revision, &source);
    assert_eq!(session.snapshot().binding, binding);
    assert_eq!(session.snapshot().scene.scene_hash, scene_hash);
    assert_eq!(session.redaction_marks(), &[mark]);
    assert_eq!(session.redaction_marks_binding(), Some(binding));
    assert_eq!(session.stage_redaction(binding, 2), Ok(staged));
    assert_eq!(
        session.redaction_candidate_status().unwrap().status(),
        NativeVerificationStatus::Pending
    );
}

#[test]
fn page_geometry_errors_preserve_their_type_in_redaction_preflight() {
    for (crop, error) in [
        ("/CropBox [-1 0 100 100]", NativeEditError::Unsupported),
        ("/CropBox null", NativeEditError::Malformed),
    ] {
        let source = pdf(
            vec![
                b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
                b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 100 100] >>".to_vec(),
                b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
                format!("<< /Type /Page /Parent 2 0 R {crop} >>").into_bytes(),
            ],
            "",
        );
        let mut session = open(&source);
        let binding = session.snapshot().binding;
        let mark = mark(&session, 0, [12_000, 12_000, 13_000, 13_000]);
        let refusal = Err(NativeSessionError::Graph(error));

        assert_eq!(session.redaction_mark_geometry(mark.page_id), Ok(GEOMETRY));
        assert_eq!(session.check_redaction_admission(), refusal.clone());
        assert_eq!(
            session.add_redaction_mark(binding, 1, mark),
            refusal.clone()
        );
        assert_eq!(session.stage_redaction(binding, 1).map(|_| ()), refusal);
        assert_eq!(session.lookup(binding, 1), Ok(NativeRequestLookup::Unseen));
        assert!(session.redaction_marks().is_empty());
        assert!(session.redaction_marks_binding().is_none());
        assert!(session.redaction_candidate_status().is_none());
        assert_accepted(&session, &source, 0, &source);
    }
}

#[test]
fn raster_budget_refuses_native_marks_and_staging_without_mutation() {
    for (size, page_count) in [(3_000, 1), (2_000, 4)] {
        let source = raster_fixture(size, "10 10 10 10 re f");
        let mut session = open(&source);
        let page_id = session.snapshot().page_state.order[0];
        let insert = NativePageCommand::InsertBlankPage {
            target: NativePageTarget {
                page: 0,
                stable_id: page_id,
            },
            side: NativePageSide::After,
        };
        for request_id in 1..page_count {
            assert!(session.redaction_admitted(), "{request_id} pages");
            session
                .apply_page(session.snapshot().binding, request_id, &insert)
                .unwrap();
        }
        assert_eq!(session.snapshot().page_state.order.len() as u64, page_count);

        let binding = session.snapshot().binding;
        let before = session.output_bytes().to_vec();
        let mark = NativeRedactionMark {
            page_id,
            geometry: session.redaction_mark_geometry(page_id).unwrap(),
            rect: [12_000, 12_000, 13_000, 13_000],
        };
        assert_eq!(
            session.add_redaction_mark(binding, page_count, mark),
            Err(NativeSessionError::Budget)
        );
        assert_eq!(
            session.stage_redaction(binding, page_count),
            Err(NativeSessionError::Budget)
        );
        assert_eq!(
            session.lookup(binding, page_count),
            Ok(NativeRequestLookup::Unseen)
        );
        assert!(!session.redaction_admitted());
        assert!(session.redaction_marks().is_empty());
        assert!(session.redaction_marks_binding().is_none());
        assert!(session.redaction_candidate_status().is_none());
        assert_accepted(&session, &before, binding.revision, &source);
    }
}

#[test]
fn an_inconclusive_raster_check_preserves_the_previous_accepted_revision() {
    // @contract: Both full-page rasters exceed the session workspace allowance.
    let mut paint = String::from("10 10 10 10 re f 40 616 m");
    for step in -23..=24 {
        write!(paint, " {} {} l", 1_000 + 40 * step, 40 + step * step).unwrap();
    }
    paint.push_str(" 1960 1960 l 40 1960 l h f");
    let source = raster_fixture(1_980, &paint);
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
    assert!(session.redaction_admitted());
    let before = session.output_bytes().to_vec();
    let page_id = session.snapshot().page_state.order[0];
    let mark = NativeRedactionMark {
        page_id,
        geometry: session.redaction_mark_geometry(page_id).unwrap(),
        rect: [12_000, 12_000, 13_000, 13_000],
    };
    session.add_redaction_mark(prior.binding, 2, mark).unwrap();
    session.stage_redaction(prior.binding, 3).unwrap();
    let evidence = session.verify_redaction(prior.binding, 3).unwrap();
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
    assert_eq!(
        session.publish_redaction(prior.binding, 3),
        Err(NativeSessionError::RedactionUnverified)
    );
    assert_accepted(&session, &before, 1, &source);
    assert_eq!(session.snapshot().scene.scene_hash, prior.scene_hash);
    assert_eq!(session.redaction_marks(), &[mark]);
    assert!(session.accepted_redaction_evidence().is_none());
}

#[test]
fn readback_budgets_both_rasters_and_preserves_partial_evidence() {
    let source = raster_fixture(100, "10 10 10 10 re f 40 40 10 10 re f");
    // @contract: One 200 × 200 RGB buffer fits this cap. The pair needs 240,000 bytes.
    let mut session = NativePdfSession::open_scoped(
        &source,
        NativeEditLimits {
            max_retained_bytes: 192 * 1024,
            ..NativeEditLimits::default()
        },
        [5; 16],
    )
    .unwrap();
    let binding = session.snapshot().binding;
    let mark = mark(&session, 0, [12_000, 12_000, 13_000, 13_000]);
    session.add_redaction_mark(binding, 1, mark).unwrap();

    for request in 2..=3 {
        let staged = session.stage_redaction(binding, request).unwrap();
        let admission_before_verification = session.check_redaction_admission();
        assert_eq!(admission_before_verification, Ok(()));
        let evidence = session.verify_redaction(binding, request).unwrap();
        assert_eq!(evidence.identity_failure(), None);
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
            evidence.raster().failure(),
            Some(NativeVerificationFailure::Budget)
        );
        assert!(evidence.raster().covered_pages().is_empty());
        assert!(!evidence.checks_passed());
        let status = session.redaction_candidate_status().unwrap();
        assert_eq!(status.status(), NativeVerificationStatus::Inconclusive);
        let wire = serde_json::to_value(status).unwrap();
        assert_eq!(
            wire["evidence"]["raster"],
            serde_json::json!({"status": "inconclusive", "failure": "budget", "coveredPages": []})
        );
        assert_eq!(
            session.check_redaction_admission(),
            admission_before_verification
        );
        assert_eq!(session.stage_redaction(binding, request).unwrap(), staged);
        let retry = session.verify_redaction(binding, request).unwrap();
        assert_eq!(serde_json::to_value(retry).unwrap(), wire["evidence"]);
        assert_eq!(
            session.publish_redaction(binding, request),
            Err(NativeSessionError::RedactionUnverified)
        );
        assert_accepted(&session, &source, 0, &source);
        assert_eq!(session.redaction_marks(), &[mark]);
        assert_eq!(session.redaction_marks_binding(), Some(binding));
        assert!(session.accepted_redaction_evidence().is_none());
        session.cancel_redaction(binding, request).unwrap();
        assert!(session.redaction_candidate_status().is_none());
        assert_eq!(session.redaction_marks(), &[mark]);
    }
    session.clear_redaction_marks(binding, 4).unwrap();
    assert!(session.redaction_marks().is_empty());
    assert!(session.redaction_marks_binding().is_none());
    assert_eq!(session.check_redaction_admission(), Ok(()));
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
            1,
            NativeRedactionMark {
                page_id,
                geometry,
                rect: [30_000, 20_000, 40_000, 30_000],
            },
        )
        .unwrap();
    session.stage_redaction(binding, 2).unwrap();
    let evidence = session.verify_redaction(binding, 2).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    session.publish_redaction(binding, 2).unwrap();
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
    add(&mut session, 1, 0, [14_000, 14_000, 15_000, 15_000]);
    assert!(session.stage_redaction(binding, 2).is_err());
    assert_accepted(&session, &source, 0, &source);
    add(&mut session, 3, 0, [57_000, 44_000, 58_000, 45_000]);
    session.stage_redaction(binding, 4).unwrap();
    let evidence = session.verify_redaction(binding, 4).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    session.publish_redaction(binding, 4).unwrap();
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
    let consumed_request = session.snapshot().consumed_request;
    let mark = mark(&session, 0, [11_000, 11_000, 13_000, 13_000]);
    assert!(session.redaction_admitted());
    session.add_redaction_mark(binding, 1, mark).unwrap();
    for _ in 0..2 {
        assert_eq!(
            session.stage_redaction(binding, 2),
            Err(NativeSessionError::Budget)
        );
        assert_eq!(session.lookup(binding, 2), Ok(NativeRequestLookup::Unseen));
    }
    assert_accepted(&session, &source, 0, &source);
    assert_eq!(session.snapshot().binding, binding);
    assert_eq!(session.snapshot().consumed_request, consumed_request);
    assert!(session.redaction_candidate_status().is_none());
    assert_eq!(session.redaction_marks(), &[mark]);
    assert_eq!(session.redaction_marks_binding(), Some(binding));
}

#[test]
fn redaction_keeps_disjoint_paint_inside_overlapping_path_bounds() {
    let diagonal = b"10 10 m\n80 80 l\nS";
    let triangle = b"10 10 m\n80 10 l\n80 80 l\nh\nf*";
    let square = b"18 63 6 6 re\nf";
    let content = [
        b"0 0 1 RG 0.001 w ".as_slice(),
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
    add(&mut session, 1, 0, [19_000, 64_000, 23_000, 68_000]);
    session.stage_redaction(binding, 2).unwrap();
    let evidence = session.verify_redaction(binding, 2).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    session.publish_redaction(binding, 2).unwrap();

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

    let mut crossing = open(&source);
    let binding = crossing.snapshot().binding;
    add(&mut crossing, 1, 0, [39_000, 39_000, 41_000, 41_000]);
    crossing.stage_redaction(binding, 2).unwrap();
    let evidence = crossing.verify_redaction(binding, 2).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    crossing.publish_redaction(binding, 2).unwrap();
    let output = crossing.output_bytes();
    assert!(
        !output
            .windows(diagonal.len())
            .any(|window| window == diagonal)
    );
    assert!(output.windows(square.len()).any(|window| window == square));

    let mut grazing = open(&source);
    let binding = grazing.snapshot().binding;
    add(&mut grazing, 1, 0, [30_000, 31_001, 31_000, 32_000]);
    assert_eq!(
        grazing.stage_redaction(binding, 2).unwrap_err(),
        NativeSessionError::Graph(NativeEditError::Unsupported)
    );
    assert!(grazing.redaction_candidate_status().is_none());
    assert_accepted(&grazing, &source, 0, &source);
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
    add(&mut session, 1, 0, [11_000, 11_000, 12_000, 12_000]);
    session.stage_redaction(binding, 2).unwrap();
    let evidence = session.verify_redaction(binding, 2).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    let receipt = session.publish_redaction(binding, 2).unwrap();
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
            1,
            NativeRedactionMark {
                page_id,
                geometry,
                rect,
            },
        )
        .unwrap();
    session.stage_redaction(binding, 2).unwrap();
    let evidence = session.verify_redaction(binding, 2).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    session.publish_redaction(binding, 2).unwrap();
    assert_eq!(session.origin_bytes(), source);
    assert_ne!(session.output_bytes(), source);
}

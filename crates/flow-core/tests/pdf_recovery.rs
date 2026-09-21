use flow_core::{
    MAX_PDF_MANIFEST_BYTES,
    canonical::{canonical_bytes, canonical_hash},
    layout::LayoutUnit,
    model::FlowDocument,
    pdf::{
        PdfExportOptions, PdfExportRequest, PdfFontManifestIdentity, PdfPagePlan, PdfRecoveryError,
        PdfRecoveryExpectation, PdfReproducibilityInputs, export_pdf, recover_owned_source,
    },
};

fn page_plan() -> PdfPagePlan {
    PdfPagePlan::empty(
        LayoutUnit::from_raw(595 * 64),
        LayoutUnit::from_raw(842 * 64),
    )
    .unwrap()
}

fn source_document() -> FlowDocument {
    FlowDocument::deterministic_sample("uk-UA").unwrap()
}

fn owned_request(document: &FlowDocument) -> PdfExportRequest {
    let canonical = canonical_bytes(document).unwrap();
    PdfExportRequest::new(
        document.revision,
        canonical_hash(&canonical),
        "layout-settings",
        vec![page_plan()],
        PdfExportOptions::default(),
    )
    .unwrap()
    .with_reproducibility_inputs(PdfReproducibilityInputs {
        layout_result_hash: Some("layout-result".to_owned()),
        font_catalog_identity: Some("catalog-identity".to_owned()),
        font_faces: vec![PdfFontManifestIdentity {
            face_id: "noto-sans".to_owned(),
            content_hash: "sha256:noto-sans".to_owned(),
        }],
        hyphenation_identity: Some("uk-v1".to_owned()),
    })
    .unwrap()
    .with_source_document(document)
    .unwrap()
}

#[test]
fn owned_manifest_is_stable_and_recovers_the_exact_canonical_source() {
    let document = source_document();
    let canonical = canonical_bytes(&document).unwrap();
    let hash = canonical_hash(&canonical);
    let request = owned_request(&document);
    let first = export_pdf(&request).unwrap();
    let second = export_pdf(&request).unwrap();

    assert_eq!(first.manifest, second.manifest);
    assert_eq!(first.private_source_stream, second.private_source_stream);
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(
        first.manifest.source_document_id,
        Some(document.document_id.clone())
    );
    assert_eq!(first.manifest.source_revision, document.revision);
    assert_eq!(first.manifest.source_hash, hash);
    assert_eq!(first.manifest.source_payload_hash, Some(hash.clone()));
    assert_eq!(first.manifest.font_faces.len(), 1);
    assert!(
        !first
            .private_source_stream
            .windows(b"NotoSans-Regular.ttf".len())
            .any(|window| window == b"NotoSans-Regular.ttf")
    );

    let recovered = recover_owned_source(
        Some(&first.private_source_stream),
        &PdfRecoveryExpectation {
            document_id: Some(document.document_id.clone()),
            revision: Some(document.revision),
            canonical_hash: Some(hash.clone()),
            export_fingerprint: Some(first.export_fingerprint.clone()),
        },
    )
    .unwrap();
    assert!(recovered.exact);
    assert_eq!(recovered.document, document);
    assert_eq!(recovered.canonical_hash, hash);
    assert_eq!(recovered.canonical_json.as_bytes(), canonical.as_slice());
}

#[test]
fn reproducibility_and_export_options_change_manifest_and_result_identity() {
    let document = source_document();
    let request = owned_request(&document);
    let first = export_pdf(&request).unwrap();

    let options = PdfExportOptions {
        producer: "FlowPDF test producer".to_owned(),
        ..PdfExportOptions::default()
    };
    let canonical = canonical_bytes(&document).unwrap();
    let changed = PdfExportRequest::new(
        document.revision,
        canonical_hash(&canonical),
        "layout-settings",
        vec![page_plan()],
        options,
    )
    .unwrap()
    .with_source_document(&document)
    .unwrap();
    let second = export_pdf(&changed).unwrap();

    assert_ne!(first.export_fingerprint, second.export_fingerprint);
    assert_ne!(first.manifest, second.manifest);
    assert_ne!(first.byte_hash, second.byte_hash);
    assert_ne!(first.private_source_stream, second.private_source_stream);
}

#[test]
fn recovery_classifies_missing_external_truncated_hash_and_identity_failures() {
    let document = source_document();
    let result = export_pdf(&owned_request(&document)).unwrap();

    assert_eq!(
        recover_owned_source(None, &PdfRecoveryExpectation::default()).unwrap_err(),
        PdfRecoveryError::MissingPayload
    );
    assert_eq!(
        recover_owned_source(Some(b"%PDF-1.7\n"), &PdfRecoveryExpectation::default()).unwrap_err(),
        PdfRecoveryError::UnsupportedExternalPdf
    );
    assert_eq!(
        recover_owned_source(
            Some(&result.private_source_stream[..result.private_source_stream.len() - 1]),
            &PdfRecoveryExpectation::default(),
        )
        .unwrap_err(),
        PdfRecoveryError::EnvelopeTruncated
    );

    let mut corrupt = result.private_source_stream.clone();
    *corrupt.last_mut().unwrap() ^= 0xff;
    assert_eq!(
        recover_owned_source(Some(&corrupt), &PdfRecoveryExpectation::default()).unwrap_err(),
        PdfRecoveryError::HashMismatch
    );

    let wrong_document_id =
        flow_core::model::DocumentId::new("00000000-0000-4000-8000-000000000999").unwrap();
    assert_eq!(
        recover_owned_source(
            Some(&result.private_source_stream),
            &PdfRecoveryExpectation {
                document_id: Some(wrong_document_id),
                ..PdfRecoveryExpectation::default()
            },
        )
        .unwrap_err(),
        PdfRecoveryError::DocumentIdMismatch
    );
    assert_eq!(
        recover_owned_source(
            Some(&result.private_source_stream),
            &PdfRecoveryExpectation {
                revision: Some(document.revision + 1),
                ..PdfRecoveryExpectation::default()
            },
        )
        .unwrap_err(),
        PdfRecoveryError::RevisionMismatch
    );

    let magic_length = b"FlowPDF\0Source\0v1\0".len();
    let mut oversized = result.private_source_stream.clone();
    oversized[magic_length..magic_length + 4]
        .copy_from_slice(&(u32::try_from(MAX_PDF_MANIFEST_BYTES + 1).unwrap()).to_be_bytes());
    assert_eq!(
        recover_owned_source(Some(&oversized), &PdfRecoveryExpectation::default()).unwrap_err(),
        PdfRecoveryError::BudgetExceeded
    );
}

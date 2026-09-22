use flow_core::pdf::{
    PdfMappingOrigin, PdfOcrCandidate, PdfReconstructionError, PdfReconstructionRequest,
    PdfReviewAction, PdfReviewDecision, PdfSceneRequest, PdfTextMapping, accept_pdf_reconstruction,
    open_pdf, read_pdf_scene, reconstruct_pdf_scene,
};
use flow_core::{LayoutRect, LayoutUnit};

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

fn text_fixture() -> Vec<u8> {
    let content = b"BT /F1 12 Tf 10 150 Td (Hello) Tj ET\n0 0 m 50 0 l 50 50 l h S\n";
    pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources 4 0 R /Contents 5 0 R >>".to_vec()),
        (4, b"<< /Font << /F1 6 0 R >> >>".to_vec()),
        stream(5, "", content),
        (6, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /MissingWidth 500 >>".to_vec()),
    ])
}

fn image_only_fixture() -> Vec<u8> {
    let content = b"0 0 m 50 0 l 50 50 l h S\n";
    pdf(vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources 4 0 R /Contents 5 0 R >>".to_vec()),
        (4, b"<< >>".to_vec()),
        stream(5, "", content),
    ])
}

fn scene(bytes: &[u8]) -> flow_core::pdf::PdfScene {
    let reader = open_pdf(bytes, Default::default()).expect("reader");
    read_pdf_scene(&reader, &PdfSceneRequest::default()).expect("scene")
}

#[test]
fn single_column_candidate_preserves_mapping_and_opaque_island() {
    let scene = scene(&text_fixture());
    let result = reconstruct_pdf_scene(&PdfReconstructionRequest {
        source_hash: scene.source_hash.clone(),
        scene: scene.clone(),
        locale: "en-US".to_owned(),
        ocr_candidates: Vec::new(),
    })
    .expect("reconstruction");

    assert_eq!(result.blocks.len(), 1);
    assert_eq!(result.blocks[0].text, "Hello");
    assert!(result.blocks[0].mappings.iter().any(|mapping| matches!(
        mapping.origin,
        PdfMappingOrigin::PdfText {
            mapping: PdfTextMapping::SimpleEncoding
        }
    )));
    assert!(
        result
            .opaque_islands
            .iter()
            .any(|island| matches!(island.kind, flow_core::pdf::PdfOpaqueIslandKind::Path))
    );
    assert_eq!(
        result.candidate.document.provenance,
        flow_core::model::Provenance::ExternalReconstruction {
            source_hash: format!("pdf:blake3:v1:{}", scene.source_hash),
            reconstruction_schema_version: 1,
        }
    );

    let second = reconstruct_pdf_scene(&PdfReconstructionRequest {
        source_hash: scene.source_hash.clone(),
        scene,
        locale: "en-US".to_owned(),
        ocr_candidates: Vec::new(),
    })
    .expect("same reconstruction");
    assert_eq!(result.result_hash, second.result_hash);
}

#[test]
fn ocr_candidate_requires_review_before_acceptance() {
    let scene = scene(&image_only_fixture());
    let result = reconstruct_pdf_scene(&PdfReconstructionRequest {
        source_hash: scene.source_hash.clone(),
        scene,
        locale: "uk-UA".to_owned(),
        ocr_candidates: vec![PdfOcrCandidate {
            page_index: 0,
            rect: LayoutRect {
                x: LayoutUnit::from_raw(640),
                y: LayoutUnit::from_raw(8_000),
                width: LayoutUnit::from_raw(2_000),
                height: LayoutUnit::from_raw(768),
            },
            text: "Тест".to_owned(),
            confidence_basis_points: 4_000,
            provider: "fixture-ocr".to_owned(),
        }],
    })
    .expect("ocr reconstruction");
    assert_eq!(result.report.review_required_count, 1);
    assert!(result.blocks[0].review_required);
    assert!(matches!(
        accept_pdf_reconstruction(&result, &[]),
        Err(PdfReconstructionError::ReviewRequired)
    ));

    let accepted = accept_pdf_reconstruction(
        &result,
        &[PdfReviewDecision {
            node_id: result.blocks[0].node_id.clone(),
            action: PdfReviewAction::Keep,
        }],
    )
    .expect("reviewed candidate");
    assert_eq!(accepted.document.content[0].text(), "Тест");
    assert!(
        serde_json::to_string(&accepted)
            .expect("candidate JSON")
            .contains("externalReconstruction")
    );
}

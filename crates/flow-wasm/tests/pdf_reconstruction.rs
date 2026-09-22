use std::io::Write;

use flow_core::pdf::{PdfSceneRequest, open_pdf, read_pdf_scene};
use serde_json::{Value, json};

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
        writeln!(
            &mut output,
            "{:010} 00000 {} ",
            offset.unwrap_or(0),
            if offset.is_some() { 'n' } else { 'f' }
        )
        .unwrap();
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

fn response(serialized: &str) -> Value {
    serde_json::from_str(serialized).expect("reconstruction response JSON")
}

#[test]
fn reconstruction_boundary_returns_verified_text_candidate() {
    let scene = scene(&text_fixture());
    let request = json!({
        "protocolVersion": 1,
        "requestId": "reconstruction-valid",
        "sourceHash": scene.source_hash,
        "scene": scene,
        "locale": "en-US",
        "ocrCandidates": []
    });
    let serialized = flow_wasm::reconstruct_pdf(serde_json::to_string(&request).unwrap());
    let value = response(&serialized);
    assert_eq!(value["ok"], true);
    assert_eq!(value["requestId"], "reconstruction-valid");
    assert_eq!(value["result"]["result"]["blocks"][0]["text"], "Hello");
    assert!(
        value["result"]["result"]["candidate"]["canonicalJson"]
            .as_str()
            .unwrap()
            .contains("externalReconstruction")
    );
    assert!(flow_wasm::verify_pdf_reconstruction_response(serialized));
}

#[test]
fn reconstruction_boundary_requires_review_and_rejects_stale_or_malformed_input() {
    let source_scene = scene(&image_only_fixture());
    let request = json!({
        "protocolVersion": 1,
        "requestId": "reconstruction-ocr",
        "sourceHash": source_scene.source_hash,
        "scene": source_scene,
        "locale": "uk-UA",
        "ocrCandidates": [{
            "pageIndex": 0,
            "rect": {"x": 640, "y": 8000, "width": 2000, "height": 768},
            "text": "Тест",
            "confidenceBasisPoints": 4000,
            "provider": "fixture-ocr"
        }]
    });
    let mut serialized = flow_wasm::reconstruct_pdf(serde_json::to_string(&request).unwrap());
    let value = response(&serialized);
    assert_eq!(value["ok"], true);
    assert_eq!(
        value["result"]["result"]["report"]["reviewRequiredCount"],
        1
    );

    let result = value["result"]["result"].clone();
    let node_id = result["blocks"][0]["nodeId"].clone();
    let accept_request = json!({
        "protocolVersion": 1,
        "requestId": "reconstruction-accept",
        "result": result,
        "decisions": [{"nodeId": node_id, "action": {"kind": "keep"}}]
    });
    let accepted =
        flow_wasm::accept_pdf_reconstruction(serde_json::to_string(&accept_request).unwrap());
    assert_eq!(response(&accepted)["ok"], true);
    assert!(flow_wasm::verify_pdf_reconstruction_accept_response(
        accepted
    ));

    let missing_decision = json!({
        "protocolVersion": 1,
        "requestId": "reconstruction-missing-review",
        "result": response(&serialized)["result"]["result"],
        "decisions": []
    });
    let missing = response(&flow_wasm::accept_pdf_reconstruction(
        serde_json::to_string(&missing_decision).unwrap(),
    ));
    assert_eq!(missing["ok"], false);
    assert_eq!(
        missing["error"]["code"],
        "FLOW_PDF_RECONSTRUCTION_REVIEW_REQUIRED"
    );

    let mut stale = request.clone();
    stale["scene"]["resultHash"] = json!("stale-scene");
    let stale_value = response(&flow_wasm::reconstruct_pdf(
        serde_json::to_string(&stale).unwrap(),
    ));
    assert_eq!(stale_value["ok"], false);
    assert_eq!(
        stale_value["error"]["code"],
        "FLOW_PDF_RECONSTRUCTION_SCENE_INVALID"
    );

    let mut malformed = response(&serialized);
    malformed["result"]["result"]["resultHash"] = json!("tampered");
    assert!(!flow_wasm::verify_pdf_reconstruction_response(
        serde_json::to_string(&malformed).unwrap()
    ));
    let mut unknown = request;
    unknown["unexpected"] = json!(true);
    assert_eq!(
        response(&flow_wasm::reconstruct_pdf(
            serde_json::to_string(&unknown).unwrap()
        ))["error"]["code"],
        "FLOW_PDF_RECONSTRUCTION_REQUEST_DECODE"
    );
    let provider_scene = scene(&image_only_fixture());
    let invalid_provider = json!({
        "protocolVersion": 1,
        "requestId": "reconstruction-provider",
        "sourceHash": provider_scene.source_hash,
        "scene": provider_scene,
        "locale": "uk-UA",
        "ocrCandidates": [{
            "pageIndex": 0,
            "rect": {"x": 1, "y": 1, "width": 1, "height": 1},
            "text": "x",
            "confidenceBasisPoints": 10001,
            "provider": "fixture-ocr"
        }]
    });
    assert_eq!(
        response(&flow_wasm::reconstruct_pdf(
            serde_json::to_string(&invalid_provider).unwrap()
        ))["error"]["code"],
        "FLOW_PDF_RECONSTRUCTION_LIMIT"
    );

    serialized.clear();
    assert!(!flow_wasm::verify_pdf_reconstruction_response(serialized));
}

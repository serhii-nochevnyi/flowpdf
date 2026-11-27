use serde_json::{Value, json};
use std::io::Write;

fn fixture_pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << >> >>",
        "<< /Length 0 >>\nstream\n\nendstream",
    ];
    write_pdf(&objects)
}

fn write_pdf(objects: &[impl AsRef<str>]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len() + 1);
    offsets.push(0usize);
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        write!(
            &mut bytes,
            "{} 0 obj\n{}\nendobj\n",
            index + 1,
            object.as_ref()
        )
        .unwrap();
    }
    let xref_offset = bytes.len();
    write!(&mut bytes, "xref\n0 {}\n", objects.len() + 1).unwrap();
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        writeln!(&mut bytes, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        &mut bytes,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
        objects.len() + 1
    )
    .unwrap();
    bytes
}

fn response(serialized: &str) -> Value {
    serde_json::from_str(serialized).expect("reader response json")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn reader_boundary_returns_verified_scene_window() {
    let bytes = fixture_pdf();
    let request = json!({
        "protocolVersion": 1,
        "requestId": "reader-boundary-1",
        "bytesHex": hex(&bytes),
        "firstPage": 0,
        "pageCount": 1
    });
    let serialized = flow_wasm::read_pdf(serde_json::to_string(&request).unwrap());
    let value = response(&serialized);
    assert_eq!(value["ok"], true);
    assert_eq!(value["requestId"], "reader-boundary-1");
    assert_eq!(value["result"]["summary"]["pageCount"], 1);
    assert_eq!(
        value["result"]["scene"]["pages"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        value["result"]["summary"]["sourceHash"],
        value["result"]["scene"]["sourceHash"]
    );
    assert!(flow_wasm::verify_pdf_reader_response(serialized));
}

#[test]
fn reader_boundary_preserves_mapped_text_around_fractional_tj_spacing() {
    for (percent, matrix, adjustment, advance, positions) in [
        (
            "50",
            "1 0 0 1 10 50",
            "-125.5",
            384,
            [(640, 1920), (1104, 1920), (1424, 1920)],
        ),
        (
            "1",
            "1 0 0 1 10 50",
            "-125.5",
            8,
            [(640, 1920), (650, 1920), (657, 1920)],
        ),
        (
            "0.07",
            "1 0 0 1 10 50",
            "-562.5",
            1,
            [(640, 1920), (642, 1920), (643, 1920)],
        ),
        (
            "50",
            "0 1 -1 0 10 50",
            "-125.5",
            384,
            [(640, 1920), (640, 2384), (640, 2704)],
        ),
    ] {
        let paint =
            format!("BT /F1 20 Tf {percent} Tz {matrix} Tm [(A) {adjustment} (B) 100 (A)] TJ ET");
        let cmap = "begincmap\n2 beginbfchar\n<41> <0410>\n<42> <0411>\nendbfchar\nendcmap";
        let content = format!("<< /Length {} >>\nstream\n{paint}\nendstream", paint.len());
        let to_unicode = format!("<< /Length {} >>\nstream\n{cmap}\nendstream", cmap.len());
        let bytes = write_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            &content,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /Widths [600 600] /ToUnicode 6 0 R >>",
            &to_unicode,
        ]);
        let request = json!({
            "protocolVersion": 1,
            "requestId": "mapped-tj-spacing",
            "bytesHex": hex(&bytes),
            "firstPage": 0,
            "pageCount": 1
        });
        let serialized = flow_wasm::read_pdf(request.to_string());
        let value = response(&serialized);
        assert_eq!(value["ok"], true, "{value}");
        let scene: flow_core::PdfScene =
            serde_json::from_value(value["result"]["scene"].clone()).unwrap();
        let page = &scene.pages[0];
        assert!(page.partial);
        assert_eq!(page.elements.len(), 3);
        let mut shared_provenance = None;
        for (element, (text, (x, y))) in page
            .elements
            .iter()
            .zip(["А", "Б", "А"].into_iter().zip(positions))
        {
            let flow_core::PdfSceneElement::Text { value } = element else {
                panic!("expected mapped text");
            };
            assert_eq!(value.text, text);
            assert_eq!(value.mapping, flow_core::PdfTextMapping::ToUnicode);
            assert_eq!(value.glyphs[0].advance.raw(), advance);
            assert_eq!(value.glyphs[0].provenance, value.provenance);
            assert_eq!(
                *shared_provenance.get_or_insert(&value.provenance),
                &value.provenance
            );
            assert_eq!((value.rect.x.raw(), value.rect.y.raw()), (x, y));
            assert_eq!(value.provenance.operator.as_deref(), Some("TJ"));
            assert_eq!(value.provenance.object_ref.object_number, 4);
        }
        assert!(flow_wasm::verify_pdf_reader_response(serialized));
    }
}

#[test]
fn reader_boundary_budgets_nested_page_discovery_for_a_single_page_window() {
    let mut objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids 4 0 R /Count 96 /MediaBox [0 0 200 200] >>",
        "<< /Type /Pages /Parent 2 0 R /Kids 5 0 R /Count 96 >>",
        "[3 0 R]",
    ]
    .map(str::to_owned)
    .to_vec();
    let children = (6..102)
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!("[{children}]"));
    objects.extend((0..96).map(|_| "<< /Type /Page /Parent 3 0 R >>".to_owned()));
    let mut request = json!({
        "protocolVersion": 1,
        "requestId": "nested-page-window",
        "bytesHex": hex(&write_pdf(&objects)),
        "firstPage": 95,
        "pageCount": 1,
        "limits": flow_core::PdfReaderLimits {
            max_input_bytes: 1024 * 1024,
            max_scene_bytes: 64 * 1024,
            ..Default::default()
        }
    });
    let serialized = flow_wasm::read_pdf(request.to_string());
    let value = response(&serialized);
    assert_eq!(value["ok"], true, "{value}");
    let result = &value["result"];
    let summary = &result["summary"];
    let scene = &result["scene"];
    assert_eq!(
        summary["schemaVersion"],
        flow_core::PDF_READER_SCHEMA_VERSION
    );
    assert_eq!(summary["pdfVersion"], "1.7");
    assert_eq!(summary["sourceHash"], scene["sourceHash"]);
    assert_eq!(summary["pageCount"], 96);
    assert_eq!(scene["pageCount"], 96);
    assert_eq!(scene["firstPage"], 95);
    assert_eq!(scene["pages"].as_array().unwrap().len(), 1);
    assert_eq!(scene["pages"][0]["pageIndex"], 95);
    assert!(flow_wasm::verify_pdf_reader_response(serialized));

    request["limits"]["maxSceneBytes"] = json!(12 * 1024);
    let value = response(&flow_wasm::read_pdf(request.to_string()));
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "FLOW_PDF_READER_STREAM_LIMIT");
}

#[test]
fn reader_boundary_accepts_original_custom_limit_fields() {
    let mut request = json!({
        "protocolVersion": 1,
        "requestId": "reader-boundary-original-limits",
        "bytesHex": hex(&fixture_pdf()),
        "firstPage": 0,
        "pageCount": 1,
        "limits": {
            "maxInputBytes": 1048576,
            "maxObjects": 128,
            "maxPages": 4,
            "maxNesting": 8,
            "maxStringBytes": 4096,
            "maxArrayItems": 128,
            "maxDictionaryEntries": 128,
            "maxStreamBytes": 32768,
            "maxDecodedStreamBytes": 32768,
            "maxContentTokens": 1000,
            "maxSceneElements": 128,
            "maxImagePixels": 4096,
            "maxFormDepth": 4
        }
    });
    let serialized = flow_wasm::read_pdf(serde_json::to_string(&request).unwrap());
    assert_eq!(response(&serialized)["ok"], true);
    assert!(flow_wasm::verify_pdf_reader_response(serialized));

    request["limits"]["maxSceneBytes"] = json!(16 * 1024 * 1024 + 1);
    let response = response(&flow_wasm::read_pdf(
        serde_json::to_string(&request).unwrap(),
    ));
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "FLOW_PDF_READER_LIMITS_INVALID");
}

#[test]
fn reader_boundary_rejects_unknown_fields_and_invalid_hex() {
    let bytes = fixture_pdf();
    let unknown = json!({
        "protocolVersion": 1,
        "requestId": "reader-boundary-unknown",
        "bytesHex": hex(&bytes),
        "firstPage": 0,
        "pageCount": 1,
        "unexpected": true
    });
    let unknown_response = response(&flow_wasm::read_pdf(
        serde_json::to_string(&unknown).unwrap(),
    ));
    assert_eq!(unknown_response["ok"], false);
    assert_eq!(
        unknown_response["error"]["code"],
        "FLOW_PDF_READER_REQUEST_DECODE"
    );

    let invalid = json!({
        "protocolVersion": 1,
        "requestId": "reader-boundary-invalid",
        "bytesHex": "zz",
        "firstPage": 0,
        "pageCount": 1
    });
    let invalid_response = response(&flow_wasm::read_pdf(
        serde_json::to_string(&invalid).unwrap(),
    ));
    assert_eq!(invalid_response["ok"], false);
    assert_eq!(
        invalid_response["error"]["code"],
        "FLOW_PDF_READER_BYTES_INVALID"
    );
}

use serde_json::{Value, json};
use std::io::Write;

fn fixture_pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << >> >>",
        "<< /Length 0 >>\nstream\n\nendstream",
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len() + 1);
    offsets.push(0usize);
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        write!(&mut bytes, "{} 0 obj\n{}\nendobj\n", index + 1, object).unwrap();
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

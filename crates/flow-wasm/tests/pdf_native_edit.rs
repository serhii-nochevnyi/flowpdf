use flow_wasm::{
    native_pdf_close, native_pdf_command, native_pdf_open, native_pdf_snapshot,
    verify_native_pdf_response,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn pdf(width: u32) -> String {
    let mut pdf = "%PDF-1.7\n".to_owned();
    let mut offsets = vec![];
    let page = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} 792] >>");
    for (index, value) in [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        &page,
    ]
    .into_iter()
    .enumerate()
    {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{value}\nendobj\n", index + 1));
    }
    let xref = pdf.len();
    pdf.push_str("xref\n0 4\n0000000000 65535 f \n");
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    pdf
}

fn call(export: fn(String) -> String, request: Value) -> Value {
    let response = export(request.to_string());
    assert!(verify_native_pdf_response(response.clone()));
    serde_json::from_str(&response).unwrap()
}

fn open_request(bytes: &[u8]) -> Value {
    let bytes_hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    json!({
        "protocolVersion": 1, "operation": "open", "requestId": 1,
        "epoch": vec![1; 16], "originStoreHash": format!("{:x}", Sha256::digest(bytes)),
        "bytesHex": bytes_hex,
    })
}

#[test]
fn open_verifies_source_bytes_and_returns_verifiable_reader_refusals() {
    let original = pdf(612);
    let replacement = pdf(600);
    let replacement_request = open_request(replacement.as_bytes());
    let mut stale = replacement_request.clone();
    stale["originStoreHash"] = open_request(original.as_bytes())["originStoreHash"].clone();

    for (request, code) in [
        (stale, "FLOW_PDF_NATIVE_BINDING_MISMATCH"),
        (open_request(b"ordinary text"), "FLOW_PDF_NATIVE_IMPORT_REFUSED"),
    ] {
        let response = call(native_pdf_open, request.clone());
        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], code);
        assert_eq!(response["requestId"], request["requestId"]);
        assert_eq!(response["originStoreHash"], request["originStoreHash"]);
        assert!(response["binding"].is_null());
        assert!(response["snapshot"].is_null());
    }

    let response = call(native_pdf_open, replacement_request.clone());
    assert_eq!(response["status"], "opened", "{response}");
    assert_eq!(response["originStoreHash"], replacement_request["originStoreHash"]);
    assert_eq!(
        response["binding"]["originHash"],
        json!(blake3::hash(replacement.as_bytes()).as_bytes())
    );
    assert_eq!(call(native_pdf_open, open_request(original.as_bytes()))["status"], "opened");
}

fn open() -> (Value, Value) {
    let request = open_request(pdf(612).as_bytes());
    let response = call(native_pdf_open, request.clone());
    assert_eq!(response["status"], "opened", "{response}");
    (request, response["snapshot"].clone())
}

fn bound(operation: &str, request_id: u64, snapshot: &Value) -> Value {
    json!({
        "protocolVersion": 1, "operation": operation, "requestId": request_id,
        "binding": snapshot["binding"], "originStoreHash": snapshot["originStoreHash"],
    })
}

#[test]
fn serialized_note_lifecycle_and_fail_closed_wire() {
    let (open_request, original) = open();
    assert_eq!(original["outputHex"], open_request["bytesHex"]);
    let mut command = bound("command", 2, &original);
    command["outputHash"] = original["outputHash"].clone();
    command["command"] = json!({"kind":"add","page":0,"rect":[1,2,3,4],"contents":"Ї"});
    for (field, value) in [
        ("unknown", json!(true)),
        ("outputHash", json!(vec![0; 32])),
        ("originStoreHash", json!("00".repeat(32))),
        ("requestId", json!(0)),
        (
            "command",
            json!({"kind":"add","page":99,"rect":[1,2,3,4],"contents":"secret"}),
        ),
        ("command", json!({"kind":"remove","note":5,"extra":true})),
        ("command", json!({"kind":"flatten"})),
    ] {
        let mut invalid = command.clone();
        invalid[field] = value;
        let response = call(native_pdf_command, invalid);
        assert_eq!(response["status"], "failed");
        assert!(response["snapshot"].is_null());
        assert!(!response.to_string().contains("secret"));
    }
    for field in ["sessionId", "originHash", "revision", "epoch"] {
        let mut invalid = command.clone();
        invalid["binding"][field] = match field {
            "originHash" => json!(vec![0; 32]),
            "epoch" => json!(vec![0; 16]),
            _ => json!(99),
        };
        assert_eq!(call(native_pdf_command, invalid)["status"], "failed");
    }
    let accepted = call(native_pdf_command, command.clone());
    assert_eq!(accepted["status"], "applied", "{accepted}");
    let current = &accepted["snapshot"];
    assert_eq!(current["binding"]["revision"], 1);
    assert_eq!(
        current["binding"]["originHash"],
        original["binding"]["originHash"]
    );
    assert_ne!(current["outputHash"], original["outputHash"]);
    assert_eq!(accepted, call(native_pdf_command, command.clone()));
    command["command"]["contents"] = json!("other");
    assert_eq!(
        call(native_pdf_command, command)["code"],
        "FLOW_PDF_NATIVE_REQUEST_CONFLICT"
    );
    let reconciled = call(native_pdf_snapshot, bound("snapshot", 2, &original));
    assert_eq!(reconciled["status"], "accepted");
    assert_eq!(reconciled["snapshot"], *current);

    for field in ["resultHash", "requestId", "operation"] {
        let mut forged = accepted.clone();
        forged[field] = json!("forged");
        assert!(!verify_native_pdf_response(forged.to_string()));
    }
    let mut forged = accepted.clone();
    forged["snapshot"]["scene"]["pages"][0]["annotations"][0]["contents"] = json!("forged");
    assert!(!verify_native_pdf_response(forged.to_string()));

    let mut forged = accepted.clone();
    forged["snapshot"]["outputHex"] = json!("00");
    assert!(!verify_native_pdf_response(forged.to_string()));

    let note = &current["scene"]["pages"][0]["annotations"][0]["id"];
    let mut latest = current.clone();
    for (request_id, command) in [
        (
            3,
            json!({"kind":"move","note":note,"page":0,"rect":[10,20,30,40]}),
        ),
        (
            4,
            json!({"kind":"edit","note":note,"contents":"Новий текст"}),
        ),
        (5, json!({"kind":"remove","note":note})),
    ] {
        let mut request = bound("command", request_id, &latest);
        request["outputHash"] = latest["outputHash"].clone();
        request["command"] = command;
        let reply = call(native_pdf_command, request);
        assert_eq!(reply["status"], "applied", "{reply}");
        latest = reply["snapshot"].clone();
        assert_eq!(
            latest["binding"]["originHash"],
            original["binding"]["originHash"]
        );
        assert_eq!(latest["originStoreHash"], original["originStoreHash"]);
        let note = &latest["scene"]["pages"][0]["annotations"][0];
        match request_id {
            3 => assert_eq!(note["rect"], json!([10, 20, 30, 40])),
            4 => assert_eq!(note["contents"], "Новий текст"),
            _ => assert!(note.is_null()),
        }
    }

    let close = bound("close", 6, &latest);
    let stale_close = call(native_pdf_close, bound("close", 5, &latest));
    assert_eq!(stale_close["code"], "FLOW_PDF_NATIVE_REQUEST_INVALID");
    assert!(stale_close["snapshot"].is_null());
    assert_eq!(call(native_pdf_close, close.clone())["status"], "closed");
    assert_eq!(call(native_pdf_close, close)["status"], "closed");
    assert_eq!(
        call(native_pdf_close, bound("close", 7, &latest))["status"],
        "failed"
    );
    assert_eq!(
        call(native_pdf_close, bound("close", 5, &latest))["status"],
        "failed"
    );
    assert_eq!(
        call(native_pdf_snapshot, bound("snapshot", 7, &latest))["status"],
        "failed"
    );
}

#[test]
fn a_skipped_request_is_not_reported_as_evicted_inside_the_history_window() {
    let (_, initial) = open();
    let mut later = bound("command", 3, &initial);
    later["outputHash"] = initial["outputHash"].clone();
    later["command"] = json!({"kind":"add","page":0,"rect":[1,2,3,4],"contents":"later"});
    let applied = call(native_pdf_command, later);
    assert_eq!(applied["status"], "applied");
    let reply = call(native_pdf_snapshot, bound("snapshot", 2, &initial));
    assert_eq!(reply["status"], "failed");
    assert!(reply["snapshot"].is_null());
}

#[test]
fn expired_receipt_reconciles_to_current_snapshot_without_replay() {
    let (_, initial) = open();
    let mut current = initial.clone();
    let mut first = Value::Null;
    for request_id in 2..=130 {
        let mut command = bound("command", request_id, &current);
        command["outputHash"] = current["outputHash"].clone();
        command["command"] = if request_id == 2 {
            json!({"kind":"add","page":0,"rect":[1,2,3,4],"contents":"first"})
        } else {
            let note = &current["scene"]["pages"][0]["annotations"][0]["id"];
            json!({"kind":"edit","note":note,"contents":request_id.to_string()})
        };
        if request_id == 2 {
            first = command.clone();
        }
        let reply = call(native_pdf_command, command);
        assert_eq!(reply["status"], "applied", "{reply}");
        current = reply["snapshot"].clone();
        if request_id == 3 {
            assert_eq!(
                call(native_pdf_snapshot, bound("snapshot", 2, &initial))["status"],
                "accepted"
            );
            let mut conflicting = first.clone();
            conflicting["command"]["contents"] = json!("conflict");
            assert_eq!(
                call(native_pdf_command, conflicting)["code"],
                "FLOW_PDF_NATIVE_REQUEST_CONFLICT"
            );
        }
    }
    assert_eq!(
        call(native_pdf_command, first)["code"],
        "FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED"
    );
    let reconciliation = call(native_pdf_snapshot, bound("snapshot", 2, &initial));
    assert_eq!(reconciliation["status"], "evicted");
    assert_eq!(reconciliation["snapshot"], current);
}

#[test]
fn enforces_wire_caps_before_decoding() {
    let (request, _) = open();
    let mut oversized = request;
    oversized["bytesHex"] = json!("0".repeat(16 * 1024 * 1024 + 2));
    assert_eq!(call(native_pdf_open, oversized)["status"], "failed");
    let unreadable = " ".repeat(64 * 1024 * 1024 + 1);
    assert!(!verify_native_pdf_response(unreadable));
}

#[test]
fn registry_reserves_two_sessions_and_releases_capacity_on_close() {
    let (request, first) = open();
    let (_, second) = open();
    assert_ne!(
        first["binding"]["sessionId"],
        second["binding"]["sessionId"]
    );
    let refused = call(native_pdf_open, request.clone());
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_BUDGET_EXHAUSTED");
    assert!(refused["snapshot"].is_null());
    assert_eq!(
        call(native_pdf_close, bound("close", 2, &first))["status"],
        "closed"
    );
    assert_eq!(call(native_pdf_open, request)["status"], "opened");
    assert_eq!(
        call(native_pdf_snapshot, bound("snapshot", 3, &second))["status"],
        "unseen"
    );
}

fn embedded_text_pdf() -> Vec<u8> {
    fn stream(bytes: &[u8], extra: &str) -> Vec<u8> {
        let mut data = format!("<< /Length {} {extra} >>\nstream\n", bytes.len()).into_bytes();
        data.extend_from_slice(bytes);
        data.extend_from_slice(b"\nendstream");
        data
    }
    let font = include_bytes!("../../flow-core/data/NotoSans-Regular.ttf");
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapName /Fixture def /CMapType 2 def\n1 begincodespacerange <00> <FF> endcodespacerange\n1 beginbfchar <41> <0041> endbfchar\nendcmap CMapName currentdict /CMap defineresource pop end end";
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_vec(),
        stream(b"BT /F1 12 Tf 20 100 Td (AAA) Tj ET", ""),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /NotoSans-Regular /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 65 /Widths [639] /FontDescriptor 6 0 R /ToUnicode 7 0 R >>".to_vec(),
        b"<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 /FontFile2 8 0 R >>".to_vec(),
        stream(cmap, ""),
        stream(font, &format!("/Length1 {}", font.len())),
    ];
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 9\n0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 9 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

#[test]
fn serialized_text_replacement_checks_capability_revision_and_output_identity() {
    let bytes = embedded_text_pdf();
    let request = open_request(&bytes);
    let original = call(native_pdf_open, request.clone())["snapshot"].clone();
    let target = &original["scene"]["textPages"][0]["islands"][0];
    assert_eq!(target["text"], "AAA");
    assert_eq!(target["profile"], "trueTypeWinAnsi");
    let mut command = bound("command", 2, &original);
    command["outputHash"] = original["outputHash"].clone();
    command["command"] = json!({"kind":"replaceText","island":target["id"],"text":"AA"});
    for (text, code) in [
        ("secret", "FLOW_PDF_NATIVE_TEXT_MAPPING"),
        ("AAAA", "FLOW_PDF_NATIVE_TEXT_OVERFLOW"),
    ] {
        let mut bad = command.clone();
        bad["command"]["text"] = json!(text);
        let refusal = call(native_pdf_command, bad);
        assert_eq!(refusal["code"], code);
        assert!(refusal["snapshot"].is_null());
        assert!(!refusal.to_string().contains("secret"));
    }
    let mut forged = command.clone();
    forged["command"]["island"] = json!(vec![0; 32]);
    assert_eq!(
        call(native_pdf_command, forged)["code"],
        "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE"
    );
    let unchanged = call(native_pdf_snapshot, bound("snapshot", 2, &original));
    assert_eq!(unchanged["snapshot"], original);

    let applied = call(native_pdf_command, command.clone());
    assert_eq!(applied["status"], "applied");
    let current = &applied["snapshot"];
    let current_target = &current["scene"]["textPages"][0]["islands"][0];
    assert_eq!(current_target["text"], "AA");
    assert_ne!(current_target["id"], target["id"]);
    assert_eq!(current["binding"]["revision"], 1);
    assert_eq!(
        current["binding"]["originHash"],
        original["binding"]["originHash"]
    );
    assert_eq!(current["originStoreHash"], original["originStoreHash"]);
    assert_ne!(current["outputHash"], original["outputHash"]);
    assert_eq!(
        call(native_pdf_command, command.clone())["snapshot"],
        *current
    );
    command["requestId"] = json!(3);
    assert_eq!(
        call(native_pdf_command, command.clone())["code"],
        "FLOW_PDF_NATIVE_REVISION_MISMATCH"
    );
    command["binding"] = current["binding"].clone();
    assert_eq!(
        call(native_pdf_command, command.clone())["code"],
        "FLOW_PDF_NATIVE_BINDING_MISMATCH"
    );
    command["outputHash"] = current["outputHash"].clone();
    assert_eq!(
        call(native_pdf_command, command)["code"],
        "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE"
    );
}

#[test]
fn wire_verifier_rejects_rehashed_capability_that_disagrees_with_output() {
    use flow_core::pdf::{PdfObjectRef, native::NativeTextPage};

    fn result_hash(json: &str, old_hash: &str) -> String {
        let mut hash = blake3::Hasher::new();
        hash.update(b"flowpdf:native-wire:v1");
        hash.update(json.replace(old_hash, "").as_bytes());
        hash.finalize().to_hex().to_string()
    }

    let response = native_pdf_open(open_request(&embedded_text_pdf()).to_string());
    let value: Value = serde_json::from_str(&response).unwrap();
    let old_result = value["resultHash"].as_str().unwrap();
    assert_eq!(result_hash(&response, old_result), old_result);
    assert!(verify_native_pdf_response(response.clone()));

    let scene = &value["snapshot"]["scene"];
    let page = &scene["pages"][0];
    let object: PdfObjectRef = serde_json::from_value(page["object"].clone()).unwrap();
    let output_hash: [u8; 32] = serde_json::from_value(scene["outputHash"].clone()).unwrap();
    let old_scene = serde_json::to_string(&scene["sceneHash"]).unwrap();
    let mut text_pages: Vec<NativeTextPage> =
        serde_json::from_value(scene["textPages"].clone()).unwrap();
    let derive_scene_hash = |text_pages: &[NativeTextPage]| {
        let mut hash = blake3::Hasher::new();
        hash.update(b"flowpdf:accepted-note-scene:v2");
        hash.update(&output_hash);
        hash.update(&serde_json::to_vec(&(object, &page["geometry"], true)).unwrap());
        hash.update(b"flowpdf:native-text-pages:v1");
        hash.update(&serde_json::to_vec(text_pages).unwrap());
        serde_json::to_string(hash.finalize().as_bytes()).unwrap()
    };
    assert_eq!(derive_scene_hash(&text_pages), old_scene);
    text_pages[0].islands[0].text = "forged replacement capability".into();
    let tampered = response
        .replace(
            r#""text":"AAA""#,
            r#""text":"forged replacement capability""#,
        )
        .replace(&old_scene, &derive_scene_hash(&text_pages));
    let forged = tampered.replace(old_result, &result_hash(&tampered, old_result));
    assert_ne!(forged, response);
    assert!(!verify_native_pdf_response(forged));
}

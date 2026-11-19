use flow_wasm::{
    native_pdf_close, native_pdf_command, native_pdf_open, native_pdf_preview,
    native_pdf_redaction, native_pdf_snapshot, verify_native_pdf_response,
    verify_native_pdf_worker_response,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn pdf(width: u32) -> String {
    fixture_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        &format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} 792] >>"),
    ])
}

fn fixture_pdf(objects: &[&str]) -> String {
    let mut pdf = "%PDF-1.7\n".to_owned();
    let mut offsets = vec![];
    for (index, value) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{value}\nendobj\n", index + 1));
    }
    let xref = pdf.len();
    let size = objects.len() + 1;
    pdf.push_str(&format!("xref\n0 {size}\n0000000000 65535 f \n"));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    pdf
}

fn call(export: fn(String) -> String, request: Value) -> Value {
    let response = export(request.to_string());
    assert!(verify_native_pdf_worker_response(response.clone()));
    serde_json::from_str(&response).unwrap()
}

fn rehash_with_replacement(response: &str, old: &str, new: &str) -> String {
    let value: Value = serde_json::from_str(response).unwrap();
    let original_hash = value["resultHash"].as_str().unwrap();
    assert!(response.contains(old));
    let changed = response.replace(old, new).replace(original_hash, "");
    let mut digest = blake3::Hasher::new();
    digest.update(b"flowpdf:native-wire:v1");
    digest.update(changed.as_bytes());
    changed.replace(
        r#""resultHash":"""#,
        &format!(r#""resultHash":"{}""#, digest.finalize().to_hex()),
    )
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
        (
            open_request(b"ordinary text"),
            "FLOW_PDF_NATIVE_IMPORT_REFUSED",
        ),
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
    assert_eq!(
        response["originStoreHash"],
        replacement_request["originStoreHash"]
    );
    assert_eq!(
        response["binding"]["originHash"],
        json!(blake3::hash(replacement.as_bytes()).as_bytes())
    );
    assert_eq!(
        call(native_pdf_open, open_request(original.as_bytes()))["status"],
        "opened"
    );
}

#[test]
fn verifies_original_storage_digest_and_stable_note_identity() {
    let source = fixture_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Annots [4 0 R 5 0 R] >>",
        "<< /Type /Annot /Subtype /Text /P 3 0 R /Contents (First) /Rect [1 2 3 4] >>",
        "<< /Type /Annot /Subtype /Text /P 3 0 R /Contents (Second) /Rect [5 6 7 8] >>",
    ]);
    let request = open_request(source.as_bytes());
    let opened = native_pdf_open(request.to_string());
    assert!(verify_native_pdf_worker_response(opened.clone()));
    let origin = request["originStoreHash"].as_str().unwrap();
    assert!(!verify_native_pdf_response(rehash_with_replacement(
        &opened,
        origin,
        &"00".repeat(32),
    )));
    let response: Value = serde_json::from_str(&opened).unwrap();
    let base = &response["snapshot"];
    let original = base["scene"]["pages"][0]["annotations"].as_array().unwrap();
    let ids = [0, 1].map(|index| {
        assert_eq!(original[index]["kind"], "plainNote");
        original[index]["id"].as_u64().unwrap()
    });
    assert_ne!(ids[0], ids[1]);

    let mut current = base.clone();
    let mut previous_reply = opened;
    for (request_id, target, expected) in [
        (2, 1, ["First", "Revised second"]),
        (3, 0, ["Revised first", "Revised second"]),
    ] {
        let mut edit = bound("command", request_id, &current);
        edit["outputHash"] = current["outputHash"].clone();
        edit["command"] = json!({"kind":"edit","note":ids[target],"contents":expected[target]});
        let applied = native_pdf_command(edit.to_string());
        assert!(verify_native_pdf_worker_response(applied.clone()));
        assert!(!verify_native_pdf_worker_response(previous_reply));
        let response: Value = serde_json::from_str(&applied).unwrap();
        assert_eq!(response["status"], "applied");
        let reopened = &response["snapshot"]["scene"]["pages"][0]["annotations"];
        assert_eq!(reopened.as_array().unwrap().len(), 2);
        for index in 0..2 {
            assert_eq!(reopened[index]["id"], ids[index]);
            assert_eq!(reopened[index]["contents"], expected[index]);
            assert_eq!(reopened[index]["rect"], original[index]["rect"]);
        }

        if request_id == 2 {
            let note_prefix = |id, index: usize| {
                let object = &reopened[index]["object"];
                format!(
                    r#""id":{id},"object":{{"objectNumber":{},"generation":{}}}"#,
                    object["objectNumber"], object["generation"],
                )
            };
            let swapped =
                rehash_with_replacement(&applied, &note_prefix(ids[0], 0), &note_prefix(ids[1], 0));
            let swapped =
                rehash_with_replacement(&swapped, &note_prefix(ids[1], 1), &note_prefix(ids[0], 1));
            assert!(!verify_native_pdf_worker_response(swapped));
            let reconciled = call(native_pdf_snapshot, bound("snapshot", request_id, &current));
            assert_eq!(reconciled["status"], "accepted");
            assert_eq!(reconciled["snapshot"], response["snapshot"]);
        }
        current = response["snapshot"].clone();
        previous_reply = applied;
    }

    let mut remove = bound("command", 4, &current);
    remove["outputHash"] = current["outputHash"].clone();
    remove["command"] = json!({"kind":"remove","note":ids[0]});
    let applied = native_pdf_command(remove.to_string());
    assert!(verify_native_pdf_worker_response(applied.clone()));
    let response: Value = serde_json::from_str(&applied).unwrap();
    assert_eq!(response["status"], "applied");
    let survivors = response["snapshot"]["scene"]["pages"][0]["annotations"]
        .as_array()
        .unwrap();
    assert_eq!(survivors.len(), 1);
    assert_ne!(
        survivors[0]["object"],
        current["scene"]["pages"][0]["annotations"][1]["object"]
    );
    assert_eq!(survivors[0]["id"], ids[1]);
    assert_eq!(survivors[0]["contents"], "Revised second");

    std::thread::spawn(move || {
        assert!(verify_native_pdf_response(applied.clone()));
        assert!(!verify_native_pdf_worker_response(applied));
    })
    .join()
    .unwrap();
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

fn redaction_request(request_id: u64, snapshot: &Value, action: Value) -> Value {
    let mut request = bound("redaction", request_id, snapshot);
    request["acceptedHash"] = snapshot["outputHash"].clone();
    request["action"] = action;
    request
}

fn status_only(reply: &Value) {
    assert!(reply["snapshot"].is_null());
    let json = reply.to_string();
    for forbidden in [
        "outputHex",
        "bytesHex",
        "scene",
        "annotations",
        "contents",
        "rasterPayload",
        "evidence",
    ] {
        assert!(!json.contains(forbidden), "{json}");
    }
}

fn redaction_pdf(size: u32) -> String {
    let paint = "10 10 10 10 re f 40 40 10 10 re f";
    fixture_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        &format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {size} {size}] /Contents 4 0 R >>"),
        &format!("<< /Length {} >>\nstream\n{paint}\nendstream", paint.len()),
    ])
}

fn preview_request(request_id: u64, snapshot: &Value, page_id: Value) -> Value {
    let mut request = bound("preview", request_id, snapshot);
    request["pageId"] = page_id;
    request["outputHash"] = snapshot["outputHash"].clone();
    request
}

#[test]
fn page_preview_binds_the_rotated_crop_and_current_accepted_revision() {
    let artwork = "1 0 0 rg 3 5 3 3 re f\n\
                   0 1 0 RG 0.04 w 3.5 9.06 m 5.5 9.06 l S\n\
                   0.996 G 0.04 w 3.5 10.56 m 5.5 10.56 l S\n\
                   0 0 1 rg 6.03 9.03 0.04 0.04 re f";
    let clipped = "q 0 0 12 12 re W n 0 0 1 rg 2 2 4 4 re f Q";
    let source = fixture_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /CropBox [2 4 14 12] /Rotate 90 /Contents 4 0 R >>",
        &format!(
            "<< /Length {} >>\nstream\n{artwork}\nendstream",
            artwork.len()
        ),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 12 12] /Contents 6 0 R >>",
        &format!(
            "<< /Length {} >>\nstream\n{clipped}\nendstream",
            clipped.len()
        ),
    ]);
    let opened = call(native_pdf_open, open_request(source.as_bytes()));
    let base = &opened["snapshot"];
    let first = &base["pageState"]["order"][0];
    let second = &base["pageState"]["order"][1];
    let original_request = preview_request(2, base, first.clone());
    let preview = call(native_pdf_preview, original_request.clone());
    assert_eq!(preview["status"], "preview");
    assert_eq!(preview["binding"], base["binding"]);
    assert!(preview["snapshot"].is_null());
    assert!(preview.get("redaction").is_none());
    let raster = &preview["preview"];
    assert_eq!(raster["outputHash"], base["outputHash"]);
    assert_eq!(raster["pageId"], *first);
    assert_eq!(raster["cropBox"], json!([2000, 4000, 14000, 12000]));
    assert_eq!(raster["rotation"], 90);
    assert_eq!(raster["width"], 16);
    assert_eq!(raster["height"], 24);
    let rgb = raster["rgbHex"].as_str().unwrap();
    let pixel = |x: usize, y: usize| &rgb[(y * 16 + x) * 6..][..6];
    assert_eq!(pixel(4, 4), "ff0000");
    assert_eq!(pixel(10, 4), "80ff80");
    assert_eq!(pixel(10, 8), "8080ff");
    assert_eq!(pixel(12, 4), "ffffff");
    assert_eq!(pixel(13, 4), "fefefe");
    assert_eq!(pixel(4, 18), "ffffff");

    for page in [second.clone(), json!(1)] {
        let unavailable = call(native_pdf_preview, preview_request(3, base, page));
        assert_eq!(unavailable["code"], "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE");
        assert!(unavailable["snapshot"].is_null());
        assert!(unavailable.get("preview").is_none());
    }
    let mut edit = bound("command", 4, base);
    edit["outputHash"] = base["outputHash"].clone();
    edit["command"] = json!({"kind":"add","page":0,"rect":[8,6,12,10],"contents":"Review note"});
    let applied = call(native_pdf_command, edit);
    assert_eq!(applied["status"], "applied");
    let current = &applied["snapshot"];
    assert_eq!(
        call(native_pdf_preview, original_request)["code"],
        "FLOW_PDF_NATIVE_REVISION_MISMATCH"
    );
    let mut wrong_hash = preview_request(5, current, first.clone());
    wrong_hash["outputHash"] = base["outputHash"].clone();
    assert_eq!(
        call(native_pdf_preview, wrong_hash)["code"],
        "FLOW_PDF_NATIVE_REVISION_MISMATCH"
    );
    let updated = call(
        native_pdf_preview,
        preview_request(6, current, first.clone()),
    );
    assert_eq!(updated["status"], "preview");
    assert_eq!(updated["preview"]["outputHash"], current["outputHash"]);
    assert_ne!(updated["preview"]["rgbHex"], raster["rgbHex"]);
    assert_eq!(
        call(native_pdf_close, bound("close", 7, current))["status"],
        "closed"
    );
}

#[test]
fn page_preview_declines_ambiguous_partial_paint_overlaps() {
    let fine = "1 0 0 RG 0.04 w 1 2.08 m 4 2.08 l S";
    let partial = "0 0 1 rg 2.1 2.04 0.3 0.35 re f";
    let opaque = "0 0 1 rg 2 2 1 1 re f";
    for (artwork, expected) in [
        (format!("{fine}\n{partial}"), None),
        (format!("{partial}\n{fine}"), None),
        (format!("{fine}\n{opaque}"), Some("0000ff")),
        (format!("{fine}\n{partial}\n{opaque}"), Some("0000ff")),
        (format!("{fine}\n{fine}"), Some("ff8080")),
    ] {
        let source = fixture_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 8 8] /Contents 4 0 R >>",
            &format!(
                "<< /Length {} >>\nstream\n{artwork}\nendstream",
                artwork.len()
            ),
        ]);
        let opened = call(native_pdf_open, open_request(source.as_bytes()));
        assert_eq!(opened["status"], "opened", "{opened}");
        let base = &opened["snapshot"];
        let page = base["pageState"]["order"][0].clone();
        let reply = call(native_pdf_preview, preview_request(2, base, page));
        if let Some(expected) = expected {
            assert_eq!(reply["status"], "preview", "{reply}");
            let rgb = reply["preview"]["rgbHex"].as_str().unwrap();
            let pixel = |x: usize, y: usize| &rgb[(y * 16 + x) * 6..][..6];
            assert_eq!(pixel(2, 11), "ff8080");
            assert_eq!(pixel(4, 11), expected);
        } else {
            assert_eq!(
                reply["code"], "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE",
                "{reply}"
            );
            assert!(reply["snapshot"].is_null());
            assert!(reply.get("preview").is_none());
        }
        assert_eq!(
            call(native_pdf_close, bound("close", 3, base))["status"],
            "closed"
        );
    }
}

#[test]
fn page_preview_refuses_partial_paint_over_mixed_image_samples() {
    for (stroke, available) in [
        ("", true),
        ("1 0 0 RG 0.04 w 2.05 2.08 m 2.2 2.08 l S", false),
    ] {
        let artwork = format!("q 0.5 0 0 0.5 2 2 cm /Photo Do Q\n{stroke}");
        let source = fixture_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 8 8] /Contents 4 0 R /Resources << /XObject << /Photo 5 0 R >> >> >>",
            &format!(
                "<< /Length {} >>\nstream\n{artwork}\nendstream",
                artwork.len()
            ),
            "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length 6 >>\nstream\nAzAzAz\nendstream",
        ]);
        let opened = call(native_pdf_open, open_request(source.as_bytes()));
        assert_eq!(opened["status"], "opened", "{opened}");
        let base = &opened["snapshot"];
        let reply = call(
            native_pdf_preview,
            preview_request(2, base, base["pageState"]["order"][0].clone()),
        );
        if available {
            assert_eq!(reply["status"], "preview", "{reply}");
        } else {
            assert_eq!(
                reply["code"], "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE",
                "{reply}"
            );
        }
        assert_eq!(
            call(native_pdf_close, bound("close", 3, base))["status"],
            "closed"
        );
    }
}

#[test]
fn redaction_admission_follows_the_accepted_pdf() {
    let paint = "q 0 0 80 80 re W n 10 10 10 10 re f Q";
    let clipped = fixture_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
        &format!("<< /Length {} >>\nstream\n{paint}\nendstream", paint.len()),
    ]);
    let opened = call(native_pdf_open, open_request(clipped.as_bytes()));
    assert_eq!(opened["status"], "opened");
    let clipped = &opened["snapshot"];
    assert_eq!(clipped["redactionAdmission"], "unavailable");
    let rehashed = native_pdf_snapshot(bound("snapshot", 2, clipped).to_string());
    assert!(verify_native_pdf_response(rehashed.clone()));
    let reply: Value = serde_json::from_str(&rehashed).unwrap();
    let previous_hash = reply["resultHash"].as_str().unwrap();
    let tampered = rehashed.replace(
        r#""redactionAdmission":"unavailable""#,
        r#""redactionAdmission":"supported""#,
    );
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"flowpdf:native-wire:v1");
    hasher.update(tampered.replace(previous_hash, "").as_bytes());
    assert!(!verify_native_pdf_response(
        tampered.replace(previous_hash, hasher.finalize().to_hex().as_str()),
    ));

    let mark = |snapshot: &Value| {
        json!({"kind":"addMark","pageId":snapshot["pageState"]["order"][0],
            "rect":[12000,12000,13000,13000]})
    };
    let refused = call(
        native_pdf_redaction,
        redaction_request(2, clipped, mark(clipped)),
    );
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_UNSUPPORTED");
    status_only(&refused);
    assert_eq!(
        call(native_pdf_close, bound("close", 3, clipped))["status"],
        "closed"
    );

    let source = redaction_pdf(100);
    let opened = call(native_pdf_open, open_request(source.as_bytes()));
    let before = &opened["snapshot"];
    assert_eq!(before["redactionAdmission"], "supported");
    let mut edit = bound("command", 2, before);
    edit["outputHash"] = before["outputHash"].clone();
    edit["command"] = json!({"kind":"addCheckboxField","page":0,"name":"approved",
        "rect":[60,60,80,80],"checked":false});
    let response = call(native_pdf_command, edit);
    assert_eq!(response["status"], "applied", "{response}");
    let after = &response["snapshot"];
    assert_eq!(after["binding"]["revision"], 1);
    assert_eq!(after["fields"][0]["checked"], false);
    assert_eq!(after["redactionAdmission"], "unavailable");
    let refused = call(
        native_pdf_redaction,
        redaction_request(3, after, mark(after)),
    );
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_UNSUPPORTED");
    status_only(&refused);
    assert_eq!(
        call(native_pdf_close, bound("close", 4, after))["status"],
        "closed"
    );
}

#[test]
fn redaction_wire_releases_only_the_verified_exact_output() {
    let source = redaction_pdf(100);
    let opened = call(native_pdf_open, open_request(source.as_bytes()));
    let base = &opened["snapshot"];
    let page = base["pageState"]["order"][0].clone();
    let preview = preview_request(15, base, page.clone());
    let accepted_raster = call(native_pdf_preview, preview.clone());
    assert_eq!(accepted_raster["status"], "preview");
    let request = |id, action| redaction_request(id, base, action);
    let add = |id| {
        request(
            id,
            json!({"kind":"addMark","pageId":page,"rect":[12000,12000,13000,13000]}),
        )
    };
    for (command, count) in [
        (add(2), 1),
        (
            request(
                3,
                json!({"kind":"removeMark","pageId":page,"rect":[12000,12000,13000,13000]}),
            ),
            0,
        ),
        (add(4), 1),
        (request(5, json!({"kind":"clearMarks"})), 0),
        (add(6), 1),
        (add(6), 1),
    ] {
        let reply = call(native_pdf_redaction, command);
        assert_eq!(reply["redaction"]["markCount"], count, "{reply}");
        status_only(&reply);
    }
    assert_eq!(call(native_pdf_redaction, add(4))["status"], "failed");
    let stale_close = call(native_pdf_close, bound("close", 5, base));
    assert_eq!(stale_close["code"], "FLOW_PDF_NATIVE_REQUEST_INVALID");
    let lookup = |candidate| request(7, json!({"kind":"status","candidate":candidate}));
    let missing = call(native_pdf_redaction, lookup(Value::Null));
    assert!(missing["redaction"]["candidate"].is_null());
    status_only(&missing);
    let forged_stage = request(7, json!({"kind":"stage","passed":true}));
    let refused = call(native_pdf_redaction, forged_stage);
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_REQUEST_DECODE");
    status_only(&refused);

    let pending = call(native_pdf_redaction, request(7, json!({"kind":"stage"})));
    assert_eq!(
        pending["redaction"]["verification"]["status"], "pending",
        "{pending}"
    );
    assert_eq!(pending["binding"]["revision"], 0);
    let busy_close = call(native_pdf_close, bound("close", 8, base));
    assert_eq!(busy_close["status"], "failed");
    status_only(&pending);
    let candidate = pending["redaction"]["candidate"].clone();
    assert_eq!(call(native_pdf_preview, preview), accepted_raster);
    assert_eq!(candidate["affectedPages"], json!([page]));
    let publish = request(7, json!({"kind":"publish","candidate":candidate}));
    let refused = call(native_pdf_redaction, publish.clone());
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_REDACTION_UNVERIFIED");
    status_only(&refused);
    let mut forged = publish.clone();
    forged["action"]["evidence"] = json!({"passed":true});
    let refused = call(native_pdf_redaction, forged);
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_REQUEST_DECODE");
    status_only(&refused);
    let verify = request(7, json!({"kind":"verify","candidate":candidate}));
    for (field, value) in [
        ("hash", json!(vec![0; 32])),
        ("validatorVersion", json!(2)),
        ("validatorProfile", json!("other")),
        ("affectedPages", json!([42])),
    ] {
        let mut wrong = verify.clone();
        wrong["action"]["candidate"][field] = value;
        let refused = call(native_pdf_redaction, wrong);
        assert_eq!(refused["status"], "failed");
        status_only(&refused);
    }
    let mut wrong_request = verify.clone();
    wrong_request["requestId"] = json!(8);
    assert_eq!(
        call(native_pdf_redaction, wrong_request)["status"],
        "failed"
    );
    let mut wrong_revision = verify.clone();
    wrong_revision["binding"]["revision"] = json!(1);
    assert_eq!(
        call(native_pdf_redaction, wrong_revision)["status"],
        "failed"
    );
    let passed = call(native_pdf_redaction, verify);
    status_only(&passed);
    assert_eq!(
        passed["redaction"]["verification"]["status"], "passed",
        "{passed}"
    );
    for lane in ["structure", "text", "raster"] {
        assert_eq!(
            passed["redaction"]["verification"][lane],
            json!({"status":"passed","coveredPages":1})
        );
    }
    let read_published = request(7, json!({"kind":"readPublished","candidate":candidate}));
    let premature_readback = call(native_pdf_redaction, read_published.clone());
    assert_eq!(
        premature_readback["code"],
        "FLOW_PDF_NATIVE_REDACTION_UNVERIFIED"
    );
    status_only(&premature_readback);
    assert_eq!(
        call(native_pdf_snapshot, bound("snapshot", 8, base))["snapshot"],
        *base
    );
    let mut edit = bound("command", 8, base);
    edit["outputHash"] = base["outputHash"].clone();
    edit["command"] = json!({"kind":"add","page":0,"rect":[1,2,3,4],"contents":"queued note"});
    let blocked = call(native_pdf_command, edit);
    assert_eq!(blocked["code"], "FLOW_PDF_NATIVE_REDACTION_BUSY");
    status_only(&blocked);
    let published = call(native_pdf_redaction, publish.clone());
    assert_eq!(published["status"], "published", "{published}");
    assert_eq!(published["snapshot"]["binding"]["revision"], 1);
    assert_eq!(published["snapshot"]["consumedRequest"], 7);
    assert_eq!(published["snapshot"]["outputHash"], candidate["hash"]);
    assert_ne!(published["snapshot"]["outputHex"], base["outputHex"]);
    let published_status = call(native_pdf_redaction, lookup(candidate.clone()));
    status_only(&published_status);
    assert_eq!(published_status["redaction"]["published"], true);
    let readback = call(native_pdf_redaction, read_published);
    assert_eq!(readback["snapshot"], published["snapshot"]);
    assert_eq!(readback["redaction"]["action"], "readPublished");
    assert_eq!(call(native_pdf_redaction, publish), published);
    let mut stale = request(7, json!({"kind":"publish","candidate":candidate}));
    stale["acceptedHash"] = json!(vec![0; 32]);
    assert_eq!(call(native_pdf_redaction, stale)["status"], "failed");
}

#[test]
fn cancelling_a_verified_redaction_keeps_the_base_and_prevents_publication() {
    let source = redaction_pdf(100);
    let opened = call(native_pdf_open, open_request(source.as_bytes()));
    let base = &opened["snapshot"];
    let page = &base["pageState"]["order"][0];
    let start = |id, action| call(native_pdf_redaction, redaction_request(id, base, action));
    let added = start(
        2,
        json!({"kind":"addMark","pageId":page,"rect":[12000,12000,13000,13000]}),
    );
    assert_eq!(added["redaction"]["markCount"], 1);
    status_only(&added);
    let staged = start(3, json!({"kind":"stage"}));
    status_only(&staged);
    let candidate = &staged["redaction"]["candidate"];
    let verified = start(3, json!({"kind":"verify","candidate":candidate}));
    assert_eq!(verified["redaction"]["verification"]["status"], "passed");
    status_only(&verified);

    let cancelled = start(3, json!({"kind":"cancel","candidate":candidate}));
    status_only(&cancelled);
    assert_eq!(cancelled["redaction"]["markCount"], 1);
    assert_eq!(&cancelled["redaction"]["candidate"], candidate);
    assert!(cancelled["redaction"]["verification"].is_null());
    let stale_close = call(native_pdf_close, bound("close", 3, base));
    assert_eq!(stale_close["code"], "FLOW_PDF_NATIVE_REQUEST_INVALID");
    for kind in ["verify", "publish", "readPublished"] {
        let reply = start(3, json!({"kind":kind,"candidate":candidate}));
        assert_eq!(reply["status"], "failed");
        status_only(&reply);
    }
    let stale_stage = start(3, json!({"kind":"stage"}));
    assert_eq!(stale_stage["status"], "failed");
    status_only(&stale_stage);
    assert_eq!(
        call(native_pdf_snapshot, bound("snapshot", 4, base))["snapshot"],
        *base
    );
    let next = start(4, json!({"kind":"stage"}));
    status_only(&next);
    assert_eq!(next["redaction"]["markCount"], 1);
    assert_eq!(next["redaction"]["verification"]["status"], "pending");
}

#[test]
fn redaction_wire_retains_marks_when_only_some_checks_pass() {
    let source = redaction_pdf(3_000);
    let opened = call(native_pdf_open, open_request(source.as_bytes()));
    let base = &opened["snapshot"];
    let page = &base["pageState"]["order"][0];
    let request = |action| redaction_request(3, base, action);
    let added = call(
        native_pdf_redaction,
        redaction_request(
            2,
            base,
            json!({"kind":"addMark","pageId":page,"rect":[12000,12000,13000,13000]}),
        ),
    );
    assert_eq!(added["redaction"]["markCount"], 1);
    let staged = call(native_pdf_redaction, request(json!({"kind":"stage"})));
    status_only(&staged);
    let candidate = &staged["redaction"]["candidate"];
    let verdict = call(
        native_pdf_redaction,
        request(json!({"kind":"verify","candidate":candidate})),
    );
    status_only(&verdict);
    assert_eq!(
        verdict["redaction"]["verification"]["status"], "inconclusive",
        "{verdict}"
    );
    assert_eq!(
        verdict["redaction"]["verification"]["structure"]["status"],
        "passed"
    );
    assert_eq!(
        verdict["redaction"]["verification"]["text"]["status"],
        "passed"
    );
    assert_eq!(
        verdict["redaction"]["verification"]["raster"]["status"],
        "inconclusive"
    );
    let publish = request(json!({"kind":"publish","candidate":candidate}));
    let refused = call(native_pdf_redaction, publish.clone());
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_REDACTION_UNVERIFIED");
    status_only(&refused);
    assert_eq!(
        call(native_pdf_snapshot, bound("snapshot", 4, base))["snapshot"],
        *base
    );
    let cancelled = call(
        native_pdf_redaction,
        request(json!({"kind":"cancel","candidate":candidate})),
    );
    status_only(&cancelled);
    assert_eq!(cancelled["redaction"]["markCount"], 1);
    let missing = call(
        native_pdf_redaction,
        request(json!({"kind":"status","candidate":candidate})),
    );
    status_only(&missing);
    assert!(missing["redaction"]["candidate"].is_null());
    let stale = call(native_pdf_redaction, publish);
    assert_eq!(
        stale["code"],
        "FLOW_PDF_NATIVE_REDACTION_CANDIDATE_UNAVAILABLE"
    );
    status_only(&stale);
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

    let wrong_revision = call(native_pdf_close, bound("close", 6, &original));
    assert_eq!(wrong_revision["code"], "FLOW_PDF_NATIVE_REVISION_MISMATCH");
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
    use flow_core::pdf::{
        PdfObjectRef,
        native::{NativeObjectPage, NativeTextPage},
    };

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
    let object_pages: Vec<NativeObjectPage> =
        serde_json::from_value(scene["objectPages"].clone()).unwrap();
    let derive_scene_hash = |text_pages: &[NativeTextPage]| {
        let mut hash = blake3::Hasher::new();
        hash.update(b"flowpdf:accepted-note-scene:v2");
        hash.update(&output_hash);
        hash.update(&serde_json::to_vec(&(object, &page["geometry"], true)).unwrap());
        hash.update(b"flowpdf:native-text-pages:v1");
        hash.update(&serde_json::to_vec(text_pages).unwrap());
        hash.update(b"flowpdf:native-object-pages:v1");
        hash.update(&serde_json::to_vec(&object_pages).unwrap());
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

#[test]
fn serialized_graphic_commands_bind_targets_and_preserve_failed_state() {
    let (_, mut snapshot) = open();
    let graphic = json!({"shape":"rectangle","points":[[10000,10000],[30000,30000]],"fill":[0,0,0],"stroke":null,"strokeWidth":0});
    let mut request = bound("command", 2, &snapshot);
    request["outputHash"] = snapshot["outputHash"].clone();
    request["command"] = json!({"kind":"addGraphic","page":0,"graphic":graphic});
    let response = call(native_pdf_command, request);
    assert_eq!(response["status"], "applied", "{response}");
    snapshot = response["snapshot"].clone();
    let object = &snapshot["scene"]["objectPages"][0]["objects"][0];
    assert_eq!(object["bounds"], json!([10000, 10000, 30000, 30000]));
    let target = json!({"id":object["id"],"page":0,"profile":object["profile"]});
    for (index, command) in [
        json!({"kind":"replaceGraphic","target":target,"graphic":graphic}),
        json!({"kind":"moveObject","target":target,"transform":[1,0,0,1,3,5]}),
    ]
    .into_iter()
    .enumerate()
    {
        let object = &snapshot["scene"]["objectPages"][0]["objects"][0];
        let mut command = command;
        command["target"]["id"] = object["id"].clone();
        let mut request = bound("command", 3 + index as u64, &snapshot);
        request["outputHash"] = snapshot["outputHash"].clone();
        request["command"] = command;
        let response = call(native_pdf_command, request);
        assert_eq!(response["status"], "applied", "{response}");
        snapshot = response["snapshot"].clone();
    }
    let mut request = bound("command", 5, &snapshot);
    request["outputHash"] = snapshot["outputHash"].clone();
    request["command"] = json!({"kind":"removeObject","target":target});
    assert_eq!(
        call(native_pdf_command, request.clone())["code"],
        "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE"
    );
    request["command"]["target"]["id"] =
        snapshot["scene"]["objectPages"][0]["objects"][0]["id"].clone();
    let response = call(native_pdf_command, request);
    assert_eq!(response["status"], "applied", "{response}");
    assert_eq!(
        response["snapshot"]["scene"]["objectPages"][0]["objects"],
        json!([])
    );
}

#[test]
fn serialized_image_crud_rejects_forged_profiles_and_malformed_assets() {
    const FIRST: &str = "89504e470d0a1a0a0000000d49484452000000010000000108000000003a7e9b550000000a49444154789c63600700000900082023c38c0000000049454e44ae426082";
    const SECOND: &str = "89504e470d0a1a0a0000000d49484452000000010000000108000000003a7e9b550000000a49444154789c63880600005d005cdc83ebd80000000049454e44ae426082";
    let (_, mut snapshot) = open();
    let bounds = json!([20000, 30000, 40000, 60000]);
    let mut request = bound("command", 2, &snapshot);
    request["outputHash"] = snapshot["outputHash"].clone();
    request["command"] = json!({"kind":"addImage","page":0,"bounds":bounds,"assetHex":FIRST});
    let response = call(native_pdf_command, request.clone());
    assert_eq!(response["status"], "applied", "{response}");
    assert_eq!(response["limits"]["imagePixels"], 16_000_000);
    snapshot = response["snapshot"].clone();
    let original_hex = snapshot["outputHex"].clone();
    request["command"]["assetHex"] = json!(SECOND);
    assert_eq!(
        call(native_pdf_command, request)["code"],
        "FLOW_PDF_NATIVE_REQUEST_CONFLICT"
    );

    let target = |snapshot: &Value| {
        let object = &snapshot["scene"]["objectPages"][0]["objects"][0];
        assert_eq!(object["profile"], "rasterGray8");
        json!({"page":0,"id":object["id"],"profile":object["profile"]})
    };
    let image = target(&snapshot);
    let mut request = bound("command", 3, &snapshot);
    request["outputHash"] = snapshot["outputHash"].clone();
    request["command"] =
        json!({"kind":"replaceImage","target":image,"assetHex":"89504e470d0a1a0a"});
    assert_eq!(
        call(native_pdf_command, request.clone())["code"],
        "FLOW_PDF_NATIVE_OBJECT_ASSET"
    );
    request["command"]["assetHex"] = json!(SECOND);
    request["command"]["target"]["profile"] = json!("line");
    assert_eq!(
        call(native_pdf_command, request.clone())["code"],
        "FLOW_PDF_NATIVE_OBJECT_PROFILE"
    );
    let current = call(native_pdf_snapshot, bound("snapshot", 3, &snapshot));
    assert_eq!(current["status"], "unseen");
    assert_eq!(current["snapshot"], snapshot);
    request["command"]["target"] = image;
    let response = call(native_pdf_command, request);
    assert_eq!(response["status"], "applied", "{response}");
    snapshot = response["snapshot"].clone();
    assert_ne!(snapshot["outputHex"], original_hex);
    assert_eq!(
        snapshot["scene"]["objectPages"][0]["objects"][0]["bounds"],
        bounds
    );

    let mut request = bound("command", 4, &snapshot);
    request["outputHash"] = snapshot["outputHash"].clone();
    request["command"] =
        json!({"kind":"moveObject","target":target(&snapshot),"transform":[1,0,0,1,1,2]});
    let response = call(native_pdf_command, request);
    assert_eq!(response["status"], "applied", "{response}");
    snapshot = response["snapshot"].clone();
    assert_eq!(
        snapshot["scene"]["objectPages"][0]["objects"][0]["bounds"],
        json!([21000, 32000, 41000, 62000])
    );

    let mut request = bound("command", 5, &snapshot);
    request["outputHash"] = snapshot["outputHash"].clone();
    request["command"] = json!({"kind":"removeObject","target":target(&snapshot)});
    let response = call(native_pdf_command, request);
    assert_eq!(response["status"], "applied", "{response}");
    assert_eq!(
        response["snapshot"]["scene"]["objectPages"][0]["objects"],
        json!([])
    );
}

fn page_edit(snapshot: &Value, request_id: u64, command: Value) -> Value {
    let mut request = bound("command", request_id, snapshot);
    request["outputHash"] = snapshot["outputHash"].clone();
    request["command"] = command;
    call(native_pdf_command, request)
}

#[test]
fn serialized_pages_report_authoritative_order_and_selection() {
    let (_, initial) = open();
    let original = initial["pageState"]["order"][0].clone();
    let target = json!({"page":0,"stableId":original});
    for (command, code) in [
        (
            json!({"kind":"deletePage","target":target}),
            "FLOW_PDF_NATIVE_FINAL_PAGE",
        ),
        (
            json!({"kind":"insertBlankPage","target":target,"side":"after","bytesHex":"00"}),
            "FLOW_PDF_NATIVE_REQUEST_DECODE",
        ),
        (
            json!({"kind":"rotatePage","target":{"page":0,"stableId":1},"degrees":90}),
            "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE",
        ),
    ] {
        let reply = page_edit(&initial, 2, command);
        assert_eq!(reply["status"], "failed", "{reply}");
        assert_eq!(reply["code"], code);
    }
    assert_eq!(initial["pageState"]["canDelete"], false);
    let mut state = initial;
    for (request_id, command) in [
        (
            2,
            json!({"kind":"add","page":0,"rect":[10,20,40,60],"contents":"Ї"}),
        ),
        (3, json!({"kind":"rotatePage","target":target,"degrees":90})),
        (
            4,
            json!({"kind":"insertBlankPage","target":target,"side":"before"}),
        ),
        (
            5,
            json!({"kind":"duplicatePage","target":{"page":1,"stableId":original}}),
        ),
        (
            6,
            json!({"kind":"reorderPage","target":{"page":1,"stableId":original},"position":2}),
        ),
        (
            7,
            json!({"kind":"deletePage","target":{"page":2,"stableId":original}}),
        ),
    ] {
        let reply = page_edit(&state, request_id, command);
        assert_eq!(reply["status"], "applied", "{reply}");
        let accepted = call(native_pdf_snapshot, bound("snapshot", request_id, &state));
        assert_eq!(accepted["status"], "accepted");
        assert_eq!(accepted["snapshot"], reply["snapshot"]);
        state = reply["snapshot"].clone();
    }
    assert_eq!(state["pageState"]["maxPosition"], 1);
    assert_eq!(
        state["pageState"]["selected"],
        state["pageState"]["order"][1]
    );
    assert_eq!(state["scene"]["pages"][1]["geometry"]["rotation"], 90);
    assert_eq!(
        state["scene"]["pages"][1]["annotations"][0]["contents"],
        "Ї"
    );
    assert_eq!(
        state["scene"]["pages"][1]["annotations"][0]["rect"],
        json!([10, 20, 40, 60])
    );
    call(native_pdf_close, bound("close", 8, &state));
}

#[test]
fn serialized_page_delete_refuses_an_unowned_destination_and_keeps_output() {
    let bytes = fixture_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /Dests << /Permanent [3 0 R /Fit] >> >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] >>",
        "<< /Type /Page /Parent 2 0 R >>",
        "<< /Type /Page /Parent 2 0 R >>",
    ]);
    let initial = call(native_pdf_open, open_request(bytes.as_bytes()))["snapshot"].clone();
    let restrictions = &initial["pageState"]["restrictions"]["pages"];
    assert_eq!(restrictions[0]["delete"], "structure");
    assert!(restrictions[1].get("delete").is_none());
    let target = json!({"page":0,"stableId":initial["pageState"]["order"][0]});
    let refused = page_edit(&initial, 2, json!({"kind":"deletePage","target":target}));
    assert_eq!(refused["status"], "failed");
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_UNSUPPORTED");
    let lookup = call(native_pdf_snapshot, bound("snapshot", 2, &initial));
    assert_eq!(lookup["status"], "unseen");
    assert_eq!(lookup["snapshot"], initial);
    call(native_pdf_close, bound("close", 3, &initial));
}

#[test]
fn page_capabilities_are_verified_against_the_reopened_tree_at_any_revision() {
    let bytes = fixture_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] /Resources <<>> >>",
        "<< /Type /Page /Parent 2 0 R /UserUnit 2 >>",
        "<< /Type /Pages /Parent 2 0 R /Kids [5 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 4 0 R >>",
    ]);
    let initial = call(native_pdf_open, open_request(bytes.as_bytes()))["snapshot"].clone();
    let scaled = &initial["pageState"]["order"][0];
    let normal = &initial["pageState"]["order"][1];
    let refusal = page_edit(
        &initial,
        2,
        json!({"kind":"rotatePage","target":{"page":0,"stableId":scaled},"degrees":90}),
    );
    assert_eq!(refusal["code"], "FLOW_PDF_NATIVE_PAGE_GEOMETRY");
    let mut request = bound("command", 3, &initial);
    request["outputHash"] = initial["outputHash"].clone();
    request["command"] = json!({
        "kind":"rotatePage","target":{"page":1,"stableId":normal},"degrees":90,
    });
    let response = native_pdf_command(request.to_string());
    assert!(verify_native_pdf_worker_response(response.clone()));
    let applied: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(applied["status"], "applied");
    let current = &applied["snapshot"];
    let pages = &current["pageState"]["restrictions"]["pages"];
    assert_eq!(pages[0]["reparent"], "geometry");
    assert_ne!(pages[0]["parent"], pages[1]["parent"]);
    let digest = applied["resultHash"].as_str().unwrap();
    let original = format!(r#""parent":{}"#, pages[0]["parent"]);
    let forged_parent = format!(r#""parent":{}"#, pages[1]["parent"]);
    assert!(response.contains(&original));
    let edited = response
        .replace(digest, "")
        .replacen(&original, &forged_parent, 1);
    let mut hash = blake3::Hasher::new();
    hash.update(b"flowpdf:native-wire:v1");
    hash.update(edited.as_bytes());
    let forged = edited.replace(
        r#""resultHash":"""#,
        &format!(r#""resultHash":"{}""#, hash.finalize().to_hex()),
    );
    assert!(!verify_native_pdf_worker_response(forged.clone()));
    call(native_pdf_close, bound("close", 4, current));
    assert!(verify_native_pdf_response(response));
    assert!(!verify_native_pdf_response(forged));
}

#[test]
fn field_commands_reopen_values_and_refuse_stale_or_forged_targets() {
    let (_, mut current) = open();
    let command = |request_id, snapshot: &Value, body: Value| {
        let mut request = bound("command", request_id, snapshot);
        request["outputHash"] = snapshot["outputHash"].clone();
        request["command"] = body;
        call(native_pdf_command, request)
    };
    for (request_id, body) in [
        (
            2,
            json!({"kind":"addTextField","page":0,"name":"recipient","rect":[10,10,150,35],"value":"Alice"}),
        ),
        (
            3,
            json!({"kind":"addCheckboxField","page":0,"name":"approved","rect":[10,50,30,70],"checked":false}),
        ),
    ] {
        let response = command(request_id, &current, body);
        assert_eq!(response["status"], "applied", "{response}");
        current = response["snapshot"].clone();
    }
    assert_eq!(current["fields"][0]["value"], "Alice");
    assert_eq!(current["fields"][1]["checked"], false);
    let stale = current["fields"][0]["id"].clone();

    let duplicate = command(
        4,
        &current,
        json!({"kind":"duplicatePage","target":{
            "page":0,"stableId":current["pageState"]["order"][0],
        }}),
    );
    assert_eq!(duplicate["status"], "applied", "{duplicate}");
    let mut current = duplicate["snapshot"].clone();
    assert_eq!(current["fields"].as_array().unwrap().len(), 4);
    assert_ne!(current["fields"][0]["name"], current["fields"][2]["name"]);
    let deletion = command(
        5,
        &current,
        json!({"kind":"deletePage","target":{
            "page":0,"stableId":current["pageState"]["order"][0],
        }}),
    );
    assert_eq!(deletion["status"], "applied", "{deletion}");
    current = deletion["snapshot"].clone();
    assert_eq!(current["fields"].as_array().unwrap().len(), 2);
    let text = current["fields"][0]["id"].clone();
    for (body, reason) in [
        (
            json!({"kind":"setFieldText","field":stale,"value":"Bob"}),
            "FLOW_PDF_NATIVE_FIELD_REFERENCE",
        ),
        (
            json!({"kind":"setFieldText","field":text,"value":"Ї"}),
            "FLOW_PDF_NATIVE_FIELD_GLYPH",
        ),
        (
            json!({"kind":"setFieldChecked","field":text,"checked":true}),
            "FLOW_PDF_NATIVE_FIELD_PROFILE",
        ),
        (
            json!({"kind":"moveField","field":text,"page":0,"rect":[30,40,800,900]}),
            "FLOW_PDF_NATIVE_FIELD_GEOMETRY",
        ),
        (
            json!({"kind":"setFieldText","field":text,"value":"Bob","canEditAppearance":true}),
            "FLOW_PDF_NATIVE_REQUEST_DECODE",
        ),
        (
            json!({"kind":"addRadioField","page":0,"name":"choice","rect":[10,10,30,30]}),
            "FLOW_PDF_NATIVE_REQUEST_DECODE",
        ),
        (
            json!({"kind":"addCheckboxField","page":0,"name":"a".repeat(65),"rect":[10,10,30,30],"checked":true}),
            "FLOW_PDF_NATIVE_REQUEST_DECODE",
        ),
        (
            json!({"kind":"setFieldText","field":text,"value":"Ї".repeat(513)}),
            "FLOW_PDF_NATIVE_REQUEST_DECODE",
        ),
    ] {
        let refusal = command(6, &current, body);
        assert_eq!(refusal["code"], reason, "{refusal}");
        assert!(refusal["snapshot"].is_null());
    }
    let response = command(
        6,
        &current,
        json!({"kind":"setFieldText","field":text,"value":"Bob"}),
    );
    assert_eq!(response["status"], "applied", "{response}");
    current = response["snapshot"].clone();
    assert_eq!(current["fields"][0]["value"], "Bob");

    let move_field = command(
        7,
        &current,
        json!({
            "kind":"moveField","field":current["fields"][0]["id"],"page":0,"rect":[25,30,170,60],
        }),
    );
    assert_eq!(move_field["status"], "applied", "{move_field}");
    current = move_field["snapshot"].clone();
    assert_eq!(current["fields"][0]["rect"], json!([25, 30, 170, 60]));
    assert_eq!(current["fields"][0]["value"], "Bob");

    let mut removal = bound("command", 8, &current);
    removal["outputHash"] = current["outputHash"].clone();
    removal["command"] = json!({"kind":"removeField","field":current["fields"][0]["id"]});
    let response = call(native_pdf_command, removal);
    assert_eq!(response["status"], "applied", "{response}");
    assert_eq!(response["snapshot"]["fields"].as_array().unwrap().len(), 1);
    let accepted = call(native_pdf_snapshot, bound("snapshot", 8, &current));
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(accepted["snapshot"], response["snapshot"]);
}

#[test]
fn field_capability_forgery_still_refuses_after_rehashing_the_response() {
    let (_, original) = open();
    let mut request = bound("command", 2, &original);
    request["outputHash"] = original["outputHash"].clone();
    request["command"] = json!({
        "kind":"addCheckboxField","page":0,"rect":[10,10,30,30],"name":"approved","checked":false,
    });
    let response = native_pdf_command(request.to_string());
    assert!(verify_native_pdf_response(response.clone()));
    let parsed: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(parsed["status"], "applied");
    let result_hash = parsed["resultHash"].as_str().unwrap();
    let without_hash = response.replace(result_hash, "");
    for (old, new) in [
        (
            r#""canEditAppearance":true"#,
            r#""canEditAppearance":false"#,
        ),
        (r#""checked":false"#, r#""checked":true"#),
    ] {
        assert!(without_hash.contains(old));
        let edited = without_hash.replacen(old, new, 1);
        let mut hash = blake3::Hasher::new();
        hash.update(b"flowpdf:native-wire:v1");
        hash.update(edited.as_bytes());
        let forged = edited.replace(
            r#""resultHash":"""#,
            &format!(r#""resultHash":"{}""#, hash.finalize().to_hex()),
        );
        assert!(!verify_native_pdf_response(forged));
    }
}

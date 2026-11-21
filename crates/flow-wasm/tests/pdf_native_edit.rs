use std::fmt::Write as _;

#[cfg(not(target_arch = "wasm32"))]
use flow_wasm::native_pdf_open_with_result_limit;
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
    String::from_utf8(fixture_pdf_bytes(objects)).unwrap()
}

fn fixture_pdf_bytes(objects: &[impl AsRef<[u8]>]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object.as_ref());
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let size = objects.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
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
    let original_request = open_request(original.as_bytes());
    stale["originStoreHash"] = original_request["originStoreHash"].clone();

    for (request, code) in [
        (stale.clone(), "FLOW_PDF_NATIVE_BINDING_MISMATCH"),
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
    assert_eq!(call(native_pdf_open, original_request)["status"], "opened");
    assert_eq!(
        call(native_pdf_open, stale)["code"],
        "FLOW_PDF_NATIVE_BINDING_MISMATCH",
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
    redaction_pdf_with_paint(size, "10 10 10 10 re f 40 40 10 10 re f")
}

fn redaction_pdf_with_paint(size: u32, paint: &str) -> String {
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
fn detached_snapshot_verification_keeps_conservative_object_availability() {
    let height = 780;
    let mut paint = String::from("q 100 0 0 100 0 0 cm /Image Do Q\n");
    for tile in 0..96 {
        let x = (tile % 12) * 20;
        let y = (tile / 12) * 20 + 110;
        for point in 0..32 {
            let angle = f64::from(point) * std::f64::consts::TAU / 32.0;
            let px = f64::from(x + 10) + 8.0 * angle.cos();
            let py = f64::from(y + 10) + 8.0 * angle.sin();
            writeln!(
                paint,
                "{px:.24} {py:.24} {}",
                if point == 0 { "m" } else { "l" }
            )
            .unwrap();
        }
        paint.push_str("h f\n");
    }
    let pixels: Vec<u8> = (0..height)
        .flat_map(|row| (0..640).map(move |column| ((row + column) / 5 % 256) as u8))
        .collect();
    let mut image = format!("<< /Type /XObject /Subtype /Image /Width 640 /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n", pixels.len()).into_bytes();
    image.extend_from_slice(&pixels);
    image.extend_from_slice(b"\nendstream");
    let content = format!("<< /Length {} >>\nstream\n{paint}\nendstream", paint.len());
    let bytes = fixture_pdf_bytes(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".as_slice(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 300] >>",
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Image 5 0 R >> >> >>",
        content.as_bytes(), &image,
    ]);
    let opened = call(native_pdf_open, open_request(&bytes));
    assert_eq!(opened["status"], "opened");
    let base = &opened["snapshot"];
    assert_eq!(
        base["scene"]["objectPages"][0]["objects"]
            .as_array()
            .unwrap()
            .len(),
        97
    );
    let mut request = bound("command", 2, base);
    request["command"] =
        json!({"kind":"add","page":0,"rect":[12,12,40,32],"contents":"Chart note"});
    request["outputHash"] = base["outputHash"].clone();
    let applied = native_pdf_command(request.to_string());
    assert!(verify_native_pdf_worker_response(applied.clone()));
    let reply: Value = serde_json::from_str(&applied).unwrap();
    assert_eq!(reply["status"], "applied", "{}", reply["code"]);
    assert_eq!(reply["binding"], base["binding"]);
    assert_eq!(reply["requestId"], request["requestId"]);
    let accepted = &reply["snapshot"];
    assert_eq!(accepted["binding"]["revision"], 1);
    assert_eq!(accepted["consumedRequest"], request["requestId"]);
    assert_eq!(
        accepted["scene"]["pages"][0]["annotations"][0]["contents"],
        "Chart note"
    );
    assert_eq!(
        accepted["scene"]["objectPages"],
        json!([
            {"page": 0, "canAdd": false, "objects": [], "refusal": null}
        ])
    );
    assert_ne!(accepted["outputHex"], base["outputHex"]);
    assert_eq!(native_pdf_command(request.to_string()), applied);
    let readback = native_pdf_snapshot(bound("snapshot", 2, base).to_string());
    assert!(verify_native_pdf_worker_response(readback.clone()));
    let snapshot_reply: Value = serde_json::from_str(&readback).unwrap();
    assert_eq!(snapshot_reply["status"], "accepted");
    assert_eq!(snapshot_reply["binding"], base["binding"]);
    assert_eq!(snapshot_reply["requestId"], request["requestId"]);
    assert_eq!(&snapshot_reply["snapshot"], accepted);

    let wrong_request =
        rehash_with_replacement(&readback, r#""requestId":2,"#, r#""requestId":3,"#);
    assert!(!verify_native_pdf_worker_response(wrong_request.clone()));
    let output_hex = accepted["outputHex"].as_str().unwrap();
    let output: Vec<_> = (0..output_hex.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&output_hex[offset..offset + 2], 16).unwrap())
        .collect();
    std::thread::spawn(move || {
        for response in [applied, readback] {
            assert!(verify_native_pdf_response(response.clone()));
            assert!(!verify_native_pdf_worker_response(response));
        }
        assert!(!verify_native_pdf_response(wrong_request));

        let reopened = call(native_pdf_open, open_request(&output));
        assert_eq!(reopened["status"], "opened", "{}", reopened["code"]);
        assert_eq!(
            reopened["snapshot"]["scene"]["objectPages"][0]["objects"]
                .as_array()
                .unwrap()
                .len(),
            97
        );
    })
    .join()
    .unwrap();
}

#[test]
fn letter_page_preview_uses_default_worker_budget() {
    let opened = call(native_pdf_open, open_request(pdf(612).as_bytes()));
    let base = &opened["snapshot"];
    assert_eq!(base["redactionAdmission"], "supported");
    let page = base["pageState"]["order"][0].clone();
    let preview = call(native_pdf_preview, preview_request(2, base, page));
    assert_eq!(preview["status"], "preview", "{preview}");
    let raster = &preview["preview"];
    assert_eq!(raster["width"], 1224);
    assert_eq!(raster["height"], 1584);
    let rgb = raster["rgbHex"].as_str().unwrap();
    assert_eq!(rgb.len(), 1224 * 1584 * 6);
    assert!(rgb.bytes().all(|digit| digit == b'f'));
    assert_eq!(
        call(native_pdf_close, bound("close", 3, base))["status"],
        "closed"
    );
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
    let original_reply = native_pdf_preview(original_request.to_string());
    assert!(verify_native_pdf_worker_response(original_reply.clone()));
    let preview: Value = serde_json::from_str(&original_reply).unwrap();
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
    edit["command"] = json!({"kind":"add","page":0,"rect":[8,6,10,8],"contents":"Review note"});
    let applied = call(native_pdf_command, edit);
    assert_eq!(applied["status"], "applied");
    assert!(!verify_native_pdf_worker_response(original_reply.clone()));
    assert!(verify_native_pdf_response(original_reply.clone()));
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
    let updated_reply = native_pdf_preview(preview_request(6, current, first.clone()).to_string());
    assert!(verify_native_pdf_worker_response(updated_reply.clone()));
    let updated: Value = serde_json::from_str(&updated_reply).unwrap();
    assert_eq!(updated["status"], "preview");
    assert_eq!(updated["preview"]["outputHash"], current["outputHash"]);
    let updated_rgb = updated["preview"]["rgbHex"].as_str().unwrap();
    let updated_pixel = |x: usize, y: usize| &updated_rgb[(y * 16 + x) * 6..][..6];
    assert_eq!(updated_pixel(4, 12), "5e552e");
    assert_eq!(updated_pixel(5, 13), "ffdc40");
    assert_eq!(updated_pixel(4, 11), pixel(4, 11));
    let mut different_rgb = updated_rgb.to_owned();
    let offset = (12 * 16 + 4) * 6;
    different_rgb.replace_range(offset..offset + 6, "000000");
    for (field, accepted, other) in [
        ("requestId", json!(6), json!(7)),
        ("pageId", first.clone(), json!(1)),
        (
            "cropBox",
            raster["cropBox"].clone(),
            json!([2500, 4000, 14500, 12000]),
        ),
        ("rotation", raster["rotation"].clone(), json!(270)),
        ("outputHash", current["outputHash"].clone(), json!(vec![0; 32])),
        (
            "originStoreHash",
            current["originStoreHash"].clone(),
            json!("00".repeat(32)),
        ),
        ("rgbHex", json!(updated_rgb), json!(different_rgb)),
    ] {
        let different = rehash_with_replacement(
            &updated_reply,
            &format!(r#""{field}":{accepted}"#),
            &format!(r#""{field}":{other}"#),
        );
        assert!(verify_native_pdf_response(different.clone()), "{field}");
        assert!(!verify_native_pdf_worker_response(different), "{field}");
    }
    let refreshed_reply =
        native_pdf_preview(preview_request(7, current, first.clone()).to_string());
    assert!(verify_native_pdf_worker_response(refreshed_reply.clone()));
    assert!(!verify_native_pdf_worker_response(updated_reply));
    assert_eq!(
        call(native_pdf_close, bound("close", 8, current))["status"],
        "closed"
    );
    assert!(!verify_native_pdf_worker_response(original_reply.clone()));
    assert!(!verify_native_pdf_worker_response(refreshed_reply.clone()));
    std::thread::spawn(move || {
        assert!(verify_native_pdf_response(original_reply));
        assert!(verify_native_pdf_response(refreshed_reply));
    })
    .join()
    .unwrap();
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
fn page_preview_refuses_unresolved_image_samples() {
    let aligned = "1 0 0 0.5 2 2";
    let compressed = "0.5 0 0 0.5 2 2";
    let edge_only = "0.2 0 0 0.2 2 2";
    let distinct = "AzAzAz";
    let uniform = "AzAAzA";
    let partial = "1 0 0 RG 0.04 w 2.05 2.08 m 2.2 2.08 l S";
    let opaque = "0 0 1 rg 2 2 0.5 0.5 re f";
    for (matrix, samples, background, repaint, expected) in [
        (aligned, distinct, "", "", Some("417a417a417a")),
        (compressed, distinct, "", "", None),
        (compressed, distinct, "", partial, None),
        (compressed, distinct, "", opaque, Some("0000ff")),
        (compressed, uniform, "", "", Some("417a41")),
        (compressed, uniform, "", partial, Some("a03d21")),
        (edge_only, distinct, "", "", None),
        (edge_only, uniform, "", "", Some("e1eae1")),
        (
            "0.35 0 0 0.5 2 2",
            uniform,
            "1 0 0 rg 2 2 0.5 0.5 re f",
            "",
            Some("7a552e"),
        ),
    ] {
        let artwork = format!("{background}\nq {matrix} cm /Photo Do Q\n{repaint}");
        let source = fixture_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 8 8] /Contents 4 0 R /Resources << /XObject << /Photo 5 0 R >> >> >>",
            &format!(
                "<< /Length {} >>\nstream\n{artwork}\nendstream",
                artwork.len()
            ),
            &format!(
                "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} >>\nstream\n{samples}\nendstream",
                samples.len()
            ),
        ]);
        let opened = call(native_pdf_open, open_request(source.as_bytes()));
        assert_eq!(opened["status"], "opened", "{opened}");
        let base = &opened["snapshot"];
        let reply = call(
            native_pdf_preview,
            preview_request(2, base, base["pageState"]["order"][0].clone()),
        );
        if let Some(expected) = expected {
            assert_eq!(reply["status"], "preview", "{reply}");
            let rgb = reply["preview"]["rgbHex"].as_str().unwrap();
            let start = (11 * 16 + 4) * 6;
            assert_eq!(&rgb[start..start + expected.len()], expected);
        } else {
            assert_eq!(
                reply["code"], "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE",
                "{reply}"
            );
            assert!(reply.get("preview").is_none());
            status_only(&reply);
        }
        assert_eq!(
            call(native_pdf_close, bound("close", 3, base))["status"],
            "closed"
        );
    }
}

#[test]
fn page_preview_resolves_full_outline_coverage_at_fractional_crop_edges() {
    for (rotation, x) in [(0, 2), (90, 1)] {
        let image = format!("q 0.2 0 0 0.2 {x} 2 cm /Photo Do Q");
        let opaque = format!("0 0 1 rg {x} 2 0.2 0.2 re f");
        let partial = format!("0 0 1 rg {x} 2 0.15 0.2 re f");
        for (background, repaint, expected) in [
            ("", opaque.as_str(), Some("0000ff")),
            ("", partial.as_str(), Some("8080ff")),
            (image.as_str(), "", None),
            (image.as_str(), partial.as_str(), None),
            (image.as_str(), opaque.as_str(), Some("0000ff")),
        ] {
            let artwork = format!("{background}\n{repaint}");
            let source = fixture_pdf(&[
                "<< /Type /Catalog /Pages 2 0 R >>",
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                &format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 4 4] /CropBox [1 1 2.2 2.2] /Rotate {rotation} /Contents 4 0 R /Resources << /XObject << /Photo 5 0 R >> >> >>"
                ),
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
            if let Some(expected) = expected {
                assert_eq!(reply["status"], "preview", "{rotation}: {artwork}\n{reply}");
                let raster = &reply["preview"];
                assert_eq!(raster["width"], 3);
                assert_eq!(raster["height"], 3);
                assert_eq!(
                    raster["rgbHex"].as_str().unwrap(),
                    format!("{}{expected}{}", "ffffff".repeat(2), "ffffff".repeat(6)),
                    "{rotation}: {artwork}"
                );
            } else {
                assert_eq!(
                    reply["code"], "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE",
                    "{rotation}: {artwork}\n{reply}"
                );
                assert!(reply.get("preview").is_none());
                status_only(&reply);
            }
            assert_eq!(
                call(native_pdf_close, bound("close", 3, base))["status"],
                "closed"
            );
        }
    }
}

#[test]
fn accepted_preview_renders_both_jpeg_filter_forms() {
    const JPEG_HEX: &str = concat!(
        "ffd8ffe000104a46494600010100000100010000ffdb00430001010101010101",
        "0101010101010101010101010101010101010101010101010101010101010101",
        "01010101010101010101010101010101010101010101010101ffc0000b080001",
        "000201011100ffc40014000100000000000000000000000000000009ffc4001a",
        "1000010501000000000000000000000000080006073878b7ffda000801010000",
        "3f003fcb2b4e4be80997a2b8d7ffd9",
    );
    let jpeg: Vec<u8> = (0..JPEG_HEX.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&JPEG_HEX[offset..offset + 2], 16).unwrap())
        .collect();
    let paint = "q 1 0 0 0.5 2 2 cm /Photo Do Q";
    let content = format!("<< /Length {} >>\nstream\n{paint}\nendstream", paint.len());

    for filter in ["/DCTDecode", "[/DCTDecode]"] {
        let mut photo = format!(
            "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter {filter} /Length {} >>\nstream\n",
            jpeg.len()
        )
        .into_bytes();
        photo.extend_from_slice(&jpeg);
        photo.extend_from_slice(b"\nendstream");
        let source = fixture_pdf_bytes(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".as_slice(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 8 8] /Contents 4 0 R /Resources << /XObject << /Photo 5 0 R >> >> >>",
            content.as_bytes(),
            &photo,
        ]);
        let opened = call(native_pdf_open, open_request(&source));
        assert_eq!(opened["status"], "opened", "{filter}: {opened}");
        let base = &opened["snapshot"];
        assert_eq!(base["redactionAdmission"], "supported", "{filter}");
        let reply = call(
            native_pdf_preview,
            preview_request(2, base, base["pageState"]["order"][0].clone()),
        );
        assert_eq!(reply["status"], "preview", "{filter}: {reply}");
        let rgb = reply["preview"]["rgbHex"].as_str().unwrap();
        let start = (11 * 16 + 4) * 6;
        let sample = |offset| u8::from_str_radix(&rgb[offset..offset + 2], 16).unwrap();
        assert!((198..=218).contains(&sample(start)), "{filter}");
        assert!((70..=90).contains(&sample(start + 6)), "{filter}");
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
    let tampered = rehash_with_replacement(
        &rehashed,
        r#""redactionAdmission":"unavailable""#,
        r#""redactionAdmission":"supported""#,
    );
    assert!(!verify_native_pdf_worker_response(tampered.clone()));
    std::thread::spawn(move || assert!(!verify_native_pdf_response(tampered)))
        .join()
        .unwrap();

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
    let unchanged = call(native_pdf_snapshot, bound("snapshot", 3, after));
    assert_eq!(unchanged["status"], "unseen");
    assert_eq!(unchanged["snapshot"], *after);
    let cleared = call(
        native_pdf_redaction,
        redaction_request(3, after, json!({"kind":"clearMarks"})),
    );
    assert_eq!(cleared["status"], "redaction");
    assert_eq!(cleared["redaction"]["markCount"], 0);
    assert_eq!(
        call(native_pdf_close, bound("close", 4, after))["status"],
        "closed"
    );
}

#[test]
fn marked_snapshot_survives_live_redaction_budget_refusal() {
    use flow_core::pdf::native::{
        NATIVE_REQUEST_HISTORY, NativeEditLimits, NativePdfSession, NativeRedactionMark,
        NativeSessionError,
    };

    let fixture = |height: usize| {
        let paint = b"q 80 0 0 80 10 10 cm /Photo Do Q";
        let mut content = format!("<< /Length {} >>\nstream\n", paint.len()).into_bytes();
        content.extend_from_slice(paint);
        content.extend_from_slice(b"\nendstream");
        let samples = 1024 * height;
        let mut image = format!("<< /Type /XObject /Subtype /Image /Width 1024 /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {samples} >>\nstream\n").into_bytes();
        image
            .extend((0..samples).map(|sample| ((sample / 1024 + (sample % 1024) / 4) % 256) as u8));
        image.extend_from_slice(b"\nendstream");
        fixture_pdf_bytes(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Resources << /XObject << /Photo 5 0 R >> >> >>".to_vec(),
            content,
            image,
        ])
    };
    let limits = NativeEditLimits {
        max_source_bytes: 8 * 1024 * 1024,
        max_output_bytes: 8 * 1024 * 1024,
        max_retained_bytes: 8 * 1024 * 1024
            - (NATIVE_REQUEST_HISTORY * size_of::<(u64, [u8; 32])>() + 1024),
        ..NativeEditLimits::default()
    };
    let open = |source: &[u8]| NativePdfSession::open_scoped(source, limits, [1; 16]);
    // @contract: Tune the fixture to the worker allowance before reserving space for marks.
    let mut low = 400;
    let mut high = 2100;
    assert!(open(&fixture(low)).unwrap().redaction_admitted());
    assert!(!open(&fixture(high)).is_ok_and(|session| session.redaction_admitted()));
    while low + 1 < high {
        let mid = (low + high) / 2;
        if open(&fixture(mid)).is_ok_and(|session| session.redaction_admitted()) {
            low = mid;
        } else {
            high = mid;
        }
    }
    let source = fixture(low);
    let mut session = open(&source).unwrap();
    let binding = session.snapshot().binding;
    let page_id = session.snapshot().page_state.order[0];
    let geometry = session.redaction_mark_geometry(page_id).unwrap();
    let rect = |index: i64| [1000 + 2 * index, 1000, 1001 + 2 * index, 1100];
    let mark = |index| NativeRedactionMark {
        page_id,
        geometry,
        rect: rect(index),
    };
    let mut marks = 0;
    while session.redaction_admitted() {
        assert!(marks < 128);
        session
            .add_redaction_mark(binding, marks as u64 + 2, mark(marks))
            .unwrap();
        marks += 1;
    }

    let opened = call(native_pdf_open, open_request(&source));
    assert_eq!(opened["status"], "opened");
    let base = &opened["snapshot"];
    assert_eq!(base["redactionAdmission"], "supported");
    let unmarked = native_pdf_snapshot(bound("snapshot", 2, base).to_string());
    assert!(verify_native_pdf_worker_response(unmarked.clone()));
    std::thread::spawn(move || assert!(verify_native_pdf_response(unmarked)))
        .join()
        .unwrap();

    let mark_request = |kind, request_id, index| {
        redaction_request(
            request_id,
            base,
            json!({
                "kind": kind, "pageId": base["pageState"]["order"][0], "rect": rect(index),
            }),
        )
    };
    for index in 0..marks {
        let response = call(
            native_pdf_redaction,
            mark_request("addMark", index as u64 + 2, index),
        );
        assert_eq!(response["status"], "redaction", "code={}", response["code"]);
        assert_eq!(response["redaction"]["markCount"], index + 1);
    }

    let request_id = marks as u64 + 2;
    let error = session
        .add_redaction_mark(binding, request_id, mark(marks))
        .unwrap_err();
    assert_eq!(error, NativeSessionError::Budget);
    assert_eq!(session.redaction_marks().len(), marks as usize);
    let refused = call(
        native_pdf_redaction,
        mark_request("addMark", request_id, marks),
    );
    assert_eq!(refused["status"], "failed");
    assert_eq!(refused["code"], error.code());
    assert!(refused.get("redaction").is_none());
    assert!(refused.get("preview").is_none());
    status_only(&refused);

    let status = call(
        native_pdf_redaction,
        redaction_request(request_id, base, json!({"kind":"status","candidate":null})),
    );
    assert_eq!(status["status"], "redaction");
    assert_eq!(status["redaction"]["markCount"], marks);

    let snapshot = native_pdf_snapshot(bound("snapshot", request_id, base).to_string());
    assert!(verify_native_pdf_worker_response(snapshot.clone()));
    let readback: Value = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(readback["status"], "unseen");
    assert_eq!(readback["snapshot"]["redactionAdmission"], "unavailable");
    for field in ["binding", "outputHash", "outputHex", "consumedRequest"] {
        assert!(
            readback["snapshot"][field] == base[field],
            "{field} changed"
        );
    }
    std::thread::spawn(move || assert!(verify_native_pdf_response(snapshot)))
        .join()
        .unwrap();

    session
        .add_redaction_mark(binding, request_id, mark(0))
        .unwrap();
    let duplicate = call(native_pdf_redaction, mark_request("addMark", request_id, 0));
    assert_eq!(duplicate["status"], "redaction");
    assert_eq!(
        duplicate["redaction"]["markCount"],
        session.redaction_marks().len()
    );

    for index in 0..marks {
        session
            .remove_redaction_mark(binding, request_id + 1 + index as u64, mark(index))
            .unwrap();
        let removed = call(
            native_pdf_redaction,
            mark_request("removeMark", request_id + 1 + index as u64, index),
        );
        assert_eq!(removed["status"], "redaction");
        assert_eq!(
            removed["redaction"]["markCount"],
            session.redaction_marks().len()
        );
    }
}

#[test]
fn redaction_wire_uses_the_accepted_revision_after_an_ordinary_edit() {
    let source = redaction_pdf(100);
    let opened = call(native_pdf_open, open_request(source.as_bytes()));
    let base = &opened["snapshot"];
    let mut command = bound("command", 2, base);
    command["outputHash"] = base["outputHash"].clone();
    command["command"] = json!({
        "kind":"add","page":0,"rect":[60,60,70,70],"contents":"review note"
    });
    let applied = native_pdf_command(command.to_string());
    assert!(verify_native_pdf_worker_response(applied.clone()));
    let reply: Value = serde_json::from_str(&applied).unwrap();
    assert_eq!(reply["status"], "applied", "{reply}");
    let accepted = &reply["snapshot"];
    assert_eq!(accepted["binding"]["revision"], 1);
    assert_eq!(
        accepted["binding"]["originHash"],
        base["binding"]["originHash"]
    );
    assert_ne!(accepted["outputHash"], accepted["binding"]["originHash"]);
    assert_eq!(accepted["redactionAdmission"], "supported");
    let forged = rehash_with_replacement(
        &applied,
        r#""redactionAdmission":"supported""#,
        r#""redactionAdmission":"unavailable""#,
    );
    assert!(!verify_native_pdf_worker_response(forged.clone()));

    let mark = json!({
        "kind":"addMark","pageId":accepted["pageState"]["order"][0],
        "rect":[12000,12000,13000,13000]
    });
    let stale = call(
        native_pdf_redaction,
        redaction_request(3, base, mark.clone()),
    );
    assert_eq!(stale["code"], "FLOW_PDF_NATIVE_REVISION_MISMATCH");
    let add = redaction_request(3, accepted, mark);
    let mut wrong_hash = add.clone();
    wrong_hash["acceptedHash"] = accepted["binding"]["originHash"].clone();
    let refused = call(native_pdf_redaction, wrong_hash);
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_BINDING_MISMATCH");
    assert_eq!(call(native_pdf_redaction, add)["redaction"]["markCount"], 1);

    let stage = redaction_request(4, accepted, json!({"kind":"stage"}));
    let staged = native_pdf_redaction(stage.to_string());
    assert!(verify_native_pdf_worker_response(staged.clone()));
    let candidate: Value = serde_json::from_str(&staged).unwrap();
    assert_eq!(candidate["redaction"]["verification"]["status"], "pending");
    assert_eq!(
        candidate["redaction"]["acceptedHash"],
        accepted["outputHash"]
    );
    assert_eq!(native_pdf_redaction(stage.to_string()), staged);
    std::thread::spawn(move || {
        assert!(verify_native_pdf_response(applied));
        assert!(verify_native_pdf_response(forged));
        assert!(verify_native_pdf_response(staged));
    })
    .join()
    .unwrap();
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
    let remove = request(
        3,
        json!({"kind":"removeMark","pageId":page,"rect":[12000,12000,13000,13000]}),
    );
    let clear = request(5, json!({"kind":"clearMarks"}));
    let first_mark = native_pdf_redaction(add(2).to_string());
    assert!(verify_native_pdf_worker_response(first_mark.clone()));
    let first_reply: Value = serde_json::from_str(&first_mark).unwrap();
    assert_eq!(first_reply["redaction"]["markCount"], 1);
    status_only(&first_reply);
    for (command, count) in [
        (remove.clone(), 0),
        (remove, 0),
        (add(4), 1),
        (clear.clone(), 0),
        (clear, 0),
        (add(6), 1),
        (add(6), 1),
    ] {
        let reply = call(native_pdf_redaction, command);
        assert_eq!(reply["redaction"]["markCount"], count, "{reply}");
        status_only(&reply);
    }
    assert!(!verify_native_pdf_worker_response(first_mark));
    let stale_mark = call(native_pdf_redaction, add(4));
    assert_eq!(stale_mark["code"], "FLOW_PDF_NATIVE_REQUEST_INVALID");
    let mark_readback = call(native_pdf_snapshot, bound("snapshot", 6, base));
    assert_eq!(mark_readback["status"], "unseen");
    for field in ["binding", "outputHash", "outputHex", "consumedRequest"] {
        assert_eq!(mark_readback["snapshot"][field], base[field]);
    }
    let conflicting = call(
        native_pdf_redaction,
        request(6, json!({"kind":"clearMarks"})),
    );
    assert_eq!(conflicting["code"], "FLOW_PDF_NATIVE_REQUEST_INVALID");
    let colliding_stage = call(native_pdf_redaction, request(6, json!({"kind":"stage"})));
    assert_eq!(colliding_stage["code"], "FLOW_PDF_NATIVE_REQUEST_INVALID");
    let mut edit = bound("command", 6, base);
    edit["outputHash"] = base["outputHash"].clone();
    edit["command"] = json!({"kind":"add","page":0,"rect":[1,2,3,4],"contents":"queued note"});
    let colliding_edit = call(native_pdf_command, edit.clone());
    assert_eq!(
        colliding_edit["code"],
        "FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED"
    );
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
    assert_eq!(
        call(native_pdf_redaction, add(6))["redaction"]["markCount"],
        1
    );
    for id in [7, 8] {
        let busy = call(native_pdf_redaction, add(id));
        assert_eq!(busy["code"], "FLOW_PDF_NATIVE_REDACTION_BUSY");
    }
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
    edit["requestId"] = json!(8);
    let blocked = call(native_pdf_command, edit.clone());
    assert_eq!(blocked["code"], "FLOW_PDF_NATIVE_REDACTION_BUSY");
    status_only(&blocked);
    let published = call(native_pdf_redaction, publish.clone());
    assert_eq!(published["status"], "published", "{published}");
    assert_eq!(published["snapshot"]["binding"]["revision"], 1);
    assert_eq!(published["snapshot"]["consumedRequest"], 7);
    assert_eq!(published["snapshot"]["outputHash"], candidate["hash"]);
    assert_ne!(published["snapshot"]["outputHex"], base["outputHex"]);
    let status_wire = native_pdf_redaction(lookup(candidate.clone()).to_string());
    assert!(verify_native_pdf_worker_response(status_wire.clone()));
    let published_status: Value = serde_json::from_str(&status_wire).unwrap();
    status_only(&published_status);
    assert_eq!(published_status["redaction"]["published"], true);
    let readback = call(native_pdf_redaction, read_published);
    assert_eq!(readback["snapshot"], published["snapshot"]);
    assert_eq!(readback["redaction"]["action"], "readPublished");
    assert_eq!(call(native_pdf_redaction, publish.clone()), published);
    let mut stale = request(7, json!({"kind":"publish","candidate":candidate}));
    stale["acceptedHash"] = json!(vec![0; 32]);
    assert_eq!(call(native_pdf_redaction, stale)["status"], "failed");

    let mut consumed = add(6);
    consumed["binding"] = published["snapshot"]["binding"].clone();
    consumed["acceptedHash"] = published["snapshot"]["outputHash"].clone();
    let refused = call(native_pdf_redaction, consumed);
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_REQUEST_INVALID");
    status_only(&refused);

    edit["binding"] = published["snapshot"]["binding"].clone();
    edit["outputHash"] = published["snapshot"]["outputHash"].clone();
    let advanced = call(native_pdf_command, edit);
    assert_eq!(advanced["status"], "applied", "{advanced}");
    assert_eq!(advanced["snapshot"]["binding"]["revision"], 2);
    assert!(!verify_native_pdf_worker_response(status_wire.clone()));
    assert!(verify_native_pdf_response(status_wire));
    let refused = call(native_pdf_redaction, publish);
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_REVISION_MISMATCH");
    status_only(&refused);

    let current = &advanced["snapshot"];
    let empty = native_pdf_redaction(
        redaction_request(9, current, json!({"kind":"status","candidate":null})).to_string(),
    );
    assert!(verify_native_pdf_worker_response(empty.clone()));
    let different = rehash_with_replacement(
        &empty,
        &format!(r#""acceptedHash":{}"#, current["outputHash"]),
        &format!(r#""acceptedHash":{}"#, base["outputHash"]),
    );
    assert!(verify_native_pdf_response(different.clone()));
    assert!(!verify_native_pdf_worker_response(different));
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
    let request = |action| native_pdf_redaction(redaction_request(3, base, action).to_string());
    let staged_wire = request(json!({"kind":"stage"}));
    assert!(verify_native_pdf_worker_response(staged_wire.clone()));
    let staged: Value = serde_json::from_str(&staged_wire).unwrap();
    status_only(&staged);
    let candidate = &staged["redaction"]["candidate"];
    let verified_wire = request(json!({"kind":"verify","candidate":candidate}));
    assert!(!verify_native_pdf_worker_response(staged_wire));
    assert!(verify_native_pdf_worker_response(verified_wire.clone()));
    let verified: Value = serde_json::from_str(&verified_wire).unwrap();
    assert_eq!(verified["redaction"]["verification"]["status"], "passed");
    status_only(&verified);
    let status_wire = request(json!({"kind":"status","candidate":candidate}));
    assert!(verify_native_pdf_worker_response(status_wire.clone()));
    assert!(verify_native_pdf_worker_response(verified_wire.clone()));

    for (field, accepted, other) in [
        ("requestId", json!(3), json!(4)),
        (
            "sessionId",
            base["binding"]["sessionId"].clone(),
            json!(base["binding"]["sessionId"].as_u64().unwrap() + 1),
        ),
        ("revision", json!(0), json!(1)),
        (
            "originStoreHash",
            base["originStoreHash"].clone(),
            json!("00".repeat(32)),
        ),
        ("markCount", json!(1), json!(2)),
        ("hash", candidate["hash"].clone(), json!(vec![0; 32])),
    ] {
        let different = rehash_with_replacement(
            &verified_wire,
            &format!(r#""{field}":{accepted}"#),
            &format!(r#""{field}":{other}"#),
        );
        assert!(verify_native_pdf_response(different.clone()), "{field}");
        assert!(!verify_native_pdf_worker_response(different), "{field}");
    }
    let different = rehash_with_replacement(
        &verified_wire,
        r#""verification":{"status":"passed""#,
        r#""verification":{"status":"failed""#,
    );
    assert!(verify_native_pdf_response(different.clone()));
    assert!(!verify_native_pdf_worker_response(different));

    let cancelled_wire = request(json!({"kind":"cancel","candidate":candidate}));
    assert!(verify_native_pdf_worker_response(cancelled_wire.clone()));
    assert!(!verify_native_pdf_worker_response(verified_wire.clone()));
    assert!(!verify_native_pdf_worker_response(status_wire));
    let cancelled: Value = serde_json::from_str(&cancelled_wire).unwrap();
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
    let unseen_wire = native_pdf_snapshot(bound("snapshot", 4, base).to_string());
    assert!(verify_native_pdf_worker_response(unseen_wire.clone()));
    let unseen: Value = serde_json::from_str(&unseen_wire).unwrap();
    assert_eq!(unseen["status"], "unseen");
    assert_eq!(unseen["snapshot"], *base);
    let different = rehash_with_replacement(&unseen_wire, r#""requestId":4,"#, r#""requestId":3,"#);
    assert!(verify_native_pdf_response(different.clone()));
    assert!(!verify_native_pdf_worker_response(different));
    let next = start(4, json!({"kind":"stage"}));
    status_only(&next);
    assert_eq!(next["redaction"]["markCount"], 1);
    assert_eq!(next["redaction"]["verification"]["status"], "pending");
    assert!(!verify_native_pdf_worker_response(cancelled_wire));
    let next_cancelled = native_pdf_redaction(
        redaction_request(
            4,
            base,
            json!({"kind":"cancel","candidate":next["redaction"]["candidate"]}),
        )
        .to_string(),
    );
    assert!(verify_native_pdf_worker_response(next_cancelled.clone()));
    assert_eq!(
        call(native_pdf_close, bound("close", 5, base))["status"],
        "closed"
    );
    assert!(!verify_native_pdf_worker_response(next_cancelled));
    assert!(!verify_native_pdf_worker_response(verified_wire.clone()));
    std::thread::spawn(move || assert!(verify_native_pdf_response(verified_wire)))
        .join()
        .unwrap();
}

#[test]
fn redaction_wire_preserves_page_geometry_refusal_codes() {
    for (crop, code) in [
        ("/CropBox [-1 0 100 100]", "FLOW_PDF_NATIVE_UNSUPPORTED"),
        ("/CropBox null", "FLOW_PDF_NATIVE_GRAPH_MALFORMED"),
    ] {
        let source = fixture_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 100 100] >>",
            "<< /Type /Page /Parent 2 0 R >>",
            &format!("<< /Type /Page /Parent 2 0 R {crop} >>"),
        ]);
        let opened = call(native_pdf_open, open_request(source.as_bytes()));
        assert_eq!(opened["status"], "opened", "{opened}");
        let base = &opened["snapshot"];
        assert_eq!(base["redactionAdmission"], "unavailable");
        let page = &base["pageState"]["order"][0];
        for action in [
            json!({"kind":"addMark","pageId":page,"rect":[12000,12000,13000,13000]}),
            json!({"kind":"stage"}),
        ] {
            let refused = call(native_pdf_redaction, redaction_request(2, base, action));
            assert_eq!(refused["status"], "failed", "{refused}");
            assert_eq!(refused["code"], code);
            assert!(refused["redaction"].is_null());
            status_only(&refused);
        }

        let unchanged = call(native_pdf_snapshot, bound("snapshot", 2, base));
        assert_eq!(unchanged["status"], "unseen");
        assert_eq!(unchanged["snapshot"], *base);
        let status = call(
            native_pdf_redaction,
            redaction_request(3, base, json!({"kind":"status","candidate":null})),
        );
        assert_eq!(status["redaction"]["markCount"], 0, "{status}");
        assert!(status["redaction"]["candidate"].is_null());
        status_only(&status);
    }
}

#[test]
fn redaction_wire_refuses_marks_on_oversized_pages() {
    let source = redaction_pdf(3_000);
    let opened = call(native_pdf_open, open_request(source.as_bytes()));
    assert_eq!(opened["status"], "opened", "{opened}");
    let base = &opened["snapshot"];
    assert_eq!(base["redactionAdmission"], "unavailable");
    let page = &base["pageState"]["order"][0];
    for action in [
        json!({"kind":"addMark","pageId":page,"rect":[12000,12000,13000,13000]}),
        json!({"kind":"stage"}),
    ] {
        let refused = call(native_pdf_redaction, redaction_request(2, base, action));
        assert_eq!(refused["status"], "failed", "{refused}");
        assert_eq!(refused["code"], "FLOW_PDF_NATIVE_BUDGET_EXHAUSTED");
        assert!(refused["redaction"].is_null());
        status_only(&refused);
    }

    let unchanged = call(native_pdf_snapshot, bound("snapshot", 2, base));
    assert_eq!(unchanged["status"], "unseen");
    assert_eq!(unchanged["snapshot"], *base);
    let status = call(
        native_pdf_redaction,
        redaction_request(3, base, json!({"kind":"status","candidate":null})),
    );
    assert_eq!(status["redaction"]["markCount"], 0, "{status}");
    status_only(&status);
}

#[test]
fn redaction_wire_retains_marks_when_only_some_checks_pass() {
    // @contract: The retained polygon exceeds the raster paint budget before sampling.
    let mut paint = String::from("10 10 10 10 re f 40 616 m");
    for step in -23..=24 {
        write!(paint, " {} {} l", 1_000 + 40 * step, 40 + step * step).unwrap();
    }
    paint.push_str(" 1960 1960 l 40 1960 l h f");
    let source = redaction_pdf_with_paint(1_980, &paint);
    let opened = call(native_pdf_open, open_request(source.as_bytes()));
    assert_eq!(opened["status"], "opened", "{opened}");
    let base = &opened["snapshot"];
    assert_eq!(base["redactionAdmission"], "supported");
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
    assert_eq!(added["redaction"]["markCount"], 1, "{added}");
    let staged = call(native_pdf_redaction, request(json!({"kind":"stage"})));
    status_only(&staged);
    let candidate = &staged["redaction"]["candidate"];
    let snapshot = || call(native_pdf_snapshot, bound("snapshot", 4, base));
    let before_verification = snapshot();
    assert_eq!(before_verification["snapshot"], *base);
    let verify = request(json!({"kind":"verify","candidate":candidate}));
    let verdict = call(native_pdf_redaction, verify.clone());
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
    assert_eq!(snapshot(), before_verification);
    assert_eq!(call(native_pdf_redaction, verify), verdict);
    let publish = request(json!({"kind":"publish","candidate":candidate}));
    let refused = call(native_pdf_redaction, publish.clone());
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_REDACTION_UNVERIFIED");
    status_only(&refused);
    assert_eq!(snapshot(), before_verification);
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
    let cleared = call(
        native_pdf_redaction,
        redaction_request(4, base, json!({"kind":"clearMarks"})),
    );
    status_only(&cleared);
    assert_eq!(cleared["redaction"]["markCount"], 0);
    assert_eq!(snapshot(), before_verification);
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
    let closed_wire = native_pdf_close(close.to_string());
    assert!(verify_native_pdf_worker_response(closed_wire.clone()));
    let closed: Value = serde_json::from_str(&closed_wire).unwrap();
    assert_eq!(closed["status"], "closed");
    assert_eq!(native_pdf_close(close.to_string()), closed_wire);
    for (field, other) in [
        ("requestId", json!(7)),
        ("revision", json!(0)),
        ("originStoreHash", json!("00".repeat(32))),
    ] {
        let original = if field == "revision" {
            &closed["binding"][field]
        } else {
            &closed[field]
        };
        let different = rehash_with_replacement(
            &closed_wire,
            &format!(r#""{field}":{original}"#),
            &format!(r#""{field}":{other}"#),
        );
        assert!(verify_native_pdf_response(different.clone()), "{field}");
        assert!(!verify_native_pdf_worker_response(different), "{field}");
    }
    std::thread::spawn(move || {
        assert!(verify_native_pdf_response(closed_wire.clone()));
        assert!(!verify_native_pdf_worker_response(closed_wire));
    })
    .join()
    .unwrap();
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
            let accepted_wire = native_pdf_snapshot(bound("snapshot", 2, &initial).to_string());
            assert!(verify_native_pdf_worker_response(accepted_wire.clone()));
            let accepted: Value = serde_json::from_str(&accepted_wire).unwrap();
            assert_eq!(accepted["status"], "accepted");
            assert_eq!(accepted["snapshot"], current);
            let different =
                rehash_with_replacement(&accepted_wire, r#""requestId":2,"#, r#""requestId":1,"#);
            assert!(verify_native_pdf_response(different.clone()));
            assert!(!verify_native_pdf_worker_response(different));
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
    let evicted_wire = native_pdf_snapshot(bound("snapshot", 2, &initial).to_string());
    assert!(verify_native_pdf_worker_response(evicted_wire.clone()));
    let reconciliation: Value = serde_json::from_str(&evicted_wire).unwrap();
    assert_eq!(reconciliation["status"], "evicted");
    assert_eq!(reconciliation["snapshot"], current);
    for (original, other) in [
        (r#""status":"evicted""#, r#""status":"accepted""#),
        (r#""requestId":2,"#, r#""requestId":3,"#),
    ] {
        let different = rehash_with_replacement(&evicted_wire, original, other);
        assert!(verify_native_pdf_response(different.clone()), "{other}");
        assert!(!verify_native_pdf_worker_response(different), "{other}");
    }
    std::thread::spawn(move || assert!(verify_native_pdf_response(evicted_wire)))
        .join()
        .unwrap();
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
    let request = open_request(pdf(612).as_bytes());
    let dropped_reply = native_pdf_open(request.to_string());
    let recovered = native_pdf_open(request.to_string());
    assert_eq!(recovered, dropped_reply);
    assert!(verify_native_pdf_worker_response(recovered.clone()));

    let mut other_epoch = request.clone();
    other_epoch["epoch"] = json!(vec![2; 16]);
    let second = call(native_pdf_open, other_epoch.clone());
    assert_eq!(second["status"], "opened", "{second}");
    assert_eq!(second["binding"]["epoch"], other_epoch["epoch"]);
    assert_eq!(native_pdf_open(request.to_string()), recovered);
    assert!(!verify_native_pdf_worker_response(rehash_with_replacement(
        &recovered,
        r#""requestId":1,"#,
        r#""requestId":3,"#,
    )));
    let first: Value = serde_json::from_str(&recovered).unwrap();
    assert_eq!(first["status"], "opened", "{first}");
    assert_ne!(
        first["binding"]["sessionId"],
        second["binding"]["sessionId"]
    );
    let mut other_request = request.clone();
    other_request["requestId"] = json!(3);
    let refused = call(native_pdf_open, other_request);
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_BUDGET_EXHAUSTED");
    assert!(refused["snapshot"].is_null());
    let close = bound("close", 2, &first["snapshot"]);
    assert_eq!(call(native_pdf_close, close.clone())["status"], "closed");
    assert_eq!(call(native_pdf_close, close)["status"], "closed");
    assert!(!verify_native_pdf_worker_response(recovered));
    let reopened = call(native_pdf_open, request);
    assert_eq!(reopened["status"], "opened", "{reopened}");
    assert_ne!(reopened["binding"], first["binding"]);
    let unchanged = call(
        native_pdf_snapshot,
        bound("snapshot", 3, &second["snapshot"]),
    );
    assert_eq!(unchanged["status"], "unseen");
    assert_eq!(unchanged["snapshot"], second["snapshot"]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn open_result_encoding_refusal_preserves_both_existing_binding_and_free_slot() {
    let (first_request, first) = open();
    let mut second_request = first_request.clone();
    second_request["requestId"] = json!(2);
    for request in [&second_request, &first_request] {
        let limit = request["bytesHex"].as_str().unwrap().len();
        let response = native_pdf_open_with_result_limit(request.to_string(), limit);
        assert!(verify_native_pdf_worker_response(response.clone()));
        let failed: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(failed["operation"], "open");
        assert_eq!(failed["requestId"], request["requestId"]);
        assert_eq!(failed["originStoreHash"], request["originStoreHash"]);
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["code"], "FLOW_PDF_NATIVE_BUDGET_EXHAUSTED");
        assert!(failed["binding"].is_null());
        assert!(failed["snapshot"].is_null());
    }

    let second = call(native_pdf_open, second_request);
    assert_eq!(second["status"], "opened", "{second}");
    assert_ne!(second["binding"], first["binding"]);
    assert_eq!(call(native_pdf_open, first_request)["snapshot"], first);
}

#[test]
fn open_retry_after_revision_advance_requires_reconciliation() {
    let (request, initial) = open();
    let mut edit = bound("command", 2, &initial);
    edit["outputHash"] = initial["outputHash"].clone();
    edit["command"] = json!({"kind":"add","page":0,"rect":[1,2,3,4],"contents":"Retained"});
    let applied = call(native_pdf_command, edit);
    assert_eq!(applied["status"], "applied", "{applied}");

    let retry = call(native_pdf_open, request);
    assert_eq!(retry["code"], "FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED");
    assert!(retry["binding"].is_null());
    assert!(retry["snapshot"].is_null());
    let lookup = call(native_pdf_snapshot, bound("snapshot", 2, &initial));
    assert_eq!(lookup["status"], "accepted");
    assert_eq!(lookup["snapshot"], applied["snapshot"]);
}

fn embedded_text_pdf(artwork: &[u8], page_size: &str) -> Vec<u8> {
    fn stream(bytes: &[u8], extra: &str) -> Vec<u8> {
        let mut data = format!("<< /Length {} {extra} >>\nstream\n", bytes.len()).into_bytes();
        data.extend_from_slice(bytes);
        data.extend_from_slice(b"\nendstream");
        data
    }
    let font = include_bytes!("../../flow-core/data/NotoSans-Regular.ttf");
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapName /Fixture-Latin def /CMapType 2 def\n1 begincodespacerange <00> <FF> endcodespacerange\n1 beginbfchar <41> <0041> endbfchar\nendcmap CMapName currentdict /CMap defineresource pop end end";
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_size}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>").into_bytes(),
        stream(artwork, ""),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /NotoSans-Regular /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 65 /Widths [639] /FontDescriptor 6 0 R /ToUnicode 7 0 R >>".to_vec(),
        b"<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 /FontBBox [-621 -389 2800 1067] /ItalicAngle 0 /Ascent 1069 /Descent -293 /CapHeight 714 /StemV 80 /FontFile2 8 0 R >>".to_vec(),
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
fn accepted_preview_preserves_fine_text_and_declines_ambiguous_paint() {
    let artwork = "BT /F1 4 Tf 2 6 Td (AAA) Tj ET\nBT /F1 0.2 Tf 2.03 2.03 Td (A) Tj ET";
    for (extra, available) in [("", true), ("\n0 0 1 rg 2.04 2.04 0.09 0.09 re f", false)] {
        let content = format!("{artwork}{extra}");
        let source = embedded_text_pdf(content.as_bytes(), "12 12");
        let opened = call(native_pdf_open, open_request(&source));
        assert_eq!(opened["status"], "opened", "{opened}");
        let base = &opened["snapshot"];
        let reply = call(
            native_pdf_preview,
            preview_request(2, base, base["pageState"]["order"][0].clone()),
        );
        if available {
            assert_eq!(reply["status"], "preview", "{reply}");
            let raster = &reply["preview"];
            assert_eq!(raster["outputHash"], base["outputHash"]);
            assert_eq!(raster["width"], 24);
            assert_eq!(raster["height"], 24);
            let rgb = raster["rgbHex"].as_str().unwrap();
            let pixel = (19 * 24 + 4) * 6;
            assert_eq!(&rgb[pixel..pixel + 6], "808080");
            assert!(
                rgb.as_bytes()[..12 * 24 * 6]
                    .chunks_exact(6)
                    .any(|pixel| pixel != b"ffffff")
            );
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
fn serialized_text_replacement_checks_capability_revision_and_output_identity() {
    let bytes = embedded_text_pdf(b"BT /F1 12 Tf 20 100 Td (AAA) Tj ET", "612 792");
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
    assert_eq!(applied["status"], "applied", "code: {}", applied["code"]);
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
    assert_eq!(current["consumedRequest"], 2);
    assert_eq!(
        call(native_pdf_command, command.clone())["snapshot"],
        *current
    );
    let readback = call(native_pdf_snapshot, bound("snapshot", 2, &original));
    assert_eq!(readback["status"], "accepted");
    assert_eq!(readback["snapshot"], *current);
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

    let response = native_pdf_open(
        open_request(&embedded_text_pdf(
            b"BT /F1 12 Tf 20 100 Td (AAA) Tj ET",
            "612 792",
        ))
        .to_string(),
    );
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

#[test]
fn serialized_object_operations_keep_the_supported_order_at_the_graphics_limit() {
    let paint = format!(
        "{}10 10 10 10 re f q 25 10 10 10 re f q 40 10 10 10 re f 10 0 0 10 60 10 cm /I Do\n{}",
        "q\n".repeat(30),
        "Q\n".repeat(32),
    );
    let source = fixture_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /XObject << /I 5 0 R >> >> >>",
        &format!("<< /Length {} >>\nstream\n{paint}\nendstream", paint.len()),
        "<< /Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8 /Length 1 >>\nstream\nA\nendstream",
    ]);
    let opened = call(native_pdf_open, open_request(source.as_bytes()));
    assert_eq!(opened["status"], "opened", "{opened}");
    let snapshot = &opened["snapshot"];
    let objects = &snapshot["scene"]["objectPages"][0]["objects"];
    assert_eq!(
        objects[0]["operations"],
        json!(["replace", "move", "remove"])
    );
    assert_eq!(objects[1]["operations"], json!(["move", "remove"]));
    assert_eq!(objects[2]["operations"], json!(["remove"]));
    assert_eq!(objects[3]["operations"], json!(["replace", "remove"]));
    let target = |object: &Value| json!({"page":0,"id":object["id"],"profile":object["profile"]});
    let refused = page_edit(
        snapshot,
        2,
        json!({"kind":"moveObject","target":target(&objects[3]),"transform":[1,0,0,1,1,0]}),
    );
    assert_eq!(refused["status"], "failed");
    assert_eq!(refused["code"], "FLOW_PDF_NATIVE_OBJECT_STATE");
    let current = call(native_pdf_snapshot, bound("snapshot", 2, snapshot));
    assert_eq!(current["status"], "unseen");
    assert_eq!(&current["snapshot"], snapshot);

    let asset = concat!(
        "89504e470d0a1a0a0000000d49484452000000010000000108000000003a7e9b55",
        "0000000a49444154789c63880600005d005cdc83ebd80000000049454e44ae426082"
    );
    let replaced = page_edit(
        snapshot,
        2,
        json!({"kind":"replaceImage","target":target(&objects[3]),"assetHex":asset}),
    );
    assert_eq!(replaced["status"], "applied", "{replaced}");
    let snapshot = &replaced["snapshot"];
    let after = &snapshot["scene"]["objectPages"][0]["objects"];
    assert_eq!(after[3]["bounds"], objects[3]["bounds"]);
    assert_eq!(after[3]["operations"], objects[3]["operations"]);
    let removed = page_edit(
        snapshot,
        3,
        json!({"kind":"removeObject","target":target(&after[2])}),
    );
    assert_eq!(removed["status"], "applied", "{removed}");
    let after = removed["snapshot"]["scene"]["objectPages"][0]["objects"]
        .as_array()
        .unwrap();
    assert_eq!(after.len(), 3);
    assert_eq!(after[2]["operations"], json!(["replace", "remove"]));
}

#[test]
fn serialized_object_operations_keep_valid_actions_when_inverse_is_unavailable() {
    let paint = "10 10 10 10 re f q 0.001 0 0 1 1100 10 cm 0 0 10000 10 re f Q q 0.001 0 0 10 1120 10 cm /I Do Q";
    let source = fixture_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1600 200] /Contents 4 0 R /Resources << /XObject << /I 5 0 R >> >> >>",
        &format!("<< /Length {} >>\nstream\n{paint}\nendstream", paint.len()),
        "<< /Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8 /Length 1 >>\nstream\nA\nendstream",
    ]);
    let opened = call(native_pdf_open, open_request(source.as_bytes()));
    assert_eq!(opened["status"], "opened");
    let snapshot = &opened["snapshot"];
    let objects = &snapshot["scene"]["objectPages"][0]["objects"];
    assert_eq!(
        objects[0]["operations"],
        json!(["replace", "move", "remove"])
    );
    assert_eq!(objects[1]["operations"], json!(["remove"]));
    assert_eq!(objects[2]["operations"], json!(["replace", "remove"]));
    let target = |object: &Value| json!({"page":0,"id":object["id"],"profile":object["profile"]});
    for object in [&objects[1], &objects[2]] {
        let refused = page_edit(
            snapshot,
            2,
            json!({
                "kind":"moveObject","target":target(object),"transform":[1,0,0,1,1,0]
            }),
        );
        assert_eq!(refused["code"], "FLOW_PDF_NATIVE_OBJECT_GEOMETRY");
    }
    let current = call(native_pdf_snapshot, bound("snapshot", 2, snapshot));
    assert_eq!(current["status"], "unseen");
    assert_eq!(&current["snapshot"], snapshot);
    let asset = concat!(
        "89504e470d0a1a0a0000000d49484452000000010000000108000000003a7e9b55",
        "0000000a49444154789c63880600005d005cdc83ebd80000000049454e44ae426082"
    );
    let replaced = page_edit(
        snapshot,
        2,
        json!({
            "kind":"replaceImage","target":target(&objects[2]),"assetHex":asset
        }),
    );
    assert_eq!(replaced["status"], "applied");
    let snapshot = &replaced["snapshot"];
    let after = &snapshot["scene"]["objectPages"][0]["objects"];
    assert_eq!(after[2]["bounds"], objects[2]["bounds"]);
    assert_eq!(after[2]["operations"], objects[2]["operations"]);
    let removed = page_edit(
        snapshot,
        3,
        json!({
            "kind":"removeObject","target":target(&after[1])
        }),
    );
    assert_eq!(removed["status"], "applied");
    let remaining = &removed["snapshot"]["scene"]["objectPages"][0]["objects"];
    assert_eq!(remaining.as_array().unwrap().len(), 2);
    assert_eq!(remaining[1]["bounds"], objects[2]["bounds"]);
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

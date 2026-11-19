use flow_core::pdf::native::{
    NativeEditLimits, NativeObjectCommand, NativePdfSession, NativeRedactionMark,
    NativeSessionError,
};
use flow_core::pdf::{PdfSceneElement, PdfSceneRequest, open_pdf, read_pdf_scene};
use serde_json::{Value, json};

fn fixture(contents: &[u8], shared: bool, resources: &str, extras: &[Vec<u8>]) -> Vec<u8> {
    fixture_with_slots(contents, shared, resources, extras, "5 0 R")
}

fn fixture_with_slots(
    contents: &[u8],
    shared: bool,
    resources: &str,
    extras: &[Vec<u8>],
    slots: &str,
) -> Vec<u8> {
    let kids = if shared { "3 0 R 4 0 R" } else { "3 0 R" };
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents {slots} {resources} >>"
    );
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [{kids}] /Count {} >>",
            if shared { 2 } else { 1 }
        )
        .into_bytes(),
        page.as_bytes().to_vec(),
        page.as_bytes().to_vec(),
        stream("", contents),
    ];
    objects.extend_from_slice(extras);
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    let size = objects.len() + 1;
    bytes.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer\n<< /Root 1 0 R /Size {size} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut bytes = format!("<< /Length {} {dict} >>\nstream\n", data.len()).into_bytes();
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(b"\nendstream");
    bytes
}

fn open(bytes: &[u8]) -> NativePdfSession {
    NativePdfSession::open_scoped(bytes, NativeEditLimits::default(), [7; 16]).unwrap()
}

fn apply(session: &mut NativePdfSession, command: Value) -> Result<(), NativeSessionError> {
    let binding = session.snapshot().binding;
    let command: NativeObjectCommand = serde_json::from_value(command).unwrap();
    session
        .apply_object(binding, binding.revision + 1, &command)
        .map(|_| ())
}

fn objects(session: &NativePdfSession, page: usize) -> Value {
    serde_json::to_value(&session.snapshot().scene.object_pages[page].objects).unwrap()
}

fn target(session: &NativePdfSession, page: usize, index: usize) -> Value {
    let view = &objects(session, page)[index];
    json!({"page":page,"id":view["id"],"profile":view["profile"]})
}

#[test]
fn path_crud_isolates_shared_content_and_roundtrips() {
    let source = fixture(b"10 20 30 40 re f\n", true, "", &[]);
    let mut session = open(&source);
    let original = target(&session, 0, 0);
    apply(
        &mut session,
        json!({"kind":"moveObject","target":original,"transform":[0,2,-1,0,100,0]}),
    )
    .unwrap();
    assert_eq!(
        objects(&session, 0)[0]["bounds"],
        json!([40000, 20000, 80000, 80000])
    );
    assert_eq!(
        objects(&session, 1)[0]["bounds"],
        json!([10000, 20000, 40000, 60000])
    );
    let before = target(&session, 0, 0);
    let line = json!({"shape":"line","points":[[20000,20000],[50000,20000]],"stroke":[255,0,0],"fill":null,"strokeWidth":2000});
    apply(
        &mut session,
        json!({"kind":"replaceGraphic","target":before,"graphic":line}),
    )
    .unwrap();
    assert_eq!(objects(&session, 0)[0]["profile"], "line");
    let before = target(&session, 0, 0);
    apply(&mut session, json!({"kind":"removeObject","target":before})).unwrap();
    assert_eq!(objects(&session, 0), json!([]));
    let polygon = json!({"shape":"polygon","points":[[20000,20000],[50000,20000],[50000,50000]],"stroke":null,"fill":[0,0,0],"strokeWidth":0});
    apply(
        &mut session,
        json!({"kind":"addGraphic","page":0,"graphic":polygon}),
    )
    .unwrap();
    let reopened = open(session.output_bytes());
    assert_eq!(objects(&reopened, 0)[0]["profile"], "polygon");
    assert_eq!(
        objects(&reopened, 1)[0]["bounds"],
        json!([10000, 20000, 40000, 60000])
    );
    assert_eq!(session.origin_bytes(), source);
}

#[test]
fn added_graphic_paints_after_shared_streams_in_initial_graphics_state() {
    let first = b"1 0 0 1 20 0 cm 0 1 0 rg 0 0 20 20 re f\n";
    let second = b"5 5 10 10 re f";
    let source = fixture_with_slots(first, true, "", &[stream("", second)], "[5 0 R 6 0 R]");
    let mut session = open(&source);
    let before = objects(&session, 0);
    let graphic = json!({
        "shape":"rectangle", "points":[[25000,5000],[35000,15000]],
        "stroke":null, "fill":[255,0,0], "strokeWidth":0
    });
    apply(
        &mut session,
        json!({"kind":"addGraphic","page":0,"graphic":graphic}),
    )
    .unwrap();

    let reopened = open(session.output_bytes());
    let edited = objects(&reopened, 0);
    assert_eq!(edited.as_array().unwrap().len(), 3);
    assert_eq!(edited[0]["bounds"], before[0]["bounds"]);
    assert_eq!(edited[1]["bounds"], before[1]["bounds"]);
    assert_eq!(edited[2]["bounds"], json!([25000, 5000, 35000, 15000]));
    assert_eq!(objects(&reopened, 1)[1]["bounds"], before[1]["bounds"]);
    for original in [first.as_slice(), second.as_slice()] {
        let body = [b"stream\n".as_slice(), original, b"\nendstream"].concat();
        assert_eq!(
            reopened
                .output_bytes()
                .windows(body.len())
                .filter(|bytes| *bytes == body.as_slice())
                .count(),
            1
        );
    }
}

#[test]
fn topmost_insertion_keeps_redaction_admission_after_translation_and_scale() {
    let transform = b"3 0 0 3 12 6 cm\n";
    let paint = b"0 1 0 rg 2 2 5 5 re f";
    let source = fixture_with_slots(transform, false, "", &[stream("", paint)], "[5 0 R 6 0 R]");
    let mut session = open(&source);
    assert!(session.redaction_admitted());
    let graphic = json!({
        "shape":"rectangle", "points":[[50000,50000],[60000,60000]],
        "stroke":null, "fill":[255,0,0], "strokeWidth":0
    });
    apply(
        &mut session,
        json!({"kind":"addGraphic","page":0,"graphic":graphic}),
    )
    .unwrap();
    assert!(session.redaction_admitted());
    let edited = objects(&session, 0);
    assert_eq!(edited.as_array().unwrap().len(), 2);
    assert_eq!(edited[0]["bounds"], json!([18000, 12000, 33000, 27000]));
    assert_eq!(edited[1]["bounds"], json!([50000, 50000, 60000, 60000]));
    for original in [transform.as_slice(), paint.as_slice()] {
        let body = [b"stream\n".as_slice(), original, b"\nendstream"].concat();
        assert!(
            session
                .output_bytes()
                .windows(body.len())
                .any(|bytes| bytes == body)
        );
    }
    let binding = session.snapshot().binding;
    let page_id = session.snapshot().page_state.order[0];
    let geometry = session.redaction_mark_geometry(page_id).unwrap();
    session
        .add_redaction_mark(
            binding,
            NativeRedactionMark {
                page_id,
                geometry,
                rect: [50_000, 50_000, 60_000, 60_000],
            },
        )
        .unwrap();
    session.stage_redaction(binding, 2).unwrap();
    let evidence = session.verify_redaction(binding, 2).unwrap();
    assert!(evidence.checks_passed(), "{evidence:?}");
    session.publish_redaction(binding, 2).unwrap();
    assert_eq!(session.origin_bytes(), source);
}

#[test]
fn nested_shared_image_invocations_are_isolated() {
    let resources = "/Resources << /XObject << /Fm 6 0 R >> >>";
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 40 40] /Resources << /XObject << /I 7 0 R >> >>",
        b"q 10 0 0 10 10 10 cm /I Do Q",
    );
    let pixels = miniz_oxide::deflate::compress_to_vec_zlib(&[7], 6);
    let image = stream(
        "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8 /Filter /FlateDecode",
        &pixels,
    );
    let source = fixture(
        b"/Fm Do q 1 0 0 1 60 0 cm /Fm Do Q",
        true,
        resources,
        &[form, image],
    );
    let mut session = open(&source);
    let selected = target(&session, 0, 1);
    apply(
        &mut session,
        json!({"kind":"moveObject","target":selected,"transform":[1,0,0,1,5,0]}),
    )
    .unwrap();
    assert_eq!(
        objects(&session, 0)[1]["bounds"],
        json!([75000, 10000, 85000, 20000])
    );
    let selected = target(&session, 0, 0);
    apply(
        &mut session,
        json!({"kind":"removeObject","target":selected}),
    )
    .unwrap();
    assert_eq!(objects(&session, 0).as_array().unwrap().len(), 1);
    assert_eq!(
        objects(&session, 1)[0]["bounds"],
        json!([10000, 10000, 20000, 20000])
    );
    assert_eq!(
        objects(&session, 1)[1]["bounds"],
        json!([70000, 10000, 80000, 20000])
    );
}

#[test]
fn invalid_targets_geometry_and_assets_are_atomic() {
    let source = fixture(b"10 20 30 40 re f", true, "", &[]);
    let mut session = open(&source);
    let selected = target(&session, 0, 0);
    let mut forged = selected.clone();
    forged["page"] = json!(1);
    for invalid in [
        json!({"kind":"removeObject","target":forged}),
        json!({"kind":"moveObject","target":selected,"transform":[1,1,0,1,0,0]}),
        json!({"kind":"moveObject","target":selected,"transform":[1,0,0,1,200,0]}),
        json!({"kind":"addImage","page":0,"bounds":[1000,1000,5000,5000],"assetHex":"0000"}),
    ] {
        assert!(apply(&mut session, invalid).is_err());
        assert_eq!(session.snapshot().binding.revision, 0);
        assert_eq!(session.output_bytes(), source);
    }
    let clipped = open(&fixture(
        b"0 0 100 100 re W n 10 20 30 40 re f",
        false,
        "",
        &[],
    ));
    assert!(!clipped.snapshot().scene.object_pages[0].can_add);
    assert_eq!(objects(&clipped, 0), json!([]));
}

#[test]
fn embedded_masks_and_unbalanced_graphics_state_refuse_object_editing() {
    let pixels = miniz_oxide::deflate::compress_to_vec_zlib(&[7], 6);
    let image = stream(
        "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8 /Filter /FlateDecode /Mask [0 0]",
        &pixels,
    );
    let source = fixture(
        b"q 10 0 0 10 5 5 cm /I Do Q",
        false,
        "/Resources << /XObject << /I 6 0 R >> >>",
        &[image],
    );
    let masked = open(&source);
    assert!(!masked.snapshot().scene.object_pages[0].can_add);
    assert_eq!(objects(&masked, 0), json!([]));
    let unbalanced = open(&fixture(b"q 10 10 5 5 re f", false, "", &[]));
    assert!(!unbalanced.snapshot().scene.object_pages[0].can_add);
    assert_eq!(objects(&unbalanced, 0), json!([]));
}

#[test]
fn jpeg_asset_replaces_only_one_use_of_a_shared_raster() {
    let pixels = miniz_oxide::deflate::compress_to_vec_zlib(&[7], 6);
    let image = stream(
        "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8 /Filter /FlateDecode",
        &pixels,
    );
    let source = fixture(
        b"q 10 0 0 10 5 5 cm /I Do Q\n",
        true,
        "/Resources << /XObject << /I 6 0 R >> >>",
        &[image],
    );
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode(&[30, 100, 200], 1, 1, image::ExtendedColorType::Rgb8)
        .unwrap();
    let hex: String = jpeg.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut session = open(&source);
    let selected = target(&session, 0, 0);
    apply(
        &mut session,
        json!({"kind":"replaceImage","target":selected,"assetHex":hex}),
    )
    .unwrap();
    let reopened = open(session.output_bytes());
    assert!(
        session
            .output_bytes()
            .windows(jpeg.len())
            .any(|bytes| bytes == jpeg)
    );
    assert_eq!(objects(&reopened, 0)[0]["profile"], "rasterRgb8");
    assert_eq!(objects(&reopened, 1)[0]["profile"], "rasterGray8");
    assert_eq!(
        objects(&reopened, 0)[0]["bounds"],
        objects(&reopened, 1)[0]["bounds"]
    );
    assert!(
        session
            .output_bytes()
            .windows(pixels.len())
            .any(|bytes| bytes == pixels)
    );
}

#[test]
fn display_p3_png_refuses_without_mutating_the_accepted_snapshot() {
    let header = "89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de";
    let display_p3 = "00000004634943500c0d00016e03e3ef";
    let pixels = "0000000c49444154789c63f8cfc0000003010100c9fe92ef0000000049454e44ae426082";
    let tagged = format!("{header}{display_p3}{pixels}");
    let encoded: Vec<_> = tagged
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    let decoded = image::load_from_memory_with_format(&encoded, image::ImageFormat::Png).unwrap();
    assert_eq!(decoded.into_rgb8().into_raw(), [255, 0, 0]);

    let source = fixture(b"", false, "", &[]);
    let mut session = open(&source);
    apply(
        &mut session,
        json!({"kind":"addImage","page":0,"bounds":[1000,1000,5000,5000],"assetHex":format!("{header}{pixels}")}),
    )
    .unwrap();
    let selected = target(&session, 0, 0);
    let state = |session: &NativePdfSession| {
        let snapshot = session.snapshot();
        (
            snapshot.binding,
            snapshot.output_hash,
            snapshot.scene.scene_hash,
            snapshot.consumed_request,
        )
    };
    let before = state(&session);
    let bytes = session.output_bytes().to_vec();
    let error = apply(
        &mut session,
        json!({"kind":"replaceImage","target":selected,"assetHex":tagged}),
    )
    .unwrap_err();
    assert_eq!(error.code(), "FLOW_PDF_NATIVE_OBJECT_ASSET");
    assert_eq!(state(&session), before);
    assert_eq!(session.output_bytes(), bytes);
}

#[test]
fn oversized_jpeg_frame_refuses_without_mutating_session() {
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode(&[7], 1, 1, image::ExtendedColorType::L8)
        .unwrap();
    let frame = jpeg
        .windows(2)
        .position(|bytes| bytes == [0xff, 0xc0])
        .unwrap();
    jpeg[frame + 5..frame + 9].copy_from_slice(&[0x10, 0x00, 0x10, 0x00]);
    let hex: String = jpeg.iter().map(|byte| format!("{byte:02x}")).collect();

    let source = fixture(b"", false, "", &[]);
    let mut session = open(&source);
    let error = apply(
        &mut session,
        json!({"kind":"addImage","page":0,"bounds":[1000,1000,5000,5000],"assetHex":hex}),
    )
    .unwrap_err();
    assert_eq!(error.code(), "FLOW_PDF_NATIVE_OBJECT_ASSET");
    assert_eq!(session.snapshot().binding.revision, 0);
    assert_eq!(session.output_bytes(), source);
}

#[test]
fn retained_jpeg_move_preserves_encoded_stream_and_rejects_invalid_profiles() {
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode(&[12, 34, 56, 78], 2, 2, image::ExtendedColorType::L8)
        .unwrap();
    let make_pdf = |width, color, filter, extra, bytes: &[u8]| {
        let image = stream(
            &format!(
                "/Type /XObject /Subtype /Image /ColorSpace /{color} /Width {width} /Height 2 /BitsPerComponent 8 /Filter {filter} {extra}"
            ),
            bytes,
        );
        fixture(
            b"q 10 0 0 10 5 5 cm /I Do Q",
            true,
            "/Resources << /XObject << /I 6 0 R >> >>",
            &[image],
        )
    };
    for filter in ["/DCTDecode", "[/DCTDecode]"] {
        let source = make_pdf(2, "DeviceGray", filter, "", &jpeg);
        let reader = open_pdf(&source, Default::default()).unwrap();
        let scene = read_pdf_scene(
            &reader,
            &PdfSceneRequest {
                first_page: 0,
                page_count: 1,
            },
        )
        .unwrap();
        assert!(scene.pages[0].elements.iter().any(|element| matches!(
            element,
            PdfSceneElement::Image { value }
                if value.filter == "DCTDecode" && value.media_type == "image/jpeg"
        )));
        let mut session = open(&source);
        let before = objects(&session, 1)[0]["bounds"].clone();
        let selected = target(&session, 0, 0);
        apply(
            &mut session,
            json!({"kind":"moveObject","target":selected,"transform":[1,0,0,1,5,0]}),
        )
        .unwrap();
        let reopened = open(session.output_bytes());
        assert_eq!(objects(&reopened, 0)[0]["profile"], "rasterGray8");
        assert_eq!(
            objects(&reopened, 0)[0]["bounds"],
            json!([10000, 5000, 20000, 15000])
        );
        assert_eq!(objects(&reopened, 1)[0]["bounds"], before);
        assert_eq!(
            session
                .output_bytes()
                .windows(jpeg.len())
                .filter(|bytes| *bytes == jpeg)
                .count(),
            1
        );
    }

    let ascii_hex = jpeg
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        + ">";
    for invalid in [
        make_pdf(
            2,
            "DeviceGray",
            "[/ASCIIHexDecode /DCTDecode]",
            "",
            ascii_hex.as_bytes(),
        ),
        make_pdf(3, "DeviceGray", "/DCTDecode", "", &jpeg),
        make_pdf(2, "DeviceRGB", "/DCTDecode", "", &jpeg),
        make_pdf(2, "DeviceGray", "/DCTDecode", "/Mask [0 0]", &jpeg),
        make_pdf(2, "DeviceGray", "/DCTDecode", "", &jpeg[..12]),
    ] {
        assert!(
            NativePdfSession::open_scoped(&invalid, NativeEditLimits::default(), [7; 16]).is_err()
        );
    }
}

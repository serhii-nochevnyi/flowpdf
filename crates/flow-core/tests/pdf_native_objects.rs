use flow_core::layout::LayoutUnit;
use flow_core::pdf::native::{
    NativeEditLimits, NativeNoteCommand, NativeObjectCommand, NativeObjectRefusal,
    NativePdfSession, NativeRedactionMark, NativeRequestLookup, NativeSessionError,
};
use flow_core::pdf::{
    PdfPathCommand, PdfReadDiagnosticCode, PdfReaderLimits, PdfSceneElement, PdfSceneRequest,
    open_pdf, read_pdf_scene,
};
use image::ImageDecoder;
use serde_json::{Value, json};
use std::io::Cursor;

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
fn filled_rectangles_reopen_with_a_path_or_a_precision_diagnostic() {
    let source = fixture(b"", false, "", &[]);
    for (right, expected_width) in [(30_000, Some(10 * 64)), (20_001, None)] {
        let mut session = open(&source);
        apply(
            &mut session,
            json!({"kind":"addGraphic","page":0,"graphic":{
                "shape":"rectangle","points":[[20_000,20_000],[right,30_000]],
                "stroke":null,"fill":[0,0,0],"strokeWidth":0
            }}),
        )
        .unwrap();
        let reopened = open(session.output_bytes());
        assert_eq!(
            objects(&reopened, 0)[0]["bounds"],
            json!([20_000, 20_000, right, 30_000])
        );

        let reader = open_pdf(session.output_bytes(), Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        if let Some(width) = expected_width {
            assert!(matches!(
                page.elements.as_slice(),
                [PdfSceneElement::Path { value }]
                    if value.fill && !value.stroke
                        && value.rect.x.raw() == 20 * 64 && value.rect.y.raw() == 20 * 64
                        && value.rect.width.raw() == width && value.rect.height.raw() == 10 * 64
            ));
            assert!(!page.partial && !scene.report.partial);
        } else {
            assert!(page.elements.is_empty());
            assert!(page.partial && scene.report.partial);
            assert!(matches!(page.report.diagnostics.as_slice(), [diagnostic]
                if diagnostic.code == PdfReadDiagnosticCode::PartialPage
                    && diagnostic.page_index == Some(0)
                    && diagnostic.object_number.is_some()));
        }
    }
}

#[test]
fn collapsed_fill_diagnostics_account_for_precise_clipping() {
    for (contents, page_entries, partial) in [
        ("0 0 10 10 re W n 5 5 0.001 1 re f", "", true),
        ("0 0 10 10 re W n 15 5 0.001 1 re f", "", false),
        ("0 0 10.004 10 re W n 10.004 5 0.001 1 re f", "", false),
        ("0 0 10.004 10 re W n 10.003 5 0.002 1 re f", "", true),
        ("5 5 0.001 10 re W n 4 4 5 5 re f", "", true),
        ("5 5 0.001 10 re W n 8 4 5 5 re f", "", false),
        ("5 5 0 1 re f", "", false),
        ("10.003 5 0.002 1 re f", "/CropBox [0 0 10.004 10]", true),
    ] {
        let source = fixture(contents.as_bytes(), false, page_entries, &[]);
        let reader = open_pdf(&source, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        assert!(
            page.elements
                .iter()
                .all(|element| matches!(element, PdfSceneElement::Clip { .. })),
            "{contents} {page_entries}"
        );
        assert_eq!(page.partial, partial, "{contents} {page_entries}");
        assert_eq!(scene.report.partial, partial);
        if partial {
            assert!(matches!(page.report.diagnostics.as_slice(), [diagnostic]
                if diagnostic.code == PdfReadDiagnosticCode::PartialPage
                    && diagnostic.object_number == Some(5)));
        } else {
            assert!(page.report.diagnostics.is_empty());
        }
    }
}

#[test]
fn moved_closing_paint_preserves_the_edge_and_does_not_join_the_next_path() {
    let unit = |points: i64| LayoutUnit::from_raw(points * LayoutUnit::UNITS_PER_POINT);
    for (operator, fill, partial) in [("s", false, false), ("b", true, false), ("b*", true, true)] {
        let content = format!("20 20 m 40 24 l 32 46 l {operator}\n80 80 m 90 85 l S");
        let source = fixture(content.as_bytes(), false, "", &[]);
        let mut session = open(&source);
        let selected = target(&session, 0, 0);
        apply(
            &mut session,
            json!({"kind":"moveObject","target":selected,"transform":[1,0,0,1,15,7]}),
        )
        .unwrap();

        for (bytes, dx, dy) in [(source.as_slice(), 0, 0), (session.output_bytes(), 15, 7)] {
            let reader = open_pdf(bytes, Default::default()).unwrap();
            let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
            let page = &scene.pages[0];
            let [
                PdfSceneElement::Path { value: closed },
                PdfSceneElement::Path { value: next },
            ] = page.elements.as_slice()
            else {
                panic!("expected a closed path and an independent stroke for {operator}");
            };
            assert_eq!(closed.provenance.operator.as_deref(), Some(operator));
            assert_eq!((closed.stroke, closed.fill), (true, fill));
            assert_eq!(
                closed.commands,
                [
                    PdfPathCommand::MoveTo {
                        x: unit(20 + dx),
                        y: unit(20 + dy)
                    },
                    PdfPathCommand::LineTo {
                        x: unit(40 + dx),
                        y: unit(24 + dy)
                    },
                    PdfPathCommand::LineTo {
                        x: unit(32 + dx),
                        y: unit(46 + dy)
                    },
                    PdfPathCommand::ClosePath,
                ]
            );
            assert_eq!(next.provenance.operator.as_deref(), Some("S"));
            assert!(next.stroke && !next.fill);
            assert_eq!(
                next.commands,
                [
                    PdfPathCommand::MoveTo {
                        x: unit(80),
                        y: unit(80)
                    },
                    PdfPathCommand::LineTo {
                        x: unit(90),
                        y: unit(85)
                    },
                ]
            );
            assert_eq!(page.partial, partial);
            assert_eq!(scene.report.partial, partial);
            assert_eq!(
                page.report.diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == PdfReadDiagnosticCode::PartialPage
                        && diagnostic.object_number
                            == Some(closed.provenance.object_ref.object_number)
                }),
                partial
            );
        }
    }
}

#[test]
fn scene_reports_partial_for_closing_paint_with_mixed_coordinates() {
    let source = fixture(
        b"20 20 m 1 0 0 1 15 7 cm 40 24 l 32 46 l b 80 80 m 90 85 l S",
        false,
        "",
        &[],
    );
    let reader = open_pdf(&source, Default::default()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    let page = &scene.pages[0];
    assert!(page.partial && scene.report.partial);
    assert!(page.report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == PdfReadDiagnosticCode::PartialPage && diagnostic.object_number == Some(5)
    }));
    assert!(matches!(
        page.elements.as_slice(),
        [PdfSceneElement::Path { value }]
            if value.provenance.operator.as_deref() == Some("S")
                && value.commands.len() == 2
                && value.rect.x.raw() == 95 * 64 && value.rect.y.raw() == 87 * 64
    ));
}

#[test]
fn scene_reports_partial_for_closing_stroke_under_a_content_clip() {
    let source = fixture(
        b"q 10 10 30 30 re W* b 30 30 m 50 30 l 50 50 l s Q 80 80 10 10 re f",
        false,
        "",
        &[],
    );
    let reader = open_pdf(&source, Default::default()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    let page = &scene.pages[0];
    assert!(page.partial && scene.report.partial);
    assert!(page.report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == PdfReadDiagnosticCode::PartialPage && diagnostic.object_number == Some(5)
    }));
    assert!(matches!(
        page.elements.as_slice(),
        [
            PdfSceneElement::Path { value: first },
            PdfSceneElement::Clip { value: clip },
            PdfSceneElement::Path { value: clipped },
            PdfSceneElement::Path { value: next },
        ] if first.provenance.operator.as_deref() == Some("b")
            && first.commands.len() == 5
            && clip.even_odd
            && clipped.provenance.operator.as_deref() == Some("s")
            && clipped.commands.last() == Some(&PdfPathCommand::ClosePath)
            && next.rect.x.raw() == 80 * 64 && next.rect.y.raw() == 80 * 64
    ));
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
fn object_allowance_insertion_accounts_for_the_enclosing_graphics_save() {
    let graphic = json!({
        "shape":"rectangle", "points":[[25000,5000],[35000,15000]],
        "stroke":null, "fill":[255,0,0], "strokeWidth":0
    });
    for (depth, translated, can_add) in [(31, true, true), (32, false, true), (32, true, false)] {
        let mut first = b"q\n".repeat(depth);
        first.extend_from_slice(b"0 1 0 rg 24 5 10 10 re f\n");
        let mut second = b"Q\n".repeat(depth);
        if translated {
            second.extend_from_slice(b"1 0 0 1 20 0 cm\n");
        }
        second.extend_from_slice(b"0 0 1 rg 3 5 10 10 re f");
        let source = fixture_with_slots(&first, false, "", &[stream("", &second)], "[5 0 R 6 0 R]");
        let mut session = open(&source);
        let before = session.snapshot().binding;
        let original = objects(&session, 0);
        let page = &session.snapshot().scene.object_pages[0];
        assert_eq!(page.refusal, None);
        assert_eq!(
            page.can_add, can_add,
            "depth {depth}, translated {translated}"
        );
        assert_eq!(original.as_array().unwrap().len(), 2);

        let result = apply(
            &mut session,
            json!({"kind":"addGraphic","page":0,"graphic":graphic}),
        );
        if can_add {
            result.unwrap();
            let edited = objects(&open(session.output_bytes()), 0);
            assert_eq!(edited.as_array().unwrap().len(), 3);
            assert_eq!(edited[0]["bounds"], original[0]["bounds"]);
            assert_eq!(edited[1]["bounds"], original[1]["bounds"]);
            assert_eq!(edited[2]["bounds"], json!([25000, 5000, 35000, 15000]));
            for original in [&first, &second] {
                let body = [b"stream\n".as_slice(), original, b"\nendstream"].concat();
                assert!(
                    session
                        .output_bytes()
                        .windows(body.len())
                        .any(|bytes| bytes == body)
                );
            }
        } else {
            assert_eq!(result, Err(NativeSessionError::Budget));
            assert_eq!(session.snapshot().binding, before);
            assert_eq!(session.lookup(before, 1), Ok(NativeRequestLookup::Unseen));
            assert_eq!(session.output_bytes(), source);
        }
    }
}

#[test]
fn object_operations_follow_local_graphics_depth_on_pages_and_in_forms() {
    let graphic = json!({
        "shape":"rectangle", "points":[[12000,10000],[22000,20000]],
        "stroke":null, "fill":[255,0,0], "strokeWidth":0
    });
    let asset = concat!(
        "89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de",
        "0000000c49444154789c63f8cfc0000003010100c9fe92ef0000000049454e44ae426082"
    );
    let full = json!(["replace", "move", "remove"]);
    let limit = Err(NativeSessionError::Object(NativeObjectRefusal::State));
    for in_form in [false, true] {
        for depth in [30, 31, 32] {
            let paint = format!(
                "{}10 10 10 10 re f\n10 0 0 10 30 30 cm /I Do\n{}",
                "q\n".repeat(depth),
                "Q\n".repeat(depth),
            );
            let image = stream(
                "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8",
                &[7],
            );
            let source = if in_form {
                let form = stream(
                    "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /XObject << /I 6 0 R >> >>",
                    paint.as_bytes(),
                );
                fixture(
                    b"1 1 2 2 re f\nq /Fm Do Q",
                    false,
                    "/Resources << /XObject << /Fm 7 0 R >> >>",
                    &[image, form],
                )
            } else {
                fixture(
                    format!("1 1 2 2 re f\n{paint}").as_bytes(),
                    false,
                    "/Resources << /XObject << /I 6 0 R >> >>",
                    &[image],
                )
            };
            let mut session = open(&source);
            let before = objects(&session, 0);
            assert_eq!(before[0]["operations"], full);
            assert_eq!(
                before[1]["operations"],
                match depth {
                    30 => full.clone(),
                    31 => json!(["move", "remove"]),
                    _ => json!(["remove"]),
                }
            );
            let image_ops = if depth == 32 {
                json!(["replace", "remove"])
            } else {
                full.clone()
            };
            assert_eq!(before[2]["operations"], image_ops);
            assert!(session.snapshot().scene.object_pages[0].can_add);

            let mut moved = open(&source);
            let selected = target(&moved, 0, 1);
            let result = apply(
                &mut moved,
                json!({"kind":"moveObject","target":selected,"transform":[1,0,0,1,5,0]}),
            );
            if depth == 32 {
                assert_eq!(result, limit);
                assert_eq!(moved.output_bytes(), source);
            } else {
                result.unwrap();
                assert_eq!(
                    objects(&moved, 0)[1]["bounds"],
                    json!([15000, 10000, 25000, 20000])
                );
            }

            let binding = session.snapshot().binding;
            let selected = target(&session, 0, 1);
            let result = apply(
                &mut session,
                json!({"kind":"replaceGraphic","target":selected,"graphic":graphic}),
            );
            if depth == 30 {
                result.unwrap();
                assert_eq!(objects(&session, 0)[1]["operations"], json!(["remove"]));
            } else {
                assert_eq!(result, limit);
                assert_eq!(session.snapshot().binding, binding);
                assert_eq!(session.lookup(binding, 1), Ok(NativeRequestLookup::Unseen));
                assert_eq!(session.output_bytes(), source);
            }

            let selected = target(&session, 0, 2);
            apply(
                &mut session,
                json!({"kind":"replaceImage","target":selected,"assetHex":asset}),
            )
            .unwrap();
            let after = objects(&session, 0);
            assert_eq!(after[2]["profile"], "rasterRgb8");
            assert_eq!(after[2]["bounds"], before[2]["bounds"]);
            assert_eq!(after[2]["operations"], image_ops);

            let selected = target(&session, 0, 1);
            apply(
                &mut session,
                json!({"kind":"removeObject","target":selected}),
            )
            .unwrap();
            let after = objects(&open(session.output_bytes()), 0);
            assert_eq!(after.as_array().unwrap().len(), 2);
            assert_eq!(after[0]["operations"], full);
            assert_eq!(after[0]["bounds"], before[0]["bounds"]);
            assert_eq!(after[1]["operations"], image_ops);
            assert_eq!(after[1]["bounds"], before[2]["bounds"]);
            assert_eq!(session.origin_bytes(), source);
        }
    }
}

#[test]
fn object_scan_workspace_refusal_keeps_vector_page_open_for_notes() {
    use std::io::Write;

    let mut content = b"10 10 m 20 40 30 40 40 10 c S\n".to_vec();
    for row in 0..24 {
        for column in 0..24 {
            let tint = (row + column) % 9;
            let x = 10 + 7 * column;
            let y = 20 + 7 * row;
            writeln!(
                &mut content,
                "[3 2] 0 d 0.25 0.{tint} 0.75 rg {x} {y} 6 6 re B"
            )
            .unwrap();
        }
    }
    let source = fixture(&content, false, "", &[]);
    let limits = NativeEditLimits {
        max_retained_bytes: 384 * 1024,
        ..NativeEditLimits::default()
    };
    let mut session = NativePdfSession::open_scoped(&source, limits, [7; 16]).unwrap();
    let binding = session.snapshot().binding;
    let view = &session.snapshot().scene.object_pages[0];
    assert!(!view.can_add);
    assert!(view.objects.is_empty());
    assert_eq!(view.refusal, None);
    assert_eq!(
        apply(
            &mut session,
            json!({
                "kind":"addGraphic", "page":0,
                "graphic":{
                    "shape":"rectangle", "points":[[5000,5000],[15000,15000]],
                    "stroke":null, "fill":[255,0,0], "strokeWidth":0
                }
            }),
        ),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(session.output_bytes(), source);
    assert_eq!(session.snapshot().binding, binding);
    assert_eq!(session.lookup(binding, 1), Ok(NativeRequestLookup::Unseen));
    let page = session.snapshot().scene.pages[0].id;
    let receipt = session
        .apply(
            binding,
            1,
            &NativeNoteCommand::Add {
                page,
                rect: [60, 60, 80, 80],
                contents: "Chart note".into(),
            },
        )
        .unwrap();
    assert_eq!(receipt.binding.revision, 1);
    assert_eq!(session.snapshot().scene.pages[0].annotations.len(), 1);
    assert!(!session.snapshot().scene.object_pages[0].can_add);
    assert_eq!(session.origin_bytes(), source);
}

#[test]
fn object_allowance_refusal_preserves_accepted_graphic_edit_state() {
    let mut content = Vec::new();
    for x in 0..96 {
        use std::io::Write;
        writeln!(&mut content, "0 1 0 rg {x} 20 4 4 re f").unwrap();
    }
    let source = fixture(&content, false, "", &[]);
    let command = |session: &NativePdfSession| {
        json!({
            "kind":"moveObject", "target":target(session, 0, 0),
            "transform":[1,0,0,1,0,10]
        })
    };
    let mut roomy = open(&source);
    let move_graphic = command(&roomy);
    apply(&mut roomy, move_graphic).unwrap();
    assert_eq!(
        objects(&roomy, 0)[0]["bounds"],
        json!([0, 30000, 4000, 34000])
    );

    let limits = NativeEditLimits {
        max_retained_bytes: 128 * 1024,
        ..NativeEditLimits::default()
    };
    let mut constrained = NativePdfSession::open_scoped(&source, limits, [7; 16]).unwrap();
    let before = constrained.snapshot().clone();
    assert_eq!(before.scene.object_pages[0].objects.len(), 96);
    let move_graphic = command(&constrained);
    assert_eq!(
        apply(&mut constrained, move_graphic),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(constrained.output_bytes(), source);
    assert_eq!(constrained.snapshot().binding, before.binding);
    assert_eq!(
        constrained.snapshot().consumed_request,
        before.consumed_request
    );
    assert_eq!(
        constrained.snapshot().scene.object_pages,
        before.scene.object_pages
    );
    assert_eq!(
        constrained.lookup(before.binding, 1),
        Ok(NativeRequestLookup::Unseen)
    );
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
fn scene_reports_partial_after_turning_an_asymmetric_form_image() {
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 40 40] /Resources << /XObject << /I 7 0 R >> >>",
        b"q 12 0 0 8 10 10 cm /I Do Q",
    );
    let pixels = miniz_oxide::deflate::compress_to_vec_zlib(&[16, 48, 80, 112, 144, 176], 6);
    let image = stream(
        "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 3 /Height 2 /BitsPerComponent 8 /Filter /FlateDecode",
        &pixels,
    );
    let source = fixture(
        b"/Fm Do q 1 0 0 1 60 0 cm /Fm Do Q",
        false,
        "/Resources << /XObject << /Fm 6 0 R >> >>",
        &[form, image],
    );
    let reader = open_pdf(&source, Default::default()).unwrap();
    let before = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    assert!(!before.pages[0].partial);
    assert_eq!(
        before.pages[0]
            .elements
            .iter()
            .filter(|element| matches!(element, PdfSceneElement::Image { .. }))
            .count(),
        2
    );

    for (transform, expected_bounds) in [
        ([0, 1, -1, 0, 90, -60], [72_000, 10_000, 80_000, 22_000]),
        ([-1, 0, 0, -1, 152, 28], [70_000, 10_000, 82_000, 18_000]),
    ] {
        let mut session = open(&source);
        let selected = target(&session, 0, 1);
        apply(
            &mut session,
            json!({"kind":"moveObject","target":selected,"transform":transform}),
        )
        .unwrap();
        assert_eq!(objects(&session, 0)[1]["bounds"], json!(expected_bounds));

        let reader = open_pdf(session.output_bytes(), Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        assert!(page.partial && scene.report.partial);
        assert!(page.report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == PdfReadDiagnosticCode::PartialPage
                && diagnostic.page_index == Some(0)
        }));
        let images: Vec<_> = page
            .elements
            .iter()
            .filter_map(|element| match element {
                PdfSceneElement::Image { value } => Some(value),
                _ => None,
            })
            .collect();
        let [sibling] = images.as_slice() else {
            panic!("expected only the unchanged image, got {images:?}");
        };
        assert_eq!((sibling.width, sibling.height), (3, 2));
        assert_eq!(sibling.filter, "FlateDecode");
        assert_eq!(sibling.data_hex, "1030507090b0");
        assert_eq!(sibling.rect.x.raw(), 10 * 64);
        assert_eq!(sibling.rect.y.raw(), 10 * 64);
        assert_eq!(sibling.rect.width.raw(), 12 * 64);
        assert_eq!(sibling.rect.height.raw(), 8 * 64);
    }
}

#[test]
fn saved_native_images_have_representable_placement_or_a_partial_diagnostic() {
    let source = fixture(b"", false, "", &[]);
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode(&[30, 100, 200], 1, 1, image::ExtendedColorType::Rgb8)
        .unwrap();
    let hex: String = jpeg.iter().map(|byte| format!("{byte:02x}")).collect();

    for (bounds, expected_size) in [
        ([20_000, 20_000, 20_001, 20_250], None),
        ([20_000, 20_000, 20_125, 20_250], Some((8, 16))),
    ] {
        let mut session = open(&source);
        apply(
            &mut session,
            json!({"kind":"addImage","page":0,"bounds":bounds,"assetHex":hex}),
        )
        .unwrap();
        let reopened = open(session.output_bytes());
        assert_eq!(objects(&reopened, 0)[0]["bounds"], json!(bounds));

        let reader = open_pdf(session.output_bytes(), Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        if let Some(size) = expected_size {
            let [PdfSceneElement::Image { value }] = page.elements.as_slice() else {
                panic!("expected a representable image");
            };
            assert_eq!((value.width, value.height), (1, 1));
            assert_eq!(value.data_hex, hex);
            assert_eq!((value.rect.x.raw(), value.rect.y.raw()), (20 * 64, 20 * 64));
            assert_eq!((value.rect.width.raw(), value.rect.height.raw()), size);
            assert!(!page.partial && !scene.report.partial);
        } else {
            assert!(page.elements.is_empty());
            assert!(page.partial && scene.report.partial);
            assert!(matches!(page.report.diagnostics.as_slice(), [diagnostic]
                if diagnostic.code == PdfReadDiagnosticCode::PartialPage
                    && diagnostic.page_index == Some(0)
                    && diagnostic.object_number.is_some()));
        }
    }
}

#[test]
fn collapsed_image_diagnostics_account_for_unrounded_clip_visibility() {
    let image = stream(
        "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8",
        &[7],
    );
    for (contents, partial) in [
        (&b"q 0 0 40 40 re W n 1 0 0 1 60 60 cm /I Do Q"[..], false),
        (
            &b"q 0 0 40 40 re W n 0.001 0 0 1 60 60 cm /I Do Q"[..],
            false,
        ),
        (
            &b"q 0 0 10 10 re W n 20 20 10 10 re W n 0.001 0 0 1 5 5 cm /I Do Q"[..],
            false,
        ),
        (
            &b"q 5 5 0.001 1 re W n 0.001 0 0 1 5 5 cm /I Do Q"[..],
            true,
        ),
        (
            &b"q 0 0 m 10 0 l 10 10 l 0 10 l h W n 20 20 10 10 re W n 0.001 0 0 1 5 5 cm /I Do Q"[..],
            false,
        ),
        (
            &b"q 20 20 m 22 20 28 20 30 20 c 30 30 l 20 30 l h W n 0 0 10 10 re W n 1 0 0 1 5 5 cm /I Do Q"[..],
            false,
        ),
        (
            &b"q 0 0 10.004 10 re W n 1 0 0 1 9.997 5 cm /I Do Q"[..],
            true,
        ),
        (
            &b"q 0 0 10.004 10 re W n 1 0 0 1 10.004 5 cm /I Do Q"[..],
            false,
        ),
    ] {
        let source = fixture(
            contents,
            false,
            "/Resources << /XObject << /I 6 0 R >> >>",
            std::slice::from_ref(&image),
        );
        let reader = open_pdf(&source, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        assert!(
            page.elements
                .iter()
                .all(|element| matches!(element, PdfSceneElement::Clip { .. }))
        );
        assert_eq!(page.partial, partial);
        assert_eq!(scene.report.partial, partial);
        if partial {
            assert!(matches!(page.report.diagnostics.as_slice(), [diagnostic]
                if diagnostic.code == PdfReadDiagnosticCode::PartialPage));
        } else {
            assert!(page.report.diagnostics.is_empty());
        }
    }
}

#[test]
fn fractional_page_crop_matches_content_clip_for_images() {
    let image = stream(
        "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8",
        &[7],
    );
    for (start, width, right, left, partial, image_count) in [
        ("0", "10.006", "10.006", "10.003", true, 0),
        ("0", "9.997", "9.997", "9.998", false, 0),
        ("0.1", "0.2", "0.3", "0.3", false, 0),
        ("0", "10.121", "10.121", "9.125", true, 1),
        ("0", "10.125", "10.125", "9.125", false, 1),
    ] {
        let paint = format!("1 0 0 1 {left} 5 cm /I Do");
        let clip = format!("{start} 0 {width} 10 re W n {paint}");
        let crop =
            format!("/CropBox [{start} 0 {right} 10] /Resources << /XObject << /I 6 0 R >> >>");
        for (content, dictionary) in [
            (paint.as_bytes(), crop.as_str()),
            (clip.as_bytes(), "/Resources << /XObject << /I 6 0 R >> >>"),
        ] {
            let source = fixture(content, false, dictionary, std::slice::from_ref(&image));
            let reader = open_pdf(&source, Default::default()).unwrap();
            let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
            let page = &scene.pages[0];
            assert_eq!(page.partial, partial, "{right} {left} {dictionary}");
            assert_eq!(scene.report.partial, partial);
            assert_eq!(page.report.diagnostics.len(), usize::from(partial));
            if let [diagnostic] = page.report.diagnostics.as_slice() {
                assert_eq!(diagnostic.code, PdfReadDiagnosticCode::PartialPage);
                assert_eq!(diagnostic.object_number, Some(5));
            }
            assert_eq!(
                page.elements
                    .iter()
                    .filter(|element| matches!(element, PdfSceneElement::Image { .. }))
                    .count(),
                image_count
            );
        }
    }
}

#[test]
fn image_readback_accounts_for_subunit_skew() {
    let image = stream(
        "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 2 /Height 1 /BitsPerComponent 8",
        &[7, 200],
    );
    for (b, c, partial) in [
        ("0.004", "0", true),
        ("0", "-0.004", true),
        ("0", "0", false),
    ] {
        let paint = format!("q 10 {b} {c} 5 20.125 20 cm /I Do Q");
        let source = fixture(
            paint.as_bytes(),
            false,
            "/Resources << /XObject << /I 6 0 R >> >>",
            std::slice::from_ref(&image),
        );
        let reader = open_pdf(&source, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        assert_eq!(page.partial, partial, "{paint}");
        assert_eq!(scene.report.partial, partial);
        if partial {
            assert!(page.elements.is_empty());
            assert!(matches!(page.report.diagnostics.as_slice(), [diagnostic]
                if diagnostic.code == PdfReadDiagnosticCode::PartialPage
                    && diagnostic.object_number == Some(5)));
        } else {
            let [PdfSceneElement::Image { value }] = page.elements.as_slice() else {
                panic!("expected the axis-aligned image");
            };
            assert_eq!(value.data_hex, "07c8");
            assert_eq!(value.rect.x.raw(), 20 * 64 + 8);
            assert!(page.report.diagnostics.is_empty());
        }
    }
}

#[test]
fn image_diagnostics_account_for_precise_form_clipping() {
    for (matrix, paint, partial) in [
        ("", "0.001 0 0 1 60 60 cm /I Do", false),
        ("/Matrix [1 0 0 1 0 0]", "0.001 0 0 1 60 60 cm /I Do", false),
        ("/Matrix 8 0 R", "0.001 0 0 1 60 60 cm /I Do", false),
        ("/Matrix [0.001 0 0 1 5 5]", "", false),
        ("/Matrix [0.001 0 0 1 5 5]", "1 0 0 1 2 0 cm /I Do", false),
        ("/Matrix [0.001 0 0 1 5 5]", "/I Do", true),
        ("/Matrix [0.001 0 0 1 500 5]", "/I Do", false),
    ] {
        let form = stream(
            &format!(
                "/Type /XObject /Subtype /Form /BBox [0 0 1 40] {matrix} /Resources << /XObject << /I 7 0 R >> >>"
            ),
            paint.as_bytes(),
        );
        let image = stream(
            "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8",
            &[7],
        );
        let source = fixture(
            b"/F Do",
            false,
            "/Resources << /XObject << /F 6 0 R >> >>",
            &[form, image, b"[1 0 0 1 0 0]".to_vec()],
        );
        let reader = open_pdf(&source, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let page = &scene.pages[0];
        assert!(matches!(
            page.elements.as_slice(),
            [PdfSceneElement::Annotation { .. }]
        ));
        assert_eq!(page.partial, partial, "{matrix} {paint}");
        assert_eq!(scene.report.partial, partial);
        if partial {
            assert!(matches!(page.report.diagnostics.as_slice(), [diagnostic]
                if diagnostic.code == PdfReadDiagnosticCode::PartialPage));
        } else {
            assert!(page.report.diagnostics.is_empty());
        }
    }
}

#[test]
fn collapsed_form_diagnostics_do_not_enqueue_discarded_children() {
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Matrix [0.001 0 0 1 20 20] /Resources << /XObject << /Child 7 0 R >> >>",
        b"/Child Do",
    );
    let child = stream("/Type /XObject /Subtype /Form /BBox [0 0 1 1]", b"");
    let source = fixture(
        b"/F Do",
        false,
        "/Resources << /XObject << /F 6 0 R >> >>",
        &[form, child],
    );
    let limits = PdfReaderLimits {
        max_scene_elements: 1,
        ..PdfReaderLimits::default()
    };
    let reader = open_pdf(&source, limits).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    assert!(matches!(
        scene.pages[0].elements.as_slice(),
        [PdfSceneElement::Annotation { .. }]
    ));
    assert!(!scene.pages[0].partial && !scene.report.partial);
}

#[test]
fn scene_reads_nested_image_replacement_and_unchanged_sibling() {
    let outer = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 40 40] /Matrix [2 0 0 1 0 0] /Resources 9 0 R",
        b"/Inner Do 1 0 0 1 5 0 cm",
    );
    let inner = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 40 40] /Matrix [1 0 0 1 4 3] /Resources << /XObject << /I 8 0 R >> >>",
        b"q 10 0 0 10 10 10 cm /I Do Q",
    );
    let pixels = miniz_oxide::deflate::compress_to_vec_zlib(&[7], 6);
    let image = stream(
        "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8 /Filter /FlateDecode",
        &pixels,
    );
    let source = fixture(
        b"/Outer Do q 1 0 0 1 60 0 cm /Outer Do Q\nq 1 0 0 1 180 0 cm /Outer Do Q\n2 3 4 5 re f\n",
        false,
        "/Resources << /XObject << /Outer 6 0 R >> >>",
        &[
            outer,
            inner,
            image,
            b"<< /XObject << /Inner 7 0 R >> >>".to_vec(),
        ],
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

    let reader = open_pdf(session.output_bytes(), Default::default()).unwrap();
    let scene = read_pdf_scene(
        &reader,
        &PdfSceneRequest {
            first_page: 0,
            page_count: 1,
        },
    )
    .unwrap();
    let images: Vec<_> = scene.pages[0]
        .elements
        .iter()
        .filter_map(|element| match element {
            PdfSceneElement::Image { value } => Some(value),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 2);
    assert!(!scene.pages[0].partial);
    assert!(matches!(
        scene.pages[0].elements.last(),
        Some(PdfSceneElement::Path { value }) if value.rect.x.raw() == 2 * 64
            && value.rect.y.raw() == 3 * 64 && value.rect.width.raw() == 4 * 64
    ));
    assert_eq!(images[0].media_type, "image/jpeg");
    assert_eq!(images[0].data_hex, hex);
    assert_eq!(images[1].filter, "FlateDecode");
    assert_eq!(images[1].data_hex, "07");
    assert_eq!(images[0].rect.x.raw(), 1_792);
    assert_eq!(images[0].rect.y.raw(), 832);
    assert_eq!(images[0].rect.width.raw(), 1_280);
    assert_eq!(images[0].rect.height.raw(), 640);
    assert_eq!((images[1].width, images[1].height), (1, 1));
    let sibling = images[1].rect;
    assert_eq!(
        (
            sibling.x.raw(),
            sibling.y.raw(),
            sibling.width.raw(),
            sibling.height.raw()
        ),
        (5_632, 832, 1_280, 640)
    );
}

#[test]
fn escaped_content_names_keep_native_admission_conservative() {
    let pixels = miniz_oxide::deflate::compress_to_vec_zlib(&[7], 6);
    let image = stream(
        "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8 /Filter /FlateDecode",
        &pixels,
    );
    let source = fixture(
        b"q 10 0 0 10 5 5 cm /#49 Do Q",
        false,
        "/Resources << /XObject << /I 6 0 R >> >>",
        &[image],
    );
    let session = open(&source);
    assert!(!session.snapshot().scene.object_pages[0].can_add);
    assert_eq!(objects(&session, 0), json!([]));
    assert!(!session.redaction_admitted());

    let reader = open_pdf(&source, Default::default()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    assert!(matches!(
        scene.pages[0].elements.as_slice(),
        [PdfSceneElement::Image { value }] if value.data_hex == "07"
    ));
}

#[test]
fn scene_inherits_form_resources_when_missing_or_null() {
    let pixels = miniz_oxide::deflate::compress_to_vec_zlib(&[7], 6);
    for resources in ["", "/Resources null", "/Resources 8 0 R"] {
        let form = stream(
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 40 40] {resources}"),
            b"/I Do",
        );
        let image = stream(
            "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8 /Filter /FlateDecode",
            &pixels,
        );
        let source = fixture(
            b"/Form Do",
            false,
            "/Resources << /XObject << /Form 6 0 R /I 7 0 R >> >>",
            &[form, image, b"null".to_vec()],
        );
        let reader = open_pdf(&source, Default::default()).unwrap();
        let scene = read_pdf_scene(
            &reader,
            &PdfSceneRequest {
                first_page: 0,
                page_count: 1,
            },
        )
        .unwrap();
        assert!(
            scene.pages[0].elements.iter().any(|element| {
                matches!(element, PdfSceneElement::Image { value } if value.data_hex == "07")
            }),
            "{resources}"
        );
    }
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
fn jpeg_asset_add_roundtrips_and_constrained_edits_are_atomic() {
    let image = stream(
        "/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width 1 /Height 1 /BitsPerComponent 8",
        &[7],
    );
    let source = fixture(
        b"q 10 0 0 10 5 5 cm /I Do Q",
        false,
        "/Resources << /XObject << /I 6 0 R >> >>",
        &[image],
    );
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode(&[30, 100, 200], 1, 1, image::ExtendedColorType::Rgb8)
        .unwrap();
    let hex: String = jpeg.iter().map(|byte| format!("{byte:02x}")).collect();
    let bounds = [20_000, 20_000, 25_000, 25_000];
    let add = json!({"kind":"addImage","page":0,"bounds":bounds,"assetHex":hex});

    let mut roomy = open(&source);
    apply(&mut roomy, add.clone()).unwrap();
    let reopened = open(roomy.output_bytes());
    let images = objects(&reopened, 0);
    assert_eq!(images.as_array().unwrap().len(), 2);
    assert_eq!(images[0]["profile"], "rasterGray8");
    assert_eq!(images[1]["profile"], "rasterRgb8");
    assert_eq!(images[1]["bounds"], json!(bounds));
    assert!(
        roomy
            .output_bytes()
            .windows(jpeg.len())
            .any(|bytes| bytes == jpeg)
    );

    // @contract: The session cap fits the JPEG peak. The live edit allowance is smaller.
    let limits = NativeEditLimits {
        max_retained_bytes: 80 * 1024,
        ..NativeEditLimits::default()
    };
    let mut constrained = NativePdfSession::open_scoped(&source, limits, [7; 16]).unwrap();
    let selected = target(&constrained, 0, 0);
    let replace = json!({"kind":"replaceImage","target":selected,"assetHex":hex});
    let before = constrained.snapshot().clone();
    assert!(before.scene.object_pages[0].can_add);
    for command in [add, replace] {
        assert_eq!(
            apply(&mut constrained, command),
            Err(NativeSessionError::Budget)
        );
        let after = constrained.snapshot();
        assert_eq!(after.binding, before.binding);
        assert_eq!(after.output_hash, before.output_hash);
        assert_eq!(after.scene.scene_hash, before.scene.scene_hash);
        assert_eq!(after.scene.object_pages, before.scene.object_pages);
        assert_eq!(after.consumed_request, before.consumed_request);
        assert_eq!(
            constrained.lookup(before.binding, 1),
            Ok(NativeRequestLookup::Unseen)
        );
        assert_eq!(constrained.output_bytes(), source);
    }
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
    let display_p3 = concat!(
        "0000016969434350446973706c61792050330000789c7590bd4bc35014c54fab52d03a88",
        "0e1d1c328943d4d20a7671682b1445305405ab539a7e096d7c24295271135729f81f58c1",
        "5970b0885470717010440711dd9c3a29b86878de9754da22dec7e5fd389c73b95cc01b50",
        "192bf60228e996914cc4a4b5d4bae47b83879e53aa66b2a8a22c0afefdbbebf3d1f5de4f",
        "88594dbb7610d94f5c97ce2e97769e02537ffd5dd59fc99a1afddfd4418d1916e0918995",
        "6d8b09de251e316829e2aae0bccbc782d32e9f3b9e95649cf89658d20a6a86b8492ca73b",
        "f47c07978a65adb583d8de9fd55797c51cea51cc611326188a5051810405e17ffcd38e3f",
        "8e2d725760502e8f022cca44491113b2c4f3d0a1611232710841ea90b873eb7e0fadfbc9",
        "6d6def15986d70ce2fdada4203389da193d5dbda7804181a006eea4c355447eaa1f6e672",
        "c0fb0930980286ef28b361e6c221777b7f0ce87be1fc630cf01d027695f3af23ceed1a85",
        "9f812bfd0717296abc31294f3e",
    );
    let pixels = "0000000c49444154789c63f8cfc0000003010100c9fe92ef0000000049454e44ae426082";
    let untagged = format!("{header}{pixels}");
    let tagged = format!("{header}{display_p3}{pixels}");
    let encoded: Vec<_> = tagged
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    let mut decoder = image::codecs::png::PngDecoder::new(Cursor::new(&encoded)).unwrap();
    let profile = decoder.icc_profile().unwrap().unwrap();
    let profile_size = u32::from_be_bytes(profile[..4].try_into().unwrap()) as usize;
    assert_eq!(profile_size, profile.len());
    assert_eq!(&profile[16..24], b"RGB XYZ ");
    assert_eq!(&profile[36..40], b"acsp");
    assert_eq!(
        &profile[84..100],
        b"\xec\xfd\xa3\x8e\x38\x85\x47\xc3\x6d\xb4\xbd\x4f\x7a\xda\x18\x2f"
    );
    let decoded = image::DynamicImage::from_decoder(decoder).unwrap();
    assert_eq!(decoded.into_rgb8().into_raw(), [255, 0, 0]);

    let source = fixture(b"", false, "", &[]);
    let mut session = open(&source);
    apply(
        &mut session,
        json!({"kind":"addImage","page":0,"bounds":[1000,1000,5000,5000],"assetHex":untagged}),
    )
    .unwrap();
    let selected = target(&session, 0, 0);
    apply(
        &mut session,
        json!({"kind":"replaceImage","target":selected,"assetHex":untagged}),
    )
    .unwrap();
    let before = session.snapshot().clone();
    assert_eq!(before.binding.revision, 2);
    assert_eq!(before.consumed_request, 2);
    let bytes = session.output_bytes().to_vec();
    let selected = target(&session, 0, 0);
    let error = apply(
        &mut session,
        json!({"kind":"replaceImage","target":selected,"assetHex":tagged}),
    )
    .unwrap_err();
    assert_eq!(
        error,
        NativeSessionError::Object(NativeObjectRefusal::Asset)
    );
    let after = session.snapshot();
    assert_eq!(after.binding, before.binding);
    assert_eq!(after.output_hash, before.output_hash);
    assert_eq!(after.scene.scene_hash, before.scene.scene_hash);
    assert_eq!(after.consumed_request, before.consumed_request);
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

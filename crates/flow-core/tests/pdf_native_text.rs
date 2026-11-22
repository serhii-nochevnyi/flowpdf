use flow_core::pdf::native::{
    NativeEditLimits, NativeNoteCommand, NativePdfSession, NativeRequestLookup, NativeSessionError,
    NativeTextCommand, NativeTextRefusal,
};
use flow_core::pdf::{PdfReaderLimits, PdfSceneElement, PdfSceneRequest, open_pdf, read_pdf_scene};

fn fixture(
    content: &str,
    simple: bool,
    embedded: bool,
    extra_mapping: &str,
    shared: bool,
) -> Vec<u8> {
    build_fixture(
        content,
        simple,
        embedded,
        extra_mapping,
        shared,
        false,
        "/F1",
    )
}

fn build_fixture(
    content: &str,
    simple: bool,
    embedded: bool,
    extra_mapping: &str,
    shared: bool,
    private_alias: bool,
    font_alias: &str,
) -> Vec<u8> {
    write_fixture(fixture_objects(
        content,
        simple,
        embedded,
        extra_mapping,
        shared,
        private_alias,
        font_alias,
    ))
}

fn fixture_objects(
    content: &str,
    simple: bool,
    embedded: bool,
    extra_mapping: &str,
    shared: bool,
    private_alias: bool,
    font_alias: &str,
) -> Vec<Vec<u8>> {
    fn stream(dict: &str, bytes: &[u8]) -> Vec<u8> {
        let mut object = format!("<< /Length {} {dict} >>\nstream\n", bytes.len()).into_bytes();
        object.extend_from_slice(bytes);
        object.extend_from_slice(b"\nendstream");
        object
    }
    let font = include_bytes!("../data/NotoSans-Regular.ttf");
    let face = ttf_parser::Face::parse(font, 0).unwrap();
    let cid_map = ['\0', 'A', 'Ї']
        .into_iter()
        .flat_map(|ch| {
            face.glyph_index(ch)
                .map_or(0, |glyph| glyph.0)
                .to_be_bytes()
        })
        .collect::<Vec<_>>();
    let (low, high, mappings, count) = if simple {
        ("00", "FF", "<41> <0041>", 1)
    } else {
        ("0000", "FFFF", "<0001> <0041>\n<0002> <0407>", 2)
    };
    let count = count + usize::from(!extra_mapping.is_empty());
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapType 2 def\n/CMapName /Example def\n1 begincodespacerange\n<{low}> <{high}>\nendcodespacerange\n{count} beginbfchar\n{mappings}\n{extra_mapping}\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend end"
    );
    let make_page = |contents: &str| {
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << {font_alias} 5 0 R >> >> /Contents {contents} >>").into_bytes()
    };
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        if shared { b"<< /Type /Pages /Kids [3 0 R 11 0 R] /Count 2 >>".to_vec() }
        else { b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec() },
        make_page(if private_alias { "[4 0 R] /PrivateData 4 0 R" } else { "4 0 R" }),
        stream("", content.as_bytes()),
        if simple {
            b"<< /Type /Font /Subtype /TrueType /BaseFont /NotoSans-Regular /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 65 /Widths [639] /FontDescriptor 8 0 R /ToUnicode 7 0 R >>".to_vec()
        } else {
            b"<< /Type /Font /Subtype /Type0 /BaseFont /NotoSans-Regular /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>".to_vec()
        },
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NotoSans-Regular /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /W [1 [639 339]] /FontDescriptor 8 0 R /CIDToGIDMap 10 0 R >>".to_vec(),
        stream("", cmap.as_bytes()),
        format!("<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 {} >>",
            if embedded { "/FontFile2 9 0 R" } else { "" }).into_bytes(),
        stream(&format!("/Length1 {}", font.len()), font),
        stream("", &cid_map),
    ];
    if shared {
        objects.push(make_page("4 0 R"));
    }
    objects
}

fn write_fixture(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let size = offsets.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

const CID_CONTENT: &str =
    "BT /F1 12 Tf 20 100 Td <000200010001> Tj ET\nBT /F1 12 Tf 20 150 Td <0001> Tj ET";
const SIMPLE_CONTENT: &str =
    "BT /F1 12 Tf 1 0 0 1 20 100 Tm [(AAA) 0] TJ ET\nBT /F1 12 Tf 20 150 Td (A) Tj ET";

fn open(bytes: &[u8]) -> NativePdfSession {
    NativePdfSession::open_scoped(bytes, NativeEditLimits::default(), [7; 16]).unwrap()
}

#[test]
fn native_text_declines_font_aliases_outside_the_exact_retained_name_profile() {
    let content = "BT /Fé 12 Tf 20 100 Td <0001> Tj ET";
    let bytes = build_fixture(content, false, true, "", false, false, "/F#C3#A9");
    let reader = open_pdf(&bytes, PdfReaderLimits::default()).unwrap();
    let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
    assert!(matches!(
        scene.pages[0].elements.as_slice(),
        [PdfSceneElement::Text { value }] if value.text == "A"
    ));

    let session = open(&bytes);
    let text = &session.snapshot().scene.text_pages[0];
    assert!(text.islands.is_empty());
    assert_eq!(text.refusal, Some(NativeTextRefusal::Layout));
    assert_eq!(session.output_bytes(), bytes);
}

#[test]
fn core_replaces_existing_english_and_ukrainian_codes_and_preserves_neighbors() {
    for (content, simple, before, replacement, retained) in [
        (
            SIMPLE_CONTENT,
            true,
            "AAA",
            "AA",
            "BT /F1 12 Tf 20 150 Td (A) Tj ET",
        ),
        (
            CID_CONTENT,
            false,
            "ЇAA",
            "ЇA",
            "BT /F1 12 Tf 20 150 Td <0001> Tj ET",
        ),
    ] {
        let bytes = fixture(content, simple, true, "", false);
        let mut session = open(&bytes);
        let source = session.snapshot();
        let island = &source.scene.text_pages[0].islands[0];
        assert_eq!(island.text, before);
        let command = NativeTextCommand::ReplaceText {
            island: island.id,
            text: replacement.into(),
        };
        let binding = source.binding;
        session.apply_text(binding, 1, &command).unwrap();
        assert_eq!(session.origin_bytes(), bytes);
        assert_eq!(session.snapshot().binding.revision, 1);
        assert_eq!(
            session.snapshot().scene.text_pages[0].islands[0].text,
            replacement
        );
        assert!(
            session
                .output_bytes()
                .windows(retained.len())
                .any(|window| window == retained.as_bytes())
        );
        assert_eq!(
            open(session.output_bytes()).snapshot().scene.text_pages[0].islands[1].text,
            "A"
        );
        let reader = open_pdf(session.output_bytes(), PdfReaderLimits::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let elements = &scene.pages[0].elements;
        let texts = elements
            .iter()
            .filter_map(|element| match element {
                PdfSceneElement::Text { value } => Some(value),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            texts
                .iter()
                .map(|value| value.text.as_str())
                .collect::<Vec<_>>(),
            [replacement, "A"]
        );
        assert_eq!(texts[0].provenance.invocation_path, [0]);
        let [start, end] = texts[0].provenance.operand_span.unwrap();
        assert_eq!(end - start, if simple { 8 } else { 10 });
        assert!(texts[0].provenance.stream_end.unwrap() > texts[0].provenance.stream_offset);
        let font = include_bytes!("../data/NotoSans-Regular.ttf");
        assert!(
            session
                .output_bytes()
                .windows(font.len())
                .any(|window| window == font)
        );
        let current = session.snapshot().binding;
        assert!(session.apply_text(current, 2, &command).is_err());
        assert!(session.apply_text(binding, 2, &command).is_err());
    }
}

#[test]
fn core_refuses_missing_codes_and_overflow_atomically() {
    let bytes = fixture(CID_CONTENT, false, true, "", false);
    let mut session = open(&bytes);
    let binding = session.snapshot().binding;
    let island = session.snapshot().scene.text_pages[0].islands[0].id;
    for (text, reason) in [
        ("Z", NativeTextRefusal::Mapping),
        ("ЇAAAA", NativeTextRefusal::Overflow),
    ] {
        let command = NativeTextCommand::ReplaceText {
            island,
            text: text.into(),
        };
        assert_eq!(
            session.apply_text(binding, 1, &command).unwrap_err().code(),
            reason.code()
        );
        assert_eq!(session.output_bytes(), bytes);
        assert_eq!(session.snapshot().binding, binding);
        assert_eq!(session.snapshot().consumed_request, 0);
    }
}

#[test]
fn core_refuses_replacement_when_recognition_workspace_is_exhausted() {
    use std::fmt::Write;

    let mut content = String::new();
    let codes = format!("0002{}", "0001".repeat(65));
    for line in 1..=70 {
        writeln!(content, "BT /F1 12 Tf 20 {} Td <{codes}> Tj ET", 11 * line).unwrap();
    }
    let bytes = fixture(&content, false, true, "", false);
    let limits = NativeEditLimits {
        max_retained_bytes: 4 * 1024 * 1024,
        ..NativeEditLimits::default()
    };
    let mut session = NativePdfSession::open_scoped(&bytes, limits, [7; 16]).unwrap();
    let binding = session.snapshot().binding;
    let islands = &session.snapshot().scene.text_pages[0].islands;
    assert_eq!(islands.len(), 70);
    let island = &islands[0];
    assert_eq!(island.text, format!("Ї{}", "A".repeat(65)));
    let command = NativeTextCommand::ReplaceText {
        island: island.id,
        text: "ЇA".into(),
    };

    assert_eq!(
        session.apply_text(binding, 1, &command),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(session.output_bytes(), bytes);
    assert_eq!(session.snapshot().binding, binding);
    assert_eq!(session.snapshot().consumed_request, 0);

    let mut roomy = open(&bytes);
    let command = NativeTextCommand::ReplaceText {
        island: roomy.snapshot().scene.text_pages[0].islands[0].id,
        text: "ЇA".into(),
    };
    roomy
        .apply_text(roomy.snapshot().binding, 1, &command)
        .unwrap();
    assert_eq!(roomy.snapshot().scene.text_pages[0].islands[0].text, "ЇA");
}

#[test]
fn core_replacement_shares_work_with_rewrite_and_exact_byte_projection() {
    let content = format!(
        "{CID_CONTENT}\nBT /F1 12 Tf 20 200 Td <0001> Tj ET\nBT /F1 12 Tf 20 250 Td <0001> Tj ET"
    );
    let bytes = fixture(&content, false, true, "", false);
    let defaults = NativeEditLimits::default();
    let accept_note = |max_work| {
        let mut session = NativePdfSession::open_scoped(
            &bytes,
            NativeEditLimits {
                max_work,
                ..defaults
            },
            [7; 16],
        )?;
        let snapshot = session.snapshot();
        let origin_binding = snapshot.binding;
        let note = NativeNoteCommand::Add {
            page: snapshot.scene.pages[0].id,
            rect: [60, 60, 80, 80],
            contents: "Review note".into(),
        };
        let receipt = session.apply(origin_binding, 1, &note)?;
        Ok::<_, NativeSessionError>((session, origin_binding, receipt))
    };
    let replace = |session: &mut NativePdfSession| {
        let snapshot = session.snapshot();
        let islands = &snapshot.scene.text_pages[0].islands;
        assert_eq!(islands.len(), 4);
        let command = NativeTextCommand::ReplaceText {
            island: islands[0].id,
            text: "ЇA".into(),
        };
        session.apply_text(snapshot.binding, 2, &command)
    };
    let (mut low, mut high) = (500_000, defaults.max_work);
    while low < high {
        let mid = low + (high - low) / 2;
        match accept_note(mid).and_then(|(mut session, _, _)| replace(&mut session)) {
            Ok(_) => high = mid,
            Err(NativeSessionError::Budget) => low = mid + 1,
            Err(error) => panic!("unexpected refusal at {mid} work units: {error:?}"),
        }
    }

    let (mut tight, origin_binding, note_receipt) = accept_note(low - 1).unwrap();
    let before = tight.snapshot().clone();
    let accepted_bytes = tight.output_bytes().to_vec();
    assert_eq!(replace(&mut tight), Err(NativeSessionError::Budget));
    assert_eq!(tight.output_bytes(), accepted_bytes);
    assert_eq!(tight.origin_bytes(), bytes);
    assert_eq!(tight.snapshot().scene, before.scene);
    assert_eq!(tight.snapshot().binding, before.binding);
    assert_eq!(tight.snapshot().output_hash, before.output_hash);
    assert_eq!(tight.snapshot().consumed_request, before.consumed_request);
    assert_eq!(
        tight.lookup(before.binding, 2),
        Ok(NativeRequestLookup::Unseen)
    );
    assert_eq!(
        tight.lookup(origin_binding, 1),
        Ok(NativeRequestLookup::Accepted(note_receipt))
    );

    let (mut exact, _, _) = accept_note(low).unwrap();
    let binding = exact.snapshot().binding;
    let receipt = replace(&mut exact).unwrap();
    let snapshot = exact.snapshot();
    let island = &snapshot.scene.text_pages[0].islands[0];
    assert_eq!(island.text, "ЇA");
    assert_eq!(receipt.binding.revision, 2);
    assert_eq!(receipt.text_island, Some(island.id));
    assert_eq!(
        exact.lookup(binding, 2),
        Ok(NativeRequestLookup::Accepted(receipt))
    );
    assert_eq!(
        NativePdfSession::verify_text_projection(
            exact.output_bytes(),
            snapshot.limits,
            snapshot.binding,
            &snapshot.scene.text_pages,
        ),
        Ok(true)
    );
}

#[test]
fn inherited_text_ownership_work_shares_the_rewrite_budget() {
    let source = |private_data: &str| {
        let mut objects = fixture_objects(CID_CONTENT, false, true, "", false, false, "/F1");
        objects[1] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources 11 0 R >>".to_vec();
        objects[2] = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /PrivateData {private_data} >>").into_bytes();
        objects.push(b"<< /Font << /F1 5 0 R >> >>".to_vec());
        write_fixture(objects)
    };
    let early = source("[4 0 R << /Values [1 2 3] >>]");
    let late = source("[<< /Values [1 2 3] >> 4 0 R]");
    let defaults = NativeEditLimits::default();
    let accept_note = |bytes: &[u8], max_work| {
        let limits = NativeEditLimits {
            max_work,
            ..defaults
        };
        let mut session = NativePdfSession::open_scoped(bytes, limits, [7; 16])?;
        let snapshot = session.snapshot();
        let origin_binding = snapshot.binding;
        let note = NativeNoteCommand::Add {
            page: snapshot.scene.pages[0].id,
            rect: [60, 60, 80, 80],
            contents: "Review note".into(),
        };
        let receipt = session.apply(origin_binding, 1, &note)?;
        Ok::<_, NativeSessionError>((session, origin_binding, receipt))
    };
    let replace = |session: &mut NativePdfSession| {
        let snapshot = session.snapshot();
        let island = &snapshot.scene.text_pages[0].islands[0];
        assert_eq!(island.text, "ЇAA");
        let command = NativeTextCommand::ReplaceText {
            island: island.id,
            text: "ЇA".into(),
        };
        session.apply_text(snapshot.binding, 2, &command)
    };

    let (mut low, mut high) = (500_000, defaults.max_work);
    while low < high {
        let mid = low + (high - low) / 2;
        let (mut session, _, _) = accept_note(&early, mid).unwrap();
        match replace(&mut session) {
            Ok(_) => high = mid,
            Err(NativeSessionError::Budget) => low = mid + 1,
            Err(error) => panic!("unexpected refusal at {mid} work units: {error:?}"),
        }
    }
    let (mut exact, _, _) = accept_note(&early, low).unwrap();
    replace(&mut exact).unwrap();

    let (mut tight, origin_binding, note_receipt) = accept_note(&late, low).unwrap();
    let before = tight.snapshot().clone();
    let accepted_bytes = tight.output_bytes().to_vec();
    assert_eq!(replace(&mut tight), Err(NativeSessionError::Budget));
    assert_eq!(tight.output_bytes(), accepted_bytes);
    assert_eq!(tight.origin_bytes(), late);
    assert_eq!(tight.snapshot().scene, before.scene);
    assert_eq!(tight.snapshot().binding, before.binding);
    assert_eq!(tight.snapshot().output_hash, before.output_hash);
    assert_eq!(tight.snapshot().consumed_request, before.consumed_request);
    assert_eq!(
        tight.lookup(before.binding, 2),
        Ok(NativeRequestLookup::Unseen)
    );
    assert_eq!(
        tight.lookup(origin_binding, 1),
        Ok(NativeRequestLookup::Accepted(note_receipt))
    );

    let (mut roomy, _, _) = accept_note(&late, defaults.max_work).unwrap();
    replace(&mut roomy).unwrap();
    let snapshot = roomy.snapshot();
    assert_eq!(snapshot.scene.text_pages[0].islands[0].text, "ЇA");
    assert!(
        roomy
            .output_bytes()
            .windows(CID_CONTENT.len())
            .any(|bytes| bytes == CID_CONTENT.as_bytes())
    );
    assert_eq!(
        NativePdfSession::verify_text_projection(
            roomy.output_bytes(),
            snapshot.limits,
            snapshot.binding,
            &snapshot.scene.text_pages,
        ),
        Ok(true)
    );
}

#[test]
fn core_refuses_replacement_when_shared_decoded_budget_is_exhausted() {
    let content = format!("{}{CID_CONTENT}", " ".repeat(1024));
    let bytes = build_fixture(&content, false, true, "", false, true, "/F1");
    let limits = NativeEditLimits {
        max_total_decoded_bytes: 2 * include_bytes!("../data/NotoSans-Regular.ttf").len() + 4500,
        ..NativeEditLimits::default()
    };
    let mut session = NativePdfSession::open_scoped(&bytes, limits, [7; 16]).unwrap();
    let binding = session.snapshot().binding;
    let command = NativeTextCommand::ReplaceText {
        island: session.snapshot().scene.text_pages[0].islands[0].id,
        text: "ЇA".into(),
    };

    assert_eq!(
        session.apply_text(binding, 1, &command).unwrap_err().code(),
        "FLOW_PDF_READER_DECODED_STREAM_LIMIT"
    );
    assert_eq!(session.output_bytes(), bytes);
    assert_eq!(session.snapshot().binding, binding);
    assert_eq!(session.snapshot().consumed_request, 0);
}

#[test]
fn core_refuses_unproven_fonts_mappings_and_layout() {
    for (content, embedded, mapping, shared, reason) in [
        (CID_CONTENT, false, "", false, NativeTextRefusal::Font),
        (
            CID_CONTENT,
            true,
            "<0003> <005A>",
            false,
            NativeTextRefusal::Mapping,
        ),
        (
            "BT /Missing 12 Tf 20 100 Td <0001> Tj ET",
            true,
            "",
            false,
            NativeTextRefusal::Font,
        ),
        (
            CID_CONTENT,
            true,
            "<0003> <0041>",
            false,
            NativeTextRefusal::Mapping,
        ),
        (
            CID_CONTENT,
            true,
            "<0001> <0041>",
            false,
            NativeTextRefusal::Mapping,
        ),
        (
            CID_CONTENT,
            true,
            "<0003> <00410041>",
            false,
            NativeTextRefusal::Mapping,
        ),
        (CID_CONTENT, true, "", true, NativeTextRefusal::Ownership),
        (
            "0 0 200 200 re W n BT /F1 12 Tf 20 100 Td <0001> Tj ET",
            true,
            "",
            false,
            NativeTextRefusal::Layout,
        ),
        (
            "BT /F1 12 Tf 1 Tr 20 100 Td <0001> Tj ET",
            true,
            "",
            false,
            NativeTextRefusal::Layout,
        ),
        (
            "BT /F1 12 Tf 1 Tc 20 100 Td <0001> Tj ET",
            true,
            "",
            false,
            NativeTextRefusal::Layout,
        ),
        (
            "BT /F1 12 Tf 20 100 Td [<0001> 10 <0001>] TJ ET",
            true,
            "",
            false,
            NativeTextRefusal::Layout,
        ),
        (
            "BT /F1 12 Tf 20 100 Td <0001> Tj ET BT /F1 12 Tf 20 100 Td <0001> Tj ET",
            true,
            "",
            false,
            NativeTextRefusal::Layout,
        ),
    ] {
        let bytes = fixture(content, false, embedded, mapping, shared);
        let session = open(&bytes);
        let page = &session.snapshot().scene.text_pages[0];
        assert!(page.islands.is_empty());
        assert_eq!(page.refusal, Some(reason));
        assert_eq!(session.output_bytes(), bytes);
    }
}

#[test]
fn private_alias_keeps_its_original_stream_during_copy_on_write() {
    let bytes = build_fixture(CID_CONTENT, false, true, "", false, true, "/F1");
    let mut session = open(&bytes);
    let binding = session.snapshot().binding;
    let island = session.snapshot().scene.text_pages[0].islands[0].id;
    let command = NativeTextCommand::ReplaceText {
        island,
        text: "ЇA".into(),
    };
    session.apply_text(binding, 1, &command).unwrap();
    let output = session.output_bytes();
    let reopened = open(output);
    assert_eq!(
        reopened.snapshot().scene.text_pages[0].islands[0].text,
        "ЇA"
    );
    assert!(
        output
            .windows(CID_CONTENT.len())
            .any(|window| window == CID_CONTENT.as_bytes())
    );
    assert!(
        output
            .windows(b"/PrivateData 4 0 R".len())
            .any(|window| window == b"/PrivateData 4 0 R")
    );
}

use flow_core::pdf::native::{
    NativeEditLimits, NativeFieldCommand, NativeFieldRefusal, NativePdfSession,
};
use serde_json::json;

fn build_pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n", index + 1).bytes());
        pdf.extend_from_slice(object);
        pdf.extend(b"\nendobj\n");
    }
    let xref = pdf.len();
    let size = offsets.len() + 1;
    pdf.extend(format!("xref\n0 {size}\n0000000000 65535 f \n").bytes());
    for offset in offsets {
        pdf.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    pdf.extend(
        format!("trailer\n<< /Root 1 0 R /Size {size} >>\nstartxref\n{xref}\n%%EOF\n").bytes(),
    );
    pdf
}

fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut object = format!("<< /Length {} {entries} >>\nstream\n", data.len()).into_bytes();
    object.extend_from_slice(data);
    object.extend(b"\nendstream");
    object
}

fn session(bytes: &[u8]) -> NativePdfSession {
    NativePdfSession::open_scoped(bytes, NativeEditLimits::default(), [7; 16]).unwrap()
}

fn apply(session: &mut NativePdfSession, command: serde_json::Value) -> Result<(), &'static str> {
    let command: NativeFieldCommand = serde_json::from_value(command).unwrap();
    let snapshot = session.snapshot();
    session
        .apply_field(snapshot.binding, snapshot.consumed_request + 1, &command)
        .map(|_| ())
        .map_err(|error| error.code())
}

fn imported_checkbox(geometry: &str, extra_page_entries: &str, state: &str) -> Vec<u8> {
    imported_field(geometry, extra_page_entries, state, "Btn", "", "")
}

fn imported_field(
    geometry: &str,
    extra_page_entries: &str,
    state: &str,
    kind: &str,
    extra_form_entries: &str,
    extra_owner_entries: &str,
) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm 5 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] >>",
        &format!("<< /Type /Page /Parent 2 0 R /Annots [7 0 R] {extra_owner_entries} >>"),
        &format!("<< /Type /Page /Parent 2 0 R {extra_page_entries} >>"),
        &format!("<< /Fields [6 0 R] {extra_form_entries} >>"),
        &format!("<< /FT /{kind} /T (agreement) /Kids [7 0 R] /V /Off /DV /Off >>"),
        &format!("<< /Type /Annot /Subtype /Widget /P 3 0 R /Parent 6 0 R /Rect {geometry} /AP << /N << /Off 8 0 R /Agreed 9 0 R >> >> /AS /{state} >>"),
    ].into_iter().map(|s| s.as_bytes().to_vec()).collect();
    let form = "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Resources <<>>";
    objects.push(stream(
        form,
        b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\nQ\n",
    ));
    objects.push(stream(
        form,
        b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\n2 w 2 2 m 18 18 l 2 18 m 18 2 l S\nQ\n",
    ));
    build_pdf(&objects)
}

#[test]
fn imported_checkbox_uses_parent_value_and_keeps_its_on_state_and_default() {
    let original = imported_checkbox("[10 10 30 30]", "", "Off");
    let mut pdf = session(&original);
    assert_eq!(pdf.snapshot().fields[0].checked, Some(false));
    let field = pdf.snapshot().fields[0].id;
    apply(
        &mut pdf,
        json!({"kind":"setFieldChecked","field":field,"checked":true}),
    )
    .unwrap();

    let bytes = pdf.output_bytes();
    let serialized = String::from_utf8_lossy(bytes);
    assert!(serialized.contains("/AS /Agreed"));
    assert!(serialized.contains("/V /Agreed"));
    assert!(serialized.contains("/DV /Off"));
    assert!(serialized.contains("/Kids [7 0 R"));
    assert!(serialized.contains("/Parent 6 0 R"));
    let reopened = session(bytes);
    assert_eq!(reopened.snapshot().fields[0].checked, Some(true));

    let id = pdf.snapshot().fields[0].id;
    apply(
        &mut pdf,
        json!({"kind":"moveField","field":id,"page":1,"rect":[45,60,75,90]}),
    )
    .unwrap();
    assert_eq!(pdf.snapshot().fields[0].page, 1);
    assert_eq!(pdf.snapshot().fields[0].rect, Some([45, 60, 75, 90]));
    let id = pdf.snapshot().fields[0].id;
    apply(&mut pdf, json!({"kind":"removeField","field":id})).unwrap();
    assert!(pdf.snapshot().fields.is_empty());
    assert!(!String::from_utf8_lossy(pdf.output_bytes()).contains("/Subtype /Widget"));
    assert_eq!(pdf.origin_bytes(), original);
}

#[test]
fn forged_geometry_appearance_and_inherited_ownership_refuse_atomically() {
    for (bounds, extra, state, reason) in [
        ("[10 10 400 30]", "", "Off", NativeFieldRefusal::Geometry),
        (
            "[10 10 30 30]",
            "/Annots [7 0 R]",
            "Off",
            NativeFieldRefusal::Reference,
        ),
        (
            "[10 10 30 30]",
            "",
            "Agreed",
            NativeFieldRefusal::Appearance,
        ),
    ] {
        let bytes = imported_checkbox(bounds, extra, state);
        let mut pdf = session(&bytes);
        let entry = &pdf.snapshot().fields[0];
        assert_eq!(entry.refusal, Some(reason));
        assert!(!entry.can_edit_appearance);
        let id = entry.id;
        assert_eq!(
            apply(&mut pdf, json!({"kind":"removeField","field":id})),
            Err(reason.code())
        );
        assert_eq!(pdf.output_bytes(), bytes);
        assert_eq!(pdf.snapshot().binding.revision, 0);
    }
}

#[test]
fn embedded_appearance_round_trips_ukrainian_and_refuses_unmapped_glyphs() {
    let font = include_bytes!("../data/NotoSans-Regular.ttf");
    let face = ttf_parser::Face::parse(font, 0).unwrap();
    let gid = face.glyph_index('Ї').unwrap().0.to_be_bytes();
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapType 2 def\n/CMapName /Example def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n1 beginbfchar\n<0001> <0407>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend end";
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 500] /Annots [11 0 R] >>".to_vec(),
        b"<< /Fields [11 0 R] /DR << /Font << /FlowField 5 0 R >> >> >>".to_vec(),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /NotoSans-Regular /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NotoSans-Regular /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /W [1 [339]] /FontDescriptor 8 0 R /CIDToGIDMap 10 0 R >>".to_vec(),
        stream("", cmap),
        b"<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 /FontFile2 9 0 R >>".to_vec(),
        stream(&format!("/Length1 {}", font.len()), font),
        stream("", &[0, 0, gid[0], gid[1]]),
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (recipient) /V () /DV () /P 3 0 R /Rect [10 10 150 40] /AP << /N 12 0 R >> >>".to_vec(),
        stream("/Type /XObject /Subtype /Form /BBox [0 0 140 30] /Resources << /Font << /FlowField 5 0 R >> >>",
            b"q 1 g 0 0 140 30 re f 0 G 1 w 1 1 138 28 re S\nBT /FlowField 10 Tf 0 g 2 4 Td <> Tj ET\nQ\n"),
    ];
    let bytes = build_pdf(&objects);
    let mut pdf = session(&bytes);
    assert_eq!(pdf.snapshot().fields[0].value.as_deref(), Some(""));
    let id = pdf.snapshot().fields[0].id;
    apply(
        &mut pdf,
        json!({"kind":"setFieldText","field":id,"value":"Ї"}),
    )
    .unwrap();
    assert_eq!(pdf.snapshot().fields[0].value.as_deref(), Some("Ї"));
    assert!(
        pdf.output_bytes()
            .windows(b"<0001> Tj".len())
            .any(|window| window == b"<0001> Tj")
    );
    let accepted = pdf.output_bytes().to_vec();
    let id = pdf.snapshot().fields[0].id;
    assert_eq!(
        apply(
            &mut pdf,
            json!({"kind":"setFieldText","field":id,"value":"Ж"})
        ),
        Err(NativeFieldRefusal::Glyph.code())
    );
    assert_eq!(pdf.output_bytes(), accepted);
    assert_eq!(pdf.snapshot().binding.revision, 1);
}

#[test]
fn unsupported_controls_and_same_owner_aliases_remain_read_only() {
    for (kind, private_data, reason) in [
        ("Ch", "", NativeFieldRefusal::Profile),
        ("Btn", "/PrivateData 7 0 R", NativeFieldRefusal::Reference),
    ] {
        let bytes = imported_field("[10 10 30 30]", "", "Off", kind, "", private_data);
        let mut pdf = session(&bytes);
        let field = &pdf.snapshot().fields[0];
        assert_eq!(field.refusal, Some(reason));
        let id = field.id;
        assert_eq!(
            apply(&mut pdf, json!({"kind":"removeField","field":id})),
            Err(reason.code())
        );
        assert_eq!(pdf.output_bytes(), bytes);
    }
}

#[test]
fn xfa_and_signature_controls_refuse_at_admission() {
    for (kind, form) in [("Btn", "/XFA (packet)"), ("Sig", "")] {
        let bytes = imported_field("[10 10 30 30]", "", "Off", kind, form, "");
        let error = NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [7; 16])
            .err()
            .expect("unsupported form must refuse");
        assert_eq!(error.code(), "FLOW_PDF_NATIVE_UNSUPPORTED");
    }
}

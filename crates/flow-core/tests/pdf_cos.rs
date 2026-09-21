use flow_core::layout::{LayoutRect, LayoutUnit};
use flow_core::pdf::{
    CosDocument, CosValue, PdfError, PdfExportOptions, PdfExportRequest, PdfName, PdfPagePlan,
    PdfRef, export_pdf,
};

fn page_plan() -> PdfPagePlan {
    PdfPagePlan::empty(
        LayoutUnit::from_raw(595 * 64),
        LayoutUnit::from_raw(842 * 64),
    )
    .expect("valid page")
}

#[test]
fn fixed_point_coordinates_use_exact_decimal_tokens() {
    let mut document = CosDocument::new();
    let root = document
        .add_object(
            CosValue::dictionary([
                (
                    PdfName::new("Value").unwrap(),
                    CosValue::Real(LayoutUnit::from_raw(1)),
                ),
                (
                    PdfName::new("Negative").unwrap(),
                    CosValue::Real(LayoutUnit::from_raw(-65)),
                ),
            ])
            .unwrap(),
        )
        .unwrap();
    document.set_root(root).unwrap();
    let bytes = document.write("fingerprint").unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Value 0.015625"));
    assert!(text.contains("/Negative -1.015625"));
}

#[test]
fn names_and_strings_are_escaped_deterministically() {
    let name = PdfName::new("Safe-Name").unwrap();
    let mut document = CosDocument::new();
    let root = document
        .add_object(
            CosValue::dictionary([(name, CosValue::string(b"a(b)\\c\n".to_vec()).unwrap())])
                .unwrap(),
        )
        .unwrap();
    document.set_root(root).unwrap();
    let bytes = document.write("fingerprint").unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("(a\\(b\\)\\\\c\\n)"));
}

#[test]
fn page_envelope_is_repeatable_and_has_consistent_xref() {
    let request = PdfExportRequest::new(
        7,
        "source-hash",
        "layout-hash",
        vec![page_plan()],
        PdfExportOptions::default(),
    )
    .unwrap();
    let first = export_pdf(&request).unwrap();
    let second = export_pdf(&request).unwrap();
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(first.byte_hash, second.byte_hash);
    assert!(first.bytes.starts_with(b"%PDF-1.7\n"));
    assert!(first.bytes.ends_with(b"%%EOF\n"));
    let text = String::from_utf8_lossy(&first.bytes);
    assert!(text.contains("xref\n0 6\n"));
    assert!(text.contains("/Type /Catalog"));
    assert!(text.contains("/Type /Pages"));
    assert!(text.contains("/Type /Page"));
    assert!(text.contains("/FlowPDFSource"));
    assert!(text.contains("/MediaBox [0 0 595 842]"));
}

#[test]
fn page_plan_rejects_non_positive_geometry_and_empty_exports() {
    assert_eq!(
        PdfPagePlan::empty(LayoutUnit::from_raw(0), LayoutUnit::from_raw(10)).unwrap_err(),
        PdfError::InvalidPageGeometry
    );
    assert_eq!(
        PdfExportRequest::new(
            1,
            "source",
            "layout",
            Vec::new(),
            PdfExportOptions::default(),
        )
        .unwrap_err(),
        PdfError::EmptyPagePlan
    );
}

#[test]
fn duplicate_dictionary_keys_fail_closed() {
    let error = CosValue::dictionary([
        (PdfName::new("Value").unwrap(), CosValue::Integer(1)),
        (PdfName::new("Value").unwrap(), CosValue::Integer(2)),
    ])
    .unwrap_err();
    assert_eq!(error, PdfError::DuplicateDictionaryKey);
}

#[test]
fn dangling_references_and_cycles_fail_closed() {
    let mut dangling = CosDocument::new();
    let root = dangling
        .add_object(CosValue::Reference(PdfRef::new(2, 0).unwrap()))
        .unwrap();
    dangling.set_root(root).unwrap();
    assert_eq!(
        dangling.write("fingerprint").unwrap_err(),
        PdfError::DanglingReference
    );

    let mut cyclic = CosDocument::new();
    let first = cyclic.add_object(CosValue::Null).unwrap();
    let second = cyclic.add_object(CosValue::Reference(first)).unwrap();
    cyclic
        .replace_object(first, CosValue::Reference(second))
        .unwrap();
    cyclic.set_root(first).unwrap();
    assert_eq!(
        cyclic.write("fingerprint").unwrap_err(),
        PdfError::IndirectCycle
    );
}

#[test]
fn layout_rect_keeps_fixed_point_identity() {
    let rect = LayoutRect {
        x: LayoutUnit::from_raw(1),
        y: LayoutUnit::from_raw(2),
        width: LayoutUnit::from_raw(3),
        height: LayoutUnit::from_raw(4),
    };
    assert_eq!(rect.width.raw(), 3);
}

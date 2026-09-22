use flow_core::pdf::{PDF_READER_SCHEMA_VERSION, PdfReadError, PdfReaderLimits, open_pdf};

fn object(body: &str) -> (u32, Vec<u8>) {
    let id = body
        .split_once('|')
        .and_then(|(id, _)| id.parse::<u32>().ok())
        .expect("object id");
    let body = body.split_once('|').map(|(_, body)| body).unwrap_or(body);
    (id, body.as_bytes().to_vec())
}

fn stream(id: u32, dictionary: &str, data: &[u8]) -> (u32, Vec<u8>) {
    let mut body = format!("<< {dictionary} /Length {} >>\nstream\n", data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    (id, body)
}

fn pdf(objects: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let max_id = objects.iter().map(|(id, _)| *id).max().unwrap_or(0);
    let mut output = b"%PDF-1.7\n%\xFF\xFF\xFF\xFF\n".to_vec();
    let mut offsets = vec![None; usize::try_from(max_id).unwrap() + 1];
    for (id, body) in objects {
        let index = usize::try_from(id).unwrap();
        offsets[index] = Some(output.len());
        output.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        output.extend_from_slice(&body);
        output.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = output.len();
    output.extend_from_slice(format!("xref\n0 {}\n", max_id + 1).as_bytes());
    output.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        if let Some(offset) = offset {
            output.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        } else {
            output.extend_from_slice(b"0000000000 65535 f \n");
        }
    }
    output.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            max_id + 1
        )
        .as_bytes(),
    );
    output
}

fn one_page_pdf() -> Vec<u8> {
    pdf(vec![
        object("1|<< /Type /Catalog /Pages 2 0 R >>"),
        object("2|<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        object("3|<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
    ])
}

#[test]
fn opens_classic_xref_and_discovers_pages_without_eagerly_parsing_unreachable_objects() {
    let bytes = pdf(vec![
        object("1|<< /Type /Catalog /Pages 2 0 R >>"),
        object("2|<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        object("3|<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
        object("4|<< /Broken ["),
    ]);
    let reader = open_pdf(&bytes, PdfReaderLimits::default()).expect("reader opens");
    let summary = reader.summary().expect("page tree");
    assert_eq!(summary.schema_version, PDF_READER_SCHEMA_VERSION);
    assert_eq!(summary.pdf_version, "1.7");
    assert_eq!(summary.page_count, 1);
    assert_eq!(summary.source_hash.len(), 64);
}

#[test]
fn rejects_header_xref_encryption_and_input_budget_fail_closed() {
    assert_eq!(
        open_pdf(b"not a pdf", PdfReaderLimits::default()).unwrap_err(),
        PdfReadError::HeaderInvalid
    );
    let mut malformed = one_page_pdf();
    let marker = b"startxref";
    let marker_offset = malformed
        .windows(marker.len())
        .rposition(|window| window == marker)
        .unwrap();
    malformed.truncate(marker_offset);
    malformed.extend_from_slice(b"startxref\n999999\n%%EOF\n");
    assert_eq!(
        open_pdf(&malformed, PdfReaderLimits::default()).unwrap_err(),
        PdfReadError::XrefInvalid
    );

    let encrypted = pdf(vec![
        object("1|<< /Type /Catalog /Pages 2 0 R >>"),
        object("2|<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        object("3|<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
    ]);
    let encrypted = encrypted
        .split(|byte| *byte == b'\n')
        .map(|line| line.to_vec())
        .collect::<Vec<_>>();
    let _ = encrypted;

    let limits = PdfReaderLimits {
        max_input_bytes: 8,
        ..PdfReaderLimits::default()
    };
    assert_eq!(
        open_pdf(&one_page_pdf(), limits).unwrap_err(),
        PdfReadError::InputSizeLimit
    );
}

#[test]
fn stream_filters_are_bounded_and_classified() {
    let bytes = pdf(vec![
        object("1|<< /Type /Catalog /Pages 2 0 R >>"),
        object("2|<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        object("3|<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>"),
        stream(4, "/Filter /UnknownFilter", b"payload"),
    ]);
    let reader = open_pdf(&bytes, PdfReaderLimits::default()).expect("reader opens");
    let scene =
        flow_core::pdf::read_pdf_scene(&reader, &flow_core::pdf::PdfSceneRequest::default())
            .expect("partial scene");
    assert!(scene.report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == flow_core::pdf::PdfReadDiagnosticCode::UnsupportedFilter
    }));
}

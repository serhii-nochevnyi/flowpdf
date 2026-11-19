use std::fmt::Write;

use flow_core::{NativeEditLimits, NativeNoteCommand, NativePdfSession};

pub(crate) fn fixture(extra: &str, annots: &str) -> Vec<u8> {
    fixture_with_contents(extra, annots, "(Imported)")
}

pub(crate) fn fixture_with_contents(extra: &str, annots: &str, contents: &str) -> Vec<u8> {
    fixture_with_resources(extra, annots, contents, "")
}

fn fixture_with_resources(
    extra: &str,
    annots: &str,
    contents: &str,
    page_resources: &str,
) -> Vec<u8> {
    let objects = [
        (11, 2, format!("<< /Type /Catalog /Pages 12 0 R /Custom 26 9 R {extra} >>")),
        (12, 0, "<< /Type /Pages /Kids [13 3 R 14 0 R] /Count 2 /MediaBox [0 0 200 120] /Resources 26 9 R >>".into()),
        (13, 3, format!("<< /Type /Page /Parent 12 0 R /CropBox [10 20 170 110] /Rotate 90 /Contents 21 0 R {annots} {page_resources} >>")),
        (14, 0, "<< /Type /Page /Parent 12 0 R /Contents 21 0 R /Resources 26 9 R >>".into()),
        (21, 0, "<< /Length 22 7 R /Filter /ASCIIHexDecode >>\nstream\n71 20 51>\nendstream".into()),
        (22, 7, "9".into()),
        (26, 9, "<< /Custom /A#23B#FF /D +.1234567890123456789 /V [true false <00FF>] >>".into()),
        (30, 5, format!("<< /Type /Annot /Subtype /Text /Contents {contents} /Rect [20 30 40 50] /P 13 3 R >>")),
        (31, 0, "<< /Type /Annot /Subtype /Square /Rect [20 30 40 50] >>".into()),
        (32, 0, "<< /Subtype /Text /Contents (With appearance) /Rect [20 30 40 50] /AP << /N 21 0 R >> >>".into()),
        (33, 0, "[30 5 R 31 0 R 32 0 R]".into()),
        (34, 0, "<< /Type /Annot /Subtype /Square /Rect [20 30 40 50] /AP << /N 35 0 R >> /AS /A >>".into()),
        (35, 0, "<< /A 36 0 R >>".into()),
        (36, 0, "<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 15 >>\nstream\n0 0 20 20 re f\n\nendstream".into()),
        (37, 0, "<< /A 36 0 R /AA 36 0 R >>".into()),
        (99, 0, "<< /Broken 98 0 R /UnreachableDecoy true >>".into()),
    ];
    let mut pdf = "%PDF-1.7\n".to_owned();
    let mut offsets = Vec::new();
    for (id, generation, value) in objects {
        offsets.push((id, generation, pdf.len()));
        write!(pdf, "{id} {generation} obj\n{value}\nendobj\n").unwrap();
    }
    let xref = pdf.len();
    pdf.push_str("xref\n0 1\n0000000000 65535 f \n");
    for (id, generation, offset) in offsets {
        write!(pdf, "{id} 1\n{offset:010} {generation:05} n \n").unwrap();
    }
    write!(
        pdf,
        "trailer\n<< /Size 100 /Root 11 2 R >>\nstartxref\n{xref}\n%%EOF\n"
    )
    .unwrap();
    pdf.into_bytes()
}

#[test]
fn rewrite_preserves_exact_values_and_shared_encoded_stream() {
    let source = fixture("", "/Annots 33 0 R");
    let mut session =
        NativePdfSession::open_scoped(&source, NativeEditLimits::default(), [1; 16]).unwrap();
    let page = session.snapshot().scene.pages[0].id;
    let binding = session.snapshot().binding;
    session
        .apply(
            binding,
            1,
            &NativeNoteCommand::Add {
                page,
                rect: [20, 30, 40, 50],
                contents: "Примітка".into(),
            },
        )
        .unwrap();
    let output = session.output_bytes();
    let syntax = String::from_utf8_lossy(output);
    assert!(syntax.contains("/D +.1234567890123456789"));
    assert!(syntax.contains("/Custom /A#23B#FF /D"));
    assert!(syntax.contains("/V [true false <00FF> ]"));
    assert_eq!(syntax.matches("/Resources 7 0 R").count(), 2);
    assert_eq!(syntax.matches("/Contents 5 0 R").count(), 2);
    assert_eq!(syntax.matches("stream\n71 20 51>\nendstream").count(), 1);
    assert!(syntax.contains("/Length 6 0 R"));
    assert!(!syntax.contains("UnreachableDecoy"));
    assert_eq!(session.origin_bytes(), source);
}

#[test]
fn unsafe_reachable_values_refuse_admission() {
    for extra in ["/Bad 99 0 R", "/Bad 26 0 R", "/OpenAction (private)"] {
        let result = NativePdfSession::open_scoped(
            &fixture(extra, ""),
            NativeEditLimits::default(),
            [1; 16],
        );
        assert!(result.is_err());
    }
}

#[test]
fn passive_resource_names_and_appearance_states_survive_rewrite() {
    let source = fixture_with_resources(
        "",
        "/Annots [34 0 R]",
        "(Imported)",
        "/Resources << /XObject 37 0 R >>",
    );
    let mut session =
        NativePdfSession::open_scoped(&source, NativeEditLimits::default(), [1; 16]).unwrap();
    let page = session.snapshot().scene.pages[0].id;
    session
        .apply(
            session.snapshot().binding,
            1,
            &NativeNoteCommand::Add {
                page,
                rect: [20, 30, 40, 50],
                contents: "Passive names".into(),
            },
        )
        .unwrap();
    let syntax = String::from_utf8_lossy(session.output_bytes());
    assert_eq!(syntax.matches("/A ").count(), 3);
    assert!(syntax.contains("/AA "));
    assert!(syntax.contains("/AS /A"));
    assert_eq!(syntax.matches("/Subtype /Form").count(), 1);
}

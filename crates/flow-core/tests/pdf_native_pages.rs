use flow_core::pdf::native::{NativeEditLimits, NativePageCommand, NativePdfSession};

fn fixture() -> Vec<u8> {
    build_pdf(&[
        (0, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 500] /Rotate 90 >>",
        ),
        (0, "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 >>"),
        (0, "<< /Type /Page /Parent 3 0 R >>"),
    ])
}

fn build_pdf(objects: &[(u16, &str)]) -> Vec<u8> {
    build_pdf_with_trailer(objects, "")
}

fn build_pdf_with_trailer(objects: &[(u16, &str)], trailer: &str) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![];
    for (index, (generation, object)) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} {generation} obj\n{object}\nendobj\n", index + 1).bytes());
    }
    let xref = pdf.len();
    let size = objects.len() + 1;
    pdf.extend(format!("xref\n0 {size}\n0000000000 65535 f \n").bytes());
    for (offset, (generation, _)) in offsets.into_iter().zip(objects) {
        pdf.extend(format!("{offset:010} {generation:05} n \n").bytes());
    }
    pdf.extend(
        format!("trailer\n<< /Root 1 0 R /Size {size}{trailer} >>\nstartxref\n{xref}\n%%EOF\n")
            .bytes(),
    );
    pdf
}

#[test]
fn page_operations_keep_identity_through_rewrite() {
    let mut session =
        NativePdfSession::open_scoped(&fixture(), NativeEditLimits::default(), [1; 16]).unwrap();
    for (step, command) in [
        r#"{"kind":"insertBlankPage","target":{"page":0,"stableId":262144},"side":"after"}"#,
        r#"{"kind":"rotatePage","target":{"page":0,"stableId":262144},"degrees":90}"#,
        r#"{"kind":"reorderPage","target":{"page":0,"stableId":262144},"position":1}"#,
        r#"{"kind":"duplicatePage","target":{"page":1,"stableId":262144}}"#,
        r#"{"kind":"deletePage","target":{"page":1,"stableId":262144}}"#,
    ]
    .iter()
    .enumerate()
    {
        let command: NativePageCommand = serde_json::from_str(command).unwrap();
        session
            .apply_page(session.snapshot().binding, step as u64 + 1, &command)
            .unwrap();
    }
    assert_eq!(session.snapshot().page_state.order.len(), 2);
    assert_eq!(
        session.snapshot().page_state.selected,
        Some(session.snapshot().page_state.order[1])
    );
    assert_eq!(
        session.snapshot().scene.pages[1]
            .geometry
            .unwrap()
            .rotation(),
        180
    );
}

fn command(
    kind: &str,
    page: usize,
    snapshot: &flow_core::pdf::native::NativeSnapshot,
) -> NativePageCommand {
    serde_json::from_value(serde_json::json!({
        "kind": kind, "target": {"page":page,"stableId":snapshot.page_state.order[page]},
    }))
    .unwrap()
}

fn dependency_fixture(catalog_entry: &str) -> Vec<u8> {
    build_pdf(&[
        (
            0,
            &format!(
                "<< /Type /Catalog /Pages 2 0 R /AcroForm 9 0 R /Outlines 12 0 R {catalog_entry} >>"
            ),
        ),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 /MediaBox [0 0 300 500] >>",
        ),
        (
            0,
            "<< /Type /Pages /Parent 2 0 R /Kids [4 7 R] /Count 1 /Rotate 90 /Resources 14 0 R >>",
        ),
        (
            7,
            "<< /Type /Page /Parent 3 0 R /Contents 11 0 R /Annots [7 0 R 8 0 R] >>",
        ),
        (
            0,
            "<< /Type /Pages /Parent 2 0 R /Kids [6 0 R] /Count 1 /Rotate 180 /Resources <<>> >>",
        ),
        (
            0,
            "<< /Type /Page /Parent 5 0 R /Contents 11 0 R /Annots [10 0 R] >>",
        ),
        (
            0,
            "<< /Type /Annot /Subtype /Text /P 4 7 R /Rect [10 20 40 60] /Contents (original) >>",
        ),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /P 4 7 R /Rect [40 60 80 90] /FT /Btn /T (check) /V /Off /AS /Off >>",
        ),
        (0, "<< /Fields [8 0 R] >>"),
        (
            0,
            "<< /Type /Annot /Subtype /Link /P 6 0 R /Rect [20 30 50 60] /Dest [4 7 R /Fit] >>",
        ),
        (0, "<< /Length 4 >>\nstream\nq Q\n\nendstream"),
        (
            0,
            "<< /Type /Outlines /First 13 0 R /Last 13 0 R /Count 1 >>",
        ),
        (0, "<< /Title (origin) /Parent 12 0 R /Dest [4 7 R /Fit] >>"),
        (0, "<< /ProcSet [/PDF ] >>"),
    ])
}

#[test]
fn independent_note_and_checkbox_clone_survive_cross_parent_reorder_and_navigation_cleanup() {
    use flow_core::pdf::NativePdfAnnotation;
    use flow_core::pdf::native::NativeNoteCommand;
    let mut session = NativePdfSession::open_scoped(
        &dependency_fixture(""),
        NativeEditLimits::default(),
        [2; 16],
    )
    .unwrap();
    assert_eq!(session.snapshot().page_state.order[0], (4 << 16) + 7);
    let duplicate = command("duplicatePage", 0, session.snapshot());
    session
        .apply_page(session.snapshot().binding, 1, &duplicate)
        .unwrap();
    let page = |session: &NativePdfSession, index: usize| {
        let NativePdfAnnotation::PlainNote(note) =
            &session.snapshot().scene.pages[index].annotations[0]
        else {
            panic!("expected an editable note");
        };
        (note.id, note.contents.clone())
    };
    let (source_note, _) = page(&session, 0);
    let (new_note, _) = page(&session, 1);
    assert_ne!(source_note, new_note);
    session
        .apply(
            session.snapshot().binding,
            2,
            &NativeNoteCommand::Edit {
                note: new_note,
                contents: "copy".into(),
            },
        )
        .unwrap();
    assert_eq!(page(&session, 0).1, "original");
    assert_eq!(page(&session, 1).1, "copy");
    let rotate = session.snapshot().scene.pages[1].geometry.unwrap();
    assert_eq!(
        rotate.to_rotated_crop([10, 20, 40, 60]),
        Some([20, 260, 60, 290])
    );
    let reorder: NativePageCommand = serde_json::from_value(serde_json::json!({
        "kind":"reorderPage", "target":{"page":1,"stableId":session.snapshot().page_state.order[1]}, "position":2
    })).unwrap();
    session
        .apply_page(session.snapshot().binding, 3, &reorder)
        .unwrap();
    assert_eq!(
        session.snapshot().scene.pages[1]
            .geometry
            .unwrap()
            .rotation(),
        180
    );
    assert_eq!(session.snapshot().scene.pages[2].geometry, Some(rotate));
    let delete = command("deletePage", 0, session.snapshot());
    session
        .apply_page(session.snapshot().binding, 4, &delete)
        .unwrap();
    assert!(session.snapshot().scene.pages[0].annotations.is_empty());
    assert_eq!(page(&session, 1).1, "copy");
    assert_eq!(session.snapshot().scene.pages[1].annotations.len(), 2);
    let output = String::from_utf8_lossy(session.output_bytes());
    assert_eq!(output.matches("/Subtype /Widget").count(), 1);
    assert_eq!(output.matches("/ProcSet [/PDF ]").count(), 1);
    assert_eq!(output.matches("/Length 4").count(), 1);
    assert!(!output.contains("/Title"));
}

#[test]
fn final_page_and_external_dependencies_refuse_without_changing_output() {
    for (bytes, code) in [
        (fixture(), "FLOW_PDF_NATIVE_FINAL_PAGE"),
        (
            dependency_fixture("/Dests << /Permanent [4 7 R /Fit] >>"),
            "FLOW_PDF_NATIVE_UNSUPPORTED",
        ),
    ] {
        let mut session =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
        let prior = session.snapshot().clone();
        let delete = command("deletePage", 0, &prior);
        assert_eq!(
            session
                .apply_page(prior.binding, 1, &delete)
                .unwrap_err()
                .code(),
            code
        );
        assert_eq!(session.output_bytes(), bytes);
        assert_eq!(session.snapshot().binding, prior.binding);
        assert_eq!(session.snapshot().page_state, prior.page_state);
    }
}

#[test]
fn named_destination_restrictions_follow_indirect_targets() {
    for catalog in ["/Dests << /Chapter 6 0 R >>", "/Names << /Dests 5 0 R >>"] {
        let bytes = build_pdf(&[
            (0, &format!("<< /Type /Catalog /Pages 2 0 R {catalog} >>")),
            (
                0,
                "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] /Resources <<>> >>",
            ),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (0, "<< /Names [(Chapter) 6 0 R] >>"),
            (0, "[3 0 R /Fit]"),
        ]);
        let mut session =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
        let prior = session.snapshot().clone();
        let pages = &prior.page_state.restrictions.as_ref().unwrap().pages;
        assert!(pages[0].delete.is_some());
        assert!(pages[1].delete.is_none());
        assert_eq!(
            session
                .apply_page(prior.binding, 1, &command("deletePage", 0, &prior))
                .unwrap_err()
                .code(),
            "FLOW_PDF_NATIVE_UNSUPPORTED",
        );
        assert_eq!(session.output_bytes(), bytes);
        session
            .apply_page(prior.binding, 2, &command("deletePage", 1, &prior))
            .unwrap();
        assert_eq!(
            session.snapshot().page_state.order,
            prior.page_state.order[..1]
        );
    }
}

#[test]
fn incoming_links_restrict_only_targets_with_unsupported_cleanup() {
    use flow_core::layout::LayoutUnit;
    use flow_core::pdf::{NativePdfAnnotation, NativeReadOnlyAnnotation};

    let bytes = build_pdf(&[
        (
            0,
            "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [9 0 R] /D << /ON [9 0 R] >> >> >>",
        ),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R] /Count 4 /MediaBox [0 0 300 500] /Resources <<>> >>",
        ),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (0, "<< /Type /Page /Parent 2 0 R /Annots [7 0 R 8 0 R] >>"),
        (
            0,
            "<< /Type /Annot /Subtype /Link /P 6 0 R /Rect [20 30 50 60] /Dest [3 0 R /Fit] >>",
        ),
        (
            0,
            "<< /Type /Annot /Subtype /Link /P 6 0 R /Rect [20 70 50 90] /Dest 10 0 R /OC 9 0 R >>",
        ),
        (0, "<< /Type /OCG /Name (Layer) >>"),
        (0, "[4 0 R /Fit]"),
    ]);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    let pages = &prior.page_state.restrictions.as_ref().unwrap().pages;
    assert!(pages[0].delete.is_none());
    assert!(pages[1].delete.is_some());
    assert!(pages[2].delete.is_none());
    assert_eq!(
        session
            .apply_page(prior.binding, 1, &command("deletePage", 1, &prior))
            .unwrap_err()
            .code(),
        "FLOW_PDF_NATIVE_UNSUPPORTED",
    );
    assert_eq!(session.output_bytes(), bytes);
    assert_eq!(session.snapshot().binding, prior.binding);
    assert_eq!(session.snapshot().page_state, prior.page_state);

    session
        .apply_page(prior.binding, 2, &command("deletePage", 0, &prior))
        .unwrap();
    let after_cleanup = session.snapshot();
    assert_eq!(after_cleanup.page_state.order, prior.page_state.order[1..]);
    let pages = &after_cleanup
        .page_state
        .restrictions
        .as_ref()
        .unwrap()
        .pages;
    assert!(pages[0].delete.is_some());
    assert!(pages[1].delete.is_none());
    session
        .apply_page(
            after_cleanup.binding,
            3,
            &command("deletePage", 1, after_cleanup),
        )
        .unwrap();
    let after_delete = session.snapshot();
    assert_eq!(
        after_delete.page_state.order,
        [prior.page_state.order[1], prior.page_state.order[3]],
    );
    let [
        NativePdfAnnotation::ReadOnly {
            profile: NativeReadOnlyAnnotation::Link,
            rect: Some(rect),
            ..
        },
    ] = after_delete.scene.pages[1].annotations.as_slice()
    else {
        panic!("expected only the read-only link to remain");
    };
    assert_eq!(
        rect.map(LayoutUnit::raw),
        [20, 70, 50, 90].map(|points| points * LayoutUnit::UNITS_PER_POINT),
    );
    assert!(
        after_delete.page_state.restrictions.as_ref().unwrap().pages[0]
            .delete
            .is_some()
    );
}

#[test]
fn trailer_owned_navigation_blocks_only_its_target() {
    let objects = [
        (0, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 300 500] /Resources <<>> >>",
        ),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (0, "<< /Type /Page /Parent 2 0 R /Annots 7 0 R >>"),
        (
            0,
            "<< /Type /Annot /Subtype /Link /P 5 0 R /Rect [20 30 50 60] /Dest 8 0 R >>",
        ),
        (0, "[6 0 R]"),
        (0, "[3 0 R /Fit]"),
    ];
    for trailer in [
        " /FlowPDFNavigation 6 0 R",
        " /FlowPDFNavigation << /Arrays [7 0 R] >>",
        " /FlowPDFNavigation 8 0 R",
        " /FlowPDFNavigation 3 0 R",
    ] {
        let bytes = build_pdf_with_trailer(&objects, trailer);
        let mut session =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
        let prior = session.snapshot().clone();
        let pages = &prior.page_state.restrictions.as_ref().unwrap().pages;
        assert!(pages[0].delete.is_some(), "{trailer}");
        assert!(pages[1].delete.is_none(), "{trailer}");
        assert_eq!(
            session
                .apply_page(prior.binding, 1, &command("deletePage", 0, &prior))
                .unwrap_err()
                .code(),
            "FLOW_PDF_NATIVE_UNSUPPORTED",
        );
        assert_eq!(session.output_bytes(), bytes);
        assert_eq!(session.snapshot().binding, prior.binding);
        assert_eq!(session.snapshot().page_state, prior.page_state);

        session
            .apply_page(prior.binding, 2, &command("deletePage", 1, &prior))
            .unwrap();
        let after = session.snapshot();
        assert_eq!(
            after.page_state.order,
            [prior.page_state.order[0], prior.page_state.order[2]]
        );
        assert_eq!(after.scene.pages[1].annotations.len(), 1);
        assert!(
            after.page_state.restrictions.as_ref().unwrap().pages[0]
                .delete
                .is_some()
        );
    }
}

#[test]
fn retained_references_outside_navigation_block_only_the_target_page() {
    for (tree_entry, page_entry) in [
        ("/FlowPDFRetained [3 0 R]", ""),
        ("", "/FlowPDFRetained 6 0 R"),
    ] {
        let bytes = build_pdf(&[
            (0, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                0,
                &format!(
                    "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 300 500] {tree_entry} >>"
                ),
            ),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (0, &format!("<< /Type /Page /Parent 2 0 R {page_entry} >>")),
            (0, "<< /References [3 0 R] >>"),
        ]);
        let mut session =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
        let prior = session.snapshot().clone();
        let capabilities = prior.page_state.restrictions.as_ref().unwrap();
        assert!(capabilities.pages[0].delete.is_some());
        assert!(capabilities.pages[1].delete.is_none());

        assert_eq!(
            session
                .apply_page(prior.binding, 1, &command("deletePage", 0, &prior))
                .unwrap_err()
                .code(),
            "FLOW_PDF_NATIVE_UNSUPPORTED"
        );
        assert_eq!(session.output_bytes(), bytes);
        assert_eq!(session.snapshot().binding, prior.binding);
        assert_eq!(session.snapshot().page_state, prior.page_state);

        session
            .apply_page(prior.binding, 2, &command("deletePage", 1, &prior))
            .unwrap();
        let after = &session.snapshot().page_state;
        assert_eq!(
            after.order,
            [prior.page_state.order[0], prior.page_state.order[2]]
        );
        assert!(
            after.restrictions.as_ref().unwrap().pages[0]
                .delete
                .is_some()
        );
        assert!(String::from_utf8_lossy(session.output_bytes()).contains("/FlowPDFRetained"));
    }
}

#[test]
fn retained_annotation_references_block_only_the_link_target() {
    for (annots, retained) in [
        ("[6 0 R]", ""),
        ("7 0 R", ""),
        ("[6 0 R]", "/FlowPDFRetained << /References [6 0 R] >>"),
        ("7 0 R", "/FlowPDFRetained << /References [7 0 R] >>"),
    ] {
        let bytes = build_pdf(&[
            (0, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                0,
                "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 300 500] /Resources <<>> >>",
            ),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (
                0,
                &format!("<< /Type /Page /Parent 2 0 R /Annots {annots} {retained} >>"),
            ),
            (
                0,
                "<< /Type /Annot /Subtype /Link /P 5 0 R /Rect [20 30 50 60] /Dest [3 0 R /Fit] >>",
            ),
            (0, "[6 0 R]"),
        ]);
        let mut session =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
        let prior = session.snapshot().clone();
        assert_eq!(prior.scene.pages[2].annotations.len(), 1);
        let restrictions = prior.page_state.restrictions.as_ref();
        assert_eq!(
            restrictions.is_some_and(|caps| caps.pages[0].delete.is_some()),
            !retained.is_empty(),
            "{annots} {retained}",
        );
        assert!(restrictions.is_none_or(|caps| caps.pages[1].delete.is_none()));
        let delete = command("deletePage", 0, &prior);
        if retained.is_empty() {
            session.apply_page(prior.binding, 1, &delete).unwrap();
            assert_eq!(
                session.snapshot().page_state.order,
                [prior.page_state.order[1], prior.page_state.order[2]]
            );
            assert!(session.snapshot().scene.pages[1].annotations.is_empty());
            continue;
        }

        assert_eq!(
            session
                .apply_page(prior.binding, 1, &delete)
                .unwrap_err()
                .code(),
            "FLOW_PDF_NATIVE_UNSUPPORTED",
        );
        assert_eq!(session.output_bytes(), bytes);
        assert_eq!(session.snapshot().binding, prior.binding);
        assert_eq!(session.snapshot().page_state, prior.page_state);

        session
            .apply_page(prior.binding, 2, &command("deletePage", 1, &prior))
            .unwrap();
        let after = session.snapshot();
        assert_eq!(
            after.page_state.order,
            [prior.page_state.order[0], prior.page_state.order[2]]
        );
        assert_eq!(after.scene.pages[1].annotations.len(), 1);
        assert!(String::from_utf8_lossy(session.output_bytes()).contains("/FlowPDFRetained"));
        assert!(
            after.page_state.restrictions.as_ref().unwrap().pages[0]
                .delete
                .is_some()
        );
    }
}

#[test]
fn deleting_page_removes_both_links_with_a_shared_destination() {
    for (first_annots, second_annots, link_owner) in
        [("", "[5 0 R 6 0 R]", 4), ("/Annots [6 0 R]", "[5 0 R]", 3)]
    {
        let bytes = build_pdf(&[
            (0, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                0,
                "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] /Resources <<>> >>",
            ),
            (
                0,
                &format!("<< /Type /Page /Parent 2 0 R {first_annots} >>"),
            ),
            (
                0,
                &format!("<< /Type /Page /Parent 2 0 R /Annots {second_annots} >>"),
            ),
            (
                0,
                "<< /Type /Annot /Subtype /Link /P 4 0 R /Rect [20 30 50 60] /Dest 7 0 R >>",
            ),
            (
                0,
                &format!(
                    "<< /Type /Annot /Subtype /Link /P {link_owner} 0 R /Rect [60 30 90 60] /Dest 7 0 R >>"
                ),
            ),
            (0, "[3 0 R /Fit]"),
        ]);
        let mut session =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
        let prior = session.snapshot().clone();
        assert!(
            prior
                .page_state
                .restrictions
                .as_ref()
                .is_none_or(|caps| caps.pages[0].delete.is_none())
        );
        session
            .apply_page(prior.binding, 1, &command("deletePage", 0, &prior))
            .unwrap();
        assert_eq!(
            session.snapshot().page_state.order,
            [prior.page_state.order[1]]
        );
        assert!(session.snapshot().scene.pages[0].annotations.is_empty());
    }
}

#[test]
fn deleting_page_removes_a_link_and_bookmark_with_a_shared_destination() {
    let bytes = build_pdf(&[
        (0, "<< /Type /Catalog /Pages 2 0 R /Outlines 7 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] /Resources <<>> >>",
        ),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (0, "<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>"),
        (
            0,
            "<< /Type /Annot /Subtype /Link /P 4 0 R /Rect [20 30 50 60] /Dest 6 0 R >>",
        ),
        (0, "[3 0 R /Fit]"),
        (0, "<< /Type /Outlines /First 8 0 R /Last 8 0 R /Count 1 >>"),
        (0, "<< /Title (Chapter) /Parent 7 0 R /Dest 6 0 R >>"),
    ]);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    assert!(
        prior
            .page_state
            .restrictions
            .as_ref()
            .is_none_or(|caps| caps.pages[0].delete.is_none())
    );
    session
        .apply_page(prior.binding, 1, &command("deletePage", 0, &prior))
        .unwrap();
    assert_eq!(
        session.snapshot().page_state.order,
        [prior.page_state.order[1]]
    );
    assert!(session.snapshot().scene.pages[0].annotations.is_empty());
    assert!(!String::from_utf8_lossy(session.output_bytes()).contains("/Dest"));
}

#[test]
fn retained_bookmark_destination_blocks_only_its_target() {
    for annotations in ["[14 0 R]", "[6 0 R 14 0 R]"] {
        let bytes = build_pdf_with_trailer(
            &[
                (0, "<< /Type /Catalog /Pages 2 0 R /Outlines 8 0 R >>"),
                (
                    0,
                    "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 300 500] /Resources <<>> >>",
                ),
                (0, "<< /Type /Page /Parent 2 0 R >>"),
                (0, "<< /Type /Page /Parent 2 0 R /Annots [12 0 R] >>"),
                (
                    0,
                    &format!("<< /Type /Page /Parent 2 0 R /Annots {annotations} >>"),
                ),
                (
                    0,
                    "<< /Type /Annot /Subtype /Link /P 5 0 R /Rect [20 30 50 60] /Dest 7 0 R >>",
                ),
                (0, "[3 0 R /Fit]"),
                (
                    0,
                    "<< /Type /Outlines /First 9 0 R /Last 13 0 R /Count 2 >>",
                ),
                (
                    0,
                    "<< /Title (Chapter) /Parent 8 0 R /Next 13 0 R /Dest 10 0 R >>",
                ),
                (0, "[3 0 R /Fit]"),
                (0, "[5 0 R /Fit]"),
                (
                    0,
                    "<< /Type /Annot /Subtype /Link /P 4 0 R /Rect [20 30 50 60] /Dest 11 0 R >>",
                ),
                (
                    0,
                    "<< /Title (Surviving) /Parent 8 0 R /Prev 9 0 R /Dest 11 0 R >>",
                ),
                (
                    0,
                    "<< /Type /Annot /Subtype /Link /P 5 0 R /Rect [80 30 110 60] /Dest [4 0 R /Fit] >>",
                ),
            ],
            " /FlowPDFNavigation 10 0 R",
        );
        let mut session =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
        let prior = session.snapshot().clone();
        let pages = &prior.page_state.restrictions.as_ref().unwrap().pages;
        assert!(pages[0].delete.is_some());
        assert!(pages[1].delete.is_none());
        assert!(
            session
                .apply_page(prior.binding, 1, &command("deletePage", 0, &prior))
                .is_err()
        );
        assert_eq!(session.output_bytes(), bytes);
        session
            .apply_page(prior.binding, 2, &command("deletePage", 1, &prior))
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(session.output_bytes())
                .matches("/Title")
                .count(),
            2
        );
        assert_eq!(
            session.snapshot().page_state.order,
            [prior.page_state.order[0], prior.page_state.order[2]]
        );
    }
}

#[test]
fn page_widget_operations_share_calculation_order_admission() {
    for (order, available) in [("[]", true), ("[5 0 R]", false)] {
        let bytes = build_pdf(&[
            (0, "<< /Type /Catalog /Pages 2 0 R /AcroForm 6 0 R >>"),
            (
                0,
                "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] /Resources <<>> >>",
            ),
            (0, "<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>"),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (
                0,
                "<< /Type /Annot /Subtype /Widget /P 3 0 R /Rect [40 60 80 90] /FT /Btn /T (approval) /V /Off /AS /Off >>",
            ),
            (0, &format!("<< /Fields [5 0 R] /CO {order} >>")),
        ]);
        for operation in ["duplicatePage", "deletePage"] {
            let mut session =
                NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16])
                    .unwrap();
            let prior = session.snapshot().clone();
            let capability = prior
                .page_state
                .restrictions
                .as_ref()
                .map(|caps| &caps.pages[0]);
            let refusal = capability.and_then(|page| {
                if operation == "duplicatePage" {
                    page.duplicate
                } else {
                    page.delete
                }
            });
            assert_eq!(refusal.is_none(), available);
            let result = session.apply_page(prior.binding, 1, &command(operation, 0, &prior));
            if available {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err().code(), "FLOW_PDF_NATIVE_UNSUPPORTED");
                assert_eq!(session.output_bytes(), bytes);
            }
        }
    }
}

#[test]
fn chapter_outline_blocks_delete_but_leaf_outline_can_be_removed() {
    let bytes = build_pdf(&[
        (0, "<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] /Resources <<>> >>",
        ),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (0, "<< /Type /Outlines /First 6 0 R /Last 6 0 R /Count 2 >>"),
        (
            0,
            "<< /Title (Chapter) /Parent 5 0 R /Dest [3 0 R /Fit] /First 7 0 R /Last 7 0 R /Count 1 >>",
        ),
        (0, "<< /Title (Section) /Parent 6 0 R /Dest [4 0 R /Fit] >>"),
    ]);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    let capabilities = &prior.page_state.restrictions.as_ref().unwrap().pages;
    assert!(capabilities[0].delete.is_some());
    assert!(capabilities[1].delete.is_none());
    assert!(
        session
            .apply_page(prior.binding, 1, &command("deletePage", 0, &prior))
            .is_err()
    );
    assert_eq!(session.output_bytes(), bytes);
    session
        .apply_page(prior.binding, 2, &command("deletePage", 1, &prior))
        .unwrap();
    assert_eq!(
        session.snapshot().page_state.order,
        prior.page_state.order[..1]
    );
}

fn outline_pdf(page_count: usize, targets: &[usize]) -> Vec<u8> {
    let bookmark_count = targets.len();
    let outline = page_count + 3;
    let first = outline + 1;
    let last = outline + bookmark_count;
    let kids = (3..outline)
        .map(|page| format!("{page} 0 R "))
        .collect::<String>();
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R /Outlines {outline} 0 R >>"),
        format!(
            "<< /Type /Pages /Kids [{kids}] /Count {page_count} /MediaBox [0 0 300 500] /Resources <<>> >>"
        ),
    ];
    objects.extend((0..page_count).map(|_| "<< /Type /Page /Parent 2 0 R >>".to_owned()));
    objects.push(format!(
        "<< /Type /Outlines /First {first} 0 R /Last {last} 0 R /Count {bookmark_count} >>"
    ));
    for (entry, &target) in (first..=last).zip(targets) {
        let prev = if entry > first {
            format!("/Prev {} 0 R ", entry - 1)
        } else {
            String::new()
        };
        let next = if entry < last {
            format!("/Next {} 0 R ", entry + 1)
        } else {
            String::new()
        };
        objects.push(format!(
            "<< /Title (Reference {target}) /Parent {outline} 0 R {prev}{next}/Dest [{} 0 R /Fit] >>",
            target + 3
        ));
    }
    let refs: Vec<_> = objects.iter().map(|value| (0, value.as_str())).collect();
    build_pdf(&refs)
}

#[test]
fn many_outline_entries_do_not_block_unreferenced_pages() {
    const PAGE_COUNT: usize = 300;
    let bytes = outline_pdf(PAGE_COUNT, &[0; 3400]);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    if let Some(capabilities) = &prior.page_state.restrictions {
        assert!(capabilities.pages[PAGE_COUNT - 1].delete.is_none());
    }
    session
        .apply_page(
            prior.binding,
            1,
            &command("deletePage", PAGE_COUNT - 1, &prior),
        )
        .unwrap();
    assert_eq!(session.snapshot().page_state.order.len(), PAGE_COUNT - 1);
}

#[test]
fn bookmarks_spread_across_pages_fit_the_default_deletion_budget() {
    const PAGE_COUNT: usize = 600;
    const TARGET: usize = 299;
    let targets: Vec<_> = (0..PAGE_COUNT - 1).flat_map(|page| [page, page]).collect();
    let bytes = outline_pdf(PAGE_COUNT, &targets);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    assert!(
        prior
            .page_state
            .restrictions
            .as_ref()
            .is_none_or(|caps| caps.pages[TARGET].delete.is_none()
                && caps.pages[PAGE_COUNT - 1].delete.is_none())
    );
    session
        .apply_page(prior.binding, 1, &command("deletePage", TARGET, &prior))
        .unwrap();
    let after = session.snapshot().clone();
    let mut expected = prior.page_state.order;
    expected.remove(TARGET);
    assert_eq!(after.page_state.order, expected);
    let output = String::from_utf8_lossy(session.output_bytes());
    assert_eq!(output.matches("/Title").count(), targets.len() - 2);

    let last_page = PAGE_COUNT - 2;
    session
        .apply_page(after.binding, 2, &command("deletePage", last_page, &after))
        .unwrap();
    expected.pop();
    assert_eq!(session.snapshot().page_state.order, expected);
    assert_eq!(
        String::from_utf8_lossy(session.output_bytes())
            .matches("/Title")
            .count(),
        targets.len() - 2
    );
}

#[test]
fn grouped_field_refuses_duplicate_without_blocking_unrelated_deletion() {
    let bytes = build_pdf(&[
        (0, "<< /Type /Catalog /Pages 2 0 R /AcroForm 6 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] /Resources <<>> >>",
        ),
        (0, "<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>"),
        (0, "<< /Type /Page /Parent 2 0 R /Annots [8 0 R] >>"),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /P 3 0 R /Rect [40 60 80 90] /FT /Btn /T (Approval) /V /Off /AS /Off >>",
        ),
        (0, "<< /Fields [5 0 R 7 0 R] >>"),
        (0, "<< /T (Section) /Kids [8 0 R] >>"),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /Parent 7 0 R /P 4 0 R /Rect [40 60 80 90] /FT /Btn /T (Approval) /V /Off /AS /Off >>",
        ),
    ]);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    let first = &prior.page_state.restrictions.as_ref().unwrap().pages[0];
    assert!(first.duplicate.is_some());
    assert!(first.delete.is_none());
    assert!(
        session
            .apply_page(prior.binding, 1, &command("duplicatePage", 0, &prior))
            .is_err()
    );
    assert_eq!(session.output_bytes(), bytes);
    session
        .apply_page(prior.binding, 2, &command("deletePage", 0, &prior))
        .unwrap();
    assert_eq!(
        session.snapshot().page_state.order,
        prior.page_state.order[1..]
    );
}

#[test]
fn scaled_page_allows_safe_structural_edits_without_editable_geometry() {
    let bytes = build_pdf(&[
        (0, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] /Resources <<>> >>",
        ),
        (
            0,
            "<< /Type /Page /Parent 2 0 R /UserUnit 2 /Annots [5 0 R] >>",
        ),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (
            0,
            "<< /Type /Annot /Subtype /Text /P 3 0 R /Rect [10 20 40 60] /Contents (Scaled note) >>",
        ),
    ]);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    assert_eq!(prior.scene.pages[0].geometry, None);
    assert!(matches!(
        prior.scene.pages[0].annotations.as_slice(),
        [flow_core::pdf::NativePdfAnnotation::ReadOnly { .. }]
    ));
    let capabilities = &prior.page_state.restrictions.as_ref().unwrap().pages;
    assert_eq!(capabilities[0].parent, capabilities[1].parent);
    assert!(capabilities[0].geometry.is_some());
    assert!(capabilities[0].delete.is_none());
    let target = serde_json::json!({"page": 0, "stableId": prior.page_state.order[0]});
    for intent in [
        serde_json::json!({"kind": "rotatePage", "target": target, "degrees": 90}),
        serde_json::json!({"kind": "insertBlankPage", "target": target, "side": "after"}),
        serde_json::json!({"kind": "duplicatePage", "target": target}),
    ] {
        let edit = serde_json::from_value(intent).unwrap();
        assert_eq!(
            session
                .apply_page(prior.binding, 1, &edit)
                .unwrap_err()
                .code(),
            "FLOW_PDF_NATIVE_PAGE_GEOMETRY"
        );
        assert_eq!(session.output_bytes(), bytes);
        assert_eq!(session.snapshot().binding, prior.binding);
    }

    let reorder = serde_json::from_value(
        serde_json::json!({"kind": "reorderPage", "target": target, "position": 1}),
    )
    .unwrap();
    session.apply_page(prior.binding, 1, &reorder).unwrap();
    let reordered = session.snapshot();
    assert_eq!(
        reordered.page_state.order,
        [prior.page_state.order[1], prior.page_state.order[0]]
    );
    assert_eq!(
        reordered.scene.pages[0].geometry,
        prior.scene.pages[1].geometry
    );
    assert_eq!(reordered.scene.pages[1].geometry, None);
    assert!(String::from_utf8_lossy(session.output_bytes()).contains("/UserUnit 2"));

    let delete = command("deletePage", 1, reordered);
    session.apply_page(reordered.binding, 2, &delete).unwrap();
    assert_eq!(
        session.snapshot().page_state.order,
        [prior.page_state.order[1]]
    );
    assert_eq!(
        session.snapshot().scene.pages[0].geometry,
        prior.scene.pages[1].geometry
    );
    assert!(!session.snapshot().page_state.can_delete);
}

#[test]
fn scaled_page_cannot_move_across_parents_without_geometry_verification() {
    let bytes = build_pdf(&[
        (0, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 3 /MediaBox [0 0 300 500] /Resources <<>> >>",
        ),
        (0, "<< /Type /Pages /Parent 2 0 R /Kids [5 0 R] /Count 1 >>"),
        (
            0,
            "<< /Type /Pages /Parent 2 0 R /Kids [6 0 R 7 0 R] /Count 2 /Rotate 90 >>",
        ),
        (0, "<< /Type /Page /Parent 3 0 R /UserUnit 2 >>"),
        (0, "<< /Type /Page /Parent 4 0 R >>"),
        (0, "<< /Type /Page /Parent 4 0 R >>"),
    ]);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    let capabilities = &prior.page_state.restrictions.as_ref().unwrap().pages;
    assert_ne!(capabilities[0].parent, capabilities[1].parent);
    assert_eq!(capabilities[1].parent, capabilities[2].parent);
    assert!(capabilities[0].reparent.is_some());
    assert!(capabilities[2].reparent.is_none());
    for (page, position) in [(0, 2), (2, 0)] {
        let reorder = serde_json::from_value(serde_json::json!({
            "kind": "reorderPage",
            "target": {"page": page, "stableId": prior.page_state.order[page]},
            "position": position,
        }))
        .unwrap();
        assert_eq!(
            session
                .apply_page(prior.binding, 1, &reorder)
                .unwrap_err()
                .code(),
            "FLOW_PDF_NATIVE_PAGE_GEOMETRY"
        );
        assert_eq!(session.output_bytes(), bytes);
        assert_eq!(session.snapshot().page_state, prior.page_state);
    }
}

#[test]
fn print_page_ranges_refuse_structural_edits_but_allow_rotation() {
    for preferences in ["<< /PrintPageRange [1 1] >>", "5 0 R"] {
        let catalog = format!("<< /Type /Catalog /Pages 2 0 R /ViewerPreferences {preferences} >>");
        let bytes = build_pdf(&[
            (0, &catalog),
            (
                0,
                "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] >>",
            ),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (0, "<< /PrintPageRange 6 0 R >>"),
            (0, "[1 1]"),
        ]);
        let mut session =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
        let prior = session.snapshot().clone();
        assert!(
            prior
                .page_state
                .restrictions
                .as_ref()
                .unwrap()
                .positions_locked
        );
        let target = serde_json::json!({"page": 0, "stableId": prior.page_state.order[0]});
        for intent in [
            serde_json::json!({"kind": "reorderPage", "target": target, "position": 1}),
            serde_json::json!({"kind": "deletePage", "target": target}),
            serde_json::json!({"kind": "insertBlankPage", "target": target, "side": "after"}),
            serde_json::json!({"kind": "duplicatePage", "target": target}),
        ] {
            let command = serde_json::from_value(intent).unwrap();
            assert_eq!(
                session
                    .apply_page(prior.binding, 1, &command)
                    .unwrap_err()
                    .code(),
                "FLOW_PDF_NATIVE_UNSUPPORTED",
            );
            assert_eq!(session.output_bytes(), bytes);
            assert_eq!(session.snapshot().binding, prior.binding);
            assert_eq!(session.snapshot().page_state, prior.page_state);
        }
        let rotate = serde_json::from_value(
            serde_json::json!({"kind": "rotatePage", "target": target, "degrees": 90}),
        )
        .unwrap();
        session.apply_page(prior.binding, 1, &rotate).unwrap();
        assert_eq!(session.snapshot().page_state.order, prior.page_state.order);
        assert_eq!(
            session.snapshot().scene.pages[0]
                .geometry
                .unwrap()
                .rotation(),
            90
        );
    }
}

#[test]
fn null_print_presets_allow_structural_edits() {
    for (preferences, indirect) in [
        ("null", "null"),
        ("5 0 R", "null"),
        ("<< /PrintPageRange null >>", "null"),
        ("<< /PrintPageRange 5 0 R >>", "null"),
        ("5 0 R", "<< /PrintPageRange 6 0 R >>"),
    ] {
        let catalog = format!("<< /Type /Catalog /Pages 2 0 R /ViewerPreferences {preferences} >>");
        let bytes = build_pdf(&[
            (0, &catalog),
            (
                0,
                "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] >>",
            ),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (0, indirect),
            (0, "null"),
        ]);
        let mut session =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
        let prior = session.snapshot().clone();
        assert!(prior.page_state.restrictions.is_none());
        let reorder = serde_json::from_value(serde_json::json!({
            "kind": "reorderPage",
            "target": {"page": 0, "stableId": prior.page_state.order[0]},
            "position": 1,
        }))
        .unwrap();
        session.apply_page(prior.binding, 1, &reorder).unwrap();
        assert_eq!(
            session.snapshot().page_state.order,
            [prior.page_state.order[1], prior.page_state.order[0]],
        );
    }
}

#[test]
fn malformed_page_counts_and_parents_refuse_on_open() {
    for (count, parent) in [(2, 2), (1, 1)] {
        let file = build_pdf(&[
            (0, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                0,
                &format!("<< /Type /Pages /Kids [3 0 R] /Count {count} /MediaBox [0 0 100 100] >>"),
            ),
            (0, &format!("<< /Type /Page /Parent {parent} 0 R >>")),
        ]);
        assert!(
            NativePdfSession::open_scoped(&file, NativeEditLimits::default(), [4; 16]).is_err()
        );
    }
}

#[test]
fn object_budget_overflow_refuses_blank_insertion() {
    let file = fixture();
    let limits = NativeEditLimits {
        max_objects: 4,
        ..NativeEditLimits::default()
    };
    let mut session = NativePdfSession::open_scoped(&file, limits, [5; 16]).unwrap();
    let binding = session.snapshot().binding;
    let insert = serde_json::from_str(
        r#"{"kind":"insertBlankPage","target":{"page":0,"stableId":262144},"side":"after"}"#,
    )
    .unwrap();
    assert_eq!(
        session.apply_page(binding, 1, &insert).unwrap_err().code(),
        "FLOW_PDF_NATIVE_BUDGET_EXHAUSTED"
    );
    assert_eq!(session.output_bytes(), file);
    assert_eq!(session.snapshot().binding, binding);
}

#[test]
fn split_text_widget_and_direct_form_clone_as_independent_owners() {
    let file = build_pdf(&[
        (
            0,
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields 6 0 R >> >>",
        ),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] /CropBox [10 10 90 90] >>",
        ),
        (0, "<< /Type /Page /Parent 2 0 R /Annots [4 0 R] >>"),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /P 3 0 R /Parent 5 0 R /Rect [20 20 40 40] >>",
        ),
        (0, "<< /FT /Tx /T (customer) /V (data) /Kids [4 0 R] >>"),
        (0, "[5 0 R]"),
    ]);
    let mut session =
        NativePdfSession::open_scoped(&file, NativeEditLimits::default(), [6; 16]).unwrap();
    let duplicate = command("duplicatePage", 0, session.snapshot());
    session
        .apply_page(session.snapshot().binding, 1, &duplicate)
        .unwrap();
    assert_eq!(
        session.snapshot().scene.pages[1].geometry.unwrap().bounds(),
        [10, 10, 90, 90]
    );
    let output = String::from_utf8_lossy(session.output_bytes());
    assert_eq!(output.matches("/Subtype /Widget").count(), 2);
    assert_eq!(output.matches("/FT /Tx").count(), 2);
    assert_eq!(output.matches("/T <637573746F6D6572>").count(), 1);

    let delete = command("deletePage", 0, session.snapshot());
    session
        .apply_page(session.snapshot().binding, 2, &delete)
        .unwrap();
    let output = String::from_utf8_lossy(session.output_bytes());
    assert_eq!(output.matches("/Subtype /Widget").count(), 1);
    assert_eq!(output.matches("/FT /Tx").count(), 1);
    assert!(!output.contains("/T <637573746F6D6572>"));
}

#[test]
fn owned_checkbox_copies_remain_independently_editable_after_page_deletion() {
    use flow_core::pdf::native::NativeFieldCommand;
    use serde_json::json;

    let bytes = fixture();
    let mut pdf =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let add = serde_json::from_value::<NativeFieldCommand>(json!({
        "kind": "addCheckboxField", "page": 0, "rect": [10,10,35,35], "name": "approval", "checked": false,
    })).unwrap();
    pdf.apply_field(pdf.snapshot().binding, 1, &add).unwrap();
    let duplicate = command("duplicatePage", 0, pdf.snapshot());
    pdf.apply_page(pdf.snapshot().binding, 2, &duplicate)
        .unwrap();
    let fields = &pdf.snapshot().fields;
    assert_eq!(fields.len(), 2);
    assert_ne!(fields[0].name, fields[1].name);
    assert_ne!(fields[0].id, fields[1].id);
    let toggle = serde_json::from_value::<NativeFieldCommand>(json!({
        "kind": "setFieldChecked", "field": fields[1].id, "checked": true,
    }))
    .unwrap();
    pdf.apply_field(pdf.snapshot().binding, 3, &toggle).unwrap();
    assert_eq!(pdf.snapshot().fields[0].checked, Some(false));
    assert_eq!(pdf.snapshot().fields[1].checked, Some(true));
    let delete = command("deletePage", 0, pdf.snapshot());
    pdf.apply_page(pdf.snapshot().binding, 4, &delete).unwrap();
    assert_eq!(pdf.snapshot().fields.len(), 1);
    assert_eq!(pdf.snapshot().fields[0].page, 0);
    assert_eq!(pdf.snapshot().fields[0].checked, Some(true));
    let toggle = serde_json::from_value::<NativeFieldCommand>(json!({
        "kind": "setFieldChecked", "field": pdf.snapshot().fields[0].id, "checked": false,
    }))
    .unwrap();
    pdf.apply_field(pdf.snapshot().binding, 5, &toggle).unwrap();
    assert_eq!(pdf.snapshot().fields[0].checked, Some(false));
}

use flow_core::pdf::native::{
    NativeEditError, NativeEditLimits, NativePageCommand, NativePageSide, NativePageTarget,
    NativePdfSession, NativeRequestLookup, NativeSessionError,
};
use flow_core::pdf::{PdfReadError, PdfSceneElement, PdfSceneRequest, open_pdf, read_pdf_scene};

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
fn direct_and_indirect_link_destinations_track_the_same_page_after_reorder() {
    let source = build_pdf(&[
        (0, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 200] >>",
        ),
        (0, "<< /Type /Page /Parent 2 0 R /Annots [5 0 R 7 0 R] >>"),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (
            0,
            "<< /Type /Annot /Subtype /Link /P 3 0 R /Rect [10 10 30 30] /Dest 6 0 R >>",
        ),
        (0, "[4 0 R /Fit]"),
        (
            0,
            "<< /Type /Annot /Subtype /Link /P 3 0 R /Rect [40 10 60 30] /Dest [4 0 R /Fit] >>",
        ),
    ]);
    let mut session =
        NativePdfSession::open_scoped(&source, NativeEditLimits::default(), [6; 16]).unwrap();
    let reorder = NativePageCommand::ReorderPage {
        target: NativePageTarget {
            page: 1,
            stable_id: session.snapshot().page_state.order[1],
        },
        position: 0,
    };
    session
        .apply_page(session.snapshot().binding, 1, &reorder)
        .unwrap();

    for (bytes, link_page, target_page) in
        [(source.as_slice(), 0, 1), (session.output_bytes(), 1, 0)]
    {
        let reader = open_pdf(bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(
            &reader,
            &PdfSceneRequest {
                first_page: 0,
                page_count: 2,
            },
        )
        .unwrap();
        let [
            PdfSceneElement::Link { value: indirect },
            PdfSceneElement::Link { value: direct },
        ] = scene.pages[link_page].elements.as_slice()
        else {
            panic!("expected both retained links");
        };
        assert!(!scene.report.partial);
        for (value, object_number) in [(indirect, 5), (direct, 7)] {
            assert_eq!(value.provenance.object_ref.object_number, object_number);
            assert!(value.internal);
            assert_eq!(value.destination_page, Some(target_page as u32));
        }
        assert_eq!(scene.pages[target_page].source_object.object_number, 4);
    }
}

#[test]
fn cross_parent_reorder_preserves_public_text_and_field_readback() {
    let paint = "BT /F1 10 Tf 20 40 Td (Page text) Tj ET\n20 50 m 40 50 l S\n";
    let source = build_pdf(&[
        (
            0,
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [7 0 R] >> >>",
        ),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 /MediaBox [0 0 200 200] >>",
        ),
        (0, "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 >>"),
        (
            0,
            "<< /Type /Page /Parent 3 0 R /Contents 9 0 R /Annots [8 0 R] /Resources << /Font << /F1 10 0 R >> >> >>",
        ),
        (
            0,
            "<< /Type /Pages /Parent 2 0 R /Kids [6 0 R] /Count 1 /CropBox [0 0 100 100] >>",
        ),
        (0, "<< /Type /Page /Parent 5 0 R >>"),
        (
            0,
            "<< /FT /Tx /T (customer) /V (saved data) /Kids [8 0 R] >>",
        ),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /P 4 0 R /Parent 7 0 R /Rect [20 70 100 90] >>",
        ),
        (
            0,
            &format!("<< /Length {} >>\nstream\n{paint}\nendstream", paint.len()),
        ),
        (
            0,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /MissingWidth 500 >>",
        ),
    ]);
    let mut session =
        NativePdfSession::open_scoped(&source, NativeEditLimits::default(), [6; 16]).unwrap();
    let reorder = NativePageCommand::ReorderPage {
        target: NativePageTarget {
            page: 0,
            stable_id: session.snapshot().page_state.order[0],
        },
        position: 1,
    };
    session
        .apply_page(session.snapshot().binding, 1, &reorder)
        .unwrap();
    assert!(String::from_utf8_lossy(session.output_bytes()).contains("/CropBox [0 0 200 200"));

    for (bytes, first_page) in [(source.as_slice(), 0), (session.output_bytes(), 1)] {
        let reader = open_pdf(bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(
            &reader,
            &PdfSceneRequest {
                first_page,
                page_count: 1,
            },
        )
        .unwrap();
        let page = &scene.pages[0];
        assert!(page.partial && scene.report.partial);
        assert!(matches!(
            page.elements.as_slice(),
            [PdfSceneElement::Form { value: field }, PdfSceneElement::Text { value: text }, PdfSceneElement::Path { value: path }]
                if field.field_type.as_deref() == Some("Tx")
                    && field.name.as_deref() == Some("customer")
                    && field.value.as_deref() == Some("saved data")
                    && text.text == "Page text" && path.stroke
        ));
    }
}

#[test]
fn inherited_page_edits_share_the_work_budget_through_rewrite() {
    let bytes = build_pdf(&[
        (0, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 300] >>",
        ),
        (
            0,
            "<< /Type /Pages /Parent 2 0 R /Kids [5 0 R] /Count 1 /Rotate 90 >>",
        ),
        (
            0,
            "<< /Type /Pages /Parent 2 0 R /Kids [6 0 R] /Count 1 /CropBox [10 20 160 260] >>",
        ),
        (0, "<< /Type /Page /Parent 3 0 R >>"),
        (0, "<< /Type /Page /Parent 4 0 R >>"),
    ]);
    let open = |max_work| {
        let mut session = NativePdfSession::open_scoped(
            &bytes,
            NativeEditLimits {
                max_work,
                ..NativeEditLimits::default()
            },
            [6; 16],
        )?;
        let original = session.snapshot().binding;
        let rotate = NativePageCommand::RotatePage {
            target: NativePageTarget {
                page: 1,
                stable_id: session.snapshot().page_state.order[1],
            },
            degrees: 90,
        };
        let accepted = session.apply_page(original, 1, &rotate)?;
        Ok((session, original, accepted))
    };
    let max_work = NativeEditLimits::default().max_work;
    let (accepted_session, _, _) = open(max_work).unwrap();
    let target = NativePageTarget {
        page: 0,
        stable_id: accepted_session.snapshot().page_state.order[0],
    };
    for edit in [
        NativePageCommand::ReorderPage {
            target,
            position: 1,
        },
        NativePageCommand::InsertBlankPage {
            target,
            side: NativePageSide::After,
        },
    ] {
        let apply = |session: &mut NativePdfSession| {
            session.apply_page(session.snapshot().binding, 2, &edit)
        };
        let (mut low, mut high) = (1, max_work);
        while low < high {
            let mid = low + (high - low) / 2;
            match open(mid).and_then(|(mut session, _, _)| apply(&mut session)) {
                Ok(_) => high = mid,
                Err(
                    NativeSessionError::Budget
                    | NativeSessionError::Graph(NativeEditError::Reader(PdfReadError::TokenLimit)),
                ) => low = mid + 1,
                Err(error) => panic!("unexpected refusal at {mid} work units: {error:?}"),
            }
        }

        let (mut constrained, original_binding, accepted) = open(low - 1).unwrap();
        let before = constrained.snapshot().clone();
        let accepted_bytes = constrained.output_bytes().to_vec();
        assert_eq!(apply(&mut constrained), Err(NativeSessionError::Budget));
        assert_eq!(constrained.output_bytes(), accepted_bytes);
        let after = constrained.snapshot();
        assert_eq!(after.binding, before.binding);
        assert_eq!(after.output_hash, before.output_hash);
        assert_eq!(after.page_state, before.page_state);
        assert_eq!(after.scene.scene_hash, before.scene.scene_hash);
        assert_eq!(after.consumed_request, before.consumed_request);
        assert_eq!(
            constrained.lookup(original_binding, 1),
            Ok(NativeRequestLookup::Accepted(accepted))
        );
        assert_eq!(
            constrained.lookup(before.binding, 2),
            Ok(NativeRequestLookup::Unseen)
        );

        let (mut exact, _, _) = open(low).unwrap();
        let exact_binding = exact.snapshot().binding;
        let receipt = apply(&mut exact).unwrap();
        assert_eq!(receipt.binding.revision, exact_binding.revision + 1);
        let mut order = before.page_state.order;
        let mut geometry: Vec<_> = before
            .scene
            .pages
            .iter()
            .map(|page| page.geometry)
            .collect();
        if matches!(edit, NativePageCommand::ReorderPage { .. }) {
            order.swap(0, 1);
            geometry.swap(0, 1);
        } else {
            order.insert(1, receipt.selected_page.unwrap());
            geometry.insert(1, geometry[0]);
        }
        let after = exact.snapshot();
        assert_eq!(after.page_state.order, order);
        assert!(
            after
                .scene
                .pages
                .iter()
                .map(|page| page.geometry)
                .eq(geometry)
        );
        assert_eq!(
            exact.lookup(exact_binding, 2),
            Ok(NativeRequestLookup::Accepted(receipt))
        );
    }
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

fn deletion_workspace_pdf() -> Vec<u8> {
    const ENTRIES: usize = 24;
    const FIRST_BOOKMARK: usize = 9;
    const FIRST_LINK: usize = FIRST_BOOKMARK + ENTRIES;
    let annots = (FIRST_LINK..FIRST_LINK + ENTRIES)
        .map(|object| format!("{object} 0 R "))
        .collect::<String>();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R /Outlines 7 0 R /Dests << /Appendix [5 0 R /Fit] >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids 6 0 R /Count 3 /MediaBox [0 0 300 500] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R /Annots [{annots}] >>"),
        "<< /Type /Page /Parent 2 0 R >>".to_owned(),
        "[3 0 R 4 0 R 5 0 R]".to_owned(),
        format!(
            "<< /Type /Outlines /First {FIRST_BOOKMARK} 0 R /Last {} 0 R /Count {ENTRIES} >>",
            FIRST_LINK - 1
        ),
        "[3 0 R /Fit]".to_owned(),
    ];
    for object in FIRST_BOOKMARK..FIRST_LINK {
        let mut entry = "<< /Title (Chapter) /Parent 7 0 R /Dest 8 0 R ".to_owned();
        if object > FIRST_BOOKMARK {
            entry.push_str(&format!("/Prev {} 0 R ", object - 1));
        }
        if object + 1 < FIRST_LINK {
            entry.push_str(&format!("/Next {} 0 R ", object + 1));
        }
        entry.push_str(">>");
        objects.push(entry);
    }
    objects.extend((0..ENTRIES).map(|_| {
        "<< /Type /Annot /Subtype /Link /P 4 0 R /Rect [20 30 50 60] /Dest 8 0 R >>".to_owned()
    }));
    let refs: Vec<_> = objects.iter().map(|value| (0, value.as_str())).collect();
    build_pdf(&refs)
}

fn first_retained_limit(mut accepts: impl FnMut(NativeEditLimits) -> bool) -> NativeEditLimits {
    let limits = |kib: usize| NativeEditLimits {
        max_retained_bytes: kib * 1024,
        ..NativeEditLimits::default()
    };
    let (mut low, mut high) = (1, 1024);
    assert!(accepts(limits(high)));
    while low < high {
        let middle = (low + high) / 2;
        if accepts(limits(middle)) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    limits(low)
}

#[test]
fn deletion_preflight_shares_workspace_with_links_bookmarks_and_parent_refs() {
    let bytes = deletion_workspace_pdf();
    let mut pdf =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let accepted = pdf.snapshot().clone();
    let caps = &accepted.page_state.restrictions.as_ref().unwrap().pages;
    assert!(caps[0].delete.is_none());
    assert!(caps[2].delete.is_some());
    assert!(caps[2].geometry.is_none());

    let scene_limit = first_retained_limit(|limits| {
        NativePdfSession::verify_edit_projection(
            &bytes,
            limits,
            accepted.binding,
            &accepted.scene.text_pages,
            &accepted.scene.object_pages,
        ) == Ok(true)
    });
    let verify_pages = |limits| {
        NativePdfSession::verify_page_projection(
            &bytes,
            limits,
            accepted.binding,
            accepted.scene.scene_hash,
            &accepted.page_state,
        )
    };
    assert_eq!(verify_pages(NativeEditLimits::default()), Ok(true));
    assert_eq!(
        verify_pages(NativeEditLimits {
            max_retained_bytes: scene_limit.max_retained_bytes + 24 * 1024,
            ..scene_limit
        }),
        Err(NativeSessionError::Budget)
    );

    let delete_limit = first_retained_limit(|limits| {
        let Ok(mut pdf) = NativePdfSession::open_scoped(&bytes, limits, [3; 16]) else {
            return false;
        };
        let before = pdf.snapshot();
        pdf.apply_page(before.binding, 1, &command("deletePage", 0, before))
            .is_ok()
    });
    let constrained = NativeEditLimits {
        max_retained_bytes: delete_limit.max_retained_bytes - 1024,
        ..delete_limit
    };
    let mut limited = NativePdfSession::open_scoped(&bytes, constrained, [3; 16]).unwrap();
    let before = limited.snapshot().clone();
    assert_eq!(before.page_state, accepted.page_state);
    assert_eq!(
        limited.apply_page(before.binding, 1, &command("deletePage", 0, &before)),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(limited.output_bytes(), bytes);
    assert_eq!(limited.snapshot().binding, before.binding);
    assert_eq!(limited.snapshot().page_state, before.page_state);
    assert_eq!(limited.snapshot().consumed_request, before.consumed_request);
    assert_eq!(
        limited.lookup(before.binding, 1),
        Ok(NativeRequestLookup::Unseen)
    );

    pdf.apply_page(accepted.binding, 1, &command("deletePage", 0, &accepted))
        .unwrap();
    assert_eq!(
        pdf.snapshot().page_state.order,
        accepted.page_state.order[1..]
    );
    assert!(pdf.snapshot().scene.pages[0].annotations.is_empty());
    let rewritten = String::from_utf8_lossy(pdf.output_bytes());
    assert!(!rewritten.contains("/Title "));
    assert!(!rewritten.contains("/Dest "));
    assert!(rewritten.contains("/Appendix"));
    let rotate = NativePageCommand::RotatePage {
        target: NativePageTarget {
            page: 1,
            stable_id: accepted.page_state.order[2],
        },
        degrees: 90,
    };
    pdf.apply_page(pdf.snapshot().binding, 2, &rotate).unwrap();
    assert_eq!(
        pdf.snapshot().scene.pages[1].geometry.unwrap().rotation(),
        90
    );
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
fn split_text_widget_with_indirect_arrays_clones_as_independent_owners() {
    assert_split_text_widget_owners("7 0 R", "8 0 R");
}

#[test]
fn split_text_widget_with_direct_arrays_clones_as_independent_owners() {
    assert_split_text_widget_owners("[4 0 R]", "[4 0 R]");
}

fn split_text_widget_pdf(annots: &str, kids: &str) -> Vec<u8> {
    build_pdf(&[
        (
            0,
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields 6 0 R >> >>",
        ),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] /CropBox [10 10 90 90] >>",
        ),
        (
            0,
            &format!("<< /Type /Page /Parent 2 0 R /Annots {annots} >>"),
        ),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /P 3 0 R /Parent 5 0 R /Rect [20 20 40 40] >>",
        ),
        (
            0,
            &format!("<< /FT /Tx /T (customer) /V (data) /Kids {kids} >>"),
        ),
        (0, "[5 0 R]"),
        (0, "[4 0 R]"),
        (0, "[4 0 R]"),
    ])
}

fn assert_split_text_widget_owners(annots: &str, kids: &str) {
    let file = split_text_widget_pdf(annots, kids);
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
    let [original, copied] = session.snapshot().fields.as_slice() else {
        panic!("expected two independent fields");
    };
    assert_eq!((original.page, copied.page), (0, 1));
    assert_ne!(original.id, copied.id);
    assert_ne!(original.object, copied.object);
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
fn duplicate_with_indirect_widget_arrays_refuses_atomically_one_byte_below_fit() {
    let bytes = split_text_widget_pdf("7 0 R", "8 0 R");
    let open = |max_retained_bytes| {
        NativePdfSession::open_scoped(
            &bytes,
            NativeEditLimits {
                max_retained_bytes,
                ..NativeEditLimits::default()
            },
            [6; 16],
        )
    };
    let duplicate = |session: &mut NativePdfSession| {
        let command = command("duplicatePage", 0, session.snapshot());
        session.apply_page(session.snapshot().binding, 1, &command)
    };
    let (mut low, mut high) = (1, NativeEditLimits::default().max_retained_bytes);
    while low < high {
        let mid = low + (high - low) / 2;
        match open(mid).and_then(|mut session| duplicate(&mut session)) {
            Ok(_) => high = mid,
            Err(
                NativeSessionError::Budget
                | NativeSessionError::Graph(NativeEditError::Reader(PdfReadError::InputSizeLimit)),
            ) => low = mid + 1,
            Err(error) => panic!("unexpected refusal at {mid} bytes: {error:?}"),
        }
    }
    let mut constrained = open(low - 1).unwrap();
    let before = constrained.snapshot().clone();
    assert_eq!(duplicate(&mut constrained), Err(NativeSessionError::Budget));
    let after = constrained.snapshot();
    assert_eq!(constrained.output_bytes(), bytes);
    assert_eq!(after.binding, before.binding);
    assert_eq!(after.output_hash, before.output_hash);
    assert_eq!(after.page_state, before.page_state);
    assert_eq!(after.fields, before.fields);
    assert_eq!(after.consumed_request, before.consumed_request);
    assert_eq!(
        constrained.lookup(before.binding, 1),
        Ok(NativeRequestLookup::Unseen)
    );

    let mut exact = open(low).unwrap();
    let receipt = duplicate(&mut exact).unwrap();
    let order = &exact.snapshot().page_state.order;
    assert_eq!(order[0], before.page_state.order[0]);
    assert_ne!(order[0], order[1]);
    assert_eq!(receipt.selected_page, Some(order[1]));
    let fields = &exact.snapshot().fields;
    assert_eq!(fields.len(), 2);
    assert_ne!(fields[0].object, fields[1].object);
}

#[test]
fn duplicate_with_widget_uses_cumulative_work() {
    let bytes = split_text_widget_pdf("7 0 R", "8 0 R");
    let open = |max_work| {
        NativePdfSession::open_scoped(
            &bytes,
            NativeEditLimits {
                max_work,
                ..NativeEditLimits::default()
            },
            [6; 16],
        )
    };
    let duplicate = |session: &mut NativePdfSession| {
        let command = command("duplicatePage", 0, session.snapshot());
        session.apply_page(session.snapshot().binding, 1, &command)
    };
    let (mut low, mut high) = (1, NativeEditLimits::default().max_work);
    while low < high {
        let mid = low + (high - low) / 2;
        match open(mid).and_then(|mut session| duplicate(&mut session)) {
            Ok(_) => high = mid,
            Err(
                NativeSessionError::Budget
                | NativeSessionError::Graph(NativeEditError::Reader(PdfReadError::TokenLimit)),
            ) => low = mid + 1,
            Err(error) => panic!("unexpected refusal at {mid} work units: {error:?}"),
        }
    }
    let work_fit = low;
    let mut constrained = open(work_fit - 1).unwrap();
    let before = constrained.snapshot().clone();
    assert_eq!(duplicate(&mut constrained), Err(NativeSessionError::Budget));
    assert_eq!(constrained.output_bytes(), bytes);
    let after = constrained.snapshot();
    assert_eq!(after.binding, before.binding);
    assert_eq!(after.output_hash, before.output_hash);
    assert_eq!(after.page_state, before.page_state);
    assert_eq!(after.fields, before.fields);
    assert_eq!(after.consumed_request, before.consumed_request);
    assert_eq!(
        constrained.lookup(before.binding, 1),
        Ok(NativeRequestLookup::Unseen)
    );

    let mut exact = open(work_fit).unwrap();
    duplicate(&mut exact).unwrap();
    let [original, copy] = exact.snapshot().fields.as_slice() else {
        panic!("expected two independent fields");
    };
    assert_ne!(original.object, copy.object);
    assert_ne!(original.page, copy.page);
}

#[test]
fn duplicate_skips_copy_names_reserved_by_read_only_fields() {
    let bytes = build_pdf(&[
        (0, "<< /Type /Catalog /Pages 2 0 R /AcroForm 6 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] /Resources <<>> >>",
        ),
        (0, "<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>"),
        (
            0,
            "<< /Type /Page /Parent 2 0 R /Annots [7 0 R 9 0 R 11 0 R 12 0 R] >>",
        ),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /P 3 0 R /Rect [40 60 80 90] /FT /Btn /T (approval) /V /Off /AS /Off >>",
        ),
        (0, "<< /Fields [5 0 R 7 0 R 9 0 R 10 0 R 12 0 R] >>"),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /P 4 0 R /Rect [40 60 80 90] /FT /Tx /Ff 1 /T 8 0 R /V (data) >>",
        ),
        (
            0,
            "<FEFF0046006C006F00770050004400460043006F007000790035005F0030>",
        ),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /P 4 0 R /Rect [40 100 80 130] /FT /Tx /Ff 1 /T (aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa) /V (data) >>",
        ),
        (
            0,
            "<< /FT /Tx /Ff 1 /T (FlowPDFCopy5_1) /V (data) /Kids [11 0 R] >>",
        ),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /P 4 0 R /Rect [40 140 80 170] /Parent 10 0 R /T <FEFF0041000A0042> >>",
        ),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /P 4 0 R /Rect [40 180 80 210] /FT /Tx /Ff 1 /T <EFBBBF466C6F77504446436F7079355F32> /V (data) >>",
        ),
    ]);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot();
    let restrictions = &prior.page_state.restrictions.as_ref().unwrap().pages;
    assert!(restrictions[0].duplicate.is_none());
    assert!(restrictions[1].duplicate.is_some());
    assert!(prior.fields.iter().any(|field| {
        field.name.as_deref() == Some("FlowPDFCopy5_0") && !field.can_edit_appearance
    }));
    let hidden_name = prior
        .fields
        .iter()
        .find(|field| field.object.object_number == 11)
        .unwrap();
    assert_eq!(hidden_name.name, None);
    assert!(!hidden_name.can_edit_appearance);

    let duplicate = command("duplicatePage", 0, prior);
    session.apply_page(prior.binding, 1, &duplicate).unwrap();
    let assert_fields = |pdf: &NativePdfSession| {
        let fields = &pdf.snapshot().fields;
        assert_eq!(fields.len(), 6);
        let copy = fields.iter().find(|field| field.page == 1).unwrap();
        assert_eq!(copy.name.as_deref(), Some("FlowPDFCopy5_2"));
        for label in ["FlowPDFCopy5_0", "ï»¿FlowPDFCopy5_2"] {
            let read_only = fields
                .iter()
                .find(|field| field.page == 2 && field.name.as_deref() == Some(label))
                .unwrap();
            assert!(!read_only.can_edit_appearance);
        }
    };
    assert_fields(&session);
    let reopened =
        NativePdfSession::open_scoped(session.output_bytes(), NativeEditLimits::default(), [3; 16])
            .unwrap();
    assert_fields(&reopened);
    let output = String::from_utf8_lossy(session.output_bytes());
    assert!(output.contains(&format!("/T <{}>", "61".repeat(65))));
    assert!(output.contains("/T <EFBBBF466C6F77504446436F7079355F32>"));
}

#[test]
fn copy_name_inventory_uses_the_live_projection_and_staging_budgets() {
    let long_name = "a".repeat(24 * 1024);
    let bytes = build_pdf(&[
        (0, "<< /Type /Catalog /Pages 2 0 R /AcroForm 6 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] >>",
        ),
        (0, "<< /Type /Page /Parent 2 0 R /Annots 8 0 R >>"),
        (0, "<< /Type /Page /Parent 2 0 R /Annots [7 0 R] >>"),
        (
            0,
            "<< /Type /Annot /Subtype /Widget /P 3 0 R /Rect [40 60 80 90] /FT /Btn /T (approval) /V /Off /AS /Off >>",
        ),
        (0, "<< /Fields [5 0 R 7 0 R] >>"),
        (
            0,
            &format!(
                "<< /Type /Annot /Subtype /Widget /P 4 0 R /Rect [40 60 80 90] /FT /Tx /Ff 1 /T ({long_name}) >>"
            ),
        ),
        (0, "[5 0 R]"),
    ]);
    let limits = NativeEditLimits {
        max_retained_bytes: 256 * 1024,
        ..NativeEditLimits::default()
    };
    let mut pdf = NativePdfSession::open_scoped(&bytes, limits, [3; 16]).unwrap();
    let accepted = pdf.snapshot().clone();
    assert_eq!(
        NativePdfSession::verify_page_projection(
            &bytes,
            NativeEditLimits {
                max_retained_bytes: 128 * 1024,
                ..limits
            },
            accepted.binding,
            accepted.scene.scene_hash,
            &accepted.page_state,
        ),
        Err(NativeSessionError::Budget)
    );
    let duplicate = command("duplicatePage", 0, &accepted);
    assert_eq!(
        pdf.apply_page(accepted.binding, 1, &duplicate),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(pdf.snapshot().binding, accepted.binding);
    assert_eq!(pdf.snapshot().consumed_request, accepted.consumed_request);
    assert_eq!(pdf.snapshot().page_state, accepted.page_state);
    assert_eq!(pdf.snapshot().scene.scene_hash, accepted.scene.scene_hash);
    assert_eq!(pdf.snapshot().fields, accepted.fields);
    assert_eq!(
        pdf.lookup(accepted.binding, 1),
        Ok(NativeRequestLookup::Unseen)
    );
    assert_eq!(pdf.output_bytes(), bytes);

    let mut near_limit = NativePdfSession::open_scoped(
        &bytes,
        NativeEditLimits {
            max_retained_bytes: 512 * 1024,
            ..limits
        },
        [3; 16],
    )
    .unwrap();
    let binding = near_limit.snapshot().binding;
    let duplicate = command("duplicatePage", 0, near_limit.snapshot());
    near_limit.apply_page(binding, 1, &duplicate).unwrap();
    assert_eq!(near_limit.snapshot().scene.pages.len(), 3);
    assert!(
        near_limit
            .snapshot()
            .fields
            .iter()
            .any(|field| { field.name.as_deref() == Some("FlowPDFCopy5_0") && field.page == 1 })
    );
}

#[test]
fn owned_checkbox_copies_remain_independently_editable_after_page_deletion() {
    use flow_core::pdf::native::NativeFieldCommand;
    use serde_json::json;

    let bytes = fixture();
    let limits = NativeEditLimits {
        max_retained_bytes: 512 * 1024,
        ..NativeEditLimits::default()
    };
    let mut pdf = NativePdfSession::open_scoped(&bytes, limits, [3; 16]).unwrap();
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

fn page_order_workspace_pdf(indirect: bool) -> Vec<u8> {
    const PER_PARENT: usize = 16;
    let first: String = (5..5 + PER_PARENT).map(|id| format!("{id} 0 R ")).collect();
    let second: String = (5 + PER_PARENT..5 + 2 * PER_PARENT)
        .map(|id| format!("{id} 0 R "))
        .collect();
    let child_arrays = [format!("[{first}]"), format!("[{second}]")];
    let kids = if indirect {
        [
            format!("{} 0 R", 5 + 2 * PER_PARENT),
            format!("{} 0 R", 6 + 2 * PER_PARENT),
        ]
    } else {
        child_arrays.clone()
    };
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!(
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count {} /Resources <<>> >>",
            2 * PER_PARENT
        ),
        format!(
            "<< /Type /Pages /Parent 2 0 R /Kids {} /Count {PER_PARENT} /MediaBox [0 0 300 500] >>",
            kids[0]
        ),
        format!(
            "<< /Type /Pages /Parent 2 0 R /Kids {} /Count {PER_PARENT} /MediaBox [0 0 400 600] /Rotate 90 >>",
            kids[1]
        ),
    ];
    objects.extend((0..PER_PARENT).map(|_| "<< /Type /Page /Parent 3 0 R >>".to_owned()));
    objects.extend((0..PER_PARENT).map(|_| "<< /Type /Page /Parent 4 0 R >>".to_owned()));
    if indirect {
        objects.extend(child_arrays);
    }
    build_pdf(
        &objects
            .iter()
            .map(|object| (0, object.as_str()))
            .collect::<Vec<_>>(),
    )
}

#[test]
fn nested_page_rotation_refuses_retained_budget_exhaustion_atomically() {
    for indirect in [false, true] {
        let bytes = page_order_workspace_pdf(indirect);
        let limits = NativeEditLimits {
            max_retained_bytes: 258 * 1024,
            ..NativeEditLimits::default()
        };
        let mut session = NativePdfSession::open_scoped(&bytes, limits, [3; 16]).unwrap();
        let before = session.snapshot().clone();
        let rotate = NativePageCommand::RotatePage {
            target: NativePageTarget {
                page: 0,
                stable_id: before.page_state.order[0],
            },
            degrees: 90,
        };

        assert_eq!(
            session.apply_page(before.binding, 1, &rotate),
            Err(NativeSessionError::Budget),
            "indirect Kids: {indirect}"
        );
        let after = session.snapshot();
        assert_eq!(session.output_bytes(), bytes);
        assert_eq!(after.binding, before.binding);
        assert_eq!(after.consumed_request, before.consumed_request);
        assert_eq!(after.output_hash, before.output_hash);
        assert_eq!(after.page_state, before.page_state);
        assert_eq!(after.scene.scene_hash, before.scene.scene_hash);
        assert_eq!(
            session.lookup(before.binding, 1),
            Ok(NativeRequestLookup::Unseen)
        );

        let mut ample =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
        let binding = ample.snapshot().binding;
        let receipt = ample.apply_page(binding, 1, &rotate).unwrap();
        let after = ample.snapshot();
        assert_eq!(receipt.selected_page, Some(before.page_state.order[0]));
        assert_eq!(after.page_state.order, before.page_state.order);
        let rotated = after.scene.pages[0].geometry.unwrap();
        assert_eq!(rotated.rotation(), 90);
        assert_eq!(
            rotated.bounds(),
            before.scene.pages[0].geometry.unwrap().bounds()
        );
        assert!(
            after.scene.pages[1..]
                .iter()
                .map(|page| page.geometry)
                .eq(before.scene.pages[1..].iter().map(|page| page.geometry))
        );
    }
}

#[test]
fn page_order_mutations_share_the_staged_allowance_for_direct_and_indirect_kids() {
    const REORDER_ALLOWANCE: usize = 256 * 1024;
    const OTHER_PAGE_EDIT_ALLOWANCE: usize = 192 * 1024;
    for indirect in [false, true] {
        let bytes = page_order_workspace_pdf(indirect);
        let open = |limits| NativePdfSession::open_scoped(&bytes, limits, [3; 16]);
        let limits_with = |max_retained_bytes| NativeEditLimits {
            max_retained_bytes,
            ..NativeEditLimits::default()
        };
        let original = open(NativeEditLimits::default()).unwrap();
        let before = original.snapshot();
        let target = NativePageTarget {
            page: 0,
            stable_id: before.page_state.order[0],
        };
        for (name, command) in [
            (
                "reorder",
                NativePageCommand::ReorderPage {
                    target,
                    position: before.page_state.max_position,
                },
            ),
            (
                "insert",
                NativePageCommand::InsertBlankPage {
                    target,
                    side: NativePageSide::After,
                },
            ),
            ("duplicate", NativePageCommand::DuplicatePage { target }),
            ("delete", NativePageCommand::DeletePage { target }),
        ] {
            let allowance = match command {
                NativePageCommand::ReorderPage { .. } => REORDER_ALLOWANCE,
                _ => OTHER_PAGE_EDIT_ALLOWANCE,
            };
            let mut constrained = open(limits_with(allowance)).unwrap();
            let before = constrained.snapshot().clone();
            let error = constrained
                .apply_page(before.binding, 1, &command)
                .unwrap_err();
            assert!(
                matches!(
                    error,
                    NativeSessionError::Budget
                        | NativeSessionError::Graph(NativeEditError::Reader(
                            PdfReadError::InputSizeLimit
                        ))
                ),
                "{name}, indirect Kids: {indirect}, unexpected refusal: {error:?}"
            );
            assert_eq!(constrained.output_bytes(), bytes);
            assert_eq!(constrained.snapshot().binding, before.binding);
            assert_eq!(
                constrained.snapshot().consumed_request,
                before.consumed_request
            );
            assert_eq!(constrained.snapshot().page_state, before.page_state);
            assert_eq!(
                constrained.snapshot().scene.scene_hash,
                before.scene.scene_hash
            );
            assert_eq!(
                constrained.lookup(before.binding, 1),
                Ok(NativeRequestLookup::Unseen)
            );

            let mut edited = open(NativeEditLimits::default()).unwrap();
            let original = edited.snapshot().clone();
            let receipt = edited.apply_page(original.binding, 1, &command).unwrap();
            let actual = edited.snapshot();
            let mut expected = original.page_state.order;
            match command {
                NativePageCommand::ReorderPage { .. } => {
                    expected.rotate_left(1);
                    let pages = &original.scene.pages;
                    let mut geometry: Vec<_> = pages.iter().map(|page| page.geometry).collect();
                    geometry.rotate_left(1);
                    let actual_geometry = actual.scene.pages.iter().map(|page| page.geometry);
                    assert!(actual_geometry.eq(geometry));
                }
                NativePageCommand::DeletePage { .. } => {
                    expected.remove(0);
                }
                NativePageCommand::InsertBlankPage { .. }
                | NativePageCommand::DuplicatePage { .. } => {
                    expected.insert(1, receipt.selected_page.unwrap());
                    assert_eq!(
                        actual.scene.pages[1].geometry,
                        original.scene.pages[0].geometry
                    );
                }
                NativePageCommand::RotatePage { .. } => unreachable!(),
            }
            assert_eq!(actual.page_state.order, expected);
            assert_eq!(edited.origin_bytes(), bytes);
        }
    }
}

#[test]
fn indirect_page_tree_children_fit_the_default_deletion_budget() {
    const PAGE_COUNT: usize = 800;
    let kids_ref = PAGE_COUNT + 3;
    let kids = (3..kids_ref)
        .map(|page| format!("{page} 0 R "))
        .collect::<String>();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Kids {kids_ref} 0 R /Count {PAGE_COUNT} /MediaBox [0 0 300 500] >>"
        ),
    ];
    objects.extend((0..PAGE_COUNT).map(|_| "<< /Type /Page /Parent 2 0 R >>".to_owned()));
    objects.push(format!("[{kids}]"));
    let refs: Vec<_> = objects.iter().map(|value| (0, value.as_str())).collect();

    let bytes = build_pdf(&refs);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    session
        .apply_page(prior.binding, 1, &command("deletePage", 399, &prior))
        .unwrap();
    let mut expected = prior.page_state.order;
    expected.remove(399);
    assert_eq!(session.snapshot().page_state.order, expected);
}

#[test]
fn indirect_page_tree_child_array_retained_by_trailer_prevents_deletion() {
    let bytes = build_pdf_with_trailer(
        &[
            (0, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                0,
                "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] >>",
            ),
            (0, "<< /Type /Pages /Parent 2 0 R /Kids 6 0 R /Count 1 >>"),
            (0, "<< /Type /Page /Parent 2 0 R >>"),
            (0, "<< /Type /Page /Parent 3 0 R >>"),
            (0, "[5 0 R]"),
        ],
        " /RetainedKids 6 0 R",
    );
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    assert!(
        prior.page_state.restrictions.as_ref().unwrap().pages[0]
            .delete
            .is_some()
    );
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

#[test]
fn indirect_page_tree_replacement_preserves_surviving_sibling_references() {
    let bytes = build_pdf(&[
        (0, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            0,
            "<< /Type /Pages /Kids 6 0 R /Count 3 /MediaBox [0 0 300 500] >>",
        ),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (0, "<< /Type /Page /Parent 2 0 R /RetainedPage 3 0 R >>"),
        (0, "<< /Type /Page /Parent 2 0 R >>"),
        (0, "7 0 R"),
        (0, "[3 0 R 4 0 R 5 0 R]"),
    ]);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    let capabilities = &prior.page_state.restrictions.as_ref().unwrap().pages;
    assert!(capabilities[0].delete.is_some());
    assert!(capabilities[2].delete.is_none());
    assert!(
        session
            .apply_page(prior.binding, 1, &command("deletePage", 0, &prior))
            .is_err()
    );
    assert_eq!(session.output_bytes(), bytes);
    session
        .apply_page(prior.binding, 2, &command("deletePage", 2, &prior))
        .unwrap();
    assert_eq!(
        session.snapshot().page_state.order,
        prior.page_state.order[..2]
    );
}

#[test]
fn indexed_links_across_pages_fit_the_default_delete_projection_budget() {
    const INDEX_PAGES: usize = 12;
    const LINKS_PER_INDEX: usize = 128;
    const LINKS: usize = INDEX_PAGES * LINKS_PER_INDEX;
    const PAGE_COUNT: usize = INDEX_PAGES + LINKS;
    const FIRST_LINK: usize = PAGE_COUNT + 3;
    let kids = (3..FIRST_LINK)
        .map(|object| format!("{object} 0 R "))
        .collect::<String>();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {PAGE_COUNT} /MediaBox [0 0 300 500] >>"),
    ];
    for index in 0..INDEX_PAGES {
        let start = FIRST_LINK + index * LINKS_PER_INDEX;
        let annots = (start..start + LINKS_PER_INDEX)
            .map(|object| format!("{object} 0 R "))
            .collect::<String>();
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /Annots [{annots}] >>"
        ));
    }
    objects.extend((0..LINKS).map(|_| "<< /Type /Page /Parent 2 0 R >>".to_owned()));
    for target in INDEX_PAGES + 3..FIRST_LINK {
        objects.push(format!(
            "<< /Type /Annot /Subtype /Link /Rect [0 0 20 20] /Dest [{target} 0 R /Fit] >>"
        ));
    }
    let refs: Vec<_> = objects.iter().map(|value| (0, value.as_str())).collect();
    let bytes = build_pdf(&refs);
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [3; 16]).unwrap();
    let prior = session.snapshot().clone();
    assert!(
        prior
            .page_state
            .restrictions
            .as_ref()
            .is_none_or(|restrictions| { restrictions.pages[INDEX_PAGES].delete.is_none() })
    );
    session
        .apply_page(
            prior.binding,
            1,
            &command("deletePage", INDEX_PAGES, &prior),
        )
        .unwrap();
    assert_eq!(session.snapshot().page_state.order.len(), PAGE_COUNT - 1);
    assert_eq!(
        String::from_utf8_lossy(session.output_bytes())
            .matches("/Subtype /Link")
            .count(),
        LINKS - 1
    );
}

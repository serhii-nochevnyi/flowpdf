#[path = "pdf_native_graph.rs"]
mod graph;

use flow_core::{
    NativeEditLimits, NativeNoteCommand, NativePdfAnnotation, NativePdfSession,
    NativeReadOnlyAnnotation, NativeRequestLookup, NativeSessionError,
};

#[test]
fn note_crud_reads_reopened_output_and_reconciles_requests() {
    let source = graph::fixture("", "/Annots 33 0 R");
    let mut session =
        NativePdfSession::open_scoped(&source, NativeEditLimits::default(), [1; 16]).unwrap();
    let initial = session.snapshot().clone();
    let page = initial.scene.pages[0].id;
    let other = initial.scene.pages[1].id;
    let rect = [20, 30, 40, 50];
    let geometry = initial.scene.pages[0].geometry.unwrap();
    assert_eq!(geometry.to_rotated_crop(rect), Some([10, 130, 30, 150]));
    assert_eq!(geometry.from_rotated_crop([10, 130, 30, 150]), Some(rect));
    assert_eq!(
        initial.scene.pages[0]
            .annotations
            .iter()
            .filter(|a| matches!(a, NativePdfAnnotation::ReadOnly { .. }))
            .count(),
        2
    );
    let add = NativeNoteCommand::Add {
        page,
        rect,
        contents: "Україна (note) 📝".into(),
    };
    let added = session.apply(initial.binding, 1, &add).unwrap();
    let id = added.note;
    assert_note(&session, 0, id, rect, "Україна (note) 📝");
    let moved = [1, 2, 31, 42];
    let moved_receipt = session
        .apply(
            session.snapshot().binding,
            2,
            &NativeNoteCommand::Move {
                note: id,
                page: other,
                rect: moved,
            },
        )
        .unwrap();
    assert_note(&session, 1, id, moved, "Україна (note) 📝");
    assert_ne!(moved_receipt.output_hash, added.output_hash);
    let edit = session
        .apply(
            session.snapshot().binding,
            3,
            &NativeNoteCommand::Edit {
                note: id,
                contents: "English / Київ".into(),
            },
        )
        .unwrap();
    assert_note(&session, 1, id, moved, "English / Київ");
    let removed = session
        .apply(
            session.snapshot().binding,
            4,
            &NativeNoteCommand::Remove { note: id },
        )
        .unwrap();
    assert!(session.snapshot().scene.pages[1].annotations.is_empty());
    let reopened = reopen_accepted(&session);
    assert!(reopened.snapshot().scene.pages[1].annotations.is_empty());
    assert!(
        reopened.snapshot().scene.pages[0].annotations.iter().all(
            |annotation| !matches!(annotation, NativePdfAnnotation::PlainNote(note) if note.contents == "English / Київ")
        )
    );
    assert_eq!(removed.binding.revision, 4);
    assert_ne!(edit.scene_hash, removed.scene_hash);
    assert_ne!(removed.output_hash, initial.output_hash);
    assert_eq!(
        session.snapshot().binding.origin_hash,
        initial.binding.origin_hash
    );
    assert_eq!(session.origin_bytes(), source);
    assert_eq!(session.apply(initial.binding, 1, &add), Ok(added));
    assert_eq!(
        session.lookup(initial.binding, 1),
        Ok(NativeRequestLookup::Accepted(added))
    );
    assert_eq!(
        session.apply(initial.binding, 1, &NativeNoteCommand::Remove { note: id }),
        Err(NativeSessionError::RequestConflict)
    );
    assert_eq!(
        session.apply(
            session.snapshot().binding,
            5,
            &NativeNoteCommand::Remove { note: id }
        ),
        Err(NativeSessionError::TargetUnavailable)
    );
    assert_eq!(session.snapshot().binding.revision, 4);
}

fn assert_note(
    session: &NativePdfSession,
    page: usize,
    id: flow_core::NativeNoteId,
    rect: [i64; 4],
    contents: &str,
) {
    let snapshot = session.snapshot();
    let note = snapshot.scene.pages[page]
        .annotations
        .iter()
        .find_map(|annotation| match annotation {
            NativePdfAnnotation::PlainNote(note) if note.id == id => Some(note),
            _ => None,
        })
        .unwrap();
    assert_eq!(note.rect, rect);
    assert_eq!(note.contents, contents);
    let reopened = reopen_accepted(session);
    assert!(
        reopened.snapshot().scene.pages[page]
            .annotations
            .iter()
            .any(
                |annotation| matches!(annotation, NativePdfAnnotation::PlainNote(reopened_note)
            if reopened_note.object == note.object
                && reopened_note.rect == rect
                && reopened_note.contents == contents)
            )
    );
    assert_eq!(
        snapshot.output_hash,
        *blake3::hash(session.output_bytes()).as_bytes()
    );
    assert_eq!(snapshot.scene.output_hash, snapshot.output_hash);
    let hex = contents
        .encode_utf16()
        .map(|unit| format!("{unit:04X}"))
        .collect::<String>();
    let token = format!("/Contents <FEFF{hex}>");
    assert!(
        session
            .output_bytes()
            .windows(token.len())
            .any(|value| value == token.as_bytes())
    );
    let [left, bottom, right, top] = rect;
    let token = format!("/Rect [{left} {bottom} {right} {top} ]");
    assert!(
        session
            .output_bytes()
            .windows(token.len())
            .any(|value| value == token.as_bytes())
    );
}

fn reopen_accepted(session: &NativePdfSession) -> NativePdfSession {
    let output = session.output_bytes();
    let reopened =
        NativePdfSession::open_scoped(output, NativeEditLimits::default(), [1; 16]).unwrap();
    let snapshot = session.snapshot();
    assert_eq!(
        reopened.snapshot().binding.origin_hash,
        snapshot.output_hash
    );
    assert_ne!(snapshot.binding.origin_hash, snapshot.output_hash);
    assert_eq!(
        reopened.snapshot().scene.scene_hash,
        snapshot.scene.scene_hash
    );
    let syntax = String::from_utf8_lossy(output);
    assert!(syntax.contains("/Custom /A#23B#FF /D +.1234567890123456789"));
    assert!(syntax.contains("/V [true false <00FF> ]"));
    assert_eq!(syntax.matches("stream\n71 20 51>\nendstream").count(), 1);
    assert!(!syntax.contains("UnreachableDecoy"));
    reopened
}

#[test]
fn old_runtime_epoch_cannot_edit_replacement_session() {
    let bytes = graph::fixture("", "");
    let limits = NativeEditLimits::default();
    let old = NativePdfSession::open_scoped(&bytes, limits, [1; 16]).unwrap();
    let mut replacement = NativePdfSession::open_scoped(&bytes, limits, [2; 16]).unwrap();
    let initial = replacement.snapshot().binding;
    let mut delayed = old.snapshot().binding;
    delayed.session_id = initial.session_id;
    let add = NativeNoteCommand::Add {
        page: replacement.snapshot().scene.pages[0].id,
        rect: [20, 30, 40, 50],
        contents: "Delayed".into(),
    };

    assert!(matches!(
        NativePdfSession::open_scoped(&bytes, limits, [0; 16]),
        Err(NativeSessionError::BindingMismatch)
    ));
    assert_eq!(
        replacement.apply(delayed, 1, &add),
        Err(NativeSessionError::BindingMismatch)
    );
    assert_eq!(replacement.snapshot().binding, initial);
    assert_eq!(replacement.output_bytes(), bytes);
    assert_eq!(
        replacement.lookup(initial, 1),
        Ok(NativeRequestLookup::Unseen)
    );
}

#[test]
fn pdf_17_notes_preserve_text_decoding_across_rewrite() {
    for (encoded, contents) in [
        ("<EFBBBF436166C3A9>", Some("ï»¿CafÃ©")),
        ("<18A080>", Some("˘€•")),
        ("<FEFFD83DDE00>", Some("😀")),
        ("<417F>", None),
        ("<419F>", None),
        ("<41AD>", None),
        ("<FEFFD800>", None),
    ] {
        let bytes = graph::fixture_with_contents("", "/Annots 33 0 R", encoded);
        let mut session =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [1; 16]).unwrap();
        let imported_note_id = |pdf: &NativePdfSession| {
            let annotation = &pdf.snapshot().scene.pages[0].annotations[0];
            match (annotation, contents) {
                (NativePdfAnnotation::PlainNote(note), Some(contents)) => {
                    assert_eq!(note.contents, contents);
                    assert_eq!(note.rect, [20, 30, 40, 50]);
                    Some(note.id)
                }
                (
                    NativePdfAnnotation::ReadOnly {
                        profile: NativeReadOnlyAnnotation::Text,
                        ..
                    },
                    None,
                ) => None,
                (annotation, _) => panic!("unexpected note for {encoded}: {annotation:?}"),
            }
        };
        let original_id = imported_note_id(&session);
        session
            .apply(
                session.snapshot().binding,
                1,
                &NativeNoteCommand::Add {
                    page: session.snapshot().scene.pages[1].id,
                    rect: [20, 30, 40, 50],
                    contents: "Another note".into(),
                },
            )
            .unwrap();
        let reopened = reopen_accepted(&session);
        assert_eq!(imported_note_id(&session), original_id);
        assert_eq!(imported_note_id(&reopened).is_some(), contents.is_some());
        let token = format!("/Contents {encoded}");
        assert!(
            session
                .output_bytes()
                .windows(token.len())
                .any(|bytes| bytes == token.as_bytes())
        );
        assert_eq!(session.origin_bytes(), bytes);
    }
}

#[test]
fn stale_or_wrong_bindings_and_invalid_payloads_preserve_accepted_output() {
    let bytes = graph::fixture("", "");
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [1; 16]).unwrap();
    let other =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [1; 16]).unwrap();
    let initial = session.snapshot().binding;
    let page = session.snapshot().scene.pages[0].id;
    let add = |rect, contents| NativeNoteCommand::Add {
        page,
        rect,
        contents,
    };
    let valid = add([10, 20, 50, 70], "Private".into());
    assert_eq!(
        session.apply(other.snapshot().binding, 1, &valid),
        Err(NativeSessionError::BindingMismatch)
    );
    let mut wrong_origin = initial;
    wrong_origin.origin_hash[0] ^= 1;
    assert_eq!(
        session.apply(wrong_origin, 1, &valid),
        Err(NativeSessionError::BindingMismatch)
    );
    assert_eq!(
        session.apply(initial, 0, &valid),
        Err(NativeSessionError::InvalidRequest)
    );

    let receipt = session.apply(initial, 1, &valid).unwrap();
    let accepted = session.output_bytes().to_vec();
    assert_eq!(
        session.apply(initial, 2, &valid),
        Err(NativeSessionError::RevisionMismatch)
    );
    assert_eq!(
        session.apply(receipt.binding, 1, &valid),
        Err(NativeSessionError::RevisionMismatch)
    );
    for command in [
        add([9, 20, 50, 70], "Private".into()),
        add(
            [10, 20, 50, 70],
            "X".repeat(flow_core::NATIVE_MAX_NOTE_BYTES + 1),
        ),
        add([10, 20, i64::MAX, 70], "Private".into()),
    ] {
        assert_eq!(
            session.apply(receipt.binding, 2, &command),
            Err(NativeSessionError::InvalidNote)
        );
    }
    assert_eq!(session.snapshot().binding, receipt.binding);
    assert_eq!(session.output_bytes(), accepted);
    assert_eq!(
        session.lookup(receipt.binding, 2),
        Ok(NativeRequestLookup::Unseen)
    );
}

#[test]
fn evicted_requests_can_only_reconcile() {
    let bytes = graph::fixture("", "");
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [1; 16]).unwrap();
    let initial = session.snapshot().binding;
    let page = session.snapshot().scene.pages[0].id;
    let add = NativeNoteCommand::Add {
        page,
        rect: [15, 25, 30, 40],
        contents: "One".into(),
    };
    let first = session.apply(initial, 1, &add).unwrap();
    let mut last = first;
    for request_id in 2..=flow_core::NATIVE_REQUEST_HISTORY as u64 + 1 {
        let command = NativeNoteCommand::Edit {
            note: first.note,
            contents: request_id.to_string(),
        };
        last = session.apply(last.binding, request_id, &command).unwrap();
    }
    assert_eq!(
        session.lookup(initial, 1),
        Ok(NativeRequestLookup::ReconciliationRequired)
    );
    assert_eq!(
        session.apply(initial, 1, &add),
        Err(NativeSessionError::ReconciliationRequired)
    );
    assert_eq!(session.snapshot().consumed_request, last.request_id);
    assert_eq!(session.snapshot().binding, last.binding);
    assert_eq!(session.snapshot().output_hash, last.output_hash);
}

#[test]
fn a_skipped_request_id_cannot_mutate_after_a_higher_id_is_accepted() {
    let bytes = graph::fixture("", "");
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [1; 16]).unwrap();
    let initial = session.snapshot().binding;
    let command = NativeNoteCommand::Add {
        page: session.snapshot().scene.pages[0].id,
        rect: [15, 25, 30, 40],
        contents: "One".into(),
    };
    let accepted = session.apply(initial, 3, &command).unwrap();
    let before = session.output_bytes().to_vec();

    assert_eq!(
        session.lookup(accepted.binding, 2),
        Ok(NativeRequestLookup::ReconciliationRequired)
    );
    assert_eq!(
        session.apply(accepted.binding, 2, &command),
        Err(NativeSessionError::ReconciliationRequired)
    );
    assert_eq!(session.snapshot().binding, accepted.binding);
    assert_eq!(session.snapshot().consumed_request, 3);
    assert_eq!(session.output_bytes(), before);
    assert_eq!(session.origin_bytes(), bytes);
}

#[test]
fn imported_notes_keep_identity_across_renumbering_and_aliases_remain_read_only() {
    let bytes = graph::fixture("", "/Annots 33 0 R");
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [1; 16]).unwrap();
    let NativePdfAnnotation::PlainNote(note) = &session.snapshot().scene.pages[0].annotations[0]
    else {
        panic!("expected a plain imported note")
    };
    assert_eq!(note.object.generation, 5);
    assert_eq!(note.contents, "Imported");
    let (id, object, rect) = (note.id, note.object, note.rect);
    session
        .apply(
            session.snapshot().binding,
            1,
            &NativeNoteCommand::Edit {
                note: id,
                contents: "Зміна".into(),
            },
        )
        .unwrap();
    assert_note(&session, 0, id, rect, "Зміна");
    let NativePdfAnnotation::PlainNote(note) = &session.snapshot().scene.pages[0].annotations[0]
    else {
        panic!("expected an accepted plain note")
    };
    assert_ne!(note.object, object);

    let bytes = graph::fixture("", "/Annots [30 5 R] /Custom 30 5 R");
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [1; 16]).unwrap();
    assert!(matches!(
        session.snapshot().scene.pages[0].annotations[0],
        NativePdfAnnotation::ReadOnly { .. }
    ));
    let binding = session.snapshot().binding;
    assert_eq!(
        session.apply(binding, 1, &NativeNoteCommand::Remove { note: id }),
        Err(NativeSessionError::TargetUnavailable)
    );
    assert_eq!(session.output_bytes(), bytes);
}

#[test]
fn imported_note_projection_checks_exact_identity() {
    let source = graph::fixture("", "/Annots [30 5 R]");
    let limits = NativeEditLimits::default();
    let session = NativePdfSession::open_scoped(&source, limits, [1; 16]).unwrap();
    let snapshot = session.snapshot();
    let page = &snapshot.scene.pages[0];
    let [NativePdfAnnotation::PlainNote(note)] = page.annotations.as_slice() else {
        panic!("expected a plain imported note");
    };
    let id = serde_json::to_value(note.id).unwrap().as_u64().unwrap();
    let verify = |limits, live, id| {
        NativePdfSession::verify_page_and_field_projection_with_objects(
            &source,
            limits,
            snapshot.binding,
            snapshot.scene.scene_hash,
            Some(&snapshot.scene.object_pages),
            &snapshot.page_state,
            &snapshot.fields,
            &[(0, note.object, id)],
            session.redaction_admitted(),
            live,
        )
    };
    assert_eq!(verify(limits, None, id), Ok(true));
    assert_eq!(verify(limits, None, id + 1), Ok(false));

    let constrained = NativeEditLimits {
        max_retained_bytes: 1,
        ..limits
    };
    assert_eq!(verify(constrained, Some(&session), id), Ok(true));
    assert_eq!(verify(constrained, Some(&session), id + 1), Ok(false));
}

fn pdf_with_objects(objects: &[&str]) -> String {
    let mut source = "%PDF-1.7\n".to_owned();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = source.len();
    let size = objects.len() + 1;
    source.push_str(&format!("xref\n0 {size}\n0000000000 65535 f \n"));
    for offset in offsets {
        source.push_str(&format!("{offset:010} 00000 n \n"));
    }
    source.push_str(&format!(
        "trailer\n<< /Root 1 0 R /Size {size} >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    source
}

#[test]
fn detached_verification_accepts_an_ordinary_rewrite_after_object_work_exhaustion() {
    assert_detached_object_projection_after_work_exhaustion(false);
}

#[test]
fn detached_verification_accepts_work_exhaustion_before_an_unsupported_curve() {
    assert_detached_object_projection_after_work_exhaustion(true);
}

fn assert_detached_object_projection_after_work_exhaustion(unsupported_curve: bool) {
    use flow_core::pdf::native::{
        NativeGraphic, NativeGraphicShape, NativeObjectCommand, NativeObjectPage,
        NativeObjectRefusal,
    };

    let stream = |data: &str| format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len());
    let late_paint = if unsupported_curve {
        "10 15 m 15 25 25 25 30 15 c S
"
    } else {
        ""
    };
    let source = pdf_with_objects(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents [6 0 R 7 0 R] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Group << /S /Transparency >> >>",
        &stream("10 20 30 40 re f\n"),
        &stream(&format!("%{}\n{late_paint}", " ".repeat(8 * 1024))),
    ]);
    let defaults = NativeEditLimits::default();
    let limits_at = |max_work| NativeEditLimits {
        max_work,
        ..defaults
    };
    let rewrite = |limits| {
        let mut session =
            NativePdfSession::open_scoped(source.as_bytes(), limits, [1; 16]).unwrap();
        let snapshot = session.snapshot();
        let add = NativeNoteCommand::Add {
            page: snapshot.scene.pages[0].id,
            rect: [60, 60, 80, 80],
            contents: "Chart note".into(),
        };
        let receipt = session.apply(snapshot.binding, 1, &add).unwrap();
        assert_eq!(receipt.binding.revision, 1);
        session
    };
    let roomy = rewrite(defaults);
    let full = roomy.snapshot();
    if unsupported_curve {
        assert!(full.scene.object_pages[0].refusal.is_some());
        assert!(full.scene.object_pages[0].objects.is_empty());
    } else {
        assert!(full.scene.object_pages[0].can_add);
        assert_eq!(full.scene.object_pages[0].objects.len(), 1);
    }

    let detached_recovers_objects = |max_work| {
        NativePdfSession::verify_page_projection_with_objects(
            roomy.output_bytes(),
            limits_at(max_work),
            full.binding,
            full.scene.scene_hash,
            Some(&full.scene.object_pages),
            &full.page_state,
        ) == Ok(true)
    };
    let (mut low, mut high) = (0, defaults.max_work);
    assert!(detached_recovers_objects(high));
    while low + 1 < high {
        let mid = low + (high - low) / 2;
        if detached_recovers_objects(mid) {
            high = mid;
        } else {
            low = mid;
        }
    }
    let limits = limits_at(high);
    let mut session = rewrite(limits);
    assert_eq!(session.output_bytes(), roomy.output_bytes());
    let snapshot = session.snapshot();
    let rows = &snapshot.scene.object_pages;
    assert!(!rows[0].can_add && rows[0].objects.is_empty() && rows[0].refusal.is_none());
    assert!(rows[1].can_add && rows[1].objects.is_empty() && rows[1].refusal.is_none());
    assert_eq!(rows[2].refusal, Some(NativeObjectRefusal::State));

    let [NativePdfAnnotation::PlainNote(note)] = snapshot.scene.pages[0].annotations.as_slice()
    else {
        panic!("expected the saved chart note");
    };
    let notes = [(
        0,
        note.object,
        serde_json::to_value(note.id).unwrap().as_u64().unwrap(),
    )];
    let hash_with_rows = |claimed: &[NativeObjectPage]| {
        let scene = &snapshot.scene;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"flowpdf:accepted-note-scene:v2");
        hasher.update(&scene.output_hash);
        for page in &scene.pages {
            hasher.update(
                &serde_json::to_vec(&(page.object, page.geometry, page.can_add_note)).unwrap(),
            );
            for annotation in &page.annotations {
                let NativePdfAnnotation::PlainNote(note) = annotation else {
                    panic!("expected only plain notes");
                };
                hasher.update(
                    &serde_json::to_vec(&(note.object, note.rect, &note.contents)).unwrap(),
                );
            }
        }
        if !scene.text_pages.is_empty() {
            hasher.update(b"flowpdf:native-text-pages:v1");
            hasher.update(&serde_json::to_vec(&scene.text_pages).unwrap());
        }
        hasher.update(b"flowpdf:native-object-pages:v1");
        hasher.update(&serde_json::to_vec(claimed).unwrap());
        *hasher.finalize().as_bytes()
    };
    let verify = |claimed: &[NativeObjectPage], live| {
        NativePdfSession::verify_page_and_field_projection_with_objects(
            session.output_bytes(),
            limits,
            snapshot.binding,
            hash_with_rows(claimed),
            Some(claimed),
            &snapshot.page_state,
            &snapshot.fields,
            &notes,
            session.redaction_admitted(),
            live,
        )
    };
    let verify_page = |claimed_scene, claimed_objects: &[NativeObjectPage]| {
        NativePdfSession::verify_page_projection_with_objects(
            session.output_bytes(),
            limits,
            snapshot.binding,
            claimed_scene,
            Some(claimed_objects),
            &snapshot.page_state,
        )
    };
    let verify_page_exact = |verification_limits, binding, claimed_scene, page_state| {
        NativePdfSession::verify_page_projection(
            session.output_bytes(),
            verification_limits,
            binding,
            claimed_scene,
            page_state,
        )
    };
    let verify_edit = |claimed_objects: &[NativeObjectPage]| {
        NativePdfSession::verify_edit_projection(
            session.output_bytes(),
            limits,
            snapshot.binding,
            &snapshot.scene.text_pages,
            claimed_objects,
        )
    };
    assert_eq!(hash_with_rows(rows), snapshot.scene.scene_hash);
    assert_eq!(verify(rows, None), Ok(true));
    assert_eq!(verify(rows, Some(&session)), Ok(true));
    assert_eq!(verify_edit(rows), Ok(true));
    assert_eq!(verify_page(snapshot.scene.scene_hash, rows), Ok(true));
    let mut wrong_scene_hash = snapshot.scene.scene_hash;
    wrong_scene_hash[0] ^= 1;
    assert_eq!(verify_page(wrong_scene_hash, rows), Ok(false));
    // @invariant: Mismatch rejection requires enough work to hash the full detached scene.
    assert_eq!(
        verify_page_exact(
            defaults,
            snapshot.binding,
            snapshot.scene.scene_hash,
            &snapshot.page_state,
        ),
        Ok(false)
    );
    assert_eq!(
        verify_page_exact(
            limits,
            full.binding,
            full.scene.scene_hash,
            &full.page_state
        ),
        Ok(true)
    );

    let verify_detached = |claimed: &[NativeObjectPage]| {
        [
            verify(claimed, None),
            verify_edit(claimed),
            verify_page(hash_with_rows(claimed), claimed),
        ]
    };
    let mut altered = rows.clone();
    altered[0].can_add = true;
    assert_eq!(verify_detached(&altered), [Ok(false), Ok(false), Ok(false)]);
    assert_eq!(verify(&altered, Some(&session)), Ok(false));
    altered[0].can_add = false;
    altered[2].refusal = None;
    assert_eq!(verify_detached(&altered), [Ok(false), Ok(false), Ok(false)]);
    altered[2].refusal = rows[2].refusal;
    altered.swap(1, 2);
    assert_eq!(verify_detached(&altered), [Ok(false), Ok(false), Ok(false)]);
    altered.swap(1, 2);
    altered[1].can_add = false;
    assert_eq!(verify(&altered, Some(&session)), Ok(false));

    let binding = snapshot.binding;
    for (page, refusal) in [
        (rows[0].page, NativeSessionError::Budget),
        (
            rows[2].page,
            NativeSessionError::Object(NativeObjectRefusal::State),
        ),
    ] {
        let add = NativeObjectCommand::AddGraphic {
            page,
            graphic: NativeGraphic {
                shape: NativeGraphicShape::Rectangle,
                points: vec![[10_000, 20_000], [40_000, 60_000]],
                fill: Some([0, 255, 0]),
                stroke: None,
                stroke_width: 0,
            },
        };
        assert_eq!(session.apply_object(binding, 2, &add), Err(refusal));
        assert_eq!(session.lookup(binding, 2), Ok(NativeRequestLookup::Unseen));
    }
    assert_eq!(session.snapshot().binding, binding);
    assert_eq!(session.output_bytes(), roomy.output_bytes());
}

#[test]
fn note_edit_refuses_when_rewrite_leaves_too_little_work_for_text_projection() {
    let content = format!("%{}\nq Q", " ".repeat(8 * 1024));
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    let source = pdf_with_objects(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Contents 4 0 R >>",
        &stream,
    ]);
    let defaults = NativeEditLimits::default();
    let limits_at = |max_work| NativeEditLimits {
        max_work,
        ..defaults
    };
    let open_at = |max_work| {
        NativePdfSession::open_scoped(source.as_bytes(), limits_at(max_work), [1; 16]).unwrap()
    };
    let mut roomy = open_at(defaults.max_work);
    let add = NativeNoteCommand::Add {
        page: roomy.snapshot().scene.pages[0].id,
        rect: [60, 60, 80, 80],
        contents: "Chart note".into(),
    };
    roomy.apply(roomy.snapshot().binding, 1, &add).unwrap();
    let snapshot = roomy.snapshot();
    assert!(snapshot.scene.text_pages.is_empty());
    let verify_text = |max_work| {
        NativePdfSession::verify_text_projection(
            roomy.output_bytes(),
            limits_at(max_work),
            snapshot.binding,
            &snapshot.scene.text_pages,
        )
    };
    let (mut low, mut high) = (content.len(), defaults.max_work);
    assert_eq!(verify_text(high), Ok(true));
    while low + 1 < high {
        let mid = low + (high - low) / 2;
        match verify_text(mid) {
            Ok(true) => high = mid,
            Err(NativeSessionError::Budget) => low = mid,
            result => panic!("unexpected detached text projection at {mid} work units: {result:?}"),
        }
    }
    assert_eq!(verify_text(low), Err(NativeSessionError::Budget));
    assert_eq!(verify_text(high), Ok(true));

    let mut tight = open_at(high);
    let before = tight.snapshot().clone();
    assert_eq!(
        tight.apply(before.binding, 1, &add),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(tight.output_bytes(), source.as_bytes());
    assert_eq!(tight.snapshot().binding, before.binding);
    assert_eq!(tight.snapshot().scene, before.scene);
    assert_eq!(tight.snapshot().consumed_request, before.consumed_request);
    assert_eq!(
        tight.lookup(before.binding, 1),
        Ok(NativeRequestLookup::Unseen)
    );
}

fn blank_page_pdf(width: u32) -> String {
    pdf_with_objects(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        &format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} 400] >>"),
    ])
}

#[test]
fn detached_projections_require_origin_identity_at_revision_zero() {
    use flow_core::pdf::native::{NativeBinding, NativePageCommand, NativePageTarget};

    let source = blank_page_pdf(300);
    let limits = NativeEditLimits::default();
    let mut session = NativePdfSession::open_scoped(source.as_bytes(), limits, [1; 16]).unwrap();
    let initial = session.snapshot().binding;
    assert_eq!(initial.revision, 0);
    assert_eq!(initial.origin_hash, session.snapshot().output_hash);

    let verify = |session: &NativePdfSession, binding: NativeBinding, expected| {
        let bytes = session.output_bytes();
        let snapshot = session.snapshot();
        let scene = &snapshot.scene;
        let combined = |live| {
            NativePdfSession::verify_page_and_field_projection_with_objects(
                bytes,
                limits,
                binding,
                scene.scene_hash,
                Some(&scene.object_pages),
                &snapshot.page_state,
                &snapshot.fields,
                &[],
                session.redaction_admitted(),
                live,
            )
        };
        for (verifier, result) in [
            ("combined detached", combined(None)),
            ("combined live", combined(Some(session))),
            (
                "page",
                NativePdfSession::verify_page_projection(
                    bytes,
                    limits,
                    binding,
                    scene.scene_hash,
                    &snapshot.page_state,
                ),
            ),
            (
                "page with objects",
                NativePdfSession::verify_page_projection_with_objects(
                    bytes,
                    limits,
                    binding,
                    scene.scene_hash,
                    Some(&scene.object_pages),
                    &snapshot.page_state,
                ),
            ),
            (
                "edit",
                NativePdfSession::verify_edit_projection(
                    bytes,
                    limits,
                    binding,
                    &scene.text_pages,
                    &scene.object_pages,
                ),
            ),
            (
                "text",
                NativePdfSession::verify_text_projection(bytes, limits, binding, &scene.text_pages),
            ),
        ] {
            assert_eq!(
                result,
                Ok(expected),
                "{verifier} at revision {}",
                binding.revision
            );
        }
    };

    verify(&session, initial, true);
    let other_source = blank_page_pdf(301);
    let mismatched = NativeBinding {
        origin_hash: *blake3::hash(other_source.as_bytes()).as_bytes(),
        ..initial
    };
    assert_ne!(mismatched.origin_hash, initial.origin_hash);
    verify(&session, mismatched, false);

    let target = NativePageTarget {
        page: 0,
        stable_id: session.snapshot().page_state.order[0],
    };
    let revised = session
        .apply_page(
            initial,
            1,
            &NativePageCommand::RotatePage {
                target,
                degrees: 90,
            },
        )
        .unwrap();
    assert_eq!(revised.binding.revision, 1);
    assert_eq!(revised.binding.origin_hash, initial.origin_hash);
    assert_ne!(revised.output_hash, revised.binding.origin_hash);
    verify(&session, revised.binding, true);
}

#[test]
fn detached_redaction_admission_uses_work_left_after_projection() {
    let source = pdf_with_objects(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] >>",
        "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 /CropBox [5 5 195 195] /Rotate 90 >>",
        "<< /Type /Page /Parent 3 0 R /Contents 5 0 R >>",
        "<< /Length 17 >>\nstream\n10 20 10 10 re f\n\nendstream",
    ]);
    let bytes = source.as_bytes();
    let defaults = NativeEditLimits::default();
    let session = NativePdfSession::open_scoped(bytes, defaults, [1; 16]).unwrap();
    assert_eq!(session.check_redaction_admission(), Ok(()));

    let verify = |session: &NativePdfSession, limits, admitted| {
        let snapshot = session.snapshot();
        NativePdfSession::verify_page_and_field_projection_with_objects(
            session.output_bytes(),
            limits,
            snapshot.binding,
            snapshot.scene.scene_hash,
            Some(&snapshot.scene.object_pages),
            &snapshot.page_state,
            &snapshot.fields,
            &[],
            admitted,
            None,
        )
    };
    let limits_at = |max_work| NativeEditLimits {
        max_work,
        ..defaults
    };
    let high = 401;
    let low = high - 1;
    assert_eq!(verify(&session, limits_at(low), false), Ok(true));
    assert_eq!(
        verify(&session, limits_at(low), true),
        Err(NativeSessionError::Budget)
    );
    assert_eq!(verify(&session, limits_at(high), true), Ok(true));

    for (max_work, admitted) in [(low, false), (high, true)] {
        let limits = limits_at(max_work);
        let live = NativePdfSession::open_scoped(bytes, limits, [1; 16]).unwrap();
        let expected = if admitted {
            Ok(())
        } else {
            Err(NativeSessionError::Budget)
        };
        assert_eq!(live.check_redaction_admission(), expected);
        assert_eq!(verify(&live, limits, admitted), Ok(true));
    }
}

#[test]
fn detached_projections_exhaust_the_shared_identity_allowance() {
    let source = blank_page_pdf(300);
    let bytes = source.as_bytes();
    let limits = NativeEditLimits::default();
    let session = NativePdfSession::open_scoped(bytes, limits, [1; 16]).unwrap();
    let snapshot = session.snapshot();
    let scene = &snapshot.scene;
    let combined = |limits, scene_hash| {
        NativePdfSession::verify_page_and_field_projection_with_objects(
            bytes,
            limits,
            snapshot.binding,
            scene_hash,
            Some(&scene.object_pages),
            &snapshot.page_state,
            &snapshot.fields,
            &[],
            false,
            None,
        )
    };
    let verify = |limits| {
        [
            combined(limits, scene.scene_hash),
            NativePdfSession::verify_page_projection(
                bytes,
                limits,
                snapshot.binding,
                scene.scene_hash,
                &snapshot.page_state,
            ),
            NativePdfSession::verify_edit_projection(
                bytes,
                limits,
                snapshot.binding,
                &scene.text_pages,
                &scene.object_pages,
            ),
            NativePdfSession::verify_text_projection(
                bytes,
                limits,
                snapshot.binding,
                &scene.text_pages,
            ),
        ]
    };
    assert_eq!(verify(limits), [Ok(true), Ok(true), Ok(true), Ok(true)]);

    let constrained = |max_retained_bytes| NativeEditLimits {
        max_retained_bytes,
        ..limits
    };
    let wrong_hash = [!scene.scene_hash[0]; 32];
    let mut low = bytes.len();
    let mut high = 64 * 1024;
    assert_eq!(combined(constrained(high), wrong_hash), Ok(false));
    while low + 1 < high {
        let mid = (low + high) / 2;
        if combined(constrained(mid), wrong_hash).is_ok() {
            high = mid;
        } else {
            low = mid;
        }
    }
    for result in verify(constrained(low)) {
        assert_eq!(result, Err(NativeSessionError::Budget));
    }
}

#[test]
fn public_scene_preserves_indirect_arrays_after_note_edit() {
    use flow_core::pdf::{PdfSceneElement, PdfSceneRequest, open_pdf, read_pdf_scene};

    let source = pdf_with_objects(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Annots 7 0 R >>",
        "[6 0 R 5 0 R]",
        "<< /Length 17 >>\nstream\n30 40 10 10 re f\n\nendstream",
        "<< /Length 17 >>\nstream\n10 20 10 10 re f\n\nendstream",
        "[9 0 R 8 0 R]",
        "<< /Type /Annot /Subtype /Text /Rect [60 60 70 70] /Contents (Original) /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Link /Rect [80 80 90 90] /Dest [3 0 R /Fit] >>",
    ]);
    let mut session =
        NativePdfSession::open_scoped(source.as_bytes(), NativeEditLimits::default(), [1; 16])
            .unwrap();
    let note = session.snapshot().scene.pages[0]
        .annotations
        .iter()
        .find_map(|annotation| match annotation {
            NativePdfAnnotation::PlainNote(note) => Some(note.id),
            _ => None,
        })
        .unwrap();
    session
        .apply(
            session.snapshot().binding,
            1,
            &NativeNoteCommand::Edit {
                note,
                contents: "Edited".into(),
            },
        )
        .unwrap();
    for bytes in [source.as_bytes(), session.output_bytes()] {
        let reader = open_pdf(bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        let [link, note, first, second] = scene.pages[0].elements.as_slice() else {
            panic!("expected two ordered annotations and two ordered paths");
        };
        assert!(matches!(link, PdfSceneElement::Link { value } if value.internal));
        assert!(
            matches!(note, PdfSceneElement::Annotation { value } if value.subtype.as_deref() == Some("Text") && value.rect.x.raw() == 60 * 64)
        );
        assert!(
            matches!(first, PdfSceneElement::Path { value } if value.fill && value.rect.x.raw() == 10 * 64)
        );
        assert!(
            matches!(second, PdfSceneElement::Path { value } if value.fill && value.rect.x.raw() == 30 * 64)
        );
    }
}

#[test]
fn public_scene_keeps_stream_provenance_and_reports_split_operands_after_note_rewrite() {
    use flow_core::pdf::{
        PdfReadDiagnosticCode, PdfSceneElement, PdfSceneRequest, open_pdf, read_pdf_scene,
    };

    let stream = |data: &str| format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len());
    for (second, third, split_operand) in [
        (" f Q\n", "10 20 5 5 re f\n", false),
        (
            " f Q BT /F1 10 Tf 40 60 Td (A)\n",
            "Tj ET 10 20 5 5 re f\n",
            true,
        ),
        (
            " f Q BT /F1 10 Tf 40 60 Td [(A)\n",
            "] TJ ET 10 20 5 5 re f\n",
            true,
        ),
    ] {
        let first = "q 1 0 0 1 20 30 cm 10 20 5 5 re\n";
        let source = pdf_with_objects(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents [4 0 R 5 0 R 6 0 R] /Resources << /Font << /F1 7 0 R >> >> >>",
            &stream(first),
            &stream(second),
            &stream(third),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        ]);
        let mut session =
            NativePdfSession::open_scoped(source.as_bytes(), NativeEditLimits::default(), [1; 16])
                .unwrap();
        let rect = [80, 80, 100, 100];
        session
            .apply(
                session.snapshot().binding,
                1,
                &NativeNoteCommand::Add {
                    page: session.snapshot().scene.pages[0].id,
                    rect,
                    contents: "Review artwork".into(),
                },
            )
            .unwrap();
        let reopened = NativePdfSession::open_scoped(
            session.output_bytes(),
            NativeEditLimits::default(),
            [1; 16],
        )
        .unwrap();
        assert!(matches!(
            reopened.snapshot().scene.pages[0].annotations.as_slice(),
            [NativePdfAnnotation::PlainNote(note)]
                if note.rect == rect && note.contents == "Review artwork"
        ));
        assert_eq!(session.origin_bytes(), source.as_bytes());

        for (bytes, has_note) in [(source.as_bytes(), false), (session.output_bytes(), true)] {
            let syntax = bytes.utf8_chunks().last().unwrap().valid();
            let syntax_offset = bytes.len() - syntax.len();
            let stream_location = |data: &str| {
                let (before, _) = syntax
                    .split_once(&format!("\nstream\n{data}\nendstream"))
                    .unwrap();
                let start = before.rfind("\nendobj\n").unwrap() + "\nendobj\n".len();
                let object_number = syntax[start..]
                    .split_ascii_whitespace()
                    .next()
                    .unwrap()
                    .parse::<u32>()
                    .unwrap();
                (object_number, u32::try_from(syntax_offset + start).unwrap())
            };
            let (second_object, second_offset) = stream_location(second);
            let reader = open_pdf(bytes, Default::default()).unwrap();
            let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
            let page = &scene.pages[0];
            assert_eq!(
                page.elements.iter().any(|element| matches!(element,
                    PdfSceneElement::Annotation { value } if value.subtype.as_deref() == Some("Text")
                )),
                has_note
            );
            let mut paint = page
                .elements
                .iter()
                .filter(|element| !matches!(element, PdfSceneElement::Annotation { .. }));
            let Some(PdfSceneElement::Path { value }) = paint.next() else {
                panic!("expected the fill in the second stream");
            };
            assert!(value.fill);
            assert_eq!((value.rect.x.raw(), value.rect.y.raw()), (30 * 64, 50 * 64));
            assert_eq!(value.provenance.object_ref.object_number, second_object);
            assert_eq!(value.provenance.invocation_path, [1]);
            assert_eq!(value.provenance.operator.as_deref(), Some("f"));
            assert_eq!(value.provenance.stream_offset, second_offset + 1);
            assert_eq!(value.provenance.stream_end, Some(second_offset + 2));
            assert_eq!(value.provenance.operand_span, None);
            assert_eq!(page.partial, split_operand);
            assert_eq!(scene.report.partial, split_operand);
            let mut partial_diagnostics = page
                .report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == PdfReadDiagnosticCode::PartialPage);
            if split_operand {
                let diagnostic = partial_diagnostics.next().unwrap();
                assert_eq!(diagnostic.page_index, Some(0));
                assert_eq!(diagnostic.object_number, Some(second_object));
            } else {
                assert!(matches!(
                    paint.next(),
                    Some(PdfSceneElement::Path { value }) if value.fill
                        && value.rect.x.raw() == 10 * 64 && value.rect.y.raw() == 20 * 64
                        && value.provenance.object_ref.object_number == stream_location(third).0
                        && value.provenance.invocation_path == [2]
                ));
            }
            assert!(partial_diagnostics.next().is_none());
            assert!(paint.next().is_none());
        }
    }
}

#[test]
fn public_scene_checks_stream_boundaries_after_note_rewrite() {
    use flow_core::pdf::{
        PdfReadDiagnosticCode, PdfSceneElement, PdfSceneRequest, open_pdf, read_pdf_scene,
    };

    let stream = |data: &str| format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len());
    for (first, second, partial, show_second) in [
        (
            "10 20 5 5 re f % Artwork",
            "\n30 40 5 5 re f\n",
            false,
            true,
        ),
        ("10 20 5 5 re f % Artwork", " 30 40 5 5 re f\n", true, false),
        ("10 20 5 5 re f ", "30 40 5 5 re f\n", true, true),
        ("10 20 5 5 re f ", "re\n", true, false),
    ] {
        let source = pdf_with_objects(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents [4 0 R 5 0 R] >>",
            &stream(first),
            &stream(second),
        ]);
        let mut session =
            NativePdfSession::open_scoped(source.as_bytes(), NativeEditLimits::default(), [1; 16])
                .unwrap();
        session
            .apply(
                session.snapshot().binding,
                1,
                &NativeNoteCommand::Add {
                    page: session.snapshot().scene.pages[0].id,
                    rect: [80, 80, 100, 100],
                    contents: "Review artwork".into(),
                },
            )
            .unwrap();

        for (bytes, has_note) in [(source.as_bytes(), false), (session.output_bytes(), true)] {
            let reader = open_pdf(bytes, Default::default()).unwrap();
            let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
            let page = &scene.pages[0];
            assert_eq!(
                page.elements.iter().any(|element| matches!(element,
                    PdfSceneElement::Annotation { value } if value.subtype.as_deref() == Some("Text")
                )),
                has_note
            );
            let mut paths = page.elements.iter().filter_map(|element| match element {
                PdfSceneElement::Path { value } => Some(value),
                _ => None,
            });
            let first = paths.next().unwrap();
            assert!(first.fill);
            assert_eq!(first.rect.x.raw(), 10 * 64);
            assert_eq!(first.provenance.invocation_path, [0]);

            assert_eq!(page.partial, partial);
            assert_eq!(scene.report.partial, partial);
            let mut partial_diagnostics = page
                .report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == PdfReadDiagnosticCode::PartialPage);
            if partial {
                let diagnostic = partial_diagnostics.next().unwrap();
                assert_eq!(diagnostic.page_index, Some(0));
                assert_eq!(
                    diagnostic.object_number,
                    Some(first.provenance.object_ref.object_number)
                );
            }
            assert!(partial_diagnostics.next().is_none());
            if show_second {
                let second = paths.next().unwrap();
                assert!(second.fill);
                assert_eq!(second.rect.x.raw(), 30 * 64);
                assert_ne!(second.provenance.object_ref, first.provenance.object_ref);
                assert_eq!(second.provenance.invocation_path, [1]);
            }
            assert!(paths.next().is_none());
        }
    }
}

#[test]
fn public_scene_preserves_distinct_resource_aliases_after_note_rewrite() {
    use flow_core::pdf::{PdfSceneElement, PdfSceneRequest, open_pdf, read_pdf_scene};

    let form = |paint: &str| {
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Length {} >>\nstream\n{paint}\nendstream",
            paint.len()
        )
    };
    let paint = "/Logo#e9 Do /Logo#c3#a9 Do";
    let source = pdf_with_objects(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /XObject << /Logo#E9 5 0 R /Logo#C3#A9 6 0 R >> >> >>",
        &format!("<< /Length {} >>\nstream\n{paint}\nendstream", paint.len()),
        &form("10 20 5 5 re f"),
        &form("30 40 5 5 re f"),
    ]);
    let painted_positions = |bytes: &[u8]| {
        let reader = open_pdf(bytes, Default::default()).unwrap();
        let scene = read_pdf_scene(&reader, &PdfSceneRequest::default()).unwrap();
        scene.pages[0]
            .elements
            .iter()
            .filter_map(|element| match element {
                PdfSceneElement::Path { value } if value.fill => Some(value.rect.x.raw()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(painted_positions(source.as_bytes()), [10 * 64, 30 * 64]);

    let mut session =
        NativePdfSession::open_scoped(source.as_bytes(), NativeEditLimits::default(), [1; 16])
            .unwrap();
    let snapshot = session.snapshot();
    session
        .apply(
            snapshot.binding,
            1,
            &NativeNoteCommand::Add {
                page: snapshot.scene.pages[0].id,
                rect: [60, 60, 80, 80],
                contents: "Review artwork".into(),
            },
        )
        .unwrap();
    assert_eq!(
        painted_positions(session.output_bytes()),
        [10 * 64, 30 * 64]
    );
    assert_eq!(session.origin_bytes(), source.as_bytes());
}

#[test]
fn public_reader_keeps_null_contents_pages_blank_after_note_edit() {
    use flow_core::pdf::{PdfSceneElement, PdfSceneRequest, open_pdf, read_pdf_scene};

    for contents in ["null", "6 0 R"] {
        let source = pdf_with_objects(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 200] >>",
            &format!("<< /Type /Page /Parent 2 0 R /Contents {contents} >>"),
            "<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>",
            "<< /Length 17 >>\nstream\n10 20 10 10 re f\n\nendstream",
            "null",
        ]);
        let mut session =
            NativePdfSession::open_scoped(source.as_bytes(), NativeEditLimits::default(), [1; 16])
                .unwrap();
        session
            .apply(
                session.snapshot().binding,
                1,
                &NativeNoteCommand::Add {
                    page: session.snapshot().scene.pages[1].id,
                    rect: [60, 60, 70, 70],
                    contents: "Review note".into(),
                },
            )
            .unwrap();

        let reader = open_pdf(session.output_bytes(), Default::default()).unwrap();
        assert_eq!(reader.summary().unwrap().page_count, 2);
        let scene = read_pdf_scene(
            &reader,
            &PdfSceneRequest {
                first_page: 0,
                page_count: 2,
            },
        )
        .unwrap();
        let [blank, populated] = scene.pages.as_slice() else {
            panic!("expected both pages in the public scene");
        };
        assert!(blank.elements.is_empty());
        assert!(!blank.partial);
        let [note, path] = populated.elements.as_slice() else {
            panic!("expected the added note and the original path");
        };
        assert!(
            matches!(note, PdfSceneElement::Annotation { value } if value.subtype.as_deref() == Some("Text"))
        );
        assert!(
            matches!(path, PdfSceneElement::Path { value } if value.fill && value.rect.x.raw() == 10 * 64)
        );
    }
}

#[test]
fn shared_annotation_array_preserves_geometry_but_disables_note_writes() {
    let bytes = graph::fixture("/Shared 33 0 R", "/Annots 33 0 R");
    let mut session =
        NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [1; 16]).unwrap();
    let scene = &session.snapshot().scene;
    let page = &scene.pages[0];
    assert_eq!(page.geometry.unwrap().bounds(), [10, 20, 170, 110]);
    assert!(!page.can_add_note);
    assert!(scene.pages[1].can_add_note);
    assert_eq!(page.annotations.len(), 3);
    assert!(
        page.annotations
            .iter()
            .all(|annotation| matches!(annotation, NativePdfAnnotation::ReadOnly { .. }))
    );
    let add = NativeNoteCommand::Add {
        page: page.id,
        rect: [20, 30, 40, 50],
        contents: "Note".into(),
    };
    let binding = session.snapshot().binding;
    assert_eq!(
        session.apply(binding, 1, &add),
        Err(NativeSessionError::TargetUnavailable)
    );
    assert_eq!(session.snapshot().binding, binding);
    assert_eq!(session.lookup(binding, 1), Ok(NativeRequestLookup::Unseen));
    assert_eq!(session.output_bytes(), bytes);
}

#[test]
fn retained_stream_work_boundary_preserves_accepted_session_state() {
    let defaults = NativeEditLimits::default();
    let boundary = |payload: &str| {
        // @contract: A catalog stream is retained without being scanned as page content.
        let stream = format!(
            "<< /Length {} >>\nstream\n{payload}\nendstream",
            payload.len()
        );
        let source = pdf_with_objects(&[
            "<< /Type /Catalog /Pages 2 0 R /Retained 4 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
            &stream,
        ]);
        let with_first_note = |max_work| {
            let mut session = NativePdfSession::open_scoped(
                source.as_bytes(),
                NativeEditLimits {
                    max_work,
                    ..defaults
                },
                [1; 16],
            )?;
            let initial = session.snapshot().binding;
            let add = NativeNoteCommand::Add {
                page: session.snapshot().scene.pages[0].id,
                rect: [20, 30, 40, 50],
                contents: "Review".into(),
            };
            let accepted = session.apply(initial, 1, &add)?;
            Ok::<_, NativeSessionError>((session, initial, accepted))
        };
        let (mut roomy, _, first) = with_first_note(defaults.max_work).unwrap();
        let edited_contents = "Ї".repeat(512);
        let edit = NativeNoteCommand::Edit {
            note: first.note,
            contents: edited_contents.clone(),
        };
        roomy.apply(first.binding, 2, &edit).unwrap();
        let (mut lower, mut upper) = (1, defaults.max_work);
        while lower < upper {
            let mid = lower + (upper - lower) / 2;
            if with_first_note(mid).is_ok_and(|(mut session, _, accepted)| {
                session.apply(accepted.binding, 2, &edit).is_ok()
            }) {
                upper = mid;
            } else {
                lower = mid + 1;
            }
        }

        let (mut exact, _, accepted) = with_first_note(lower).unwrap();
        let receipt = exact.apply(accepted.binding, 2, &edit).unwrap();
        assert_eq!(receipt.binding.revision, 2);
        assert!(matches!(
            exact.snapshot().scene.pages[0].annotations.as_slice(),
            [NativePdfAnnotation::PlainNote(note)]
                if note.id == accepted.note && note.contents == edited_contents
        ));
        assert_eq!(exact.output_bytes(), roomy.output_bytes());
        let retained = format!("stream\n{payload}\nendstream");
        assert!(
            exact
                .output_bytes()
                .windows(retained.len())
                .any(|bytes| bytes == retained.as_bytes())
        );

        let (mut tight, initial, accepted) = with_first_note(lower - 1).unwrap();
        let before = tight.snapshot().clone();
        let accepted_bytes = tight.output_bytes().to_vec();
        assert_eq!(before.binding.revision, 1);
        for _ in 0..2 {
            assert_eq!(
                tight.apply(before.binding, 2, &edit),
                Err(NativeSessionError::Budget)
            );
            assert_eq!(
                tight.lookup(before.binding, 2),
                Ok(NativeRequestLookup::Unseen)
            );
        }
        assert_eq!(
            tight.lookup(initial, 1),
            Ok(NativeRequestLookup::Accepted(accepted))
        );
        assert_eq!(tight.snapshot().binding, before.binding);
        assert_eq!(tight.snapshot().consumed_request, before.consumed_request);
        assert_eq!(tight.snapshot().scene, before.scene);
        assert_eq!(tight.snapshot().page_state, before.page_state);
        assert_eq!(tight.snapshot().output_hash, before.output_hash);
        assert_eq!(tight.output_bytes(), accepted_bytes);
        assert_eq!(tight.origin_bytes(), source.as_bytes());
        lower
    };
    let short = "x".repeat(1024);
    let long = "x".repeat(9217);
    let serialization_work = long.len().div_ceil(16) - short.len().div_ceil(16);
    let preservation_work = 2 * serialization_work;
    assert_eq!(
        boundary(&long),
        boundary(&short) + serialization_work + preservation_work
    );
}

#[test]
fn request_history_budget_refuses_before_acceptance() {
    let bytes = graph::fixture("", "");
    let limits = NativeEditLimits {
        max_retained_bytes: 128 * 1024,
        ..NativeEditLimits::default()
    };
    let mut session = NativePdfSession::open_scoped(&bytes, limits, [1; 16]).unwrap();
    let page = session.snapshot().scene.pages[0].id;
    let add = NativeNoteCommand::Add {
        page,
        rect: [15, 25, 30, 40],
        contents: "One".into(),
    };
    let first = session.apply(session.snapshot().binding, 1, &add).unwrap();
    let edit = NativeNoteCommand::Edit {
        note: first.note,
        contents: "One".into(),
    };
    for id in 2..=flow_core::NATIVE_REQUEST_HISTORY as u64 {
        let binding = session.snapshot().binding;
        let before = session.output_bytes().to_vec();
        match session.apply(binding, id, &edit) {
            Ok(_) => assert_eq!(session.output_bytes(), before),
            Err(NativeSessionError::Budget) => {
                assert_eq!(session.snapshot().binding, binding);
                assert_eq!(session.snapshot().consumed_request, id - 1);
                assert_eq!(session.lookup(binding, id), Ok(NativeRequestLookup::Unseen));
                assert_eq!(session.output_bytes(), before);
                assert_eq!(session.origin_bytes(), bytes);
                return;
            }
            Err(error) => panic!("unexpected error {}", error.code()),
        }
    }
    panic!("receipt history should exhaust the lowered retained budget");
}

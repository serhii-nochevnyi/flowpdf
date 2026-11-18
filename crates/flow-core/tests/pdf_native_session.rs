#[path = "pdf_native_graph.rs"]
mod graph;

use flow_core::{
    NativeEditLimits, NativeNoteCommand, NativePdfAnnotation, NativePdfSession,
    NativeRequestLookup, NativeSessionError,
};

#[test]
fn note_crud_reads_reopened_output_and_reconciles_requests() {
    let source = graph::fixture("", "/Annots 33 0 R");
    let mut session = NativePdfSession::open(&source, NativeEditLimits::default()).unwrap();
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
    let reopened = NativePdfSession::open(output, NativeEditLimits::default()).unwrap();
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
fn stale_or_wrong_bindings_and_invalid_payloads_preserve_accepted_output() {
    let bytes = graph::fixture("", "");
    let mut session = NativePdfSession::open(&bytes, NativeEditLimits::default()).unwrap();
    let other = NativePdfSession::open(&bytes, NativeEditLimits::default()).unwrap();
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
    let mut session = NativePdfSession::open(&bytes, NativeEditLimits::default()).unwrap();
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
    let mut session = NativePdfSession::open(&bytes, NativeEditLimits::default()).unwrap();
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
    let mut session = NativePdfSession::open(&bytes, NativeEditLimits::default()).unwrap();
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
    let mut session = NativePdfSession::open(&bytes, NativeEditLimits::default()).unwrap();
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
fn shared_annotation_array_preserves_geometry_but_disables_note_writes() {
    let bytes = graph::fixture("/Shared 33 0 R", "/Annots 33 0 R");
    let mut session = NativePdfSession::open(&bytes, NativeEditLimits::default()).unwrap();
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
fn request_history_budget_refuses_before_acceptance() {
    let bytes = graph::fixture("", "");
    let limits = NativeEditLimits {
        max_retained_bytes: 60 * 1024,
        ..NativeEditLimits::default()
    };
    let mut session = NativePdfSession::open(&bytes, limits).unwrap();
    let page = session.snapshot().scene.pages[0].id;
    let add = NativeNoteCommand::Add {
        page,
        rect: [15, 25, 30, 40],
        contents: "One".into(),
    };
    let first = session.apply(session.snapshot().binding, 1, &add).unwrap();
    // Keep the PDF graph and text constant so only the receipt history grows.
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

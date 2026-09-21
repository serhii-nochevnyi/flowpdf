use flow_core::{
    EditorSessionAction, EditorSessionRequest, EditorSessionState, EditorViewRequest,
    FormattingState, apply_editor_session,
    canonical::canonical_bytes,
    model::{Affinity, FlowDocument, LogicalPosition, MarkSet, NodeId},
    query_editor_view,
};

fn document() -> FlowDocument {
    FlowDocument::deterministic_sample("uk-UA").expect("valid sample")
}

fn canonical_json(document: &FlowDocument) -> String {
    String::from_utf8(canonical_bytes(document).expect("canonical sample")).expect("utf-8")
}

fn session() -> (FlowDocument, String, EditorSessionState) {
    let document = document();
    let canonical_json = canonical_json(&document);
    let session = EditorSessionState::from_document(&document).expect("initial session");
    (document, canonical_json, session)
}

#[test]
fn initial_session_is_rust_derived_and_noncanonical() {
    let (document, _, state) = session();

    assert_eq!(state.document_id, document.document_id);
    assert_eq!(state.revision, document.revision);
    assert_eq!(state.session_generation, 0);
    assert_eq!(state.selection.anchor, state.selection.focus);
    assert_eq!(state.formatting.bold, FormattingState::Off);
    assert_eq!(state.formatting.italic, FormattingState::Off);
    assert_eq!(state.formatting.underline, FormattingState::Off);
    assert_eq!(state.capabilities.len(), 26);
    assert!(
        state
            .capabilities
            .iter()
            .take(2)
            .all(|capability| capability.enabled)
    );
    assert!(
        state
            .capabilities
            .iter()
            .any(|capability| capability.name == flow_core::EditorCapability::SplitTextBlock)
    );
}

#[test]
fn directional_selection_preserves_endpoint_order_and_affinity() {
    let (document, canonical_json, state) = session();
    let node = &document.content[0];
    let end = node.text().encode_utf16().count() as u32;
    let selection = flow_core::DirectionalSelection {
        anchor: LogicalPosition {
            node_id: node.id.clone(),
            utf16_offset: end.into(),
            affinity: Affinity::Backward,
        },
        focus: LogicalPosition {
            node_id: node.id.clone(),
            utf16_offset: 0.into(),
            affinity: Affinity::Forward,
        },
    };

    let response = apply_editor_session(EditorSessionRequest {
        canonical_json,
        session: state.clone(),
        action: EditorSessionAction::SetSelection {
            selection: selection.clone(),
        },
    });
    assert!(response.ok, "{response:?}");
    let updated = response.value.expect("session response");
    assert_eq!(updated.session.selection, selection);
    assert_eq!(updated.view.selection, selection);
    assert_eq!(updated.session.revision, document.revision);
    assert_eq!(updated.session.session_generation, 1);
}

#[test]
fn session_only_actions_change_generation_but_not_document_identity_or_revision() {
    let (document, canonical_json, state) = session();
    let before = canonical_json.clone();
    let response = apply_editor_session(EditorSessionRequest {
        canonical_json,
        session: state.clone(),
        action: EditorSessionAction::SetPendingMarks {
            marks: MarkSet {
                bold: true,
                ..MarkSet::default()
            },
        },
    });
    assert!(response.ok, "{response:?}");
    let updated = response.value.expect("session response");
    assert_eq!(updated.session.document_id, document.document_id);
    assert_eq!(updated.session.revision, state.revision);
    assert_eq!(updated.session.session_generation, 1);
    assert!(updated.session.pending_marks.bold);
    assert_eq!(updated.view.revision, document.revision);
    assert_eq!(
        String::from_utf8(canonical_bytes(&document).expect("canonical sample")).expect("utf-8"),
        before
    );
    assert!(
        !serde_json::to_string(&updated)
            .expect("serializable")
            .contains("commit")
    );
}

#[test]
fn query_revalidates_without_advancing_session_generation() {
    let (document, canonical_json, state) = session();
    let response = query_editor_view(EditorViewRequest {
        canonical_json,
        session: state.clone(),
    });
    assert!(response.ok, "{response:?}");
    let view = response.value.expect("view");
    assert_eq!(view.document_id, document.document_id);
    assert_eq!(view.revision, document.revision);
    assert_eq!(view.session_generation, state.session_generation);
    assert_eq!(view.selection, state.selection);
}

#[test]
fn stale_revision_and_unknown_node_reject_without_a_session_result() {
    let (document, canonical_json, state) = session();
    let mut stale = state.clone();
    stale.revision = document.revision.saturating_sub(1);
    let stale_response = apply_editor_session(EditorSessionRequest {
        canonical_json: canonical_json.clone(),
        session: stale,
        action: EditorSessionAction::SetSelection {
            selection: state.selection.clone(),
        },
    });
    assert!(!stale_response.ok);
    assert_eq!(
        stale_response.error.expect("stale error").code,
        "FLOW_STALE_EDITOR_SESSION"
    );

    let mut unknown = state.selection.anchor.clone();
    unknown.node_id = NodeId::new("00000000-0000-4000-8000-000000009999").expect("id");
    let unknown_response = apply_editor_session(EditorSessionRequest {
        canonical_json,
        session: state.clone(),
        action: EditorSessionAction::SetSelection {
            selection: flow_core::DirectionalSelection {
                anchor: unknown,
                focus: state.selection.focus.clone(),
            },
        },
    });
    assert!(!unknown_response.ok);
    assert_eq!(
        unknown_response.error.expect("unknown node error").code,
        "FLOW_UNKNOWN_POSITION_NODE"
    );
}

#[test]
fn serialized_request_round_trip_has_the_same_result_and_closed_fields() {
    let (_, canonical_json, state) = session();
    let request = EditorSessionRequest {
        canonical_json,
        session: state.clone(),
        action: EditorSessionAction::SetPendingMarks {
            marks: MarkSet::default(),
        },
    };
    let encoded = serde_json::to_value(&request).expect("request json");
    let decoded: EditorSessionRequest = serde_json::from_value(encoded).expect("closed request");
    let native = apply_editor_session(request);
    let serialized = apply_editor_session(decoded);
    assert_eq!(
        serde_json::to_value(native).expect("native json"),
        serde_json::to_value(serialized).expect("serialized json")
    );

    let mut unknown = serde_json::to_value(EditorViewRequest {
        canonical_json: session().1,
        session: state,
    })
    .expect("view json");
    unknown["domPath"] = serde_json::json!("0/0");
    assert!(serde_json::from_value::<EditorViewRequest>(unknown).is_err());
}

#[test]
fn pending_marks_are_rejected_for_directional_ranges() {
    let (document, canonical_json, state) = session();
    let selection = flow_core::DirectionalSelection {
        anchor: state.selection.anchor.clone(),
        focus: LogicalPosition {
            node_id: document.content[0].id.clone(),
            utf16_offset: 2.into(),
            affinity: Affinity::Backward,
        },
    };
    let selected = apply_editor_session(EditorSessionRequest {
        canonical_json: canonical_json.clone(),
        session: state,
        action: EditorSessionAction::SetSelection { selection },
    });
    assert!(selected.ok, "{selected:?}");
    let selected = selected.value.expect("selected session");
    let response = apply_editor_session(EditorSessionRequest {
        canonical_json,
        session: selected.session,
        action: EditorSessionAction::SetPendingMarks {
            marks: MarkSet {
                italic: true,
                ..MarkSet::default()
            },
        },
    });
    assert!(!response.ok);
    assert_eq!(
        response.error.expect("range pending mark error").code,
        "FLOW_PENDING_MARKS_REQUIRES_COLLAPSED_SELECTION"
    );
}

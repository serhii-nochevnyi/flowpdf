use flow_core::{
    anchor::{
        AnchorError, EditorPositionError, GraphemeBoundaryMap, NodePositionMap, ResolvedPosition,
        Utf16Offset, resolve_utf16_offset,
    },
    model::{Affinity, BlockKind, ContentNode, FlowDocument, InlineRun, LogicalPosition, MarkSet},
    schema::DocumentLimits,
};

fn paragraph(text: &str) -> ContentNode {
    let sample = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    ContentNode::paragraph(sample.content[0].id.clone(), None, text.into())
}

fn position(node: &ContentNode, offset: u32, affinity: Affinity) -> LogicalPosition {
    LogicalPosition {
        node_id: node.id.clone(),
        utf16_offset: offset.into(),
        affinity,
    }
}

#[test]
fn unicode_17_full_corpus_matches_utf8_and_utf16_boundaries() {
    let corpus = include_str!("../../../fixtures/unicode/17.0.0/GraphemeBreakTest.txt");
    let mut cases = 0;
    for (line_number, line) in corpus.lines().enumerate() {
        let payload = line.split('#').next().unwrap().trim();
        if payload.is_empty() {
            continue;
        }
        let mut text = String::new();
        let mut expected = Vec::new();
        for token in payload.split_whitespace() {
            match token {
                "÷" => expected.push((text.len(), text.encode_utf16().count() as u32)),
                "×" => {}
                _ => text.push(char::from_u32(u32::from_str_radix(token, 16).unwrap()).unwrap()),
            }
        }
        let map = GraphemeBoundaryMap::new(&text).unwrap();
        let actual: Vec<_> = map
            .boundaries()
            .iter()
            .map(|b| (b.byte_offset.get(), b.utf16_offset.get()))
            .collect();
        assert_eq!(actual, expected, "Unicode corpus line {}", line_number + 1);
        for offset in 0..=text.encode_utf16().count() as u32 {
            let result = map.resolve(Utf16Offset::new(offset));
            if let Some(&(byte, _)) = expected.iter().find(|&&(_, units)| units == offset) {
                assert_eq!(result.unwrap().get(), byte);
            } else {
                let expected_error = match resolve_utf16_offset(&text, offset.into()) {
                    Err(error) => EditorPositionError::Scalar(error),
                    Ok(_) => EditorPositionError::InvalidGraphemeBoundary,
                };
                assert_eq!(result, Err(expected_error));
            }
        }
        cases += 1;
    }
    assert_eq!(cases, 766, "no skipped or missing official cases");
}

#[test]
fn edit_01_empty_has_only_zero_with_both_text_affinities() {
    let node = paragraph("");
    let map = NodePositionMap::new(&node, 7).unwrap();
    assert_eq!(map.graphemes().unwrap().boundaries().len(), 1);
    for affinity in [Affinity::Forward, Affinity::Backward] {
        assert_eq!(
            map.validate(&position(&node, 0, affinity), 7).unwrap(),
            ResolvedPosition::Text(resolve_utf16_offset("", 0.into()).unwrap())
        );
    }
    assert_eq!(
        map.validate(&position(&node, 1, Affinity::Forward), 7)
            .unwrap_err()
            .code(),
        "FLOW_INVALID_RANGE"
    );
}

#[test]
fn edit_01_encoding_rejects_surrogate_combining_zwj_and_regional_interiors() {
    for (text, offset, code) in [
        ("😀", 1, "FLOW_INVALID_UTF16_BOUNDARY"),
        ("и\u{0306}", 1, "FLOW_INVALID_GRAPHEME_BOUNDARY"),
        ("👩\u{200d}💻", 2, "FLOW_INVALID_GRAPHEME_BOUNDARY"),
        ("🇺🇦", 2, "FLOW_INVALID_GRAPHEME_BOUNDARY"),
        ("𝄞", 3, "FLOW_INVALID_RANGE"),
    ] {
        let map = GraphemeBoundaryMap::new(text).unwrap();
        assert_eq!(map.resolve(offset.into()).unwrap_err().code(), code);
    }
    let decomposed = "Привіт, и\u{0306}!";
    let original = decomposed.as_bytes().to_vec();
    let map = GraphemeBoundaryMap::new(decomposed).unwrap();
    assert_eq!(map.text().as_bytes(), original);
    assert_ne!(map.text(), "Привіт, й!");
}

#[test]
fn edit_01_boundary_atomic_edges_are_exhaustive_by_node_kind() {
    let sample = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    for body in [
        sample.content[2].body.clone(),
        BlockKind::Table {
            header_rows: 0,
            rows: vec![],
        },
        BlockKind::PageBreak,
    ] {
        let mut node = paragraph("");
        node.body = body;
        let map = NodePositionMap::new(&node, 3).unwrap();
        assert!(map.graphemes().is_none());
        for offset in [0, 1, 2, u32::MAX] {
            for affinity in [Affinity::Forward, Affinity::Backward] {
                let expected = match (offset, &affinity) {
                    (0, Affinity::Forward) => Ok(ResolvedPosition::BeforeAtom),
                    (1, Affinity::Backward) => Ok(ResolvedPosition::AfterAtom),
                    _ => Err(EditorPositionError::InvalidAtomicPosition),
                };
                assert_eq!(
                    map.validate(&position(&node, offset, affinity), 3),
                    expected
                );
            }
        }
    }
    for body in [
        BlockKind::OrderedList { items: vec![] },
        BlockKind::UnorderedList { items: vec![] },
        BlockKind::ListItem { children: vec![] },
        BlockKind::TableRow { cells: vec![] },
        BlockKind::TableCell { children: vec![] },
    ] {
        let mut node = paragraph("");
        node.body = body;
        assert_eq!(
            NodePositionMap::new(&node, 0).unwrap_err(),
            EditorPositionError::UnsupportedNodeKind
        );
    }
}

#[test]
fn edit_01_ordering_preserves_direction_affinity_and_whole_atom_selections() {
    for node in [
        paragraph("Україна😀"),
        FlowDocument::deterministic_sample("uk-UA")
            .expect("sample")
            .content[2]
            .clone(),
    ] {
        let end = if node.runs().is_some() { 9 } else { 1 };
        let first = position(&node, 0, Affinity::Forward);
        let last = position(&node, end, Affinity::Backward);
        let map = NodePositionMap::new(&node, 0).unwrap();
        for selection in [(first.clone(), last.clone()), (last.clone(), first.clone())] {
            let before = selection.clone();
            map.validate(&selection.0, 0).unwrap();
            map.validate(&selection.1, 0).unwrap();
            assert_eq!(selection, before);
        }
    }
}

#[test]
fn immutable_revision_and_node_identity_reject_without_changing_inputs() {
    let document = FlowDocument::deterministic_sample("uk-UA").expect("sample");
    let before = document.clone();
    let node = &document.content[0];
    let map = NodePositionMap::new(node, document.revision).unwrap();
    let valid = position(node, 0, Affinity::Backward);
    let mut missing = valid.clone();
    missing.node_id = document.content[1].id.clone();
    assert_eq!(
        map.validate(&missing, document.revision),
        Err(EditorPositionError::WrongNode)
    );
    assert_eq!(
        map.validate(&valid, document.revision + 1),
        Err(EditorPositionError::StaleRevision)
    );
    assert_eq!(document, before);
    assert_eq!(map.revision(), document.revision);
    assert_eq!(map.node_id(), &node.id);
}

#[test]
fn graphemes_span_inline_marks_and_heading_runs_without_normalizing() {
    let mut node = paragraph("");
    node.body = BlockKind::Heading {
        level: 2,
        attrs: Default::default(),
        runs: vec![
            InlineRun {
                text: "и".into(),
                marks: MarkSet::default(),
            },
            InlineRun {
                text: "\u{0306}😀".into(),
                marks: MarkSet {
                    bold: true,
                    ..Default::default()
                },
            },
        ],
    };
    let map = NodePositionMap::new(&node, 0).unwrap();
    assert_eq!(map.graphemes().unwrap().text(), "и\u{0306}😀");
    assert_eq!(
        map.validate(&position(&node, 1, Affinity::Forward), 0),
        Err(EditorPositionError::InvalidGraphemeBoundary)
    );
    for offset in [0, 2, 4] {
        map.validate(&position(&node, offset, Affinity::Backward), 0)
            .unwrap();
    }
}

#[test]
fn boundary_vectors_enforce_exact_text_and_run_limits() {
    let maximum = DocumentLimits::V1.text_node_bytes;
    let text = "a".repeat(maximum);
    let map = GraphemeBoundaryMap::new(&text).unwrap();
    assert_eq!(map.boundaries().len(), maximum + 1);
    assert_eq!(map.resolve((maximum as u32).into()).unwrap().get(), maximum);
    assert_eq!(
        GraphemeBoundaryMap::new(&(text + "a")).unwrap_err(),
        EditorPositionError::TextLimit
    );
    let mut node = paragraph("");
    if let BlockKind::Paragraph { runs, .. } = &mut node.body {
        *runs = vec![
            InlineRun {
                text: String::new(),
                marks: Default::default()
            };
            4097
        ];
    }
    assert_eq!(
        NodePositionMap::new(&node, 0).unwrap_err(),
        EditorPositionError::TextLimit
    );
}

#[test]
fn public_position_error_codes_are_distinct_and_stable() {
    let errors = [
        (
            EditorPositionError::Scalar(AnchorError::OutOfRange),
            "FLOW_INVALID_RANGE",
        ),
        (
            EditorPositionError::Scalar(AnchorError::InvalidUtf16Boundary),
            "FLOW_INVALID_UTF16_BOUNDARY",
        ),
        (
            EditorPositionError::InvalidGraphemeBoundary,
            "FLOW_INVALID_GRAPHEME_BOUNDARY",
        ),
        (
            EditorPositionError::InvalidAtomicPosition,
            "FLOW_INVALID_ATOMIC_POSITION",
        ),
        (
            EditorPositionError::UnsupportedNodeKind,
            "FLOW_INVALID_POSITION_NODE_KIND",
        ),
        (
            EditorPositionError::WrongNode,
            "FLOW_POSITION_NODE_MISMATCH",
        ),
        (EditorPositionError::StaleRevision, "FLOW_STALE_REVISION"),
        (EditorPositionError::TextLimit, "FLOW_POSITION_TEXT_LIMIT"),
    ];
    let mut codes = std::collections::BTreeSet::new();
    for (error, code) in errors {
        assert_eq!(error.code(), code);
        assert!(codes.insert(code));
    }
}

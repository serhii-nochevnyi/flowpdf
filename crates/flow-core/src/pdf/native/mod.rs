// @contract: Bounded graph-level editing of admitted classic-xref PDFs.
// @security: Errors exclude document data and authored text.
mod candidate;
mod forms;
#[cfg_attr(not(test), allow(dead_code))]
mod graph;
mod notes;
mod objects;
mod pages;
mod redaction;
mod session;
mod text {
    include!("text.rs");

    pub(super) struct FieldFont(Font);

    pub(super) struct RedactionText<'a, 'w>(Recognition<'a, 'w>);

    impl<'a, 'w> RedactionText<'a, 'w> {
        pub(super) fn new(graph: &'a NativePdfGraph, workspace: &'w Workspace) -> Self {
            Self(Recognition::new(graph, workspace))
        }

        pub(super) fn measure(
            &mut self,
            resources: Option<&Dict>,
            block: &[u8],
            work: &mut usize,
        ) -> Result<Option<(String, Bounds, Reservation<'w>)>, super::NativeEditError> {
            use super::NativeEditError::Unsupported;
            let refusal = |error| match error {
                NativeSessionError::Budget => NativeEditError::Budget,
                _ => Unsupported,
            };
            let mut memory = self.0.workspace.scope();
            let mut shows = shows(block, self.0.workspace, &mut memory).map_err(refusal)?;
            let Some(show) = shows.pop() else {
                return Ok(None);
            };
            if !shows.is_empty() || show.codes.len() > 8192 || show.size > 10_000 * SCALE {
                return Err(Unsupported);
            }
            super::graph::spend(work, show.codes.len() * 4 + 1)?;
            self.0.work = *work;
            let font = self
                .0
                .font(resources.ok_or(Unsupported)?, &show.font)
                .map_err(refusal)?;
            let codes = font.decode(&show.codes).map_err(|_| Unsupported)?;
            let metrics = font
                .measure(&show, codes, |_| {})
                .map_err(|_| Unsupported)?;
            let bounds = metrics.wire_bounds().map_err(|_| Unsupported)?;
            *work = self.0.work;
            Ok(Some((show.font, bounds, memory)))
        }
    }

    pub(super) fn redaction_number(raw: &[u8]) -> Option<i64> {
        milli(raw)
    }

    impl FieldFont {
        pub(super) fn admit(graph: &NativePdfGraph, dict: &Dict) -> Option<Self> {
            let workspace = Workspace::new(graph.remaining_retained());
            let mut memory = workspace.scope();
            font_profile(graph, dict, &workspace, &mut memory)
                .ok()
                .map(Self)
        }

        pub(super) fn operand(
            &self,
            value: &str,
            width: i64,
            height: i64,
        ) -> Result<String, super::NativeFieldRefusal> {
            use super::NativeFieldRefusal::{Appearance, Geometry, Glyph};
            use std::fmt::Write as _;
            let font = &self.0;
            let codes: Vec<_> = value
                .chars()
                .map(|scalar| font.reverse.get(&scalar).copied().ok_or(Glyph))
                .collect::<Result<_, _>>()?;
            if !codes.is_empty() {
                let show = Show {
                    operand: 0..0,
                    font: String::new(),
                    size: 10 * SCALE,
                    origin: [2 * SCALE, 4 * SCALE],
                    codes: Vec::new(),
                    array: false,
                };
                if value.chars().any(|scalar| scalar != ' ') {
                    let metrics = font
                        .measure(&show, codes.iter().copied(), |_| {})
                        .map_err(|_| Geometry)?;
                    if !contains(
                        [0, 0, width * SCALE, height * SCALE],
                        metrics.wire_bounds().map_err(|_| Geometry)?,
                    ) {
                        return Err(Geometry);
                    }
                } else if codes.iter().map(|code| font.codes[code].width).sum::<i64>() * 10
                    > (width - 2) * SCALE * SCALE
                {
                    return Err(Geometry);
                }
            }
            let mut encoded = String::new();
            for code in codes {
                if font.code_width == 1 {
                    write!(encoded, "{code:02X}").map_err(|_| Appearance)?;
                } else {
                    write!(encoded, "{code:04X}").map_err(|_| Appearance)?;
                }
            }
            Ok(encoded)
        }
    }
}
mod verifier;
mod verify_raster;
mod verify_structure;
mod verify_text;
mod writer;

pub use candidate::{
    NativeRedactionCandidateIdentity, NativeRedactionCandidateStatus, NativeRedactionStage,
};
pub use forms::{
    NATIVE_MAX_FIELD_NAME_BYTES, NATIVE_MAX_FIELD_TEXT_BYTES, NativeFieldCommand, NativeFieldKind,
    NativeFieldRefusal, NativeFieldView,
};
pub use graph::{NativeEditError, NativeEditLimits};
pub use notes::{NativeNoteCommand, NativeNoteId, NativePageGeometry, NativePageId};
pub use objects::{
    NATIVE_MAX_GRAPHIC_POINTS, NATIVE_MAX_IMAGE_BYTES, NATIVE_MAX_IMAGE_DIMENSION,
    NATIVE_MAX_IMAGE_PIXELS, NATIVE_MAX_STROKE_WIDTH, NativeGraphic, NativeGraphicShape,
    NativeObjectCommand, NativeObjectKind, NativeObjectOperation, NativeObjectPage,
    NativeObjectProfile, NativeObjectRefusal, NativeObjectTarget, NativeObjectView,
};
pub use pages::{NativePageCommand, NativePageSide, NativePageState, NativePageTarget};
pub use session::{
    NATIVE_MAX_NOTE_BYTES, NATIVE_REQUEST_HISTORY, NativeBinding, NativeEditReceipt,
    NativePdfSession, NativeRequestLookup, NativeSessionError, NativeSnapshot,
};

pub use text::{
    NATIVE_MAX_TEXT_BYTES, NativeTextCommand, NativeTextIsland, NativeTextOperator, NativeTextPage,
    NativeTextProfile, NativeTextRefusal,
};

pub use verifier::{
    NATIVE_REDACTION_VALIDATOR_PROFILE, NATIVE_REDACTION_VALIDATOR_VERSION, NativeRedactionContext,
    NativeRedactionEvidenceIdentity, NativeRedactionGeometry, NativeRedactionMark,
    NativeRedactionPageEvidence, NativeRedactionPolicy, NativeVerificationFailure,
    NativeVerificationLane, NativeVerificationStatus, VersionedNativeRedactionEvidence,
    verify_native_redaction,
};
pub use verify_raster::{
    NATIVE_PREVIEW_MAX_DIMENSION, NATIVE_PREVIEW_MAX_PIXELS, NativePageRaster,
};

#[cfg(test)]
use graph::{NativePdfGraph, PlainTextNote};

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn redaction_fixture(paint: &str) -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{paint}\nendstream", paint.len()),
        ];
        let mut pdf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.into_iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        pdf
    }

    pub(super) fn fixture(extra: &str, stream: &[u8]) -> Vec<u8> {
        let mut bytes = b"%PDF-1.7\n".to_vec();
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R /Custom 5 7 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_vec(),
            format!("<< /Length 6 0 R /Filter /FlateDecode >>\nstream\n").into_bytes(),
            format!(
                "<< /T true /F false /Decimal +0.1234567890123456789012345 /A#23B /N#FF {extra} >>"
            )
            .into_bytes(),
            stream.len().to_string().into_bytes(),
            b"(unreachable decoy)".to_vec(),
        ];
        objects[3].extend_from_slice(stream);
        objects[3].extend_from_slice(b"\nendstream");
        let mut offsets = vec![0];
        for (i, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(
                format!("{} {} obj\n", i + 1, if i == 4 { 7 } else { 0 }).as_bytes(),
            );
            bytes.extend_from_slice(object);
            bytes.extend_from_slice(b"\nendobj\n");
        }
        let xref = bytes.len();
        bytes.extend_from_slice(b"xref\n0 8\n0000000000 65535 f \n");
        for (i, offset) in offsets.iter().enumerate().skip(1) {
            bytes.extend_from_slice(
                format!("{offset:010} {:05} n \n", if i == 5 { 7 } else { 0 }).as_bytes(),
            );
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size 8 /Root 1 0 R /ID [<0123> <4567>] >>\nstartxref\n{xref}\n%%EOF\n"
            )
            .as_bytes(),
        );
        bytes
    }

    #[test]
    fn imported_note_rewrite_and_repeat_preserve_values_and_source() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(b"q\nQ\n", 6);
        let original = fixture("", &payload);
        let snapshot = original.clone();
        let mut graph = NativePdfGraph::open(&original, NativeEditLimits::default()).unwrap();
        let note = PlainTextNote {
            page_index: 0,
            contents: "Note (one) Україна".into(),
            rect: [10, 20, 30, 40],
        };
        graph.add_note(note.clone()).unwrap();
        let output = graph.rewrite().unwrap();
        assert_eq!(original, snapshot);
        assert!(!output.windows(17).any(|w| w == b"unreachable decoy"));
        assert!(output.windows(payload.len()).any(|w| w == payload));
        assert!(
            output
                .windows(b"+0.1234567890123456789012345".len())
                .any(|w| w == b"+0.1234567890123456789012345")
        );
        let mut reopened = NativePdfGraph::open(&output, NativeEditLimits::default()).unwrap();
        assert_eq!(reopened.notes(0).unwrap(), vec![note.clone()]);
        let second = PlainTextNote {
            contents: "Second".into(),
            rect: [1, 2, 3, 4],
            ..note.clone()
        };
        reopened.add_note(second.clone()).unwrap();
        let twice = reopened.rewrite().unwrap();
        let twice = NativePdfGraph::open(&twice, NativeEditLimits::default()).unwrap();
        assert_eq!(twice.notes(0).unwrap(), vec![note, second]);
        assert_eq!(output, graph.rewrite().unwrap());
    }

    #[test]
    fn hostile_closure_and_budgets_refuse() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        for extra in [
            "/Broken 8 0 R",
            "/Broken 5 0 R",
            "/OpenAction (secret)",
            "/Type /Sig",
            "/Type /ObjStm",
        ] {
            assert!(
                NativePdfGraph::open(&fixture(extra, &payload), NativeEditLimits::default())
                    .is_err()
            );
        }
        let bytes = fixture("", &payload);
        for limits in [
            NativeEditLimits {
                max_source_bytes: 10,
                ..Default::default()
            },
            NativeEditLimits {
                max_objects: 2,
                ..Default::default()
            },
            NativeEditLimits {
                max_retained_bytes: 10,
                ..Default::default()
            },
            NativeEditLimits {
                max_decoded_stream_bytes: 2,
                ..Default::default()
            },
            NativeEditLimits {
                max_total_decoded_bytes: 2,
                ..Default::default()
            },
            NativeEditLimits {
                max_work: 2,
                ..Default::default()
            },
            NativeEditLimits {
                max_nesting: 1,
                ..Default::default()
            },
        ] {
            assert!(NativePdfGraph::open(&bytes, limits).is_err());
        }
        let graph = NativePdfGraph::open(
            &bytes,
            NativeEditLimits {
                max_output_bytes: 20,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(graph.rewrite().is_err());
    }
}

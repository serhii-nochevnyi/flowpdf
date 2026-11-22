use std::collections::{BTreeMap, BTreeSet};

use super::super::reader::{PdfObjectRef, PdfValue};
use super::graph::{NativeEditError, NativePdfGraph, collect_refs, spend};

fn hex_digits(byte: u8) -> [u8; 2] {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0x0f)]]
}

struct Output {
    bytes: Vec<u8>,
    limit: usize,
    peak_limit: usize,
    work: usize,
}

impl Output {
    fn append(&mut self, bytes: &[u8]) -> Result<(), NativeEditError> {
        let size = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or(NativeEditError::Budget)?;
        if size > self.limit {
            return Err(NativeEditError::Budget);
        }
        let capacity = self.bytes.capacity();
        if size > capacity {
            // @security: A moving reallocation can hold both buffer capacities at once.
            let available = self.peak_limit - capacity;
            let target = capacity
                .saturating_mul(2)
                .max(size)
                .min(self.limit.min(available));
            if target < size {
                return Err(NativeEditError::Budget);
            }
            self.bytes
                .try_reserve_exact(target - self.bytes.len())
                .map_err(|_| NativeEditError::Budget)?;
            debug_assert!(self.bytes.capacity() <= available);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn append_payload(&mut self, bytes: &[u8]) -> Result<(), NativeEditError> {
        spend(&mut self.work, bytes.len().div_ceil(16))?;
        self.append(bytes)
    }

    fn name(&mut self, name: &str) -> Result<(), NativeEditError> {
        spend(&mut self.work, name.len().div_ceil(16))?;
        self.append(b"/")?;
        for character in name.chars() {
            let byte =
                u8::try_from(u32::from(character)).map_err(|_| NativeEditError::Unsupported)?;
            if (33..=126).contains(&byte) && !b"()<>[]{}/%#".contains(&byte) {
                self.append(&[byte])?;
            } else {
                let [upper, lower] = hex_digits(byte);
                self.append(&[b'#', upper, lower])?;
            }
        }
        Ok(())
    }

    fn dict(
        &mut self,
        dict: &BTreeMap<String, PdfValue>,
        map: &BTreeMap<PdfObjectRef, PdfObjectRef>,
        depth: usize,
        max_depth: usize,
    ) -> Result<(), NativeEditError> {
        self.append(b"<<")?;
        for (key, value) in dict {
            self.append(b" ")?;
            self.name(key)?;
            self.append(b" ")?;
            self.value(value, map, depth + 1, max_depth)?;
        }
        self.append(b" >>")
    }

    fn value(
        &mut self,
        value: &PdfValue,
        map: &BTreeMap<PdfObjectRef, PdfObjectRef>,
        depth: usize,
        max_depth: usize,
    ) -> Result<(), NativeEditError> {
        spend(&mut self.work, 1)?;
        if depth > max_depth {
            return Err(NativeEditError::Budget);
        }
        match value {
            PdfValue::Null => self.append(b"null"),
            PdfValue::Boolean(true) => self.append(b"true"),
            PdfValue::Boolean(false) => self.append(b"false"),
            PdfValue::Integer(number) => self.append(number.to_string().as_bytes()),
            PdfValue::ExactReal(token) => self.append_payload(token.as_bytes()),
            PdfValue::Real(_) => Err(NativeEditError::Unsupported),
            PdfValue::Name(name) => self.name(name),
            PdfValue::String(bytes) => {
                spend(&mut self.work, bytes.len())?;
                self.append(b"<")?;
                for &byte in bytes {
                    self.append(&hex_digits(byte))?;
                }
                self.append(b">")
            }
            PdfValue::Reference(reference) => {
                let mapped = map.get(reference).ok_or(NativeEditError::Malformed)?;
                self.append(format!("{} {} R", mapped.object_number, mapped.generation).as_bytes())
            }
            PdfValue::Array(values) => {
                self.append(b"[")?;
                for value in values {
                    self.value(value, map, depth + 1, max_depth)?;
                    self.append(b" ")?;
                }
                self.append(b"]")
            }
            PdfValue::Dictionary(dict) => self.dict(dict, map, depth, max_depth),
            PdfValue::Stream(stream) => {
                if depth != 0 {
                    return Err(NativeEditError::Malformed);
                }
                self.dict(&stream.dict, map, depth, max_depth)?;
                self.append(b"\nstream\n")?;
                self.append_payload(&stream.encoded)?;
                self.append(b"\nendstream")
            }
        }
    }
}

pub(super) fn rewrite(graph: &NativePdfGraph) -> Result<Vec<u8>, NativeEditError> {
    Ok(rewrite_with_workspace(graph, graph.remaining_retained())?.bytes)
}

pub(super) struct Rewrite {
    pub bytes: Vec<u8>,
    pub reopened: NativePdfGraph,
    pub mapping: BTreeMap<PdfObjectRef, PdfObjectRef>,
}

pub(super) fn rewrite_with_workspace(
    graph: &NativePdfGraph,
    mut workspace: usize,
) -> Result<Rewrite, NativeEditError> {
    let mut work = graph.remaining_work();
    spend(
        &mut workspace,
        graph
            .objects
            .len()
            .checked_mul(512)
            .ok_or(NativeEditError::Budget)?,
    )?;
    let mut refs = Vec::new();
    for (key, value) in &graph.trailer {
        if key != "Size" {
            collect_refs(value, &mut refs, &mut work, &mut workspace)?;
        }
    }
    let mut reachable = BTreeSet::new();
    while let Some(reference) = refs.pop() {
        spend(&mut work, 1)?;
        if reachable.insert(reference) {
            let value = graph
                .objects
                .get(&reference)
                .ok_or(NativeEditError::Malformed)?;
            collect_refs(value, &mut refs, &mut work, &mut workspace)?;
        }
    }
    let mut map = BTreeMap::new();
    for (index, reference) in reachable.iter().enumerate() {
        map.insert(
            *reference,
            PdfObjectRef {
                object_number: u32::try_from(index + 1).map_err(|_| NativeEditError::Budget)?,
                generation: 0,
            },
        );
    }
    let mut out = Output {
        bytes: Vec::new(),
        limit: graph.limits.max_output_bytes,
        peak_limit: workspace,
        work,
    };
    out.append(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n")?;
    let mut offsets = Vec::new();
    for (reference, mapped) in &map {
        offsets.push(out.bytes.len());
        out.append(format!("{} {} obj\n", mapped.object_number, mapped.generation).as_bytes())?;
        out.value(&graph.objects[reference], &map, 0, graph.limits.max_nesting)?;
        out.append(b"\nendobj\n")?;
    }
    let xref = out.bytes.len();
    out.append(format!("xref\n0 {}\n0000000000 65535 f \n", map.len() + 1).as_bytes())?;
    for offset in offsets {
        out.append(format!("{offset:010} 00000 n \n").as_bytes())?;
    }
    out.append(b"trailer\n<< /Size ")?;
    out.append((map.len() + 1).to_string().as_bytes())?;
    for (key, value) in &graph.trailer {
        if key == "Size" {
            continue;
        }
        out.append(b" ")?;
        out.name(key)?;
        out.append(b" ")?;
        out.value(value, &map, 0, graph.limits.max_nesting)?;
    }
    out.append(format!(" >>\nstartxref\n{xref}\n%%EOF\n").as_bytes())?;
    // @invariant: The exact output must reopen before any bytes are returned.
    let mut reopen_limits = graph.limits;
    // @security: Reopen uses the remaining work and decoded budgets and charges output capacity.
    spend(&mut workspace, out.bytes.capacity())?;
    if workspace == 0 || out.work == 0 || graph.remaining_decoded() == 0 {
        return Err(NativeEditError::Budget);
    }
    reopen_limits.max_retained_bytes = workspace;
    reopen_limits.max_total_decoded_bytes = graph.remaining_decoded();
    reopen_limits.max_work = out.work;
    let reopened = NativePdfGraph::open(&out.bytes, reopen_limits)?;
    Ok(Rewrite {
        bytes: out.bytes,
        reopened,
        mapping: map,
    })
}

#[cfg(test)]
mod tests {
    use crate::pdf::reader::{PdfReadError, PdfReaderLimits, open_pdf};

    use super::super::{NativeEditLimits, PlainTextNote};
    use super::*;

    #[test]
    fn payload_work_is_required_before_encoding_or_copying() {
        let map = BTreeMap::new();
        for (value, work, expected) in [
            (
                PdfValue::Name("NNNNNNNNNNNNNN é".into()),
                3,
                "/NNNNNNNNNNNNNN#20#E9",
            ),
            (PdfValue::String(vec![0, 255]), 3, "<00FF>"),
            (
                PdfValue::ExactReal("+.123456789012345".into()),
                3,
                "+.123456789012345",
            ),
        ] {
            let mut output = Output {
                bytes: Vec::new(),
                limit: 64,
                peak_limit: 128,
                work: work - 1,
            };
            assert_eq!(
                output.value(&value, &map, 0, 0),
                Err(NativeEditError::Budget)
            );
            assert_eq!(output.bytes.capacity(), 0);

            output.work = work;
            output.value(&value, &map, 0, 0).unwrap();
            assert_eq!(output.work, 0);
            assert_eq!(output.bytes, expected.as_bytes());
        }
    }

    #[test]
    fn output_buffer_grows_geometrically_up_to_limit() {
        let limit = 257;
        let mut output = Output {
            bytes: Vec::new(),
            limit,
            peak_limit: 2 * limit - 1,
            work: 0,
        };
        let mut growths = 0;
        for _ in 0..limit - 1 {
            let capacity = output.bytes.capacity();
            output.append(b"x").unwrap();
            growths += usize::from(output.bytes.capacity() != capacity);
            assert!(output.bytes.capacity() <= limit);
        }
        assert!(growths <= 10);
        let capacity = output.bytes.capacity();
        assert_eq!(output.append(b"yz"), Err(NativeEditError::Budget));
        assert_eq!(output.bytes.capacity(), capacity);
        assert_eq!(output.bytes, vec![b'x'; limit - 1]);
        output.append(b"x").unwrap();
        assert!(output.bytes.capacity() <= limit);
        assert_eq!(output.bytes, vec![b'x'; limit]);
    }

    #[test]
    fn output_growth_counts_both_allocations_against_workspace() {
        let mut output = Output {
            bytes: Vec::new(),
            limit: 24,
            peak_limit: 35,
            work: 0,
        };
        output.append(b"12345678").unwrap();
        output.append(b"abcdefgh").unwrap();
        let old_capacity = output.bytes.capacity();
        assert_eq!(old_capacity, 16);

        assert_eq!(output.append(b"ijkl"), Err(NativeEditError::Budget));
        assert_eq!(output.bytes.capacity(), old_capacity);
        assert_eq!(output.bytes.as_slice(), b"12345678abcdefgh");

        output.append(b"ijk").unwrap();
        assert_eq!(old_capacity + output.bytes.capacity(), output.peak_limit);
        assert_eq!(output.bytes.as_slice(), b"12345678abcdefghijk");
        assert_eq!(output.append(b"l"), Err(NativeEditError::Budget));
    }

    fn independent(streams: &[(&[u8], &str)], trailer: &str) -> Vec<u8> {
        let mut bytes = b"%PDF-1.4\r\n".to_vec();
        let mut objects = vec![
            (11, 2, b"<< /Type /Catalog /Pages 12 0 R >>".to_vec()),
            (
                12,
                0,
                b"<< /Type /Pages /Kids [13 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
            ),
            (
                13,
                0,
                format!(
                    "<< /Type /Page /Parent 12 0 R /Contents [{}] >>",
                    streams
                        .iter()
                        .enumerate()
                        .map(|(i, _)| format!("{} 0 R", 20 + i * 2))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
                .into_bytes(),
            ),
            (99, 0, b"<< /Broken 999 0 R /Type /Sig >>".to_vec()),
        ];
        for (i, (data, dict)) in streams.iter().enumerate() {
            let n = 20 + i * 2;
            let mut stream =
                format!("<< /Length {} 9 R {dict} >>\r\nstream\r\n", n + 1).into_bytes();
            stream.extend_from_slice(data);
            stream.extend_from_slice(b"\r\nendstream");
            objects.push((n, 0, stream));
            objects.push((n + 1, 9, data.len().to_string().into_bytes()));
        }
        objects.sort_by_key(|o| o.0);
        let mut offsets = Vec::new();
        for (n, generation, value) in objects {
            offsets.push((n, generation, bytes.len()));
            bytes.extend_from_slice(format!("{n} {generation} obj\r\n").as_bytes());
            bytes.extend_from_slice(&value);
            bytes.extend_from_slice(b"\r\nendobj\r\n");
        }
        let xref = bytes.len();
        bytes.extend_from_slice(b"xref\r\n0 1\r\n0000000000 65535 f \r\n");
        for (n, generation, offset) in offsets {
            bytes.extend_from_slice(
                format!("{n} 1\r\n{offset:010} {generation:05} n \r\n").as_bytes(),
            );
        }
        bytes.extend_from_slice(format!("trailer\r\n<< /Size 100 /Root 11 2 R {trailer} >>\r\nstartxref\r\n{xref}\r\n%%EOF\r\n").as_bytes());
        bytes
    }

    #[test]
    fn independent_encoding_preserves_generic_trailer_and_indirect_lengths() {
        let source = independent(
            &[(b"71 20 51>", "/Filter /ASCIIHexDecode"), (b"q Q", "")],
            "/Custom [true false +.000000000000000000000001 /N#20#FF <00ff>]",
        );
        let mut graph = NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
        graph
            .add_note(PlainTextNote {
                page_index: 0,
                contents: "test".into(),
                rect: [1, 2, 3, 4],
            })
            .unwrap();
        let output = graph.rewrite().unwrap();
        let canonical_values = b"/N#20#FF <00FF>";
        assert!(
            output
                .windows(canonical_values.len())
                .any(|window| window == canonical_values)
        );
        assert_eq!(output, graph.rewrite().unwrap());
        let reopened = NativePdfGraph::open(&output, NativeEditLimits::default()).unwrap();
        assert_eq!(reopened.original_bytes(), output);
        let values = reopened.trailer["Custom"].as_array().unwrap();
        assert!(matches!(values[0], PdfValue::Boolean(true)));
        assert!(matches!(values[1], PdfValue::Boolean(false)));
        assert!(matches!(&values[2], PdfValue::ExactReal(t) if t == "+.000000000000000000000001"));
        assert_eq!(values[3].as_name(), Some("N ÿ"));
        assert_eq!(values[4].as_bytes(), Some(&[0, 255][..]));
        assert_eq!(reopened.objects.len(), 8);
        for value in reopened.objects.values() {
            if let PdfValue::Stream(stream) = value {
                let length = stream.dict["Length"].as_reference().unwrap();
                assert_eq!(
                    reopened.objects[&length].as_integer(),
                    Some(stream.encoded.len() as i64)
                );
            }
        }
        assert_eq!(graph.original_bytes(), source);

        graph.limits.max_output_bytes = output.len();
        assert_eq!(graph.rewrite().unwrap(), output);
        graph.limits.max_output_bytes -= 1;
        assert_eq!(graph.rewrite(), Err(NativeEditError::Budget));
    }

    #[test]
    fn name_keys_and_values_survive_canonical_escaping_in_both_readers() {
        let raw_name = "é".repeat(64);
        let retained_name = "\u{c3}\u{a9}".repeat(64);
        let source = independent(&[], &format!("/Custom << /{raw_name} /{raw_name} >>"));
        assert!(open_pdf(&source, PdfReaderLimits::default()).is_ok());
        let graph = NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
        let output = graph.rewrite().unwrap();
        let scene_reader = open_pdf(&output, PdfReaderLimits::default()).unwrap();
        let scene_dictionary = scene_reader.retained_trailer()["Custom"]
            .as_dictionary()
            .unwrap();
        assert_eq!(
            scene_dictionary[&raw_name].as_name(),
            Some(raw_name.as_str())
        );
        let reopened = NativePdfGraph::open(&output, NativeEditLimits::default()).unwrap();
        for graph in [&graph, &reopened] {
            let dictionary = graph.trailer["Custom"].as_dictionary().unwrap();
            assert_eq!(
                dictionary[&retained_name].as_name(),
                Some(retained_name.as_str())
            );
        }

        let over_limit = independent(&[], &format!("/Custom /{}", "A".repeat(129)));
        assert!(matches!(
            open_pdf(&over_limit, PdfReaderLimits::default()),
            Err(PdfReadError::Syntax)
        ));
        assert!(matches!(
            NativePdfGraph::open(&over_limit, NativeEditLimits::default()),
            Err(NativeEditError::Reader(PdfReadError::Syntax))
        ));
    }

    #[test]
    fn unsafe_trailers_and_streams_produce_no_output() {
        for trailer in [
            "/Prev 1",
            "/XRefStm 1",
            "/Encrypt 20 0 R",
            "/Custom 999 0 R",
            "/Custom << /Type /Action /S /Named /N /NextPage >>",
            "/Custom << /S /ResetForm >>",
            "/Custom << /S /Hide >>",
            "/Custom << /S /SetOCGState >>",
            "/Custom << /S /Trans >>",
            "/Custom << /S /GoTo3DView >>",
        ] {
            assert!(
                NativePdfGraph::open(
                    &independent(&[(b"q Q", "")], trailer),
                    NativeEditLimits::default()
                )
                .is_err()
            );
        }
        for dict in [
            "/F (file)",
            "/DecodeParms << /Predictor 12 >>",
            "/Filter /Unknown",
            "/Filter /DCTDecode",
            "/Filter [/FlateDecode /FlateDecode]",
            "/Type /ObjStm",
            "/Type /XRef",
        ] {
            assert!(
                NativePdfGraph::open(
                    &independent(&[(b"q Q", dict)], ""),
                    NativeEditLimits::default()
                )
                .is_err()
            );
        }
        let mut bytes = independent(&[(b"q Q", "")], "");
        bytes.extend_from_slice(b"garbage");
        assert!(NativePdfGraph::open(&bytes, NativeEditLimits::default()).is_err());
    }

    #[test]
    fn bounded_flate_and_cumulative_decoded_budget() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(&[b' '; 1024], 6);
        let source = independent(
            &[
                (&payload, "/Filter /FlateDecode"),
                (&payload, "/Filter /FlateDecode"),
            ],
            "",
        );
        for max in [1, 1023] {
            let result = NativePdfGraph::open(
                &source,
                NativeEditLimits {
                    max_decoded_stream_bytes: max,
                    ..Default::default()
                },
            );
            assert!(matches!(
                result,
                Err(NativeEditError::Reader(
                    super::super::super::reader::PdfReadError::DecodedStreamLimit
                ))
            ));
        }
        assert!(
            NativePdfGraph::open(
                &source,
                NativeEditLimits {
                    max_total_decoded_bytes: 2047,
                    ..Default::default()
                }
            )
            .is_err()
        );
        let graph = NativePdfGraph::open(
            &source,
            NativeEditLimits {
                max_decoded_stream_bytes: 1024,
                max_total_decoded_bytes: 2048,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(graph.rewrite(), Err(NativeEditError::Budget));
        let graph = NativePdfGraph::open(
            &source,
            NativeEditLimits {
                max_decoded_stream_bytes: 1024,
                max_total_decoded_bytes: 4096,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(graph.rewrite().is_ok());
        let corrupt = independent(&[(b"not zlib", "/Filter /FlateDecode")], "");
        assert!(NativePdfGraph::open(&corrupt, NativeEditLimits::default()).is_err());
    }

    #[test]
    fn invalid_note_is_atomic_and_limits_cannot_be_raised() {
        let bytes = independent(&[(b"q Q", "")], "");
        let mut graph = NativePdfGraph::open(&bytes, NativeEditLimits::default()).unwrap();
        let before = graph.rewrite().unwrap();
        for (page_index, rect, contents) in [
            (1, [1, 2, 3, 4], "text".into()),
            (0, [4, 3, 2, 1], "text".into()),
            (0, [1, 2, 3, 4], "x".repeat(16385)),
        ] {
            assert!(
                graph
                    .add_note(PlainTextNote {
                        page_index,
                        rect,
                        contents
                    })
                    .is_err()
            );
            assert_eq!(before, graph.rewrite().unwrap());
        }
        assert!(matches!(
            NativePdfGraph::open(
                &bytes,
                NativeEditLimits {
                    max_objects: 4097,
                    ..Default::default()
                }
            ),
            Err(NativeEditError::InvalidLimits)
        ));
        assert!(matches!(
            NativePdfGraph::open(
                &bytes,
                NativeEditLimits {
                    max_work: 0,
                    ..Default::default()
                }
            ),
            Err(NativeEditError::InvalidLimits)
        ));
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::super::{NativeEditLimits, PlainTextNote};
    use super::*;

    #[test]
    fn flate_hard_boundary_is_enforced_before_growth() {
        let at_limit = vec![b' '; 8 * 1024 * 1024];
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&at_limit, 6);
        let bytes = super::super::tests::fixture("", &compressed);
        assert!(NativePdfGraph::open(&bytes, NativeEditLimits::default()).is_ok());
        let over = vec![b' '; at_limit.len() + 1];
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&over, 6);
        let bytes = super::super::tests::fixture("", &compressed);
        assert!(matches!(
            NativePdfGraph::open(&bytes, NativeEditLimits::default()),
            Err(NativeEditError::Reader(
                super::super::super::reader::PdfReadError::DecodedStreamLimit
            ))
        ));
    }

    #[test]
    fn second_note_can_exhaust_object_budget_without_mutation() {
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &compressed);
        let mut graph = NativePdfGraph::open(
            &bytes,
            NativeEditLimits {
                max_objects: 7,
                ..Default::default()
            },
        )
        .unwrap();
        let note = PlainTextNote {
            page_index: 0,
            contents: "one".into(),
            rect: [1, 2, 3, 4],
        };
        graph.add_note(note.clone()).unwrap();
        let before = graph.rewrite().unwrap();
        assert_eq!(graph.add_note(note), Err(NativeEditError::Budget));
        assert_eq!(before, graph.rewrite().unwrap());
    }
}

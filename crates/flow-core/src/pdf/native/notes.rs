use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::super::reader::{PdfObjectRef, PdfValue};
use super::super::scene::{
    NativePdfAnnotation, NativePdfNote, NativePdfNotePage, NativeReadOnlyAnnotation,
};
use super::graph::{NativePdfGraph, collect_refs, spend};
use super::session::{NATIVE_MAX_NOTE_BYTES, NativeSessionError};

const MAX_POINT: i64 = 1_000_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct NativePageId(pub(super) u32);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct NativeNoteId(pub(super) u64);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NativeNoteCommand {
    Add {
        page: NativePageId,
        rect: [i64; 4],
        contents: String,
    },
    Edit {
        note: NativeNoteId,
        contents: String,
    },
    Move {
        note: NativeNoteId,
        page: NativePageId,
        rect: [i64; 4],
    },
    Remove {
        note: NativeNoteId,
    },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativePageGeometry {
    bounds: [i64; 4],
    rotation: i64,
}

impl NativePageGeometry {
    pub fn bounds(self) -> [i64; 4] {
        self.bounds
    }

    pub fn rotation(self) -> i64 {
        self.rotation
    }

    fn contains(self, [l, b, r, t]: [i64; 4]) -> bool {
        let [left, bottom, right, top] = self.bounds;
        left <= l && l < r && r <= right && bottom <= b && b < t && t <= top
    }

    pub fn to_rotated_crop(self, rect: [i64; 4]) -> Option<[i64; 4]> {
        if !self.contains(rect) {
            return None;
        }
        let [left, bottom, right, top] = self.bounds;
        let [l, b, r, t] = rect;
        Some(match self.rotation {
            90 => [b - bottom, right - r, t - bottom, right - l],
            180 => [right - r, top - t, right - l, top - b],
            270 => [top - t, l - left, top - b, r - left],
            _ => [l - left, b - bottom, r - left, t - bottom],
        })
    }

    pub fn from_rotated_crop(self, [l, b, r, t]: [i64; 4]) -> Option<[i64; 4]> {
        let [left, bottom, right, top] = self.bounds;
        let (width, height) = if matches!(self.rotation, 90 | 270) {
            (top - bottom, right - left)
        } else {
            (right - left, top - bottom)
        };
        if l < 0 || b < 0 || l >= r || b >= t || r > width || t > height {
            return None;
        }
        Some(match self.rotation {
            90 => [right - t, bottom + l, right - b, bottom + r],
            180 => [right - r, top - t, right - l, top - b],
            270 => [left + b, top - r, left + t, top - l],
            _ => [left + l, bottom + b, left + r, bottom + t],
        })
    }
}

fn resolve<'a>(graph: &'a NativePdfGraph, mut value: &'a PdfValue) -> Option<&'a PdfValue> {
    for _ in 0..=graph.limits.max_nesting {
        match value {
            PdfValue::Reference(reference) => value = graph.objects.get(reference)?,
            _ => return Some(value),
        }
    }
    None
}

fn geometry(graph: &NativePdfGraph, mut page: PdfObjectRef) -> Option<NativePageGeometry> {
    let mut media = None;
    let mut crop = None;
    let mut rotation = None;
    for _ in 0..=graph.limits.max_nesting {
        let dict = graph.objects.get(&page)?.as_dictionary()?;
        if let Some(unit) = dict.get("UserUnit")
            && !matches!(resolve(graph, unit)?, PdfValue::Integer(1))
        {
            return None;
        }
        if media.is_none()
            && let Some(value) = dict.get("MediaBox")
        {
            media = Some(box_bounds(resolve(graph, value)?)?);
        }
        if crop.is_none()
            && let Some(value) = dict.get("CropBox")
        {
            crop = Some(box_bounds(resolve(graph, value)?)?);
        }
        if rotation.is_none()
            && let Some(value) = dict.get("Rotate")
        {
            let angle = resolve(graph, value)?.as_integer()?.rem_euclid(360);
            if angle % 90 != 0 {
                return None;
            }
            rotation = Some(angle);
        }
        match dict.get("Parent") {
            Some(value) => page = value.as_reference()?,
            None => break,
        }
    }
    let [ml, mb, mr, mt] = media?;
    let [cl, cb, cr, ct] = crop.unwrap_or([ml, mb, mr, mt]);
    let bounds = [ml.max(cl), mb.max(cb), mr.min(cr), mt.min(ct)];
    (bounds[0] < bounds[2] && bounds[1] < bounds[3]).then_some(NativePageGeometry {
        bounds,
        rotation: rotation.unwrap_or(0),
    })
}

fn box_bounds(value: &PdfValue) -> Option<[i64; 4]> {
    let [l, b, r, t] = value.as_array()? else {
        return None;
    };
    Some([
        inward(l, true)?,
        inward(b, true)?,
        inward(r, false)?,
        inward(t, false)?,
    ])
}

fn inward(value: &PdfValue, lower: bool) -> Option<i64> {
    let point = match value {
        PdfValue::Integer(value) => *value,
        PdfValue::ExactReal(token) => {
            let negative = token.starts_with('-');
            let (whole, fraction) = token.trim_start_matches(['-', '+']).split_once('.')?;
            let magnitude: i64 = if whole.is_empty() {
                0
            } else {
                whole.parse().ok()?
            };
            let signed = if negative { -magnitude } else { magnitude };
            if fraction.bytes().any(|b| b != b'0') {
                match (lower, negative) {
                    (true, false) => signed.checked_add(1)?,
                    (false, true) => signed.checked_sub(1)?,
                    _ => signed,
                }
            } else {
                signed
            }
        }
        _ => return None,
    };
    (point.unsigned_abs() <= MAX_POINT as u64).then_some(point)
}

fn page_annots(
    graph: &NativePdfGraph,
    page: PdfObjectRef,
) -> Result<&[PdfValue], NativeSessionError> {
    let dict = graph
        .objects
        .get(&page)
        .and_then(PdfValue::as_dictionary)
        .ok_or(NativeSessionError::Postcondition)?;
    match dict.get("Annots") {
        Some(value) => resolve(graph, value)
            .and_then(PdfValue::as_array)
            .ok_or(NativeSessionError::Postcondition),
        None => Ok(&[]),
    }
}

fn exclusive(
    graph: &NativePdfGraph,
    trailer_refs: &BTreeSet<PdfObjectRef>,
    target: PdfObjectRef,
    owner: PdfObjectRef,
) -> bool {
    !trailer_refs.contains(&target)
        && graph
            .reverse
            .get(&target)
            .is_some_and(|owners| owners.len() == 1 && owners.contains(&owner))
}

struct RefMultiplicity {
    refs: Vec<PdfObjectRef>,
}

impl RefMultiplicity {
    fn read(
        graph: &NativePdfGraph,
        object: PdfObjectRef,
        work: &mut usize,
        mut scratch: usize,
    ) -> Result<Self, NativeSessionError> {
        let mut refs = Vec::new();
        collect_refs(&graph.objects[&object], &mut refs, work, &mut scratch)?;
        refs.sort_unstable();
        Ok(Self { refs })
    }

    fn unique(&self, target: PdfObjectRef) -> bool {
        let first = self.refs.partition_point(|value| *value < target);
        self.refs.get(first) == Some(&target) && self.refs.get(first + 1) != Some(&target)
    }
}

fn plain_note(
    dict: &BTreeMap<String, PdfValue>,
    page: PdfObjectRef,
    geometry: NativePageGeometry,
) -> Option<([i64; 4], String)> {
    if dict
        .keys()
        .any(|key| !matches!(key.as_str(), "Type" | "Subtype" | "Contents" | "Rect" | "P"))
        || dict
            .get("Type")
            .is_some_and(|v| v.as_name() != Some("Annot"))
        || dict
            .get("P")
            .is_some_and(|v| v.as_reference() != Some(page))
    {
        return None;
    }
    let [l, b, r, t] = dict.get("Rect")?.as_array()? else {
        return None;
    };
    let rect = [
        l.as_integer()?,
        b.as_integer()?,
        r.as_integer()?,
        t.as_integer()?,
    ];
    if !geometry.contains(rect) {
        return None;
    }
    let bytes = dict.get("Contents")?.as_bytes()?;
    let contents = decode_contents(bytes)?;
    Some((rect, contents))
}

pub(super) fn project(
    graph: &NativePdfGraph,
    ids: &BTreeMap<PdfObjectRef, NativeNoteId>,
    remaining: &mut usize,
) -> Result<Vec<NativePdfNotePage>, NativeSessionError> {
    let mut work = graph.remaining_work();
    let mut scratch = *remaining;
    let mut refs = Vec::new();
    for value in graph.trailer.values() {
        collect_refs(value, &mut refs, &mut work, &mut scratch)?;
    }
    let trailer_refs: BTreeSet<_> = refs.into_iter().collect();
    let mut pages = Vec::new();
    for (index, &page) in graph.pages.iter().enumerate() {
        spend(&mut work, (graph.limits.max_nesting + 1) * 4)?;
        let geometry = geometry(graph, page);
        let mut owner_refs = RefMultiplicity::read(graph, page, &mut work, *remaining)?;
        let dict = graph.objects[&page]
            .as_dictionary()
            .ok_or(NativeSessionError::Postcondition)?;
        let mut owner = page;
        let mut can_add_note = geometry.is_some();
        if let Some(reference) = dict.get("Annots").and_then(PdfValue::as_reference) {
            if owner_refs.unique(reference)
                && exclusive(graph, &trailer_refs, reference, page)
                && graph
                    .objects
                    .get(&reference)
                    .and_then(PdfValue::as_array)
                    .is_some()
            {
                owner = reference;
                owner_refs = RefMultiplicity::read(graph, owner, &mut work, *remaining)?;
            } else {
                can_add_note = false;
            }
        }
        spend(remaining, 256)?;
        let mut annotations = Vec::new();
        let values = page_annots(graph, page)?;
        for value in values {
            spend(&mut work, (graph.limits.max_nesting + 1) * 2)?;
            spend(remaining, 256)?;
            let dict = resolve(graph, value)
                .and_then(PdfValue::as_dictionary)
                .ok_or(NativeSessionError::Postcondition)?;
            let object = value.as_reference();
            let subtype = dict.get("Subtype").and_then(PdfValue::as_name);
            if let (Some(object), Some(geometry), Some("Text")) = (object, geometry, subtype)
                && can_add_note
                && owner_refs.unique(object)
                && exclusive(graph, &trailer_refs, object, owner)
                && let Some(id) = ids.get(&object)
                && graph
                    .objects
                    .get(&object)
                    .and_then(PdfValue::as_dictionary)
                    .is_some()
                && let Some((rect, contents)) = plain_note(dict, page, geometry)
            {
                spend(remaining, contents.capacity())?;
                annotations.push(NativePdfAnnotation::PlainNote(NativePdfNote {
                    id: *id,
                    object,
                    rect,
                    contents,
                }));
            } else {
                let profile = if dict.contains_key("AP") {
                    NativeReadOnlyAnnotation::Appearance
                } else {
                    match subtype {
                        Some("Text") => NativeReadOnlyAnnotation::Text,
                        Some("Link") => NativeReadOnlyAnnotation::Link,
                        Some("Widget") => NativeReadOnlyAnnotation::Widget,
                        _ => NativeReadOnlyAnnotation::Other,
                    }
                };
                let rect = dict
                    .get("Rect")
                    .and_then(|value| resolve(graph, value))
                    .and_then(PdfValue::as_array)
                    .and_then(|values| {
                        let [l, b, r, t] = values else { return None };
                        Some([
                            l.as_number()?,
                            b.as_number()?,
                            r.as_number()?,
                            t.as_number()?,
                        ])
                    });
                annotations.push(NativePdfAnnotation::ReadOnly {
                    object,
                    profile,
                    rect,
                });
            }
        }
        pages.push(NativePdfNotePage {
            id: NativePageId(index as u32),
            object: page,
            geometry,
            can_add_note,
            annotations,
        });
    }
    Ok(pages)
}

fn decode_contents(bytes: &[u8]) -> Option<String> {
    if bytes.len() > NATIVE_MAX_NOTE_BYTES * 2 + 2 {
        return None;
    }
    let contents = if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            return None;
        }
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&units).ok()?
    } else if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        std::str::from_utf8(bytes).ok()?.to_owned()
    } else {
        const ACCENTS: [char; 8] = ['˘', 'ˇ', 'ˆ', '˙', '˝', '˛', '˚', '˜'];
        const SPECIAL: [char; 31] = [
            '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’',
            '‚', '™', 'ﬁ', 'ﬂ', 'Ł', 'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž',
        ];
        bytes
            .iter()
            .map(|&byte| match byte {
                0x18..=0x1f => Some(ACCENTS[usize::from(byte - 0x18)]),
                0x80..=0x9e => Some(SPECIAL[usize::from(byte - 0x80)]),
                0xa0 => Some('€'),
                0x7f | 0x9f | 0xad => None,
                _ => Some(char::from(byte)),
            })
            .collect::<Option<String>>()?
    };
    (contents.len() <= NATIVE_MAX_NOTE_BYTES).then_some(contents)
}

fn pdf_contents(contents: &str) -> PdfValue {
    let mut encoded = vec![0xfe, 0xff];
    encoded.extend(contents.encode_utf16().flat_map(u16::to_be_bytes));
    PdfValue::String(encoded)
}

fn pdf_rect(rect: [i64; 4]) -> PdfValue {
    PdfValue::Array(rect.into_iter().map(PdfValue::Integer).collect())
}

fn note_dict(
    graph: &mut NativePdfGraph,
    object: PdfObjectRef,
) -> Result<&mut BTreeMap<String, PdfValue>, NativeSessionError> {
    match graph.objects.get_mut(&object) {
        Some(PdfValue::Dictionary(dict)) => Ok(dict),
        _ => Err(NativeSessionError::Postcondition),
    }
}

fn annots_mut(
    graph: &mut NativePdfGraph,
    page: PdfObjectRef,
) -> Result<&mut Vec<PdfValue>, NativeSessionError> {
    let owner = graph
        .objects
        .get(&page)
        .and_then(PdfValue::as_dictionary)
        .and_then(|dict| dict.get("Annots"))
        .and_then(PdfValue::as_reference);
    let value = match owner {
        Some(owner) => graph
            .objects
            .get_mut(&owner)
            .ok_or(NativeSessionError::Postcondition)?,
        None => note_dict(graph, page)?
            .entry("Annots".into())
            .or_insert_with(|| PdfValue::Array(Vec::new())),
    };
    match value {
        PdfValue::Array(values) => Ok(values),
        _ => Err(NativeSessionError::Postcondition),
    }
}

fn unlink(
    graph: &mut NativePdfGraph,
    page: PdfObjectRef,
    object: PdfObjectRef,
) -> Result<(), NativeSessionError> {
    let annotations = annots_mut(graph, page)?;
    let index = annotations
        .iter()
        .position(|value| value.as_reference() == Some(object))
        .ok_or(NativeSessionError::Postcondition)?;
    annotations.remove(index);
    Ok(())
}

pub(super) struct ExpectedNote<'a> {
    pub id: NativeNoteId,
    pub location: Option<(NativePageId, [i64; 4], &'a str)>,
}

pub(super) fn stage<'a>(
    graph: &mut NativePdfGraph,
    pages: &'a [NativePdfNotePage],
    ids: &mut BTreeMap<PdfObjectRef, NativeNoteId>,
    command: &'a NativeNoteCommand,
    revision: u64,
) -> Result<ExpectedNote<'a>, NativeSessionError> {
    let destination = |id: NativePageId, rect| {
        let page = pages
            .get(id.0 as usize)
            .filter(|page| page.can_add_note)
            .ok_or(NativeSessionError::TargetUnavailable)?;
        let geometry = page.geometry.ok_or(NativeSessionError::TargetUnavailable)?;
        if !geometry.contains(rect) {
            return Err(NativeSessionError::InvalidNote);
        }
        Ok(page.object)
    };
    let target = |id| {
        pages
            .iter()
            .find_map(|page| {
                page.annotations
                    .iter()
                    .find_map(|annotation| match annotation {
                        NativePdfAnnotation::PlainNote(note) if note.id == id => Some((page, note)),
                        _ => None,
                    })
            })
            .ok_or(NativeSessionError::TargetUnavailable)
    };
    let (id, location) = match command {
        NativeNoteCommand::Add {
            page,
            rect,
            contents,
        } => {
            let owner = destination(*page, *rect)?;
            if graph.objects.len() == graph.limits.max_objects {
                return Err(NativeSessionError::Budget);
            }
            let object_number = graph
                .objects
                .keys()
                .next_back()
                .and_then(|reference| reference.object_number.checked_add(1))
                .ok_or(NativeSessionError::Budget)?;
            let object = PdfObjectRef {
                object_number,
                generation: 0,
            };
            let id = NativeNoteId(
                (1_u64 << 48)
                    .checked_add(revision)
                    .ok_or(NativeSessionError::Budget)?,
            );
            let dict = BTreeMap::from([
                ("Type".into(), PdfValue::Name("Annot".into())),
                ("Subtype".into(), PdfValue::Name("Text".into())),
                ("P".into(), PdfValue::Reference(owner)),
                ("Contents".into(), pdf_contents(contents)),
                ("Rect".into(), pdf_rect(*rect)),
            ]);
            graph.objects.insert(object, PdfValue::Dictionary(dict));
            annots_mut(graph, owner)?.push(PdfValue::Reference(object));
            ids.insert(object, id);
            (id, Some((*page, *rect, contents.as_str())))
        }
        NativeNoteCommand::Edit { note, contents } => {
            let (page, current) = target(*note)?;
            note_dict(graph, current.object)?.insert("Contents".into(), pdf_contents(contents));
            (*note, Some((page.id, current.rect, contents.as_str())))
        }
        NativeNoteCommand::Move { note, page, rect } => {
            let (from, current) = target(*note)?;
            let to = destination(*page, *rect)?;
            if from.object != to {
                unlink(graph, from.object, current.object)?;
                annots_mut(graph, to)?.push(PdfValue::Reference(current.object));
                note_dict(graph, current.object)?.insert("P".into(), PdfValue::Reference(to));
            }
            note_dict(graph, current.object)?.insert("Rect".into(), pdf_rect(*rect));
            (*note, Some((*page, *rect, current.contents.as_str())))
        }
        NativeNoteCommand::Remove { note } => {
            let (page, current) = target(*note)?;
            unlink(graph, page.object, current.object)?;
            graph.objects.remove(&current.object);
            ids.remove(&current.object);
            (*note, None)
        }
    };
    Ok(ExpectedNote { id, location })
}

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use super::super::reader::{
    PdfObjectRef, PdfReadError, PdfReaderLimits, PdfValue, open_pdf_retained,
};
use super::verifier::{Reservation, Workspace};

// @contract: Callers may lower the hard edit ceilings.
#[derive(Debug, Clone, Copy)]
pub struct NativeEditLimits {
    pub max_objects: usize,
    pub max_nesting: usize,
    pub max_source_bytes: usize,
    pub max_retained_bytes: usize,
    pub max_output_bytes: usize,
    pub max_decoded_stream_bytes: usize,
    pub max_total_decoded_bytes: usize,
    pub max_work: usize,
}

impl Default for NativeEditLimits {
    fn default() -> Self {
        Self {
            max_objects: 4096,
            max_nesting: 64,
            max_source_bytes: 64 * 1024 * 1024,
            max_retained_bytes: 64 * 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
            max_decoded_stream_bytes: 8 * 1024 * 1024,
            max_total_decoded_bytes: 64 * 1024 * 1024,
            max_work: 1_000_000,
        }
    }
}

impl NativeEditLimits {
    fn validate(self) -> Result<(), NativeEditError> {
        let hard = Self::default();
        for (value, max) in [
            (self.max_objects, hard.max_objects),
            (self.max_nesting, hard.max_nesting),
            (self.max_source_bytes, hard.max_source_bytes),
            (self.max_retained_bytes, hard.max_retained_bytes),
            (self.max_output_bytes, hard.max_output_bytes),
            (self.max_decoded_stream_bytes, hard.max_decoded_stream_bytes),
            (self.max_total_decoded_bytes, hard.max_total_decoded_bytes),
            (self.max_work, hard.max_work),
        ] {
            if value == 0 || value > max {
                return Err(NativeEditError::InvalidLimits);
            }
        }
        Ok(())
    }
}

// @contract: Refusals use stable codes and return no partial output.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NativeEditError {
    #[error("native PDF edit limits are invalid")]
    InvalidLimits,
    #[error("native PDF edit budget exhausted")]
    Budget,
    #[error("native PDF structure is unsupported")]
    Unsupported,
    #[error("native PDF graph dependency is malformed")]
    Malformed,
    #[error("native PDF note is invalid")]
    InvalidNote,
    #[error("native PDF reader refused: {0}")]
    Reader(#[from] PdfReadError),
}

impl NativeEditError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidLimits => "FLOW_PDF_NATIVE_LIMITS_INVALID",
            Self::Budget => "FLOW_PDF_NATIVE_BUDGET_EXHAUSTED",
            Self::Unsupported => "FLOW_PDF_NATIVE_UNSUPPORTED",
            Self::Malformed => "FLOW_PDF_NATIVE_GRAPH_MALFORMED",
            Self::InvalidNote => "FLOW_PDF_NATIVE_NOTE_INVALID",
            Self::Reader(error) => error.code(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainTextNote {
    pub page_index: u32,
    pub contents: String,
    // @contract: PDF user-space rectangle in whole points, [left, bottom, right, top].
    pub rect: [i64; 4],
}

#[derive(Default)]
pub(super) struct PageTreeEdges<'a> {
    pub removed: Option<(PdfObjectRef, PdfObjectRef)>,
    pub retained_children: Option<(PdfObjectRef, &'a BTreeSet<PdfObjectRef>)>,
    pub removed_links: Option<&'a BTreeSet<(PdfObjectRef, PdfObjectRef)>>,
}

// @invariant: Source bytes stay immutable and object identities include generations.
pub struct NativePdfGraph {
    pub(super) source: Vec<u8>,
    pub(super) trailer: BTreeMap<String, PdfValue>,
    pub(super) objects: BTreeMap<PdfObjectRef, PdfValue>,
    pub(super) reverse: ReverseMap,
    reverse_bytes: usize,
    pub(super) pages: Vec<PdfObjectRef>,
    pub(super) limits: NativeEditLimits,
    retained: usize,
    work: usize,
    decoded: usize,
}

type ReverseMap = BTreeMap<PdfObjectRef, BTreeSet<PdfObjectRef>>;

fn insert_reverse_edge(
    reverse: &mut ReverseMap,
    charge: &mut Reservation<'_>,
    target: PdfObjectRef,
    owner: PdfObjectRef,
) -> Result<(), NativeEditError> {
    if !reverse.contains_key(&target) {
        charge
            .tree_entry::<(PdfObjectRef, BTreeSet<PdfObjectRef>)>()
            .map_err(|_| NativeEditError::Budget)?;
    }
    charge
        .insert(reverse.entry(target).or_default(), owner)
        .map_err(|_| NativeEditError::Budget)?;
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) enum Reachability<'a> {
    Page(&'a BTreeSet<PdfObjectRef>),
    Object,
}

struct PageValidation<'a> {
    seen: BTreeSet<PdfObjectRef>,
    order: Vec<PdfObjectRef>,
    seen_bytes: Reservation<'a>,
    order_bytes: Reservation<'a>,
    work: usize,
}

pub(super) fn spend(remaining: &mut usize, amount: usize) -> Result<(), NativeEditError> {
    *remaining = remaining
        .checked_sub(amount)
        .ok_or(NativeEditError::Budget)?;
    Ok(())
}

pub(super) fn decode_pdf_doc_encoding(bytes: &[u8]) -> Result<String, NativeEditError> {
    const ACCENTS: [char; 8] = ['˘', 'ˇ', 'ˆ', '˙', '˝', '˛', '˚', '˜'];
    const SPECIAL: [char; 31] = [
        '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚',
        '™', 'ﬁ', 'ﬂ', 'Ł', 'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž',
    ];
    let mut contents = String::new();
    let max_utf8_bytes = bytes.len().checked_mul(3).ok_or(NativeEditError::Budget)?;
    contents
        .try_reserve_exact(max_utf8_bytes)
        .map_err(|_| NativeEditError::Budget)?;
    contents.extend(bytes.iter().map(|&byte| match byte {
        0x18..=0x1f => ACCENTS[usize::from(byte - 0x18)],
        0x80..=0x9e => SPECIAL[usize::from(byte - 0x80)],
        0xa0 => '€',
        0x7f | 0x9f | 0xad => char::REPLACEMENT_CHARACTER,
        _ => char::from(byte),
    }));
    Ok(contents)
}

impl NativePdfGraph {
    pub fn open(bytes: &[u8], limits: NativeEditLimits) -> Result<Self, NativeEditError> {
        limits.validate()?;
        if bytes.len() > limits.max_source_bytes {
            return Err(NativeEditError::Budget);
        }
        let reader = open_pdf_retained(
            bytes,
            PdfReaderLimits {
                max_input_bytes: limits.max_source_bytes,
                max_objects: limits.max_objects,
                max_nesting: limits.max_nesting,
                max_decoded_stream_bytes: limits.max_decoded_stream_bytes,
                max_stream_bytes: limits.max_source_bytes,
                max_content_tokens: limits.max_work,
                ..Default::default()
            },
            limits.max_retained_bytes,
        )?;
        let mut retained = limits.max_retained_bytes;
        spend(&mut retained, bytes.len())?;
        spend(&mut retained, reader.retained_cost)?;
        let mut work = limits.max_work;
        spend(&mut work, reader.open_work)?;
        let mut decoded = limits.max_total_decoded_bytes;
        let mut objects = BTreeMap::new();
        let mut reverse = BTreeMap::<_, BTreeSet<_>>::new();
        let trailer = reader.retained_trailer();
        for key in ["Prev", "XRefStm", "Encrypt"] {
            if trailer.contains_key(key) {
                return Err(NativeEditError::Unsupported);
            }
        }
        let workspace = Workspace::new(retained);
        let mut object_charge = workspace.scope();
        let mut reverse_charge = workspace.scope();
        let mut queue_charge = workspace.scope();
        let mut seen_charge = workspace.scope();
        let mut queue = Vec::new();
        let mut queued = BTreeSet::new();
        let mut enqueue = |queue: &mut Vec<_>, dependency| {
            if seen_charge
                .insert(&mut queued, dependency)
                .map_err(|_| NativeEditError::Budget)?
            {
                queue_charge
                    .push(queue, dependency)
                    .map_err(|_| NativeEditError::Budget)?;
            }
            Ok(())
        };
        inspect_dict(
            trailer,
            DictionaryRole::Trailer,
            0,
            &limits,
            &mut work,
            &mut |dependency| enqueue(&mut queue, dependency),
        )?;
        let mut next = 0;
        while let Some(&(reference, role)) = queue.get(next) {
            next += 1;
            if !objects.contains_key(&reference) {
                if objects.len() >= limits.max_objects {
                    return Err(NativeEditError::Budget);
                }
                object_charge
                    .grow(512)
                    .map_err(|_| NativeEditError::Budget)?;
                let (value, cost, tokens) = reader.retained_object(
                    reference,
                    workspace.remaining(),
                    decoded.min(limits.max_decoded_stream_bytes),
                    work,
                )?;
                object_charge
                    .grow(cost)
                    .map_err(|_| NativeEditError::Budget)?;
                spend(&mut work, tokens)?;
                if let PdfValue::Stream(stream) = &value {
                    spend(&mut decoded, stream.data.len())?;
                }
                objects.insert(reference, value);
            }
            inspect(
                &objects[&reference],
                role,
                0,
                &limits,
                &mut work,
                &mut |dependency| {
                    insert_reverse_edge(
                        &mut reverse,
                        &mut reverse_charge,
                        dependency.0,
                        reference,
                    )?;
                    enqueue(&mut queue, dependency)
                },
            )?;
        }
        validate_split_checkbox_states(&objects, &mut work)?;
        drop(queue);
        drop(queued);
        drop(queue_charge);
        drop(seen_charge);
        let retained = workspace.remaining();
        let reverse_bytes = reverse_charge.into_charge();
        let (source, trailer) = reader.into_retained_parts();
        let mut graph = Self {
            source,
            trailer,
            objects,
            reverse,
            reverse_bytes,
            pages: Vec::new(),
            limits,
            retained,
            work,
            decoded,
        };
        graph.validate_pages()?;
        Ok(graph)
    }

    pub(super) fn remaining_retained(&self) -> usize {
        self.retained
    }

    pub(super) fn remaining_work(&self) -> usize {
        self.work
    }

    pub(super) fn remaining_decoded(&self) -> usize {
        self.decoded
    }

    pub(super) fn reserve_replacement_stream(
        &mut self,
        size: usize,
    ) -> Result<(), NativeEditError> {
        let cost = size
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(1024))
            .ok_or(NativeEditError::Budget)?;
        let mut retained = self.retained;
        let mut decoded = self.decoded;
        spend(&mut retained, cost)?;
        spend(&mut decoded, size)?;
        self.retained = retained;
        self.decoded = decoded;
        Ok(())
    }

    #[must_use]
    pub fn original_bytes(&self) -> &[u8] {
        &self.source
    }

    pub fn rewrite(&self) -> Result<Vec<u8>, NativeEditError> {
        super::writer::rewrite(self)
    }

    // @invariant: Mutation preparation and budget checks precede graph changes.
    pub fn add_note(&mut self, note: PlainTextNote) -> Result<(), NativeEditError> {
        if note.contents.len() > 16 * 1024
            || note.rect[0] >= note.rect[2]
            || note.rect[1] >= note.rect[3]
            || note
                .rect
                .iter()
                .any(|value| value.unsigned_abs() > 1_000_000)
        {
            return Err(NativeEditError::InvalidNote);
        }
        let page = *self
            .pages
            .get(note.page_index as usize)
            .ok_or(NativeEditError::InvalidNote)?;
        if self.objects.len() >= self.limits.max_objects {
            return Err(NativeEditError::Budget);
        }
        let mut remaining = self.retained;
        let cost = note
            .contents
            .len()
            .checked_mul(4)
            .and_then(|n| n.checked_add(4096))
            .ok_or(NativeEditError::Budget)?;
        spend(&mut remaining, cost)?;
        let mut work = self.work;
        spend(
            &mut work,
            note.contents
                .len()
                .checked_add(128)
                .ok_or(NativeEditError::Budget)?,
        )?;
        let page_dict = self
            .objects
            .get(&page)
            .and_then(PdfValue::as_dictionary)
            .ok_or(NativeEditError::Malformed)?;
        let existing = match page_dict.get("Annots") {
            None => &[][..],
            Some(value) => self
                .resolve_metered(value, &mut work)?
                .as_array()
                .ok_or(NativeEditError::Malformed)?,
        };
        spend(
            &mut remaining,
            existing
                .len()
                .checked_mul(std::mem::size_of::<PdfValue>() * 4)
                .ok_or(NativeEditError::Budget)?,
        )?;
        // @security: Reject direct annotation dictionaries here to avoid cloning unbounded subtrees.
        if existing.iter().any(|value| value.as_reference().is_none()) {
            return Err(NativeEditError::Unsupported);
        }
        let workspace = Workspace::new(remaining);
        let mut scratch = workspace.scope();
        let mut edges = workspace.scope();
        let mut old_refs = BTreeSet::new();
        let mut new_refs = BTreeSet::new();
        for (key, value) in page_dict {
            collect_reference_set(value, &mut old_refs, &mut work, &mut scratch)?;
            if key != "Annots" {
                collect_reference_set(value, &mut new_refs, &mut work, &mut scratch)?;
            }
        }
        for value in existing {
            collect_reference_set(value, &mut new_refs, &mut work, &mut scratch)?;
        }
        let mut annots = existing.to_vec();
        let mut reference = None;
        for number in 1..=self
            .limits
            .max_objects
            .checked_add(1)
            .ok_or(NativeEditError::Budget)?
        {
            spend(&mut work, 1)?;
            let candidate = PdfObjectRef {
                object_number: u32::try_from(number).map_err(|_| NativeEditError::Budget)?,
                generation: 0,
            };
            if !self.objects.contains_key(&candidate) {
                reference = Some(candidate);
                break;
            }
        }
        let reference = reference.ok_or(NativeEditError::Budget)?;
        let mut contents = vec![0xfe, 0xff];
        for unit in note.contents.encode_utf16() {
            contents.extend_from_slice(&unit.to_be_bytes());
        }
        let dict = BTreeMap::from([
            ("Type".into(), PdfValue::Name("Annot".into())),
            ("Subtype".into(), PdfValue::Name("Text".into())),
            ("Contents".into(), PdfValue::String(contents)),
            (
                "Rect".into(),
                PdfValue::Array(note.rect.into_iter().map(PdfValue::Integer).collect()),
            ),
            ("P".into(), PdfValue::Reference(page)),
        ]);
        annots.push(PdfValue::Reference(reference));
        scratch
            .insert(&mut new_refs, reference)
            .map_err(|_| NativeEditError::Budget)?;
        for (target, owner) in new_refs
            .iter()
            .map(|&target| (target, page))
            .chain(std::iter::once((page, reference)))
        {
            match self.reverse.get(&target) {
                Some(owners) if owners.contains(&owner) => continue,
                Some(_) => {}
                None if target == page && owner == reference && new_refs.contains(&page) => {}
                None => edges
                    .tree_entry::<(PdfObjectRef, BTreeSet<PdfObjectRef>)>()
                    .map_err(|_| NativeEditError::Budget)?,
            }
            edges
                .tree_entry::<PdfObjectRef>()
                .map_err(|_| NativeEditError::Budget)?;
        }
        // @invariant: Replacing the page entry preserves the array for its other owners.
        if let Some(PdfValue::Dictionary(page_dict)) = self.objects.get_mut(&page) {
            page_dict.insert("Annots".into(), PdfValue::Array(annots));
        }
        for old in old_refs.difference(&new_refs) {
            if let Some(owners) = self.reverse.get_mut(old) {
                owners.remove(&page);
            }
        }
        for target in new_refs {
            self.reverse.entry(target).or_default().insert(page);
        }
        self.reverse.entry(page).or_default().insert(reference);
        self.objects.insert(reference, PdfValue::Dictionary(dict));
        drop(old_refs);
        drop(scratch);
        self.reverse_bytes += edges.into_charge();
        self.retained = workspace.remaining();
        self.work = work;
        Ok(())
    }

    pub fn notes(&self, page_index: u32) -> Result<Vec<PlainTextNote>, NativeEditError> {
        let page = self
            .pages
            .get(page_index as usize)
            .ok_or(NativeEditError::InvalidNote)?;
        let dict = self
            .objects
            .get(page)
            .and_then(PdfValue::as_dictionary)
            .ok_or(NativeEditError::Malformed)?;
        let Some(annots) = dict.get("Annots") else {
            return Ok(Vec::new());
        };
        let mut work = self.limits.max_work;
        let annots = self
            .resolve_metered(annots, &mut work)?
            .as_array()
            .ok_or(NativeEditError::Malformed)?;
        let mut result = Vec::new();
        let mut remaining = self.limits.max_retained_bytes;
        for value in annots {
            spend(&mut work, 1)?;
            let dict = self
                .resolve_metered(value, &mut work)?
                .as_dictionary()
                .ok_or(NativeEditError::Malformed)?;
            if dict.get("Subtype").and_then(PdfValue::as_name) != Some("Text") {
                continue;
            }
            let Some(contents) = dict.get("Contents") else {
                continue;
            };
            let contents = self.resolve_metered(contents, &mut work)?;
            if matches!(contents, PdfValue::Null) {
                continue;
            }
            let values = self
                .resolve_metered(
                    dict.get("Rect").ok_or(NativeEditError::Malformed)?,
                    &mut work,
                )?
                .as_array()
                .ok_or(NativeEditError::Malformed)?;
            if values.len() != 4
                || values
                    .iter()
                    .any(|value| !matches!(value, PdfValue::Integer(_) | PdfValue::ExactReal(_)))
            {
                return Err(NativeEditError::Malformed);
            }
            let [Some(left), Some(bottom), Some(right), Some(top)] =
                std::array::from_fn(|index| values[index].as_integer())
            else {
                continue;
            };
            let rect = [left, bottom, right, top];
            let bytes = contents.as_bytes().ok_or(NativeEditError::Malformed)?;
            spend(
                &mut remaining,
                bytes
                    .len()
                    .checked_mul(4)
                    .and_then(|v| v.checked_add(256))
                    .ok_or(NativeEditError::Budget)?,
            )?;
            let contents = if bytes.starts_with(&[0xfe, 0xff]) {
                if bytes.len() % 2 != 0 {
                    return Err(NativeEditError::Malformed);
                }
                let units: Vec<_> = bytes[2..]
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&units).map_err(|_| NativeEditError::Malformed)?
            } else {
                decode_pdf_doc_encoding(bytes)?
            };
            result.push(PlainTextNote {
                page_index,
                contents,
                rect,
            });
        }
        Ok(result)
    }

    pub(super) fn resolve_metered<'a>(
        &'a self,
        mut value: &'a PdfValue,
        work: &mut usize,
    ) -> Result<&'a PdfValue, NativeEditError> {
        for _ in 0..=self.limits.max_nesting {
            match value {
                PdfValue::Reference(reference) => {
                    spend(work, 1)?;
                    value = self
                        .objects
                        .get(reference)
                        .ok_or(NativeEditError::Malformed)?
                }
                _ => return Ok(value),
            }
        }
        Err(NativeEditError::Malformed)
    }

    pub(super) fn reserve_page_copy(
        &mut self,
        reference: PdfObjectRef,
    ) -> Result<(), NativeEditError> {
        let value = self
            .objects
            .get(&reference)
            .ok_or(NativeEditError::Malformed)?;
        reserve_page_value(value, &mut self.work, &mut self.retained)
    }

    pub(super) fn reserve_page_value(
        &self,
        owner: PdfObjectRef,
        work: &mut usize,
        retained: &mut usize,
    ) -> Result<(), NativeEditError> {
        let value = self.objects.get(&owner).ok_or(NativeEditError::Malformed)?;
        reserve_page_value(value, work, retained)
    }

    pub(super) fn charge_page_retained(&mut self, retained: usize) -> Result<(), NativeEditError> {
        spend(&mut self.retained, retained)
    }

    pub(super) fn release_page_retained(&mut self, released: usize) {
        self.retained += released;
    }

    pub(super) fn trailer_references(
        &self,
        work: &mut usize,
        reservation: &mut Reservation<'_>,
    ) -> Result<BTreeSet<PdfObjectRef>, NativeEditError> {
        let mut refs = BTreeSet::new();
        for value in self.trailer.values() {
            collect_reference_set(value, &mut refs, work, reservation)?;
        }
        Ok(refs)
    }

    pub(super) fn check_page_removals(
        &self,
        removed: &BTreeSet<PdfObjectRef>,
        replacements: &BTreeMap<PdfObjectRef, BTreeMap<String, PdfValue>>,
        page_tree_edges: PageTreeEdges<'_>,
        trailer: &BTreeSet<PdfObjectRef>,
        work: &mut usize,
        workspace: &Workspace,
    ) -> Result<(), NativeEditError> {
        use NativeEditError::Budget;

        let mut edges = workspace.scope();
        let mut replacement_owners = BTreeMap::<_, BTreeSet<_>>::new();
        for (&owner, values) in replacements {
            for value in values.values() {
                visit_refs(value, work, &mut |target| {
                    if !replacement_owners.contains_key(&target) {
                        edges
                            .tree_entry::<(PdfObjectRef, BTreeSet<PdfObjectRef>)>()
                            .map_err(|_| Budget)?;
                    }
                    edges
                        .insert(replacement_owners.entry(target).or_default(), owner)
                        .map_err(|_| Budget)?;
                    Ok(())
                })?;
            }
        }

        let mut visited = workspace.scope();
        let mut seen = BTreeSet::new();
        let mut stack = workspace.scope();
        let mut pending = Vec::new();
        for &target in removed {
            stack.push(&mut pending, target).map_err(|_| Budget)?;
        }
        while let Some(target) = pending.pop() {
            spend(work, 1)?;
            if !visited.insert(&mut seen, target).map_err(|_| Budget)? {
                continue;
            }
            if trailer.contains(&target) {
                return Err(NativeEditError::Unsupported);
            }
            if let Some(owners) = self.reverse.get(&target) {
                spend(work, owners.len())?;
                for &owner in owners {
                    if !replacements.contains_key(&owner)
                        && page_tree_edges.removed != Some((owner, target))
                        && !page_tree_edges
                            .removed_links
                            .is_some_and(|edges| edges.contains(&(owner, target)))
                    {
                        stack.push(&mut pending, owner).map_err(|_| Budget)?;
                    }
                }
            }
            if let Some((parent, children)) = page_tree_edges.retained_children
                && !removed.contains(&target)
                && children.contains(&target)
            {
                stack.push(&mut pending, parent).map_err(|_| Budget)?;
            }
            if let Some(owners) = replacement_owners.get(&target) {
                for &owner in owners {
                    stack.push(&mut pending, owner).map_err(|_| Budget)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn charge_page_work(&mut self, work: usize) -> Result<(), NativeEditError> {
        spend(&mut self.work, work)
    }

    pub(super) fn refresh_page_graph(
        &mut self,
        removed: &BTreeSet<PdfObjectRef>,
    ) -> Result<(), NativeEditError> {
        let workspace = Workspace::new(self.retained);
        let mut work = self.work;
        self.rebuild_reverse(&mut work, &workspace, Reachability::Page(removed))?;
        self.retained = workspace.remaining();
        self.work = work;
        self.validate_pages()
    }

    pub(super) fn rebuild_reverse(
        &mut self,
        work: &mut usize,
        workspace: &Workspace,
        mode: Reachability<'_>,
    ) -> Result<(), NativeEditError> {
        use NativeEditError::{Budget, Malformed, Unsupported};

        let mut pending_charge = workspace.scope();
        let mut reachable_charge = workspace.scope();
        let mut reverse_charge = workspace.scope();
        let mut pending = Vec::new();
        let mut reachable = BTreeSet::new();
        let mut reverse = ReverseMap::new();
        for value in self.trailer.values() {
            visit_refs(value, work, &mut |target| {
                pending_charge
                    .push(&mut pending, target)
                    .map_err(|_| Budget)
            })?;
        }
        let object_edit = matches!(mode, Reachability::Object);
        while let Some(owner) = pending.pop() {
            spend(work, 1)?;
            if matches!(mode, Reachability::Page(removed) if removed.contains(&owner)) {
                return Err(Unsupported);
            }
            if !reachable_charge
                .insert(&mut reachable, owner)
                .map_err(|_| Budget)?
            {
                continue;
            }
            if object_edit && reachable.len() > self.limits.max_objects {
                return Err(Budget);
            }
            let value = self.objects.get(&owner).ok_or(Malformed)?;
            let edge_work = if object_edit { 2 } else { 0 };
            visit_refs_charged(value, work, edge_work, &mut |target| {
                insert_reverse_edge(&mut reverse, &mut reverse_charge, target, owner)?;
                pending_charge
                    .push(&mut pending, target)
                    .map_err(|_| Budget)
            })?;
        }
        if object_edit {
            spend(work, self.objects.len())?;
        }
        self.objects
            .retain(|reference, _| reachable.contains(reference));
        drop(pending);
        drop(reachable);
        drop(pending_charge);
        drop(reachable_charge);

        let new_bytes = reverse_charge.into_charge();
        let old_bytes = std::mem::replace(&mut self.reverse_bytes, new_bytes);
        drop(std::mem::replace(&mut self.reverse, reverse));
        workspace.credit(old_bytes);
        self.retained = self.retained - new_bytes + old_bytes;
        Ok(())
    }

    fn validate_pages(&mut self) -> Result<(), NativeEditError> {
        let root = self
            .trailer
            .get("Root")
            .and_then(PdfValue::as_reference)
            .ok_or(NativeEditError::Malformed)?;
        let catalog = self
            .objects
            .get(&root)
            .and_then(PdfValue::as_dictionary)
            .ok_or(NativeEditError::Malformed)?;
        if catalog.get("Type").and_then(PdfValue::as_name) != Some("Catalog") {
            return Err(NativeEditError::Malformed);
        }
        let pages = catalog
            .get("Pages")
            .and_then(PdfValue::as_reference)
            .ok_or(NativeEditError::Malformed)?;
        let workspace = Workspace::new(self.retained);
        let mut validation = PageValidation {
            seen: BTreeSet::new(),
            order: Vec::new(),
            seen_bytes: workspace.scope(),
            order_bytes: workspace.scope(),
            work: self.work,
        };
        self.walk_pages(pages, None, false, 0, &mut validation)?;
        if validation.order.is_empty() {
            return Err(NativeEditError::Malformed);
        }
        let PageValidation {
            seen,
            order,
            seen_bytes,
            order_bytes,
            work,
        } = validation;
        let previous_order_bytes = self
            .pages
            .capacity()
            .checked_mul(size_of::<PdfObjectRef>())
            .ok_or(NativeEditError::Budget)?;
        drop(seen);
        drop(seen_bytes);
        let retained = workspace
            .remaining()
            .checked_add(previous_order_bytes)
            .ok_or(NativeEditError::Budget)?;
        self.pages = order;
        self.retained = retained;
        self.work = work;
        drop(order_bytes);
        Ok(())
    }

    fn walk_pages(
        &self,
        reference: PdfObjectRef,
        parent: Option<PdfObjectRef>,
        inherited_box: bool,
        depth: usize,
        validation: &mut PageValidation<'_>,
    ) -> Result<usize, NativeEditError> {
        let work = &mut validation.work;
        spend(work, 1)?;
        if depth > self.limits.max_nesting
            || !validation
                .seen_bytes
                .insert(&mut validation.seen, reference)
                .map_err(|_| NativeEditError::Budget)?
        {
            return Err(NativeEditError::Malformed);
        }
        let dict = self
            .objects
            .get(&reference)
            .and_then(PdfValue::as_dictionary)
            .ok_or(NativeEditError::Malformed)?;
        let parent_valid = match parent {
            Some(expected) => dict.get("Parent").and_then(PdfValue::as_reference) == Some(expected),
            None => {
                !dict.contains_key("Parent")
                    && dict.get("Type").and_then(PdfValue::as_name) == Some("Pages")
            }
        };
        if !parent_valid {
            return Err(NativeEditError::Malformed);
        }
        let has_box = if let Some(value) = dict.get("MediaBox") {
            let [left, bottom, right, top] = self
                .resolve_metered(value, work)?
                .as_array()
                .ok_or(NativeEditError::Malformed)?
            else {
                return Err(NativeEditError::Malformed);
            };
            let [Some(left), Some(bottom), Some(right), Some(top)] =
                [left, bottom, right, top].map(PdfValue::as_number)
            else {
                return Err(NativeEditError::Malformed);
            };
            if left.raw() >= right.raw() || bottom.raw() >= top.raw() {
                return Err(NativeEditError::Malformed);
            }
            true
        } else {
            inherited_box
        };
        if let Some(resources) = dict.get("Resources") {
            if self
                .resolve_metered(resources, work)?
                .as_dictionary()
                .is_none()
            {
                return Err(NativeEditError::Malformed);
            }
        }
        match dict.get("Type").and_then(PdfValue::as_name) {
            Some("Page") => {
                if !has_box {
                    return Err(NativeEditError::Malformed);
                }
                if let Some(contents) = dict.get("Contents") {
                    match self.resolve_metered(contents, work)? {
                        PdfValue::Stream(_) | PdfValue::Null => {}
                        PdfValue::Array(values) => {
                            for value in values {
                                spend(work, 1)?;
                                if !matches!(
                                    self.resolve_metered(value, work)?,
                                    PdfValue::Stream(_)
                                ) {
                                    return Err(NativeEditError::Malformed);
                                }
                            }
                        }
                        _ => return Err(NativeEditError::Malformed),
                    }
                }
                if let Some(annots) = dict.get("Annots") {
                    for annot in self
                        .resolve_metered(annots, work)?
                        .as_array()
                        .ok_or(NativeEditError::Malformed)?
                    {
                        spend(work, 1)?;
                        if self.resolve_metered(annot, work)?.as_dictionary().is_none() {
                            return Err(NativeEditError::Malformed);
                        }
                    }
                }
                validation
                    .order_bytes
                    .push(&mut validation.order, reference)
                    .map_err(|_| NativeEditError::Budget)?;
                Ok(1)
            }
            Some("Pages") => {
                let kids = self
                    .resolve_metered(dict.get("Kids").ok_or(NativeEditError::Malformed)?, work)?
                    .as_array()
                    .ok_or(NativeEditError::Malformed)?;
                spend(work, kids.len())?;
                let expected = dict
                    .get("Count")
                    .and_then(PdfValue::as_integer)
                    .ok_or(NativeEditError::Malformed)?;
                let mut count = 0usize;
                for child in kids {
                    let child = child.as_reference().ok_or(NativeEditError::Malformed)?;
                    count = count
                        .checked_add(self.walk_pages(
                            child,
                            Some(reference),
                            has_box,
                            depth + 1,
                            validation,
                        )?)
                        .ok_or(NativeEditError::Budget)?;
                }
                if i64::try_from(count).ok() != Some(expected) {
                    return Err(NativeEditError::Malformed);
                }
                Ok(count)
            }
            _ => Err(NativeEditError::Malformed),
        }
    }
}

fn reserve_page_value(
    value: &PdfValue,
    work: &mut usize,
    retained: &mut usize,
) -> Result<(), NativeEditError> {
    spend(work, 1)?;
    spend(retained, 128)?;
    match value {
        PdfValue::String(bytes) => spend(retained, bytes.len())?,
        PdfValue::Name(text) | PdfValue::ExactReal(text) => spend(retained, text.len())?,
        PdfValue::Array(values) => {
            for value in values {
                reserve_page_value(value, work, retained)?;
            }
        }
        PdfValue::Dictionary(dict) => {
            for (key, value) in dict {
                spend(retained, key.len())?;
                reserve_page_value(value, work, retained)?;
            }
        }
        PdfValue::Stream(_) => return Err(NativeEditError::Unsupported),
        _ => {}
    }
    Ok(())
}

pub(super) fn collect_refs(
    value: &PdfValue,
    refs: &mut Vec<PdfObjectRef>,
    work: &mut usize,
    retained: &mut usize,
) -> Result<(), NativeEditError> {
    visit_refs(value, work, &mut |reference| {
        spend(retained, 128)?;
        refs.push(reference);
        Ok(())
    })
}

pub(super) fn collect_reference_set(
    value: &PdfValue,
    refs: &mut BTreeSet<PdfObjectRef>,
    work: &mut usize,
    reservation: &mut Reservation<'_>,
) -> Result<(), NativeEditError> {
    visit_refs(value, work, &mut |reference| {
        reservation
            .insert(refs, reference)
            .map_err(|_| NativeEditError::Budget)?;
        Ok(())
    })
}

pub(super) fn visit_refs(
    value: &PdfValue,
    work: &mut usize,
    visit: &mut impl FnMut(PdfObjectRef) -> Result<(), NativeEditError>,
) -> Result<(), NativeEditError> {
    visit_refs_charged(value, work, 0, visit)
}

fn visit_refs_charged(
    value: &PdfValue,
    work: &mut usize,
    edge_work: usize,
    visit: &mut impl FnMut(PdfObjectRef) -> Result<(), NativeEditError>,
) -> Result<(), NativeEditError> {
    spend(work, 1)?;
    match value {
        PdfValue::Reference(reference) => {
            spend(work, edge_work)?;
            visit(*reference)?;
        }
        PdfValue::Array(values) => {
            for value in values {
                visit_refs_charged(value, work, edge_work, visit)?;
            }
        }
        PdfValue::Dictionary(dict) => {
            for value in dict.values() {
                visit_refs_charged(value, work, edge_work, visit)?;
            }
        }
        PdfValue::Stream(stream) => {
            for value in stream.dict.values() {
                visit_refs_charged(value, work, edge_work, visit)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DictionaryRole {
    Object,
    Trailer,
    Catalog,
    AcroForm,
    Resources,
    Appearance,
    Names,
    CheckboxState,
}

impl DictionaryRole {
    fn child(self, key: &str) -> Self {
        match (self, key) {
            (Self::Trailer, "Root") => Self::Catalog,
            (Self::Catalog, "AcroForm") => Self::AcroForm,
            (Self::AcroForm, "DR") => Self::Resources,
            (Self::Object | Self::Trailer | Self::Catalog | Self::AcroForm, "Resources") => {
                Self::Resources
            }
            (Self::Object | Self::Trailer | Self::Catalog | Self::AcroForm, "AP") => {
                Self::Appearance
            }
            (Self::Appearance, "N" | "R" | "D")
            | (
                Self::Resources,
                "ExtGState" | "ColorSpace" | "Pattern" | "Shading" | "XObject" | "Font"
                | "Properties",
            ) => Self::Names,
            _ => Self::Object,
        }
    }
}

fn inspect(
    value: &PdfValue,
    role: DictionaryRole,
    depth: usize,
    limits: &NativeEditLimits,
    work: &mut usize,
    visit: &mut impl FnMut((PdfObjectRef, DictionaryRole)) -> Result<(), NativeEditError>,
) -> Result<(), NativeEditError> {
    spend(work, 1)?;
    if depth > limits.max_nesting {
        return Err(NativeEditError::Budget);
    }
    match value {
        PdfValue::Reference(reference) => {
            spend(work, 1)?;
            visit((*reference, role))?;
        }
        PdfValue::Array(values) => {
            for value in values {
                inspect(
                    value,
                    DictionaryRole::Object,
                    depth + 1,
                    limits,
                    work,
                    visit,
                )?;
            }
        }
        PdfValue::Dictionary(dict) => inspect_dict(dict, role, depth, limits, work, visit)?,
        PdfValue::Stream(stream) => {
            if stream.dict.contains_key("F")
                || stream.dict.contains_key("FFilter")
                || stream.dict.contains_key("FDecodeParms")
                || stream.dict.contains_key("DecodeParms")
            {
                return Err(NativeEditError::Unsupported);
            }
            inspect_dict(
                &stream.dict,
                DictionaryRole::Object,
                depth,
                limits,
                work,
                visit,
            )?;
            // @contract: Admission preserves encoded content without interpreting operators.
        }
        PdfValue::Name(_) if role == DictionaryRole::CheckboxState => {}
        PdfValue::Name(name) if restricted_name(name) => {
            return Err(NativeEditError::Unsupported);
        }
        PdfValue::Real(_) => return Err(NativeEditError::Unsupported),
        _ => {}
    }
    Ok(())
}

fn restricted_name(name: &str) -> bool {
    matches!(
        name,
        "Sig"
            | "DocTimeStamp"
            | "XRef"
            | "ObjStm"
            | "Action"
            | "GoTo"
            | "Named"
            | "Hide"
            | "ResetForm"
            | "SetOCGState"
            | "Trans"
            | "GoTo3DView"
            | "Thread"
            | "JavaScript"
            | "Launch"
            | "URI"
            | "GoToR"
            | "GoToE"
            | "SubmitForm"
            | "ImportData"
            | "Rendition"
            | "RichMedia"
            | "Movie"
            | "Sound"
            | "Screen"
            | "3D"
            | "FileAttachment"
            | "Filespec"
            | "EmbeddedFile"
    )
}

fn checkbox_owner(dict: &BTreeMap<String, PdfValue>) -> bool {
    dict.get("FT").and_then(PdfValue::as_name) == Some("Btn")
        && !dict.contains_key("Parent")
        && dict
            .get("Ff")
            .is_none_or(|flags| matches!(flags.as_integer(), Some(0 | 2)))
}

fn checkbox_widget(dict: &BTreeMap<String, PdfValue>) -> bool {
    dict.get("Subtype").and_then(PdfValue::as_name) == Some("Widget")
        && dict
            .get("Type")
            .is_none_or(|value| value.as_name() == Some("Annot"))
}

fn checkbox_state_mapped(dict: &BTreeMap<String, PdfValue>, state: &str) -> bool {
    dict.get("AP")
        .and_then(PdfValue::as_dictionary)
        .filter(|ap| ap.len() == 1)
        .and_then(|ap| ap.get("N"))
        .and_then(PdfValue::as_dictionary)
        .and_then(|states| {
            let on = super::forms::checkbox_on_state(states)?;
            let off_form = states.get("Off")?.as_reference()?;
            let on_form = states.get(on)?.as_reference()?;
            (off_form != on_form).then_some(on)
        })
        .is_some_and(|on| state == "Off" || state == on)
}

fn split_checkbox_owner(dict: &BTreeMap<String, PdfValue>) -> bool {
    checkbox_owner(dict) && !dict.contains_key("Type") && !dict.contains_key("Subtype")
}

fn split_checkbox_parent(dict: &BTreeMap<String, PdfValue>) -> Option<PdfObjectRef> {
    if checkbox_widget(dict)
        && ["FT", "V", "DV", "Kids"]
            .iter()
            .all(|key| !dict.contains_key(*key))
    {
        return dict.get("Parent").and_then(PdfValue::as_reference);
    }
    None
}

fn provisional_checkbox_state(
    dict: &BTreeMap<String, PdfValue>,
    key: &str,
    value: &PdfValue,
) -> bool {
    let Some(state) = value.as_name().filter(|name| restricted_name(name)) else {
        return false;
    };
    let combined = checkbox_owner(dict) && checkbox_widget(dict) && !dict.contains_key("Kids");
    match key {
        "V" | "DV" => {
            (combined && checkbox_state_mapped(dict, state))
                || (split_checkbox_owner(dict) && dict.contains_key("Kids"))
        }
        "AS" => {
            (combined || split_checkbox_parent(dict).is_some())
                && checkbox_state_mapped(dict, state)
        }
        _ => false,
    }
}

fn split_checkbox_child(
    objects: &BTreeMap<PdfObjectRef, PdfValue>,
    owner: &BTreeMap<String, PdfValue>,
) -> Option<PdfObjectRef> {
    let kids = owner.get("Kids")?;
    let children = match kids.as_reference() {
        Some(reference) => objects.get(&reference)?,
        None => kids,
    };
    let [child] = children.as_array()? else {
        return None;
    };
    child.as_reference()
}

fn validate_split_checkbox_states(
    objects: &BTreeMap<PdfObjectRef, PdfValue>,
    work: &mut usize,
) -> Result<(), NativeEditError> {
    for (&reference, value) in objects {
        let Some(dict) = value.as_dictionary() else {
            continue;
        };
        if split_checkbox_owner(dict) {
            for key in ["V", "DV"] {
                let Some(state) = dict
                    .get(key)
                    .and_then(PdfValue::as_name)
                    .filter(|name| restricted_name(name))
                else {
                    continue;
                };
                spend(work, 1)?;
                let Some(widget) = split_checkbox_child(objects, dict)
                    .and_then(|child| objects.get(&child))
                    .and_then(PdfValue::as_dictionary)
                else {
                    return Err(NativeEditError::Unsupported);
                };
                if split_checkbox_parent(widget) != Some(reference)
                    || !checkbox_state_mapped(widget, state)
                {
                    return Err(NativeEditError::Unsupported);
                }
            }
        } else if let Some(parent) = split_checkbox_parent(dict) {
            let Some(state) = dict
                .get("AS")
                .and_then(PdfValue::as_name)
                .filter(|name| restricted_name(name))
            else {
                continue;
            };
            spend(work, 1)?;
            let Some(owner) = objects.get(&parent).and_then(PdfValue::as_dictionary) else {
                return Err(NativeEditError::Unsupported);
            };
            if !split_checkbox_owner(owner)
                || split_checkbox_child(objects, owner) != Some(reference)
                || !checkbox_state_mapped(dict, state)
            {
                return Err(NativeEditError::Unsupported);
            }
        }
    }
    Ok(())
}

fn inspect_dict(
    dict: &BTreeMap<String, PdfValue>,
    role: DictionaryRole,
    depth: usize,
    limits: &NativeEditLimits,
    work: &mut usize,
    visit: &mut impl FnMut((PdfObjectRef, DictionaryRole)) -> Result<(), NativeEditError>,
) -> Result<(), NativeEditError> {
    for (key, value) in dict {
        if role != DictionaryRole::Names
            && matches!(
                key.as_str(),
                "JS" | "JavaScript"
                    | "OpenAction"
                    | "AA"
                    | "A"
                    | "Launch"
                    | "SubmitForm"
                    | "GoToR"
                    | "URI"
                    | "RichMedia"
                    | "Movie"
                    | "Sound"
                    | "EmbeddedFiles"
                    | "EF"
                    | "External"
                    | "ByteRange"
                    | "Perms"
                    | "XFA"
            )
        {
            return Err(NativeEditError::Unsupported);
        }
        if role != DictionaryRole::Names
            && matches!(key.as_str(), "Type" | "FT" | "S" | "Subtype")
            && matches!(
                value.as_name(),
                Some(
                    "Sig"
                        | "DocTimeStamp"
                        | "XRef"
                        | "ObjStm"
                        | "JavaScript"
                        | "Launch"
                        | "URI"
                        | "GoToR"
                        | "SubmitForm"
                        | "ImportData"
                        | "Rendition"
                        | "RichMedia"
                        | "Movie"
                        | "Sound"
                )
            )
        {
            return Err(NativeEditError::Unsupported);
        }
        // @security: Split checkbox state names require reciprocal ownership after graph discovery.
        let child_role = if role == DictionaryRole::Object
            && depth == 0
            && provisional_checkbox_state(dict, key, value)
        {
            DictionaryRole::CheckboxState
        } else {
            role.child(key)
        };
        inspect(value, child_role, depth + 1, limits, work, visit)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_checkbox_values_do_not_change_action_or_signature_admission() {
        let limits = NativeEditLimits::default();
        let child = PdfObjectRef {
            object_number: 7,
            generation: 0,
        };
        let field = BTreeMap::from([
            ("FT".into(), PdfValue::Name("Btn".into())),
            (
                "Kids".into(),
                PdfValue::Array(vec![PdfValue::Reference(child)]),
            ),
            ("V".into(), PdfValue::Name("Launch".into())),
        ]);
        let inspect_field = |field: &BTreeMap<String, PdfValue>| {
            let mut work = limits.max_work;
            inspect_dict(
                field,
                DictionaryRole::Object,
                0,
                &limits,
                &mut work,
                &mut |_| Ok(()),
            )
        };
        assert_eq!(inspect_field(&field), Ok(()));
        for (key, value) in [
            ("A", PdfValue::Null),
            ("FT", PdfValue::Name("Sig".into())),
            ("Unrelated", PdfValue::Name("Launch".into())),
        ] {
            let mut field = field.clone();
            field.insert(key.into(), value);
            assert_eq!(inspect_field(&field), Err(NativeEditError::Unsupported));
        }
    }

    #[test]
    fn action_keys_are_refused_outside_name_maps() {
        let limits = NativeEditLimits::default();
        for key in ["A", "AA", "OpenAction"] {
            let dict = BTreeMap::from([(key.into(), PdfValue::Null)]);
            let inspect_role = |role| {
                let mut work = limits.max_work;
                inspect_dict(&dict, role, 0, &limits, &mut work, &mut |_| Ok(()))
            };
            assert!(inspect_role(DictionaryRole::Names).is_ok());
            assert!(matches!(
                inspect_role(DictionaryRole::Object),
                Err(NativeEditError::Unsupported)
            ));
        }
    }

    #[test]
    fn cumulative_mutation_budget_refusal_leaves_graph_unchanged() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &payload);
        let mut graph = NativePdfGraph::open(&bytes, NativeEditLimits::default()).unwrap();
        let before = graph.rewrite().unwrap();
        let note = PlainTextNote {
            page_index: 0,
            contents: "private text".into(),
            rect: [1, 2, 3, 4],
        };
        let work = graph.work;
        graph.work = 0;
        let error = graph.add_note(note.clone()).unwrap_err();
        assert_eq!(error, NativeEditError::Budget);
        assert!(!error.to_string().contains("private text"));
        graph.work = work;
        assert_eq!(before, graph.rewrite().unwrap());
        let retained = graph.retained;
        graph.retained = 0;
        assert_eq!(graph.add_note(note), Err(NativeEditError::Budget));
        graph.retained = retained;
        assert_eq!(before, graph.rewrite().unwrap());
        assert_eq!(graph.original_bytes(), bytes);
    }

    #[test]
    fn page_removal_preflight_checks_replacement_edges_and_terminates_on_detached_cycles() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &payload);
        let mut graph = NativePdfGraph::open(&bytes, NativeEditLimits::default()).unwrap();
        let page = graph.pages[0];
        let root = graph.trailer["Root"].as_reference().unwrap();
        let parent = graph.objects[&page].as_dictionary().unwrap()["Parent"]
            .as_reference()
            .unwrap();
        let detached = PdfObjectRef {
            object_number: 100,
            generation: 0,
        };
        graph.reverse.entry(page).or_default().insert(detached);
        graph.reverse.insert(detached, BTreeSet::from([page]));
        let removed = BTreeSet::from([page]);
        let mut replacements = BTreeMap::from([(parent, BTreeMap::new())]);
        let mut work = graph.remaining_work();
        let workspace = Workspace::new(graph.remaining_retained());
        let mut trailer_refs = workspace.scope();
        let trailer = graph
            .trailer_references(&mut work, &mut trailer_refs)
            .unwrap();

        assert_eq!(
            graph.check_page_removals(
                &removed,
                &replacements,
                PageTreeEdges::default(),
                &trailer,
                &mut work,
                &workspace
            ),
            Ok(())
        );
        replacements.insert(
            root,
            BTreeMap::from([("Retained".into(), PdfValue::Reference(detached))]),
        );
        assert_eq!(
            graph.check_page_removals(
                &removed,
                &replacements,
                PageTreeEdges::default(),
                &trailer,
                &mut work,
                &workspace
            ),
            Err(NativeEditError::Unsupported)
        );
        assert_eq!(
            graph.check_page_removals(
                &removed,
                &replacements,
                PageTreeEdges::default(),
                &trailer,
                &mut 0,
                &workspace
            ),
            Err(NativeEditError::Budget)
        );
    }

    #[test]
    fn object_edge_work_is_charged_before_visiting_the_reference() {
        let reference = PdfObjectRef {
            object_number: 1,
            generation: 0,
        };
        let value = PdfValue::Reference(reference);
        let mut visits = 0;
        let mut visit = |target| {
            assert_eq!(target, reference);
            visits += 1;
            Ok(())
        };
        assert_eq!(
            visit_refs_charged(&value, &mut 2, 2, &mut visit),
            Err(NativeEditError::Budget)
        );
        visit_refs_charged(&value, &mut 3, 2, &mut visit).unwrap();
        assert_eq!(visits, 1);
    }

    #[test]
    fn reverse_rebuilds_charge_the_peak_and_refund_previous_and_scratch_storage() {
        let bytes = super::super::tests::redaction_fixture("10 10 20 20 re f");
        let open = || NativePdfGraph::open(&bytes, NativeEditLimits::default()).unwrap();
        let empty = BTreeSet::new();
        for mode in [Reachability::Page(&empty), Reachability::Object] {
            let mut graph = open();
            let installed = 3 * (128 + 12 * size_of::<(PdfObjectRef, BTreeSet<PdfObjectRef>)>())
                + 4 * (128 + 12 * size_of::<PdfObjectRef>());
            let visited = 4 * (128 + 12 * size_of::<PdfObjectRef>());
            let pending = 4 * size_of::<PdfObjectRef>();
            let peak = installed + visited + pending;
            assert_eq!(graph.reverse_bytes, installed);
            let reverse = graph.reverse.clone();
            let mut work = graph.remaining_work();

            graph.retained = peak - 1;
            let workspace = Workspace::new(graph.retained);
            assert_eq!(
                graph.rebuild_reverse(&mut work, &workspace, mode),
                Err(NativeEditError::Budget)
            );
            assert_eq!(graph.reverse, reverse);
            assert_eq!(graph.reverse_bytes, installed);
            assert_eq!(graph.retained, peak - 1);
            assert_eq!(workspace.remaining(), peak - 1);

            graph.retained = peak;
            let workspace = Workspace::new(graph.retained);
            for _ in 0..2 {
                graph.rebuild_reverse(&mut work, &workspace, mode).unwrap();
                assert_eq!(graph.reverse, reverse);
                assert_eq!(graph.reverse_bytes, installed);
                assert_eq!(graph.retained, peak);
                assert_eq!(workspace.remaining(), peak);
            }
        }
    }

    #[test]
    fn direct_note_reverse_debits_are_refundable_by_the_next_rebuild() {
        let bytes = super::super::tests::redaction_fixture("");
        let mut graph = NativePdfGraph::open(&bytes, NativeEditLimits::default()).unwrap();
        let page = graph.pages[0];
        let array =
            super::super::pages::allocate_value(&mut graph, PdfValue::Array(Vec::new())).unwrap();
        super::super::pages::dict_mut(&mut graph, page)
            .unwrap()
            .insert("Annots".into(), PdfValue::Reference(array));
        graph.refresh_page_graph(&BTreeSet::new()).unwrap();
        let old_bytes = graph.reverse_bytes;
        graph
            .add_note(PlainTextNote {
                page_index: 0,
                contents: "Delivery note".into(),
                rect: [10, 10, 20, 20],
            })
            .unwrap();
        let key = 128 + 12 * size_of::<(PdfObjectRef, BTreeSet<PdfObjectRef>)>();
        let edge = 128 + 12 * size_of::<PdfObjectRef>();
        assert_eq!(graph.reverse_bytes, old_bytes + key + 2 * edge);
        assert!(graph.reverse[&array].is_empty());
        let remaining = graph.remaining_retained();
        for _ in 0..2 {
            graph.refresh_page_graph(&BTreeSet::new()).unwrap();
            assert!(!graph.reverse.contains_key(&array));
            assert_eq!(graph.reverse_bytes, old_bytes + edge);
            assert_eq!(graph.remaining_retained(), remaining + key + edge);
        }
    }

    #[test]
    fn page_validation_charges_kid_entries_and_contents_reference() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &payload);
        let mut graph = NativePdfGraph::open(&bytes, NativeEditLimits::default()).unwrap();

        graph.pages.clear();
        graph.work = 3;
        assert_eq!(graph.validate_pages(), Err(NativeEditError::Budget));

        graph.pages.clear();
        graph.work = 4;
        graph.validate_pages().unwrap();
        assert_eq!(graph.work, 0);
        assert_eq!(graph.pages.len(), 1);
    }

    #[test]
    fn page_validation_charges_visited_nodes_and_order_together() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &payload);
        let mut graph = NativePdfGraph::open(&bytes, NativeEditLimits::default()).unwrap();
        let order = graph.pages.clone();
        let work = graph.work;

        let allowance = graph.retained;
        let workspace = Workspace::new(allowance);
        let mut visited = workspace.scope();
        visited.tree_entry::<PdfObjectRef>().unwrap();
        visited.tree_entry::<PdfObjectRef>().unwrap();
        let visited_bytes = allowance - workspace.remaining();
        let required = visited_bytes + graph.pages.capacity() * size_of::<PdfObjectRef>();

        for retained in [0, visited_bytes, required - 1] {
            graph.retained = retained;
            assert_eq!(graph.validate_pages(), Err(NativeEditError::Budget));
            assert_eq!(graph.pages, order);
            assert_eq!(graph.work, work);
            assert_eq!(graph.retained, retained);
        }
        graph.retained = required;
        graph.validate_pages().unwrap();
        assert_eq!(graph.pages, order);
        assert_eq!(graph.retained, required);
        graph.validate_pages().unwrap();
        assert_eq!(graph.retained, required);
    }

    #[test]
    fn decimal_annotation_survives_rewrite_alongside_whole_point_note() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &payload);
        let mut graph = NativePdfGraph::open(&bytes, NativeEditLimits::default()).unwrap();
        let note = PlainTextNote {
            page_index: 0,
            contents: "Whole-point note".into(),
            rect: [1, 2, 3, 4],
        };
        graph.add_note(note.clone()).unwrap();
        for object in graph.objects.values_mut() {
            if let PdfValue::Dictionary(dict) = object
                && let Some(PdfValue::Array(rect)) = dict.get_mut("Rect")
            {
                rect[0] = PdfValue::ExactReal("1.25".into());
            }
        }
        let imported = graph.rewrite().unwrap();
        let mut graph = NativePdfGraph::open(&imported, NativeEditLimits::default()).unwrap();
        graph.add_note(note.clone()).unwrap();
        let output = graph.rewrite().unwrap();
        let reopened = NativePdfGraph::open(&output, NativeEditLimits::default()).unwrap();
        assert_eq!(reopened.notes(0).unwrap(), vec![note]);
        assert!(reopened.objects.values().any(|object| {
            matches!(
                object.as_dictionary().and_then(|dict| dict.get("Rect")).and_then(PdfValue::as_array),
                Some([PdfValue::ExactReal(token), ..]) if token == "1.25"
            )
        }));
    }

    #[test]
    fn imported_annotation_without_contents_survives_alongside_readable_note() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &payload);
        let original = PlainTextNote {
            page_index: 0,
            contents: String::new(),
            rect: [10, 20, 30, 40],
        };
        let added = PlainTextNote {
            page_index: 0,
            contents: "Review note".into(),
            rect: [1, 2, 3, 4],
        };

        for explicit_null in [false, true] {
            let mut graph = NativePdfGraph::open(&bytes, NativeEditLimits::default()).unwrap();
            graph.add_note(original.clone()).unwrap();
            let annotation = graph
                .objects
                .values_mut()
                .find_map(|value| match value {
                    PdfValue::Dictionary(dict) if dict.contains_key("Rect") => Some(dict),
                    _ => None,
                })
                .unwrap();
            annotation.remove("Rect");
            if explicit_null {
                annotation.insert("Contents".into(), PdfValue::Null);
            } else {
                annotation.remove("Contents");
            }
            let imported = graph.rewrite().unwrap();
            let mut graph = NativePdfGraph::open(&imported, NativeEditLimits::default()).unwrap();
            graph.add_note(added.clone()).unwrap();
            let output = graph.rewrite().unwrap();
            let reopened = NativePdfGraph::open(&output, NativeEditLimits::default()).unwrap();
            assert_eq!(reopened.notes(0).unwrap(), vec![added.clone()]);

            let page = reopened.objects[&reopened.pages[0]]
                .as_dictionary()
                .unwrap();
            let annots = page["Annots"].as_array().unwrap();
            assert_eq!(annots.len(), 2);
            let annotation = reopened.objects[&annots[0].as_reference().unwrap()]
                .as_dictionary()
                .unwrap();
            assert_eq!(annotation["Subtype"].as_name(), Some("Text"));
            assert!(!annotation.contains_key("Rect"));
            assert!(match annotation.get("Contents") {
                Some(PdfValue::Null) => explicit_null,
                None => !explicit_null,
                _ => false,
            });
        }
    }

    #[test]
    fn imported_pdfdocencoded_note_survives_alongside_added_note() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &payload);
        let original = PlainTextNote {
            page_index: 0,
            contents: "Café — €".into(),
            rect: [10, 20, 30, 40],
        };
        let added = PlainTextNote {
            page_index: 0,
            contents: "Друга примітка".into(),
            rect: [1, 2, 3, 4],
        };
        let encoded = b"Caf\xe9 \x84 \xa0";

        let mut graph = NativePdfGraph::open(&bytes, NativeEditLimits::default()).unwrap();
        graph.add_note(original.clone()).unwrap();
        let annotation = graph
            .objects
            .values_mut()
            .find_map(|value| match value {
                PdfValue::Dictionary(dict) => dict
                    .get_mut("Contents")
                    .filter(|value| matches!(value, PdfValue::String(_))),
                _ => None,
            })
            .unwrap();
        *annotation = PdfValue::String(encoded.to_vec());
        let imported = graph.rewrite().unwrap();

        let mut graph = NativePdfGraph::open(&imported, NativeEditLimits::default()).unwrap();
        assert_eq!(graph.notes(0).unwrap(), vec![original.clone()]);
        graph.add_note(added.clone()).unwrap();
        let output = graph.rewrite().unwrap();
        let reopened = NativePdfGraph::open(&output, NativeEditLimits::default()).unwrap();
        assert_eq!(reopened.notes(0).unwrap(), vec![original, added]);
        assert_eq!(graph.original_bytes(), imported);

        let page = reopened.objects[&reopened.pages[0]]
            .as_dictionary()
            .unwrap();
        let annots = page["Annots"].as_array().unwrap();
        let annotation = reopened.objects[&annots[0].as_reference().unwrap()]
            .as_dictionary()
            .unwrap();
        assert_eq!(annotation["Contents"].as_bytes(), Some(encoded.as_slice()));
    }

    #[test]
    fn original_generation_keys_and_reverse_edges_are_retained() {
        let payload = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let graph = NativePdfGraph::open(
            &super::super::tests::fixture("", &payload),
            NativeEditLimits::default(),
        )
        .unwrap();
        let custom = PdfObjectRef {
            object_number: 5,
            generation: 7,
        };
        let catalog = PdfObjectRef {
            object_number: 1,
            generation: 0,
        };
        assert!(graph.reverse[&custom].contains(&catalog));
        let dict = graph.objects[&custom].as_dictionary().unwrap();
        assert!(matches!(dict["T"], PdfValue::Boolean(true)));
        assert!(matches!(dict["F"], PdfValue::Boolean(false)));
        assert_eq!(dict["A#B"].as_name(), Some("Nÿ"));
    }
}

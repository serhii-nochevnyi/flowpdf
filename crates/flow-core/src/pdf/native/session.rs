use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::reader::{PdfObjectRef, PdfValue};
use super::super::scene::{NativePdfAnnotation, NativePdfNoteScene};
use super::graph::{NativeEditError, NativeEditLimits, NativePdfGraph, collect_refs, spend};
use super::notes::{self, ExpectedNote, NativeNoteCommand, NativeNoteId};

use super::objects::{self, NativeObjectCommand, NativeObjectPage, NativeObjectRefusal};
use super::text::{self, NativeTextCommand, NativeTextPage, NativeTextRefusal};

pub const NATIVE_REQUEST_HISTORY: usize = 128;
pub const NATIVE_MAX_NOTE_BYTES: usize = 16 * 1024;
const REQUEST_RECORD_BYTES: usize = 512;
const SESSION_BYTES: usize = 1024;
const OBJECT_ID_BYTES: usize = 128;

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeBinding {
    pub epoch: [u8; 16],
    pub session_id: u64,
    pub origin_hash: [u8; 32],
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeEditReceipt {
    pub binding: NativeBinding,
    pub request_id: u64,
    pub note: NativeNoteId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_island: Option<[u8; 32]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<[u8; 32]>,
    pub output_hash: [u8; 32],
    pub scene_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct NativeSnapshot {
    pub binding: NativeBinding,
    pub output_hash: [u8; 32],
    pub limits: NativeEditLimits,
    pub max_note_bytes: usize,
    pub request_history_limit: usize,
    pub consumed_request: u64,
    pub scene: NativePdfNoteScene,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRequestLookup {
    Accepted(NativeEditReceipt),
    Unseen,
    ReconciliationRequired,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NativeSessionError {
    #[error("{0}")]
    Graph(NativeEditError),
    #[error("native PDF session binding does not match")]
    BindingMismatch,
    #[error("native PDF revision does not match")]
    RevisionMismatch,
    #[error("native PDF request identity is invalid")]
    InvalidRequest,
    #[error("native PDF request identity was reused")]
    RequestConflict,
    #[error("native PDF request requires snapshot reconciliation")]
    ReconciliationRequired,
    #[error("native PDF edit target is unavailable")]
    TargetUnavailable,
    #[error("native PDF text replacement refused")]
    Text(NativeTextRefusal),
    #[error("native PDF object edit refused")]
    Object(NativeObjectRefusal),
    #[error("native PDF note is invalid")]
    InvalidNote,
    #[error("native PDF session budget exhausted")]
    Budget,
    #[error("native PDF rewrite postcondition failed")]
    Postcondition,
}

impl From<NativeEditError> for NativeSessionError {
    fn from(error: NativeEditError) -> Self {
        match error {
            NativeEditError::Budget => Self::Budget,
            error => Self::Graph(error),
        }
    }
}

impl From<NativeTextRefusal> for NativeSessionError {
    fn from(error: NativeTextRefusal) -> Self {
        Self::Text(error)
    }
}

impl From<NativeObjectRefusal> for NativeSessionError {
    fn from(error: NativeObjectRefusal) -> Self {
        Self::Object(error)
    }
}

impl NativeSessionError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Graph(error) => error.code(),
            Self::BindingMismatch => "FLOW_PDF_NATIVE_BINDING_MISMATCH",
            Self::RevisionMismatch => "FLOW_PDF_NATIVE_REVISION_MISMATCH",
            Self::InvalidRequest => "FLOW_PDF_NATIVE_REQUEST_INVALID",
            Self::RequestConflict => "FLOW_PDF_NATIVE_REQUEST_CONFLICT",
            Self::ReconciliationRequired => "FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED",
            Self::TargetUnavailable => "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE",
            Self::Text(error) => error.code(),
            Self::Object(error) => error.code(),
            Self::InvalidNote => "FLOW_PDF_NATIVE_NOTE_INVALID",
            Self::Budget => "FLOW_PDF_NATIVE_BUDGET_EXHAUSTED",
            Self::Postcondition => "FLOW_PDF_NATIVE_POSTCONDITION_FAILED",
        }
    }
}

#[derive(Clone, Copy)]
enum Command<'a> {
    Note(&'a NativeNoteCommand),
    Text(&'a NativeTextCommand),
    Object(&'a NativeObjectCommand),
}

enum Expected<'a> {
    Note(ExpectedNote<'a>),
    Text(text::ExpectedText<'a>),
    Object(objects::ExpectedObject),
}

struct RequestRecord {
    digest: [u8; 32],
    base_revision: u64,
    receipt: NativeEditReceipt,
}

pub struct NativePdfSession {
    origin: Option<Vec<u8>>,
    graph: NativePdfGraph,
    ids: BTreeMap<PdfObjectRef, NativeNoteId>,
    snapshot: NativeSnapshot,
    records: VecDeque<RequestRecord>,
    remaining_retained: usize,
    #[cfg(test)]
    fail_at: Option<FailurePoint>,
}

impl NativePdfSession {
    // @contract: Callers provide a fresh nonzero epoch for each runtime.
    pub fn open_scoped(
        bytes: &[u8],
        limits: NativeEditLimits,
        epoch: [u8; 16],
    ) -> Result<Self, NativeSessionError> {
        if epoch == [0; 16] {
            return Err(NativeSessionError::BindingMismatch);
        }
        let graph = NativePdfGraph::open(bytes, limits)?;
        let session_id = NEXT_SESSION
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| NativeSessionError::Budget)?;
        let origin_hash = *blake3::hash(bytes).as_bytes();
        let mut remaining = graph.remaining_retained();
        spend(&mut remaining, graph.objects.len() * OBJECT_ID_BYTES)?;
        let ids = initial_ids(&graph);
        let binding = NativeBinding {
            epoch,
            session_id,
            origin_hash,
            revision: 0,
        };
        let (scene, remaining_retained) = accepted_scene(&graph, &ids, origin_hash, binding, 0, 0)?;
        let snapshot = NativeSnapshot {
            binding,
            output_hash: origin_hash,
            limits,
            max_note_bytes: NATIVE_MAX_NOTE_BYTES,
            request_history_limit: NATIVE_REQUEST_HISTORY,
            consumed_request: 0,
            scene,
        };
        Ok(Self {
            origin: None,
            graph,
            ids,
            snapshot,
            records: VecDeque::new(),
            remaining_retained,
            #[cfg(test)]
            fail_at: None,
        })
    }

    pub fn snapshot(&self) -> &NativeSnapshot {
        &self.snapshot
    }

    pub fn origin_bytes(&self) -> &[u8] {
        self.origin
            .as_deref()
            .unwrap_or_else(|| self.graph.original_bytes())
    }

    pub fn output_bytes(&self) -> &[u8] {
        self.graph.original_bytes()
    }

    fn check_identity(
        &self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<(), NativeSessionError> {
        let current = self.snapshot.binding;
        if binding.origin_hash != current.origin_hash
            || binding.session_id != current.session_id
            || binding.epoch != current.epoch
        {
            return Err(NativeSessionError::BindingMismatch);
        }
        if request_id == 0 {
            return Err(NativeSessionError::InvalidRequest);
        }
        Ok(())
    }

    pub fn lookup(
        &self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<NativeRequestLookup, NativeSessionError> {
        self.check_identity(binding, request_id)?;
        if let Some(record) = self
            .records
            .iter()
            .find(|r| r.receipt.request_id == request_id)
        {
            if binding.revision != record.base_revision {
                return Err(NativeSessionError::RevisionMismatch);
            }
            return Ok(NativeRequestLookup::Accepted(record.receipt));
        }
        if request_id <= self.snapshot.consumed_request {
            return Ok(NativeRequestLookup::ReconciliationRequired);
        }
        if binding.revision != self.snapshot.binding.revision {
            return Err(NativeSessionError::RevisionMismatch);
        }
        Ok(NativeRequestLookup::Unseen)
    }

    pub fn apply(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        command: &NativeNoteCommand,
    ) -> Result<NativeEditReceipt, NativeSessionError> {
        self.apply_command(binding, request_id, Command::Note(command))
    }

    pub fn apply_text(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        command: &NativeTextCommand,
    ) -> Result<NativeEditReceipt, NativeSessionError> {
        self.apply_command(binding, request_id, Command::Text(command))
    }

    pub fn apply_object(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        command: &NativeObjectCommand,
    ) -> Result<NativeEditReceipt, NativeSessionError> {
        self.apply_command(binding, request_id, Command::Object(command))
    }

    pub fn verify_edit_projection(
        bytes: &[u8],
        limits: NativeEditLimits,
        binding: NativeBinding,
        claimed_text: &[NativeTextPage],
        claimed_objects: &[NativeObjectPage],
    ) -> Result<bool, NativeSessionError> {
        let graph = NativePdfGraph::open(bytes, limits)?;
        let ids = initial_ids(&graph);
        let hash = *blake3::hash(bytes).as_bytes();
        let (scene, _) = accepted_scene(&graph, &ids, hash, binding, 0, 0)?;
        Ok(scene.text_pages == claimed_text && scene.object_pages == claimed_objects)
    }

    pub fn verify_text_projection(
        bytes: &[u8],
        limits: NativeEditLimits,
        binding: NativeBinding,
        claimed: &[NativeTextPage],
    ) -> Result<bool, NativeSessionError> {
        let graph = NativePdfGraph::open(bytes, limits)?;
        let ids = initial_ids(&graph);
        let hash = *blake3::hash(bytes).as_bytes();
        let (scene, _) = accepted_scene(&graph, &ids, hash, binding, 0, 0)?;
        Ok(scene.text_pages == claimed)
    }

    fn apply_command(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        command: Command<'_>,
    ) -> Result<NativeEditReceipt, NativeSessionError> {
        self.check_identity(binding, request_id)?;
        if request_id <= self.snapshot.consumed_request {
            let record = self
                .records
                .iter()
                .find(|r| r.receipt.request_id == request_id)
                .ok_or(NativeSessionError::ReconciliationRequired)?;
            if binding.revision != record.base_revision {
                return Err(NativeSessionError::RevisionMismatch);
            }
            if command_digest(command) != Ok(record.digest) {
                return Err(NativeSessionError::RequestConflict);
            }
            return Ok(record.receipt);
        }
        if binding.revision != self.snapshot.binding.revision {
            return Err(NativeSessionError::RevisionMismatch);
        }
        let revision = binding
            .revision
            .checked_add(1)
            .ok_or(NativeSessionError::Budget)?;
        let record_count = (self.records.len() + 1).min(NATIVE_REQUEST_HISTORY);
        let content_cost = match command {
            Command::Note(
                NativeNoteCommand::Add { contents, .. } | NativeNoteCommand::Edit { contents, .. },
            ) => {
                if contents.len() > NATIVE_MAX_NOTE_BYTES {
                    return Err(NativeSessionError::InvalidNote);
                }
                contents.len() * 8
            }
            Command::Text(NativeTextCommand::ReplaceText { text, .. }) => {
                if text.len() > text::NATIVE_MAX_TEXT_BYTES {
                    return Err(NativeTextRefusal::Layout.into());
                }
                text.len() * 8
            }
            _ => 0,
        };
        let annotation_cost = match command {
            Command::Note(
                NativeNoteCommand::Add { page, .. } | NativeNoteCommand::Move { page, .. },
            ) => self
                .snapshot
                .scene
                .pages
                .get(page.0 as usize)
                .map_or(0, |page| {
                    page.annotations.len() * 2 * std::mem::size_of::<PdfValue>()
                }),
            _ => 0,
        };
        let mut remaining = self.remaining_retained;
        spend(
            &mut remaining,
            4096 + content_cost + annotation_cost + (self.ids.len() + 1) * OBJECT_ID_BYTES,
        )?;
        let digest = command_digest(command)?;
        // @security: Reserve output bytes separately from the writer workspace and reopen graph.
        let output_share = match command {
            Command::Text(_) | Command::Object(_) => remaining / 5,
            Command::Note(_) => remaining / 4,
        };
        let output_budget = self.snapshot.limits.max_output_bytes.min(output_share);
        spend(&mut remaining, output_budget)?;
        if remaining == 0 || output_budget == 0 {
            return Err(NativeSessionError::Budget);
        }
        let mut staged_limits = self.snapshot.limits;
        if binding.revision != 0 {
            staged_limits.max_source_bytes = staged_limits.max_output_bytes;
        }
        staged_limits.max_retained_bytes = remaining;
        staged_limits.max_output_bytes = output_budget;
        let mut staged = NativePdfGraph::open(self.output_bytes(), staged_limits)?;
        let mut ids = self.ids.clone();
        let mut staged_remaining = None;
        let expected = match command {
            Command::Note(command) => Expected::Note(notes::stage(
                &mut staged,
                &self.snapshot.scene.pages,
                &mut ids,
                command,
                revision,
            )?),
            Command::Text(command @ NativeTextCommand::ReplaceText { island, .. }) => {
                let page = self
                    .snapshot
                    .scene
                    .text_pages
                    .iter()
                    .find(|page| page.islands.iter().any(|target| target.id == *island))
                    .and_then(|page| self.snapshot.scene.pages.get(page.page.0 as usize))
                    .ok_or(NativeSessionError::TargetUnavailable)?;
                let expected = text::stage(
                    &mut staged,
                    page,
                    binding,
                    self.snapshot.output_hash,
                    command,
                )?;
                if let Some(object) = expected.new_object {
                    ids.insert(object, NativeNoteId(0));
                }
                Expected::Text(expected)
            }
            Command::Object(command) => {
                use NativeObjectCommand::*;
                let page_id = match command {
                    AddGraphic { page, .. } | AddImage { page, .. } => *page,
                    ReplaceGraphic { target, .. }
                    | ReplaceImage { target, .. }
                    | MoveObject { target, .. }
                    | RemoveObject { target } => target.page,
                };
                let page = self
                    .snapshot
                    .scene
                    .pages
                    .get(page_id.0 as usize)
                    .ok_or(NativeSessionError::TargetUnavailable)?;
                let capabilities = &self.snapshot.scene.object_pages[page_id.0 as usize];
                if let Some(reason) = capabilities.refusal {
                    return Err(reason.into());
                }
                if !capabilities.can_add && matches!(command, AddGraphic { .. } | AddImage { .. }) {
                    return Err(NativeSessionError::Budget);
                }
                let (expected, remaining) = objects::stage(
                    &mut staged,
                    page,
                    binding,
                    self.snapshot.output_hash,
                    command,
                )?;
                staged_remaining = Some(remaining);
                ids.retain(|reference, _| staged.objects.contains_key(reference));
                for reference in staged.objects.keys() {
                    ids.entry(*reference).or_insert(NativeNoteId(0));
                }
                Expected::Object(expected)
            }
        };
        #[cfg(test)]
        if self.fail_at == Some(FailurePoint::Serialize) {
            staged.limits.max_output_bytes = 1;
        }
        staged.limits.max_source_bytes = self.snapshot.limits.max_output_bytes;
        let output = staged.rewrite()?;
        #[cfg(test)]
        let output = if self.fail_at == Some(FailurePoint::Reopen) {
            Vec::new()
        } else {
            output
        };
        let mut reopen_limits = self.snapshot.limits;
        reopen_limits.max_source_bytes = reopen_limits.max_output_bytes;
        reopen_limits.max_total_decoded_bytes = staged.remaining_decoded();
        reopen_limits.max_retained_bytes = staged_remaining
            .unwrap_or_else(|| staged.remaining_retained())
            + (output_budget - output.len());
        if reopen_limits.max_retained_bytes == 0 {
            return Err(NativeSessionError::Budget);
        }
        let reopened = NativePdfGraph::open(&output, reopen_limits)?;
        let ids = preserve_and_remap(&staged, &reopened, &ids)?;
        let output_hash = *blake3::hash(&output).as_bytes();
        drop(staged);
        drop(output);
        let next_binding = NativeBinding {
            revision,
            ..binding
        };
        let (scene, scene_remaining) = accepted_scene(
            &reopened,
            &ids,
            output_hash,
            next_binding,
            self.origin_bytes().len(),
            record_count,
        )?;
        let (note, text_island, object) = match expected {
            Expected::Note(expected) => {
                verify_note(&scene, &expected)?;
                (expected.id, None, None)
            }
            Expected::Text(expected) => {
                let island = text::verify(&scene.text_pages, &expected)?;
                (NativeNoteId(0), Some(island), None)
            }
            Expected::Object(expected) => {
                let object =
                    objects::verify(&reopened, &scene.pages, &scene.object_pages, &expected)?;
                (NativeNoteId(0), None, object)
            }
        };
        let receipt = NativeEditReceipt {
            binding: next_binding,
            request_id,
            note,
            text_island,
            object,
            output_hash,
            scene_hash: scene.scene_hash,
        };
        self.records
            .try_reserve_exact(usize::from(self.records.len() < NATIVE_REQUEST_HISTORY))
            .map_err(|_| NativeSessionError::Budget)?;

        let previous = std::mem::replace(&mut self.graph, reopened);
        if self.origin.is_none() {
            self.origin = Some(previous.source);
        }
        self.remaining_retained = self.snapshot.limits.max_retained_bytes
            - reopen_limits.max_retained_bytes
            + scene_remaining;
        self.ids = ids;
        self.snapshot.binding = receipt.binding;
        self.snapshot.consumed_request = request_id;
        self.snapshot.output_hash = output_hash;
        self.snapshot.scene = scene;
        if self.records.len() == NATIVE_REQUEST_HISTORY {
            self.records.pop_front();
        }
        self.records.push_back(RequestRecord {
            digest,
            base_revision: binding.revision,
            receipt,
        });
        Ok(receipt)
    }
}

fn command_digest(command: Command<'_>) -> Result<[u8; 32], NativeSessionError> {
    let mut digest = blake3::Hasher::new();
    match command {
        Command::Note(command) => {
            if let NativeNoteCommand::Add { contents, .. }
            | NativeNoteCommand::Edit { contents, .. } = command
                && contents.len() > NATIVE_MAX_NOTE_BYTES
            {
                return Err(NativeSessionError::InvalidNote);
            }
            serde_json::to_writer(&mut digest, command)
        }
        Command::Text(command @ NativeTextCommand::ReplaceText { text, .. }) => {
            if text.len() > text::NATIVE_MAX_TEXT_BYTES {
                return Err(NativeTextRefusal::Layout.into());
            }
            serde_json::to_writer(&mut digest, command)
        }
        Command::Object(command) => {
            command.validate_size()?;
            serde_json::to_writer(&mut digest, command)
        }
    }
    .map_err(|_| NativeSessionError::InvalidRequest)?;
    Ok(*digest.finalize().as_bytes())
}

fn initial_ids(graph: &NativePdfGraph) -> BTreeMap<PdfObjectRef, NativeNoteId> {
    graph
        .objects
        .keys()
        .map(|&object| {
            (
                object,
                NativeNoteId(
                    (u64::from(object.object_number) << 16) | u64::from(object.generation),
                ),
            )
        })
        .collect()
}

fn accepted_scene(
    graph: &NativePdfGraph,
    ids: &BTreeMap<PdfObjectRef, NativeNoteId>,
    output_hash: [u8; 32],
    binding: NativeBinding,
    separate_origin: usize,
    request_count: usize,
) -> Result<(NativePdfNoteScene, usize), NativeSessionError> {
    let mut remaining = graph.remaining_retained();
    spend(&mut remaining, separate_origin)?;
    spend(
        &mut remaining,
        SESSION_BYTES + ids.len() * OBJECT_ID_BYTES + request_count * REQUEST_RECORD_BYTES,
    )?;
    let pages = notes::project(graph, ids, &mut remaining)?;
    let text_pages = text::project(graph, &pages, output_hash, binding, &mut remaining)?;
    let object_pages = objects::project(graph, &pages, output_hash, binding, &mut remaining)?;
    Ok((
        NativePdfNoteScene::new(output_hash, pages, text_pages, object_pages)?,
        remaining,
    ))
}

fn verify_note(
    scene: &NativePdfNoteScene,
    expected: &ExpectedNote<'_>,
) -> Result<(), NativeSessionError> {
    let mut matches = scene.pages.iter().flat_map(|page| {
        page.annotations
            .iter()
            .filter_map(move |annotation| match annotation {
                NativePdfAnnotation::PlainNote(note) if note.id == expected.id => {
                    Some((page.id, note.rect, note.contents.as_str()))
                }
                _ => None,
            })
    });
    if matches.next() == expected.location && matches.next().is_none() {
        Ok(())
    } else {
        Err(NativeSessionError::Postcondition)
    }
}

fn preserve_and_remap(
    staged: &NativePdfGraph,
    reopened: &NativePdfGraph,
    ids: &BTreeMap<PdfObjectRef, NativeNoteId>,
) -> Result<BTreeMap<PdfObjectRef, NativeNoteId>, NativeSessionError> {
    let mut work = staged.remaining_work();
    let mut remaining = reopened.remaining_retained();
    spend(&mut remaining, staged.objects.len() * 512)?;
    let mut pending = Vec::new();
    for (key, value) in &staged.trailer {
        if key != "Size" {
            collect_refs(value, &mut pending, &mut work, &mut remaining)?;
        }
    }
    let mut reachable = BTreeSet::new();
    while let Some(reference) = pending.pop() {
        spend(&mut work, 1)?;
        if reachable.insert(reference) {
            let value = staged
                .objects
                .get(&reference)
                .ok_or(NativeSessionError::Postcondition)?;
            collect_refs(value, &mut pending, &mut work, &mut remaining)?;
        }
    }
    let mapping: BTreeMap<_, _> = reachable
        .into_iter()
        .enumerate()
        .map(|(i, reference)| {
            (
                reference,
                PdfObjectRef {
                    object_number: (i + 1) as u32,
                    generation: 0,
                },
            )
        })
        .collect();
    if mapping.len() != staged.objects.len()
        || mapping.len() != reopened.objects.len()
        || reopened.trailer.get("Size").and_then(PdfValue::as_integer)
            != Some(mapping.len() as i64 + 1)
        || staged
            .pages
            .iter()
            .map(|page| mapping.get(page).copied())
            .ne(reopened.pages.iter().copied().map(Some))
        || staged.trailer.len() != reopened.trailer.len()
        || staged.trailer.iter().any(|(key, value)| {
            key != "Size"
                && !reopened
                    .trailer
                    .get(key)
                    .is_some_and(|rhs| equivalent(value, rhs, &mapping))
        })
    {
        return Err(NativeSessionError::Postcondition);
    }

    mapping
        .iter()
        .map(|(before, after)| {
            let value = &staged.objects[before];
            let reopened_value = reopened
                .objects
                .get(after)
                .ok_or(NativeSessionError::Postcondition)?;
            if !equivalent(value, reopened_value, &mapping) {
                return Err(NativeSessionError::Postcondition);
            }
            let id = ids.get(before).ok_or(NativeSessionError::Postcondition)?;
            Ok((*after, *id))
        })
        .collect()
}

fn same_dict(
    lhs: &BTreeMap<String, PdfValue>,
    rhs: &BTreeMap<String, PdfValue>,
    mapping: &BTreeMap<PdfObjectRef, PdfObjectRef>,
) -> bool {
    lhs.len() == rhs.len()
        && lhs
            .iter()
            .zip(rhs)
            .all(|((left, a), (right, b))| left == right && equivalent(a, b, mapping))
}

fn equivalent(
    lhs: &PdfValue,
    rhs: &PdfValue,
    mapping: &BTreeMap<PdfObjectRef, PdfObjectRef>,
) -> bool {
    match (lhs, rhs) {
        (PdfValue::Null, PdfValue::Null) => true,
        (PdfValue::Boolean(a), PdfValue::Boolean(b)) => a == b,
        (PdfValue::Integer(a), PdfValue::Integer(b)) => a == b,
        (PdfValue::ExactReal(a), PdfValue::ExactReal(b))
        | (PdfValue::Name(a), PdfValue::Name(b)) => a == b,
        (PdfValue::String(a), PdfValue::String(b)) => a == b,
        (PdfValue::Reference(a), PdfValue::Reference(b)) => mapping.get(a) == Some(b),
        (PdfValue::Array(a), PdfValue::Array(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(lhs, rhs)| equivalent(lhs, rhs, mapping))
        }
        (PdfValue::Dictionary(a), PdfValue::Dictionary(b)) => same_dict(a, b, mapping),
        (PdfValue::Stream(a), PdfValue::Stream(b)) => {
            a.encoded == b.encoded && a.data == b.data && same_dict(&a.dict, &b.dict, mapping)
        }
        _ => false,
    }
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum FailurePoint {
    Serialize,
    Reopen,
}

#[cfg(test)]
mod tests {
    use super::super::notes::NativePageId;
    use super::*;

    #[test]
    fn accepted_scene_shares_the_graph_admission_work_budget() {
        let encoded = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &encoded);
        let defaults = NativeEditLimits::default();
        let graph = NativePdfGraph::open(&bytes, defaults).unwrap();
        let limits = NativeEditLimits {
            max_work: defaults.max_work - graph.remaining_work() + 10,
            ..defaults
        };
        assert!(NativePdfGraph::open(&bytes, limits).is_ok());
        assert!(matches!(
            NativePdfSession::open_scoped(&bytes, limits, [1; 16]),
            Err(NativeSessionError::Budget)
        ));
    }

    #[test]
    fn accepted_scene_charges_note_decoding_to_the_work_budget() {
        let encoded = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &encoded);
        let defaults = NativeEditLimits::default();
        let mut session = NativePdfSession::open_scoped(&bytes, defaults, [1; 16]).unwrap();
        session
            .apply(
                session.snapshot().binding,
                1,
                &NativeNoteCommand::Add {
                    page: NativePageId(0),
                    rect: [1, 2, 3, 4],
                    contents: "Ї".repeat(1024),
                },
            )
            .unwrap();

        let output = session.output_bytes();
        let graph = NativePdfGraph::open(output, defaults).unwrap();
        let limits = NativeEditLimits {
            max_work: defaults.max_work - graph.remaining_work() + 1024,
            ..defaults
        };
        assert!(NativePdfGraph::open(output, limits).is_ok());
        assert!(matches!(
            NativePdfSession::open_scoped(output, limits, [1; 16]),
            Err(NativeSessionError::Budget)
        ));
    }

    #[test]
    fn serialization_and_reopen_failures_leave_accepted_state_untouched() {
        let encoded = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &encoded);
        let mut session =
            NativePdfSession::open_scoped(&bytes, NativeEditLimits::default(), [1; 16]).unwrap();
        let add = NativeNoteCommand::Add {
            page: NativePageId(0),
            rect: [1, 2, 3, 4],
            contents: "test".into(),
        };
        let first = session.apply(session.snapshot().binding, 1, &add).unwrap();
        let accepted = session.output_bytes().to_vec();
        let scene = session.snapshot().scene.clone();
        for point in [FailurePoint::Serialize, FailurePoint::Reopen] {
            session.fail_at = Some(point);
            assert!(session.apply(first.binding, 2, &add).is_err());
            assert_eq!(session.snapshot().binding, first.binding);
            assert_eq!(session.snapshot().scene, scene);
            assert_eq!(session.output_bytes(), accepted);
            assert_eq!(session.origin_bytes(), bytes);
            assert_eq!(
                session.lookup(first.binding, 2),
                Ok(NativeRequestLookup::Unseen)
            );
        }
        session.fail_at = None;
        assert_eq!(
            session
                .apply(first.binding, 2, &add)
                .unwrap()
                .binding
                .revision,
            2
        );
    }
}

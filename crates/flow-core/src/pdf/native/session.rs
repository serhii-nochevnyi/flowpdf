use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::reader::{PdfObjectRef, PdfValue};
use super::super::scene::{NativePdfAnnotation, NativePdfNoteScene};
use super::graph::{NativeEditError, NativeEditLimits, NativePdfGraph, collect_refs, spend};
use super::notes::{self, ExpectedNote, NativeNoteCommand, NativeNoteId};

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
            Self::InvalidNote => "FLOW_PDF_NATIVE_NOTE_INVALID",
            Self::Budget => "FLOW_PDF_NATIVE_BUDGET_EXHAUSTED",
            Self::Postcondition => "FLOW_PDF_NATIVE_POSTCONDITION_FAILED",
        }
    }
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
    #[cfg(test)]
    fail_at: Option<FailurePoint>,
}

impl NativePdfSession {
    pub fn open(bytes: &[u8], limits: NativeEditLimits) -> Result<Self, NativeSessionError> {
        Self::open_scoped(bytes, limits, [0; 16])
    }

    pub fn open_scoped(
        bytes: &[u8],
        limits: NativeEditLimits,
        epoch: [u8; 16],
    ) -> Result<Self, NativeSessionError> {
        let graph = NativePdfGraph::open(bytes, limits)?;
        let session_id = NEXT_SESSION
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| NativeSessionError::Budget)?;
        let origin_hash = *blake3::hash(bytes).as_bytes();
        let ids = graph
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
            .collect();
        let scene = accepted_scene(&graph, &ids, origin_hash, 0, 0)?;
        let snapshot = NativeSnapshot {
            binding: NativeBinding {
                epoch,
                session_id,
                origin_hash,
                revision: 0,
            },
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
        let digest = command_digest(command)?;
        let revision = binding
            .revision
            .checked_add(1)
            .ok_or(NativeSessionError::Budget)?;
        let record_count = (self.records.len() + 1).min(NATIVE_REQUEST_HISTORY);
        let mut reopen_limits = self.snapshot.limits;
        reopen_limits.max_source_bytes = reopen_limits.max_output_bytes;
        let mut staged = NativePdfGraph::open(self.output_bytes(), self.graph.limits)?;
        staged.limits.max_source_bytes = reopen_limits.max_source_bytes;
        let mut remaining = staged.remaining_retained();
        let content_cost = match command {
            NativeNoteCommand::Add { contents, .. } | NativeNoteCommand::Edit { contents, .. } => {
                contents.len() * 4
            }
            _ => 0,
        };
        spend(
            &mut remaining,
            4096 + content_cost + self.ids.len() * OBJECT_ID_BYTES,
        )?;
        let mut ids = self.ids.clone();
        let expected = notes::stage(
            &mut staged,
            &self.snapshot.scene.pages,
            &mut ids,
            command,
            revision,
        )?;
        #[cfg(test)]
        if self.fail_at == Some(FailurePoint::Serialize) {
            staged.limits.max_output_bytes = 1;
        }
        let output = staged.rewrite()?;
        #[cfg(test)]
        let output = if self.fail_at == Some(FailurePoint::Reopen) {
            Vec::new()
        } else {
            output
        };
        let reopened = NativePdfGraph::open(&output, reopen_limits)?;
        let ids = preserve_and_remap(&staged, &reopened, &ids)?;
        let output_hash = *blake3::hash(&output).as_bytes();
        let scene = accepted_scene(
            &reopened,
            &ids,
            output_hash,
            self.origin_bytes().len(),
            record_count,
        )?;
        verify_note(&scene, &expected)?;
        let receipt = NativeEditReceipt {
            binding: NativeBinding {
                revision,
                ..binding
            },
            request_id,
            note: expected.id,
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

fn command_digest(command: &NativeNoteCommand) -> Result<[u8; 32], NativeSessionError> {
    if let NativeNoteCommand::Add { contents, .. } | NativeNoteCommand::Edit { contents, .. } =
        command
        && contents.len() > NATIVE_MAX_NOTE_BYTES
    {
        return Err(NativeSessionError::InvalidNote);
    }
    let encoded = serde_json::to_vec(command).map_err(|_| NativeSessionError::InvalidRequest)?;
    Ok(*blake3::hash(&encoded).as_bytes())
}

fn accepted_scene(
    graph: &NativePdfGraph,
    ids: &BTreeMap<PdfObjectRef, NativeNoteId>,
    output_hash: [u8; 32],
    separate_origin: usize,
    request_count: usize,
) -> Result<NativePdfNoteScene, NativeSessionError> {
    let mut remaining = graph.remaining_retained();
    spend(&mut remaining, separate_origin)?;
    spend(
        &mut remaining,
        SESSION_BYTES + ids.len() * OBJECT_ID_BYTES + request_count * REQUEST_RECORD_BYTES,
    )?;
    let pages = notes::project(graph, ids, &mut remaining)?;
    NativePdfNoteScene::new(output_hash, pages)
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
    let mut remaining = staged.remaining_retained();
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
            NativePdfSession::open(&bytes, limits),
            Err(NativeSessionError::Budget)
        ));
    }

    #[test]
    fn accepted_scene_charges_note_decoding_to_the_work_budget() {
        let encoded = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &encoded);
        let defaults = NativeEditLimits::default();
        let mut session = NativePdfSession::open(&bytes, defaults).unwrap();
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
            NativePdfSession::open(output, limits),
            Err(NativeSessionError::Budget)
        ));
    }

    #[test]
    fn serialization_and_reopen_failures_leave_accepted_state_untouched() {
        let encoded = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &encoded);
        let mut session = NativePdfSession::open(&bytes, NativeEditLimits::default()).unwrap();
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

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::reader::{PdfObjectRef, PdfReadError, PdfValue};
use super::super::scene::{NativePdfAnnotation, NativePdfNotePage, NativePdfNoteScene};
use super::candidate::{Candidate, NativeRedactionCandidateStatus, NativeRedactionStage};
use super::forms::{self, NativeFieldCommand, NativeFieldRefusal, NativeFieldView};
use super::graph::{NativeEditError, NativeEditLimits, NativePdfGraph, spend};
use super::notes::{self, ExpectedNote, NativeNoteCommand, NativeNoteId};

use super::objects::{self, NativeObjectCommand, NativeObjectPage, NativeObjectRefusal};
use super::pages::{self, NativePageCommand, NativePageState};
use super::redaction;
use super::text::{self, NativeTextCommand, NativeTextPage, NativeTextRefusal};
use super::verifier::{
    NativeRedactionContext, NativeRedactionMark, NativeVerificationFailure,
    VersionedNativeRedactionEvidence, Workspace,
};
use super::writer::{Rewrite, rewrite_with_workspace};

pub const NATIVE_REQUEST_HISTORY: usize = 128;
pub const NATIVE_MAX_NOTE_BYTES: usize = 16 * 1024;
const REQUEST_RECORD_BYTES: usize = 512;
const SESSION_BYTES: usize = 1024 + std::mem::size_of::<Option<NativeRedactionMarkReceipt>>();
const FIRST_ADDED_NOTE_ID: u64 = 1 << 48;
const FIRST_PAGE_OBJECT_ID: u64 = 1 << 52;
const PAGE_OBJECT_ID_LIMIT: u64 = 1 << 53;
pub(super) const OBJECT_ID_BYTES: usize = 128;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_page: Option<u64>,
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
    pub page_state: NativePageState,
    pub fields: Vec<NativeFieldView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "Keep the accepted receipt inline so request lookup stays Copy and needs no heap allocation"
)]
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
    #[error("native PDF page geometry cannot be verified for this operation")]
    PageGeometry,
    #[error("native PDF must retain at least one page")]
    FinalPage,
    #[error("native PDF text replacement refused")]
    Text(NativeTextRefusal),
    #[error("native PDF object edit refused")]
    Object(NativeObjectRefusal),
    #[error("native PDF field edit refused")]
    Field(NativeFieldRefusal),
    #[error("native PDF note is invalid")]
    InvalidNote,
    #[error("native PDF session budget exhausted")]
    Budget,
    #[error("native PDF rewrite postcondition failed")]
    Postcondition,
    #[error("native PDF redaction mark is invalid")]
    InvalidRedactionMark,
    #[error("native PDF redaction candidate is unavailable")]
    RedactionCandidateUnavailable,
    #[error("native PDF redaction candidate lacks complete verification")]
    RedactionUnverified,
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

impl From<NativeFieldRefusal> for NativeSessionError {
    fn from(error: NativeFieldRefusal) -> Self {
        Self::Field(error)
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
            Self::FinalPage => "FLOW_PDF_NATIVE_FINAL_PAGE",
            Self::TargetUnavailable => "FLOW_PDF_NATIVE_TARGET_UNAVAILABLE",
            Self::PageGeometry => "FLOW_PDF_NATIVE_PAGE_GEOMETRY",
            Self::Text(error) => error.code(),
            Self::Object(error) => error.code(),
            Self::Field(error) => error.code(),
            Self::InvalidNote => "FLOW_PDF_NATIVE_NOTE_INVALID",
            Self::Budget => "FLOW_PDF_NATIVE_BUDGET_EXHAUSTED",
            Self::Postcondition => "FLOW_PDF_NATIVE_POSTCONDITION_FAILED",
            Self::InvalidRedactionMark => "FLOW_PDF_NATIVE_REDACTION_MARK_INVALID",
            Self::RedactionCandidateUnavailable => {
                "FLOW_PDF_NATIVE_REDACTION_CANDIDATE_UNAVAILABLE"
            }
            Self::RedactionUnverified => "FLOW_PDF_NATIVE_REDACTION_UNVERIFIED",
        }
    }
}

#[derive(Clone, Copy)]
enum Command<'a> {
    Note(&'a NativeNoteCommand),
    Text(&'a NativeTextCommand),
    Object(&'a NativeObjectCommand),
    Page(NativePageCommand),
    Field(&'a NativeFieldCommand),
}

enum Expected<'a> {
    Note(ExpectedNote<'a>),
    Text(text::ExpectedText<'a>),
    Object(objects::ExpectedObject),
    Page(u64),
    Field(forms::ExpectedField),
}

struct RequestRecord {
    digest: Option<[u8; 32]>,
    base_revision: u64,
    receipt: NativeEditReceipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRedactionMarkAction {
    Add { page_id: u64, rect: [i64; 4] },
    Remove { page_id: u64, rect: [i64; 4] },
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeRedactionMarkReceipt {
    pub request_id: u64,
    pub binding: NativeBinding,
    pub accepted_hash: [u8; 32],
    pub action: NativeRedactionMarkAction,
    removed_geometry: Option<super::NativeRedactionGeometry>,
}

pub struct NativePdfSession {
    origin: Option<Vec<u8>>,
    graph: NativePdfGraph,
    ids: BTreeMap<PdfObjectRef, NativeNoteId>,
    snapshot: NativeSnapshot,
    records: VecDeque<RequestRecord>,
    remaining_retained: usize,
    // @invariant: Capture the projection balance before transaction-only postconditions.
    projection_work_left: usize,
    next_page_object_id: u64,
    redaction_admission: OnceLock<Result<(), NativeSessionError>>,
    redaction_marks: Vec<NativeRedactionMark>,
    redaction_marks_binding: Option<NativeBinding>,
    last_redaction_request: u64,
    last_redaction_mark: Option<NativeRedactionMarkReceipt>,
    candidate: Option<Candidate>,
    accepted_redaction_evidence: Option<VersionedNativeRedactionEvidence>,
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
        let binding = NativeBinding {
            epoch,
            session_id,
            origin_hash,
            revision: 0,
        };
        let (ids, scene, mut remaining, mut work) =
            initial_projection(&graph, origin_hash, binding, None)?;
        let fields = forms::project(&graph, &scene.pages, binding, &mut remaining, &mut work)?;
        let page_state = page_state(&graph, &ids, None, &scene, &mut remaining, &mut work)?;
        let snapshot = NativeSnapshot {
            binding,
            output_hash: origin_hash,
            limits,
            max_note_bytes: NATIVE_MAX_NOTE_BYTES,
            request_history_limit: NATIVE_REQUEST_HISTORY,
            consumed_request: 0,
            scene,
            page_state,
            fields,
        };
        Ok(Self {
            origin: None,
            graph,
            ids,
            snapshot,
            records: VecDeque::new(),
            remaining_retained: remaining,
            projection_work_left: work,
            next_page_object_id: FIRST_PAGE_OBJECT_ID,
            redaction_admission: OnceLock::new(),
            redaction_marks: Vec::new(),
            redaction_marks_binding: None,
            last_redaction_request: 0,
            last_redaction_mark: None,
            candidate: None,
            accepted_redaction_evidence: None,
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

    pub fn accepted_page_raster(
        &self,
        binding: NativeBinding,
        output_hash: [u8; 32],
        page_id: u64,
    ) -> Result<super::NativePageRaster, NativeSessionError> {
        self.check_current_binding(binding)?;
        if self.snapshot.output_hash != output_hash {
            return Err(NativeSessionError::RevisionMismatch);
        }
        let index = self
            .snapshot
            .page_state
            .order
            .iter()
            .position(|&id| id == page_id)
            .ok_or(NativeSessionError::TargetUnavailable)?;
        let geometry = redaction::geometry(&self.graph, index)
            .map_err(|_| NativeSessionError::TargetUnavailable)?;
        let workspace = Workspace::new(self.redaction_workspace()?);
        super::verify_raster::accepted_page(self.output_bytes(), index, geometry, &workspace)
            .map_err(|error| match error {
                NativeVerificationFailure::Budget => NativeSessionError::Budget,
                _ => NativeSessionError::TargetUnavailable,
            })
    }

    pub fn redaction_admitted(&self) -> bool {
        self.check_redaction_admission().is_ok()
    }

    pub fn check_redaction_admission(&self) -> Result<(), NativeSessionError> {
        if !self.redaction_marks.is_empty()
            && self.redaction_marks_binding != Some(self.snapshot.binding)
        {
            return Err(NativeSessionError::RevisionMismatch);
        }
        self.redaction_admission
            .get_or_init(|| {
                redaction::check_document(
                    &self.graph,
                    &self.snapshot.page_state.order,
                    Some(self.origin_bytes().len()),
                    self.redaction_workspace()?,
                    self.projection_work_left,
                )
                .map_err(|error| redaction_graph_error(error.into()))
            })
            .clone()
    }

    pub fn redaction_mark_geometry(
        &self,
        page_id: u64,
    ) -> Result<super::NativeRedactionGeometry, NativeSessionError> {
        let index = self
            .snapshot
            .page_state
            .order
            .iter()
            .position(|&id| id == page_id)
            .ok_or(NativeSessionError::InvalidRedactionMark)?;
        Ok(redaction::geometry(&self.graph, index)?)
    }

    pub fn redaction_marks(&self) -> &[NativeRedactionMark] {
        &self.redaction_marks
    }

    pub fn redaction_marks_binding(&self) -> Option<NativeBinding> {
        self.redaction_marks_binding
    }

    fn check_current_binding(&self, binding: NativeBinding) -> Result<(), NativeSessionError> {
        self.check_identity(binding, 1)?;
        if binding != self.snapshot.binding {
            return Err(NativeSessionError::RevisionMismatch);
        }
        Ok(())
    }

    pub fn last_redaction_mark(&self) -> Option<NativeRedactionMarkReceipt> {
        self.last_redaction_mark
    }

    fn mark_request_recorded(&self, request_id: u64) -> bool {
        self.last_redaction_mark
            .is_some_and(|last| request_id <= last.request_id)
    }

    pub fn is_redaction_mark_replay(
        &self,
        binding: NativeBinding,
        request_id: u64,
        accepted_hash: [u8; 32],
        action: NativeRedactionMarkAction,
    ) -> Result<bool, NativeSessionError> {
        self.check_identity(binding, request_id)?;
        if binding != self.snapshot.binding {
            return Err(NativeSessionError::RevisionMismatch);
        }
        if accepted_hash != self.snapshot.output_hash {
            return Err(NativeSessionError::BindingMismatch);
        }
        if self.last_redaction_mark.is_some_and(|last| {
            last.request_id == request_id
                && last.binding == binding
                && last.accepted_hash == accepted_hash
                && last.action == action
        }) {
            return Ok(true);
        }
        if request_id <= self.snapshot.consumed_request || self.mark_request_recorded(request_id) {
            return Err(NativeSessionError::InvalidRequest);
        }
        Ok(false)
    }

    fn begin_redaction_mark(
        &self,
        binding: NativeBinding,
        request_id: u64,
        action: NativeRedactionMarkAction,
    ) -> Result<bool, NativeSessionError> {
        let replay =
            self.is_redaction_mark_replay(binding, request_id, self.snapshot.output_hash, action)?;
        if !replay && request_id <= self.last_redaction_request {
            return Err(NativeSessionError::ReconciliationRequired);
        }
        Ok(replay)
    }

    fn record_redaction_mark(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        action: NativeRedactionMarkAction,
        removed_geometry: Option<super::NativeRedactionGeometry>,
    ) {
        self.last_redaction_mark = Some(NativeRedactionMarkReceipt {
            request_id,
            binding,
            accepted_hash: self.snapshot.output_hash,
            action,
            removed_geometry,
        });
    }

    pub fn acknowledge_existing_redaction_mark(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        page_id: u64,
        rect: [i64; 4],
    ) -> Result<bool, NativeSessionError> {
        let Some(mark) = self
            .redaction_marks
            .iter()
            .find(|mark| mark.page_id == page_id && mark.rect == rect)
            .copied()
        else {
            return Ok(false);
        };
        self.add_redaction_mark(binding, request_id, mark)?;
        Ok(true)
    }

    pub fn add_redaction_mark(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        mark: NativeRedactionMark,
    ) -> Result<(), NativeSessionError> {
        let action = NativeRedactionMarkAction::Add {
            page_id: mark.page_id,
            rect: mark.rect,
        };
        let replay = self.begin_redaction_mark(binding, request_id, action)?;
        if self
            .redaction_marks_binding
            .is_some_and(|base| base != binding)
        {
            return Err(NativeSessionError::RevisionMismatch);
        }
        if self.redaction_mark_geometry(mark.page_id)? != mark.geometry
            || !redaction::valid_rect(mark.rect)
            || !redaction::contains(mark.geometry.crop_box, mark.rect)
        {
            return Err(NativeSessionError::InvalidRedactionMark);
        }
        if replay {
            return Ok(());
        }
        if self.redaction_marks.contains(&mark) {
            self.record_redaction_mark(binding, request_id, action, None);
            return Ok(());
        }
        self.check_redaction_admission()?;
        if self.redaction_marks.len() == redaction::MAX_MARKS {
            return Err(NativeSessionError::Budget);
        }
        let mut remaining = self.remaining_retained;
        spend(
            &mut remaining,
            self.candidate.as_ref().map_or(0, Candidate::retained),
        )?;
        let mark_bytes = self.mark_bytes();
        let new_capacity = self.redaction_marks.len() + 1;
        let peak_mark_bytes = if new_capacity > self.redaction_marks.capacity() {
            new_capacity
                .checked_mul(std::mem::size_of::<NativeRedactionMark>())
                .and_then(|new_bytes| mark_bytes.checked_add(new_bytes))
                .ok_or(NativeSessionError::Budget)?
        } else {
            mark_bytes
        };
        spend(&mut remaining, peak_mark_bytes)?;
        self.redaction_marks
            .try_reserve_exact(1)
            .map_err(|_| NativeSessionError::Budget)?;
        self.redaction_marks.push(mark);
        self.redaction_marks_binding = Some(binding);
        self.candidate = None;
        self.record_redaction_mark(binding, request_id, action, None);
        self.redaction_admission.take();
        Ok(())
    }

    pub fn remove_redaction_mark(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        mark: NativeRedactionMark,
    ) -> Result<(), NativeSessionError> {
        let action = NativeRedactionMarkAction::Remove {
            page_id: mark.page_id,
            rect: mark.rect,
        };
        if self.begin_redaction_mark(binding, request_id, action)? {
            if self
                .last_redaction_mark
                .and_then(|last| last.removed_geometry)
                != Some(mark.geometry)
            {
                return Err(NativeSessionError::InvalidRedactionMark);
            }
            return Ok(());
        }
        let index = self
            .redaction_marks
            .iter()
            .position(|value| *value == mark)
            .ok_or(NativeSessionError::InvalidRedactionMark)?;
        self.candidate = None;
        self.redaction_marks.remove(index);
        if self.redaction_marks.is_empty() {
            self.reset_marks();
        }
        self.record_redaction_mark(binding, request_id, action, Some(mark.geometry));
        self.redaction_admission.take();
        Ok(())
    }

    pub fn clear_redaction_marks(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<(), NativeSessionError> {
        let action = NativeRedactionMarkAction::Clear;
        if self.begin_redaction_mark(binding, request_id, action)? {
            return Ok(());
        }
        self.candidate = None;
        self.reset_marks();
        self.record_redaction_mark(binding, request_id, action, None);
        self.redaction_admission.take();
        Ok(())
    }

    fn reset_marks(&mut self) {
        self.redaction_marks = Vec::new();
        self.redaction_marks_binding = None;
    }

    fn mark_bytes(&self) -> usize {
        self.redaction_marks.capacity() * std::mem::size_of::<NativeRedactionMark>()
    }

    fn redaction_workspace(&self) -> Result<usize, NativeSessionError> {
        let mut remaining = self.remaining_retained;
        spend(
            &mut remaining,
            self.mark_bytes() + self.candidate.as_ref().map_or(0, Candidate::retained),
        )?;
        Ok(remaining)
    }

    pub fn redaction_candidate_status(&self) -> Option<NativeRedactionCandidateStatus<'_>> {
        self.candidate.as_ref().map(Candidate::status)
    }

    pub fn accepted_redaction_evidence(&self) -> Option<&VersionedNativeRedactionEvidence> {
        self.accepted_redaction_evidence.as_ref()
    }

    pub fn redacted_output_bytes(&self) -> Option<&[u8]> {
        self.accepted_redaction_evidence
            .as_ref()
            .map(|_| self.output_bytes())
    }

    fn redaction_context(&self, request_id: u64) -> NativeRedactionContext<'_> {
        NativeRedactionContext {
            binding: self.snapshot.binding,
            request_id,
            accepted_hash: self.snapshot.output_hash,
            page_order: &self.snapshot.page_state.order,
        }
    }

    fn redaction_receipt(
        &self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<Option<NativeEditReceipt>, NativeSessionError> {
        self.check_identity(binding, request_id)?;
        if request_id <= self.snapshot.consumed_request {
            let record = self
                .records
                .iter()
                .find(|record| record.receipt.request_id == request_id)
                .ok_or(NativeSessionError::ReconciliationRequired)?;
            if record.base_revision != binding.revision {
                return Err(NativeSessionError::RevisionMismatch);
            }
            if record.digest.is_some() {
                return Err(NativeSessionError::RequestConflict);
            }
            if record.receipt.binding != self.snapshot.binding
                || record.receipt.output_hash != self.snapshot.output_hash
            {
                return Err(NativeSessionError::RevisionMismatch);
            }
            return Ok(Some(record.receipt));
        }
        self.check_current_binding(binding)?;
        Ok(None)
    }

    fn check_candidate(
        &self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<(), NativeSessionError> {
        self.check_identity(binding, request_id)?;
        self.check_current_binding(binding)?;
        let status = self
            .redaction_candidate_status()
            .ok_or(NativeSessionError::RedactionCandidateUnavailable)?;
        if status.identity.request_id != request_id || status.identity.binding != binding {
            return Err(NativeSessionError::RequestConflict);
        }
        Ok(())
    }

    pub fn stage_redaction(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<NativeRedactionStage, NativeSessionError> {
        if let Some(receipt) = self.redaction_receipt(binding, request_id)? {
            return Ok(NativeRedactionStage::Accepted(receipt));
        }
        if let Some(status) = self.redaction_candidate_status() {
            self.check_candidate(binding, request_id)?;
            return Ok(NativeRedactionStage::Pending(status.identity));
        }
        if self.mark_request_recorded(request_id) {
            return Err(NativeSessionError::InvalidRequest);
        }
        if request_id <= self.last_redaction_request {
            return Err(NativeSessionError::ReconciliationRequired);
        }
        let mut limits = accepted_input_limits(self.snapshot.limits, binding);
        let mut preflight_work = redaction::ScanWork::new(limits.max_work);
        redaction::check_input_budget(
            &self.graph,
            Some(self.origin_bytes().len()),
            &mut preflight_work,
        )?;
        if self.redaction_marks_binding != Some(binding) {
            return Err(NativeSessionError::InvalidRedactionMark);
        }
        let max_input = super::verify_structure::MAX_INPUT;
        let mut remaining = self.remaining_retained;
        spend(
            &mut remaining,
            self.mark_bytes() + self.ids.len() * OBJECT_ID_BYTES,
        )?;
        let workspace = Workspace::new(remaining);
        let mut page_bytes = workspace.scope();
        let affected = page_bytes
            .collect_affected_pages(&self.snapshot.page_state.order, &self.redaction_marks)
            .map_err(|_| NativeSessionError::Budget)?;
        let remaining = workspace.remaining();
        let output_budget = limits.max_output_bytes.min(max_input).min(remaining / 5);
        limits.max_work = preflight_work.remaining;
        if output_budget == 0 || limits.max_work == 0 {
            return Err(NativeSessionError::Budget);
        }
        limits.max_output_bytes = output_budget;
        limits.max_retained_bytes = remaining - output_budget;
        let mut staged =
            NativePdfGraph::open(self.output_bytes(), limits).map_err(|error| match error {
                NativeEditError::Reader(PdfReadError::TokenLimit) => NativeSessionError::Budget,
                error => redaction_graph_error(error),
            })?;
        redaction::scrub(
            &mut staged,
            &self.snapshot.page_state.order,
            &self.redaction_marks,
        )?;
        staged.limits.max_source_bytes = output_budget;
        let RemappedRewrite { output, ids, .. } = rewrite_and_remap(
            &staged,
            staged.remaining_retained() + output_budget,
            &self.ids,
        )?;
        let candidate = Candidate::new(output, ids, self.redaction_context(request_id), affected);
        let mut remaining = self.remaining_retained;
        spend(&mut remaining, self.mark_bytes() + candidate.retained())?;
        let result = NativeRedactionStage::Pending(candidate.status().identity);
        self.candidate = Some(candidate);
        self.last_redaction_request = request_id;
        self.redaction_admission.take();
        Ok(result)
    }

    pub fn verify_redaction(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<&VersionedNativeRedactionEvidence, NativeSessionError> {
        self.check_candidate(binding, request_id)?;
        let remaining = self.redaction_workspace()?;
        let accepted = self.graph.original_bytes();
        let origin = self.origin.as_deref().unwrap_or(accepted);
        let context = NativeRedactionContext {
            binding: self.snapshot.binding,
            request_id,
            accepted_hash: self.snapshot.output_hash,
            page_order: &self.snapshot.page_state.order,
        };
        let candidate = self
            .candidate
            .as_mut()
            .ok_or(NativeSessionError::RedactionCandidateUnavailable)?;
        if candidate.status().evidence.is_none() {
            self.redaction_admission.take();
        }
        Ok(candidate.verify(origin, accepted, context, &self.redaction_marks, remaining))
    }

    pub fn cancel_redaction(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<(), NativeSessionError> {
        self.check_candidate(binding, request_id)?;
        self.candidate = None;
        self.redaction_admission.take();
        Ok(())
    }

    pub fn publish_redaction(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<NativeEditReceipt, NativeSessionError> {
        if let Some(receipt) = self.redaction_receipt(binding, request_id)? {
            return Ok(receipt);
        }
        self.check_candidate(binding, request_id)?;
        let context = self.redaction_context(request_id);
        let candidate = self
            .candidate
            .as_ref()
            .ok_or(NativeSessionError::RedactionCandidateUnavailable)?;
        let bytes = candidate
            .verified_bytes(context, &self.redaction_marks)
            .ok_or(NativeSessionError::RedactionUnverified)?;
        let output_hash = candidate.status().identity.candidate_hash;
        let revision = binding
            .revision
            .checked_add(1)
            .filter(|&revision| revision < FIRST_PAGE_OBJECT_ID - FIRST_ADDED_NOTE_ID)
            .ok_or(NativeSessionError::Budget)?;
        let next_binding = NativeBinding {
            revision,
            ..binding
        };
        let mut remaining = self.remaining_retained;
        spend(&mut remaining, self.mark_bytes() + candidate.retained())?;
        let mut limits = self.snapshot.limits;
        limits.max_source_bytes = limits.max_output_bytes;
        limits.max_retained_bytes = remaining;
        let mut reopened = NativePdfGraph::open(bytes, limits)?;
        let record_count = (self.records.len() + 1).min(NATIVE_REQUEST_HISTORY);
        let (scene, mut scene_remaining, mut work) = accepted_scene(
            &reopened,
            candidate.ids(),
            output_hash,
            next_binding,
            self.origin_bytes().len(),
            record_count,
            reopened.remaining_retained(),
        )?;
        let fields = forms::project(
            &reopened,
            &scene.pages,
            next_binding,
            &mut scene_remaining,
            &mut work,
        )?;
        let state = page_state(
            &reopened,
            candidate.ids(),
            self.snapshot.page_state.selected,
            &scene,
            &mut scene_remaining,
            &mut work,
        )?;
        let projection_work_left = work;
        // @invariant: Verified bytes preserve geometry and the accepted order proves unique IDs.
        spend(&mut work, state.order.len())?;
        if !state.has_shape_for_count(scene.pages.len())
            || state.order != self.snapshot.page_state.order
        {
            return Err(NativeSessionError::Postcondition);
        }
        let receipt = NativeEditReceipt {
            binding: next_binding,
            request_id,
            note: NativeNoteId(0),
            text_island: None,
            object: None,
            selected_page: None,
            output_hash,
            scene_hash: scene.scene_hash,
        };
        self.records
            .try_reserve_exact(usize::from(self.records.len() < NATIVE_REQUEST_HISTORY))
            .map_err(|_| NativeSessionError::Budget)?;

        let context = NativeRedactionContext {
            binding: self.snapshot.binding,
            request_id,
            accepted_hash: self.snapshot.output_hash,
            page_order: &self.snapshot.page_state.order,
        };
        let verified = self
            .candidate
            .take()
            .and_then(|candidate| candidate.into_verified(context, &self.redaction_marks))
            .ok_or(NativeSessionError::RedactionUnverified)?;
        let source_spare = verified.bytes.capacity() - verified.bytes.len();
        // @invariant: The candidate's evidence reservation transfers with the verdict.
        let retained_refund = self.snapshot.limits.max_retained_bytes
            - limits.max_retained_bytes
            - source_spare
            - verified.evidence.retained();
        reopened.source = verified.bytes;
        let previous = std::mem::replace(&mut self.graph, reopened);
        self.redaction_admission.take();
        if self.origin.is_none() {
            self.origin = Some(previous.source);
        }
        self.ids = verified.ids;
        self.snapshot.binding = next_binding;
        self.snapshot.consumed_request = request_id;
        self.snapshot.output_hash = output_hash;
        self.snapshot.scene = scene;
        self.snapshot.page_state = state;
        self.snapshot.fields = fields;
        self.accepted_redaction_evidence = Some(verified.evidence);
        self.remaining_retained = retained_refund + scene_remaining;
        self.projection_work_left = projection_work_left;
        self.reset_marks();
        if self.records.len() == NATIVE_REQUEST_HISTORY {
            self.records.pop_front();
        }
        self.records.push_back(RequestRecord {
            digest: None,
            base_revision: binding.revision,
            receipt,
        });
        Ok(receipt)
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

    pub fn check_close(
        &self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<(), NativeSessionError> {
        self.check_current_binding(binding)?;
        if request_id <= self.snapshot.consumed_request
            || request_id <= self.last_redaction_request
            || self.mark_request_recorded(request_id)
        {
            return Err(NativeSessionError::InvalidRequest);
        }
        if self.candidate.is_some() {
            return Err(NativeSessionError::ReconciliationRequired);
        }
        Ok(())
    }

    fn request_record(
        &self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<Option<&RequestRecord>, NativeSessionError> {
        self.check_identity(binding, request_id)?;
        let record = self
            .records
            .iter()
            .find(|record| record.receipt.request_id == request_id);
        if record.is_some_and(|record| binding.revision != record.base_revision) {
            return Err(NativeSessionError::RevisionMismatch);
        }
        Ok(record)
    }

    pub fn ordinary_command_receipt(
        &self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<Option<NativeEditReceipt>, NativeSessionError> {
        Ok(self
            .request_record(binding, request_id)?
            .filter(|record| record.digest.is_some())
            .map(|record| record.receipt))
    }

    pub fn lookup(
        &self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<NativeRequestLookup, NativeSessionError> {
        if let Some(record) = self.request_record(binding, request_id)? {
            return Ok(NativeRequestLookup::Accepted(record.receipt));
        }
        if request_id <= self.snapshot.consumed_request || request_id <= self.last_redaction_request
        {
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

    pub fn apply_page(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        command: &NativePageCommand,
    ) -> Result<NativeEditReceipt, NativeSessionError> {
        self.apply_command(binding, request_id, Command::Page(*command))
    }

    pub fn apply_field(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
        command: &NativeFieldCommand,
    ) -> Result<NativeEditReceipt, NativeSessionError> {
        self.apply_command(binding, request_id, Command::Field(command))
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Preserve the public signature and its explicit binding and projection claims"
    )]
    pub fn verify_page_and_field_projection(
        bytes: &[u8],
        limits: NativeEditLimits,
        binding: NativeBinding,
        claimed_scene: [u8; 32],
        state: &NativePageState,
        claimed_fields: &[NativeFieldView],
        claimed_notes: &[(u32, PdfObjectRef, u64)],
        redaction_admitted: bool,
        live: Option<&Self>,
    ) -> Result<bool, NativeSessionError> {
        Self::verify_page_and_field_projection_with_objects(
            bytes,
            limits,
            binding,
            claimed_scene,
            None,
            state,
            claimed_fields,
            claimed_notes,
            redaction_admitted,
            live,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Preserve the public signature and its explicit optional object claim"
    )]
    pub fn verify_page_and_field_projection_with_objects(
        bytes: &[u8],
        limits: NativeEditLimits,
        binding: NativeBinding,
        claimed_scene: [u8; 32],
        claimed_objects: Option<&[NativeObjectPage]>,
        state: &NativePageState,
        claimed_fields: &[NativeFieldView],
        claimed_notes: &[(u32, PdfObjectRef, u64)],
        redaction_admitted: bool,
        live: Option<&Self>,
    ) -> Result<bool, NativeSessionError> {
        Self::verify_page_and_field_projection_with_note_identities(
            bytes,
            limits,
            binding,
            claimed_scene,
            claimed_objects,
            state,
            claimed_fields,
            claimed_notes.iter().copied(),
            redaction_admitted,
            live,
        )
    }

    #[doc(hidden)]
    #[expect(
        clippy::too_many_arguments,
        reason = "Keep the exact-byte WASM verification claims and borrowed note iterator explicit"
    )]
    pub fn verify_page_and_field_projection_with_note_identities(
        bytes: &[u8],
        limits: NativeEditLimits,
        binding: NativeBinding,
        claimed_scene: [u8; 32],
        claimed_objects: Option<&[NativeObjectPage]>,
        state: &NativePageState,
        claimed_fields: &[NativeFieldView],
        claimed_notes: impl IntoIterator<Item = (u32, PdfObjectRef, u64)>,
        redaction_admitted: bool,
        live: Option<&Self>,
    ) -> Result<bool, NativeSessionError> {
        // @security: Live projections are authoritative only for the exact accepted bytes and binding.
        if let Some(session) = live {
            let snapshot = &session.snapshot;
            return Ok(binding == snapshot.binding
                && bytes == session.output_bytes()
                && claimed_scene == snapshot.scene.scene_hash
                && claimed_objects.is_none_or(|rows| rows == snapshot.scene.object_pages)
                && *state == snapshot.page_state
                && claimed_fields == snapshot.fields
                && projected_notes(&snapshot.scene).eq(claimed_notes)
                && redaction_admitted == session.redaction_admitted());
        }
        let Some(hash) = detached_output_hash(bytes, binding) else {
            return Ok(false);
        };
        let graph = NativePdfGraph::open(bytes, accepted_input_limits(limits, binding))
            .map_err(redaction_graph_error)?;
        let (ids, scene, mut remaining, mut work) =
            initial_projection(&graph, hash, binding, claimed_objects)?;
        if claimed_objects.is_some_and(|rows| rows != scene.object_pages)
            || scene.scene_hash != claimed_scene
            || !detached_notes_match(&scene, binding, claimed_notes, remaining)?
            || !state.valid_in_workspace(scene.pages.len(), &Workspace::new(remaining))?
            || forms::project(&graph, &scene.pages, binding, &mut remaining, &mut work)?
                != claimed_fields
        {
            return Ok(false);
        }
        let projected = page_state(&graph, &ids, None, &scene, &mut remaining, &mut work)?;
        if state.restrictions != projected.restrictions
            || (binding.revision == 0 && *state != projected)
        {
            return Ok(false);
        }
        // @contract: A byte-only verifier cannot reconstruct live workspace reservations.
        if !redaction_admitted {
            return Ok(true);
        }
        match redaction::check_document(&graph, &state.order, None, remaining, work) {
            Ok(()) => Ok(true),
            Err(redaction::AdmissionRefusal::Work(_)) => Err(NativeSessionError::Budget),
            Err(redaction::AdmissionRefusal::Scan(_)) => Ok(false),
        }
    }

    pub fn verify_page_projection(
        bytes: &[u8],
        limits: NativeEditLimits,
        binding: NativeBinding,
        claimed_scene: [u8; 32],
        state: &NativePageState,
    ) -> Result<bool, NativeSessionError> {
        Self::verify_page_projection_with_objects(
            bytes,
            limits,
            binding,
            claimed_scene,
            None,
            state,
        )
    }

    pub fn verify_page_projection_with_objects(
        bytes: &[u8],
        limits: NativeEditLimits,
        binding: NativeBinding,
        claimed_scene: [u8; 32],
        claimed_objects: Option<&[NativeObjectPage]>,
        state: &NativePageState,
    ) -> Result<bool, NativeSessionError> {
        let graph = NativePdfGraph::open(bytes, accepted_input_limits(limits, binding))?;
        let Some(hash) = detached_output_hash(bytes, binding) else {
            return Ok(false);
        };
        let (ids, scene, mut remaining, mut work) =
            initial_projection(&graph, hash, binding, claimed_objects)?;
        if claimed_objects.is_some_and(|rows| rows != scene.object_pages) {
            return Ok(false);
        }
        let projected = page_state(&graph, &ids, None, &scene, &mut remaining, &mut work)?;
        Ok(scene.scene_hash == claimed_scene
            && state.valid_in_workspace(scene.pages.len(), &Workspace::new(remaining))?
            && state.restrictions == projected.restrictions
            && (binding.revision != 0 || *state == projected))
    }

    pub fn verify_edit_projection(
        bytes: &[u8],
        limits: NativeEditLimits,
        binding: NativeBinding,
        claimed_text: &[NativeTextPage],
        claimed_objects: &[NativeObjectPage],
    ) -> Result<bool, NativeSessionError> {
        let graph = NativePdfGraph::open(bytes, accepted_input_limits(limits, binding))?;
        let Some(hash) = detached_output_hash(bytes, binding) else {
            return Ok(false);
        };
        let (_, scene, _, _) = initial_projection(&graph, hash, binding, Some(claimed_objects))?;
        Ok(scene.text_pages == claimed_text && scene.object_pages == claimed_objects)
    }

    pub fn verify_text_projection(
        bytes: &[u8],
        limits: NativeEditLimits,
        binding: NativeBinding,
        claimed: &[NativeTextPage],
    ) -> Result<bool, NativeSessionError> {
        let graph = NativePdfGraph::open(bytes, accepted_input_limits(limits, binding))?;
        let Some(hash) = detached_output_hash(bytes, binding) else {
            return Ok(false);
        };
        let (_, scene, _, _) = initial_projection(&graph, hash, binding, None)?;
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
            let Some(digest) = record.digest else {
                return Err(NativeSessionError::RequestConflict);
            };
            if command_digest(command) != Ok(digest) {
                return Err(NativeSessionError::RequestConflict);
            }
            return Ok(record.receipt);
        }
        if binding.revision != self.snapshot.binding.revision {
            return Err(NativeSessionError::RevisionMismatch);
        }
        if request_id <= self.last_redaction_request || self.mark_request_recorded(request_id) {
            return Err(NativeSessionError::ReconciliationRequired);
        }
        if let Command::Page(command) = command {
            let target = command.target();
            if self.snapshot.page_state.order.get(target.page as usize) != Some(&target.stable_id) {
                return Err(NativeSessionError::TargetUnavailable);
            }
            if matches!(command, NativePageCommand::DeletePage { .. })
                && !self.snapshot.page_state.can_delete
            {
                return Err(NativeSessionError::FinalPage);
            }
            if matches!(command, NativePageCommand::ReorderPage { position, .. }
                if position > self.snapshot.page_state.max_position)
            {
                return Err(NativeSessionError::TargetUnavailable);
            }
            if let Some(refusal) = self
                .snapshot
                .page_state
                .restrictions
                .as_ref()
                .and_then(|restrictions| restrictions.refusal(command))
            {
                return Err(refusal.into());
            }
        }
        let revision = binding
            .revision
            .checked_add(1)
            .ok_or(NativeSessionError::Budget)?;
        if revision >= FIRST_PAGE_OBJECT_ID - FIRST_ADDED_NOTE_ID {
            return Err(NativeSessionError::Budget);
        }
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
            self.mark_bytes() + self.candidate.as_ref().map_or(0, Candidate::retained),
        )?;
        let mut staged_id_bytes = (self.ids.len()
            + usize::from(matches!(
                command,
                Command::Note(NativeNoteCommand::Add { .. })
            )))
            * OBJECT_ID_BYTES;
        spend(
            &mut remaining,
            4096 + content_cost + annotation_cost + staged_id_bytes,
        )?;
        let digest = command_digest(command)?;
        // @security: Reserve output bytes separately from the writer workspace and reopen graph.
        let output_share = match command {
            Command::Text(_) | Command::Object(_) | Command::Page(_) | Command::Field(_) => {
                remaining / 5
            }
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
        let mut next_page_object_id = self.next_page_object_id;
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
                let workspace = Workspace::new(staged.remaining_retained());
                let expected = text::stage(
                    &mut staged,
                    page,
                    binding,
                    self.snapshot.output_hash,
                    command,
                    &workspace,
                )?;
                if let Some(object) = expected.new_object {
                    staged.charge_page_retained(OBJECT_ID_BYTES)?;
                    staged_id_bytes += OBJECT_ID_BYTES;
                    ids.insert(object, NativeNoteId(0));
                }
                Expected::Text(expected)
            }
            Command::Field(command) => {
                let expected = forms::stage(
                    &mut staged,
                    &self.snapshot.scene.pages,
                    &self.snapshot.fields,
                    command,
                )?;
                let mut remaining = staged.remaining_retained();
                staged_id_bytes += reconcile_staged_ids(&staged, &mut ids, &mut remaining, || {
                    Ok(NativeNoteId(0))
                })?;
                staged_remaining = Some(remaining);
                Expected::Field(expected)
            }
            Command::Page(command) => {
                let target = command.target();
                let index = target.page as usize;
                let selected =
                    pages::stage(&mut staged, command, index, &self.snapshot.scene.pages)?;
                let mut remaining = staged.remaining_retained();
                staged_id_bytes += reconcile_staged_ids(&staged, &mut ids, &mut remaining, || {
                    if next_page_object_id >= PAGE_OBJECT_ID_LIMIT {
                        return Err(NativeSessionError::Budget);
                    }
                    let id = NativeNoteId(next_page_object_id);
                    next_page_object_id += 1;
                    Ok(id)
                })?;
                staged_remaining = Some(remaining);
                Expected::Page(ids[&selected].0)
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
                let (expected, mut remaining) = objects::stage(
                    &mut staged,
                    page,
                    binding,
                    self.snapshot.output_hash,
                    command,
                )?;
                staged_id_bytes += reconcile_staged_ids(&staged, &mut ids, &mut remaining, || {
                    Ok(NativeNoteId(0))
                })?;
                staged_remaining = Some(remaining);
                Expected::Object(expected)
            }
        };
        staged.limits.max_source_bytes = self.snapshot.limits.max_output_bytes;
        #[cfg(test)]
        match self.fail_at {
            Some(FailurePoint::Serialize) => staged.limits.max_output_bytes = 1,
            Some(FailurePoint::Reopen) => staged.limits.max_source_bytes = 1,
            _ => {}
        }
        let staged_remaining = staged_remaining.unwrap_or_else(|| staged.remaining_retained());
        let RemappedRewrite {
            output,
            reopened,
            ids: remapped_ids,
        } = rewrite_and_remap(&staged, staged_remaining + output_budget, &ids)?;
        drop(ids);
        let ids = remapped_ids;
        let output_hash = *blake3::hash(&output).as_bytes();
        let output_capacity = output.capacity();
        drop(staged);
        drop(output);
        let released = output_capacity + staged_id_bytes;
        let mut scene_limit = reopened.limits.max_retained_bytes + released;
        let next_binding = NativeBinding {
            revision,
            ..binding
        };
        let (scene, mut scene_remaining, mut work) = accepted_scene(
            &reopened,
            &ids,
            output_hash,
            next_binding,
            self.origin_bytes().len(),
            record_count,
            reopened.remaining_retained() + released,
        )?;
        let fields = forms::project(
            &reopened,
            &scene.pages,
            next_binding,
            &mut scene_remaining,
            &mut work,
        )?;
        let selected_page = match &expected {
            Expected::Page(id) => Some(*id),
            _ => None,
        };
        let state = page_state(
            &reopened,
            &ids,
            selected_page.or(self.snapshot.page_state.selected),
            &scene,
            &mut scene_remaining,
            &mut work,
        )?;
        let projection_work_left = work;
        let (note, text_island, object) = match expected {
            Expected::Page(_) => (NativeNoteId(0), None, None),
            Expected::Field(expected) => {
                expected.verify(&fields, &mut work)?;
                let text_bytes = expected.retained_text_bytes();
                drop(expected);
                scene_limit += text_bytes;
                scene_remaining += text_bytes;
                (NativeNoteId(0), None, None)
            }
            Expected::Note(expected) => {
                verify_note(&scene, &expected, &mut work)?;
                (expected.id, None, None)
            }
            Expected::Text(expected) => {
                let island = text::verify(&scene.text_pages, &expected, &mut work)?;
                (NativeNoteId(0), Some(island), None)
            }
            Expected::Object(expected) => {
                let object = objects::verify(
                    &reopened,
                    &scene.pages,
                    &scene.object_pages,
                    &expected,
                    scene_remaining,
                    &mut work,
                )?;
                let expected_bytes = expected.retained_bytes();
                drop(expected);
                scene_limit += expected_bytes;
                scene_remaining += expected_bytes;
                (NativeNoteId(0), None, object)
            }
        };
        let receipt = NativeEditReceipt {
            binding: next_binding,
            request_id,
            note,
            text_island,
            object,
            selected_page,
            output_hash,
            scene_hash: scene.scene_hash,
        };
        spend(&mut work, state.order.len())?;
        if let Command::Page(command) = command {
            let workspace = Workspace::new(scene_remaining);
            #[cfg(test)]
            let workspace = match self.fail_at {
                Some(FailurePoint::PageVerificationBudget(allowance)) => {
                    Workspace::new(workspace.remaining().min(allowance))
                }
                _ => workspace,
            };
            if !state.valid_in_workspace(scene.pages.len(), &workspace)? {
                return Err(NativeSessionError::Postcondition);
            }
            self.verify_page_geometry(command, &scene, &state, &workspace, &mut work)?;
        } else if !state.has_shape_for_count(scene.pages.len())
            || state.order != self.snapshot.page_state.order
        {
            return Err(NativeSessionError::Postcondition);
        }
        self.records
            .try_reserve_exact(usize::from(self.records.len() < NATIVE_REQUEST_HISTORY))
            .map_err(|_| NativeSessionError::Budget)?;
        let previous = std::mem::replace(&mut self.graph, reopened);
        self.redaction_admission.take();
        if self.origin.is_none() {
            self.origin = Some(previous.source);
        }
        self.remaining_retained =
            self.snapshot.limits.max_retained_bytes - scene_limit + scene_remaining;
        self.projection_work_left = projection_work_left;
        self.ids = ids;
        self.next_page_object_id = next_page_object_id;
        self.snapshot.page_state = state;
        self.snapshot.binding = receipt.binding;
        self.snapshot.consumed_request = request_id;
        self.snapshot.output_hash = output_hash;
        self.snapshot.scene = scene;
        self.snapshot.fields = fields;
        self.candidate = None;
        self.accepted_redaction_evidence = None;
        if self.records.len() == NATIVE_REQUEST_HISTORY {
            self.records.pop_front();
        }
        self.records.push_back(RequestRecord {
            digest: Some(digest),
            base_revision: binding.revision,
            receipt,
        });
        Ok(receipt)
    }

    fn verify_page_geometry(
        &self,
        command: NativePageCommand,
        scene: &NativePdfNoteScene,
        state: &NativePageState,
        workspace: &Workspace,
        work: &mut usize,
    ) -> Result<(), NativeSessionError> {
        let target = command.target();
        let prior = &self.snapshot;
        spend(work, prior.scene.pages.len() + scene.pages.len())?;
        let mut positions_bytes = workspace.scope();
        let mut positions = BTreeMap::new();
        for (&id, page) in prior.page_state.order.iter().zip(&prior.scene.pages) {
            positions_bytes
                .tree_entry::<(u64, &NativePdfNotePage)>()
                .map_err(|_| NativeSessionError::Budget)?;
            positions.insert(id, page);
        }
        let source = &prior.scene.pages[target.page as usize];
        for (page, id) in scene.pages.iter().zip(&state.order) {
            let before = positions.get(id).copied().unwrap_or(source).geometry;
            let valid = if let NativePageCommand::RotatePage { degrees, .. } = command
                && *id == target.stable_id
            {
                before.zip(page.geometry).is_some_and(|(before, after)| {
                    before.bounds() == after.bounds()
                        && (before.rotation() + i64::from(degrees)).rem_euclid(360)
                            == after.rotation()
                })
            } else {
                before == page.geometry
            };
            if !valid {
                return Err(NativeSessionError::Postcondition);
            }
        }
        Ok(())
    }
}

fn detached_notes_match(
    scene: &NativePdfNoteScene,
    binding: NativeBinding,
    claimed: impl IntoIterator<Item = (u32, PdfObjectRef, u64)>,
    remaining: usize,
) -> Result<bool, NativeSessionError> {
    let workspace = Workspace::new(remaining);
    let mut uniqueness = workspace.scope();
    let mut seen = BTreeSet::new();
    let mut projected = projected_notes(scene);
    for (claimed_page, claimed_object, id) in claimed {
        let Some((page, object, projected_id)) = projected.next() else {
            return Ok(false);
        };
        let valid_id = if binding.revision == 0 {
            id == projected_id
        } else {
            id != 0
                && (id < FIRST_ADDED_NOTE_ID
                    || (id > FIRST_ADDED_NOTE_ID && id - FIRST_ADDED_NOTE_ID <= binding.revision)
                    || (FIRST_PAGE_OBJECT_ID..PAGE_OBJECT_ID_LIMIT).contains(&id))
        };
        if page != claimed_page
            || object != claimed_object
            || !valid_id
            || !uniqueness
                .insert(&mut seen, id)
                .map_err(|_| NativeSessionError::Budget)?
        {
            return Ok(false);
        }
    }
    Ok(projected.next().is_none())
}

fn command_digest(command: Command<'_>) -> Result<[u8; 32], NativeSessionError> {
    let mut digest = blake3::Hasher::new();
    match command {
        Command::Page(command) => serde_json::to_writer(&mut digest, &command),
        Command::Field(command) => {
            command.validate()?;
            serde_json::to_writer(&mut digest, command)
        }
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

fn page_state(
    graph: &NativePdfGraph,
    ids: &BTreeMap<PdfObjectRef, NativeNoteId>,
    selected: Option<u64>,
    scene: &NativePdfNoteScene,
    remaining: &mut usize,
    work: &mut usize,
) -> Result<NativePageState, NativeSessionError> {
    use pages::NativePageCapability;
    let capability_bytes = graph.pages.len() * std::mem::size_of::<NativePageCapability>();
    spend(remaining, capability_bytes)?;
    let workspace = Workspace::new(*remaining);
    let restrictions = pages::restrictions(graph, &scene.pages, &workspace, work)?;
    if restrictions.is_none() {
        *remaining += capability_bytes;
    }
    Ok(NativePageState {
        order: graph
            .pages
            .iter()
            .map(|reference| ids[reference].0)
            .collect(),
        selected,
        can_delete: graph.pages.len() > 1,
        max_position: (graph.pages.len() - 1) as u32,
        restrictions,
    })
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

fn initial_projection(
    graph: &NativePdfGraph,
    output_hash: [u8; 32],
    binding: NativeBinding,
    claimed_objects: Option<&[NativeObjectPage]>,
) -> Result<
    (
        BTreeMap<PdfObjectRef, NativeNoteId>,
        NativePdfNoteScene,
        usize,
        usize,
    ),
    NativeSessionError,
> {
    let mut remaining = graph.remaining_retained();
    // @invariant: Reserve the identity map before allocation and carry the charge through projection.
    spend(&mut remaining, graph.objects.len() * OBJECT_ID_BYTES)?;
    let ids = initial_ids(graph);
    let (scene, remaining, work) = project_scene(
        graph,
        &ids,
        output_hash,
        binding,
        remaining,
        claimed_objects,
    )?;
    Ok((ids, scene, remaining, work))
}

fn detached_output_hash(bytes: &[u8], binding: NativeBinding) -> Option<[u8; 32]> {
    let hash = *blake3::hash(bytes).as_bytes();
    (binding.revision != 0 || hash == binding.origin_hash).then_some(hash)
}

fn accepted_input_limits(mut limits: NativeEditLimits, binding: NativeBinding) -> NativeEditLimits {
    if binding.revision != 0 {
        limits.max_source_bytes = limits.max_output_bytes;
    }
    limits
}

fn redaction_graph_error(error: NativeEditError) -> NativeSessionError {
    match error {
        NativeEditError::Reader(
            PdfReadError::InputSizeLimit | PdfReadError::DecodedStreamLimit,
        ) => NativeSessionError::Budget,
        error => error.into(),
    }
}

fn projected_notes(scene: &NativePdfNoteScene) -> impl Iterator<Item = (u32, PdfObjectRef, u64)> {
    scene.pages.iter().flat_map(|page| {
        page.annotations
            .iter()
            .filter_map(move |annotation| match annotation {
                NativePdfAnnotation::PlainNote(note) => Some((page.id.0, note.object, note.id.0)),
                _ => None,
            })
    })
}

fn accepted_scene(
    graph: &NativePdfGraph,
    ids: &BTreeMap<PdfObjectRef, NativeNoteId>,
    output_hash: [u8; 32],
    binding: NativeBinding,
    separate_origin: usize,
    request_count: usize,
    mut remaining: usize,
) -> Result<(NativePdfNoteScene, usize, usize), NativeSessionError> {
    spend(&mut remaining, separate_origin)?;
    spend(
        &mut remaining,
        ids.len() * OBJECT_ID_BYTES + request_count * REQUEST_RECORD_BYTES,
    )?;
    project_scene(graph, ids, output_hash, binding, remaining, None)
}

fn project_scene(
    graph: &NativePdfGraph,
    ids: &BTreeMap<PdfObjectRef, NativeNoteId>,
    output_hash: [u8; 32],
    binding: NativeBinding,
    mut remaining: usize,
    claimed_objects: Option<&[NativeObjectPage]>,
) -> Result<(NativePdfNoteScene, usize, usize), NativeSessionError> {
    spend(
        &mut remaining,
        SESSION_BYTES + graph.pages.len() * std::mem::size_of::<u64>(),
    )?;
    // @invariant: Every projection and postcondition shares the work left by the exact-byte graph.
    let mut work = graph.remaining_work();
    let pages = notes::project(graph, ids, &mut remaining, &mut work)?;
    // @invariant: Revised text projections must agree with detached verification of the exact bytes.
    let work_exhaustion = if binding.revision == 0 {
        text::WorkExhaustion::PageRefusal
    } else {
        text::WorkExhaustion::BudgetError
    };
    let text_pages = text::project(
        graph,
        &pages,
        output_hash,
        binding,
        &mut remaining,
        &mut work,
        work_exhaustion,
    )?;
    let object_pages = objects::project(
        graph,
        &pages,
        output_hash,
        binding,
        &mut remaining,
        &mut work,
        claimed_objects,
    )?;
    let scene = NativePdfNoteScene::new(output_hash, pages, text_pages, object_pages, &mut work)?;
    Ok((scene, remaining, work))
}

fn verify_note(
    scene: &NativePdfNoteScene,
    expected: &ExpectedNote<'_>,
    work: &mut usize,
) -> Result<(), NativeSessionError> {
    spend(work, scene.pages.len())?;
    for page in &scene.pages {
        spend(work, page.annotations.len())?;
    }
    if let Some((_, _, contents)) = expected.location {
        spend(work, contents.len())?;
    }
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

fn reconcile_staged_ids(
    staged: &NativePdfGraph,
    ids: &mut BTreeMap<PdfObjectRef, NativeNoteId>,
    remaining: &mut usize,
    mut new_id: impl FnMut() -> Result<NativeNoteId, NativeSessionError>,
) -> Result<usize, NativeSessionError> {
    let missing = staged
        .objects
        .keys()
        .filter(|object| !ids.contains_key(object))
        .count();
    let bytes = missing
        .checked_mul(OBJECT_ID_BYTES)
        .ok_or(NativeSessionError::Budget)?;
    spend(remaining, bytes)?;
    ids.retain(|reference, _| staged.objects.contains_key(reference));
    for &reference in staged.objects.keys() {
        if let std::collections::btree_map::Entry::Vacant(entry) = ids.entry(reference) {
            entry.insert(new_id()?);
        }
    }
    Ok(bytes)
}

struct RemappedRewrite {
    output: Vec<u8>,
    reopened: NativePdfGraph,
    ids: BTreeMap<PdfObjectRef, NativeNoteId>,
}

fn rewrite_and_remap(
    staged: &NativePdfGraph,
    workspace: usize,
    ids: &BTreeMap<PdfObjectRef, NativeNoteId>,
) -> Result<RemappedRewrite, NativeSessionError> {
    let Rewrite {
        bytes: output,
        mut reopened,
        mapping,
    } = rewrite_with_workspace(staged, workspace).map_err(|error| match error {
        NativeEditError::Reader(PdfReadError::TokenLimit) => NativeSessionError::Budget,
        error => error.into(),
    })?;
    let ids = preserve_and_remap(staged, &mut reopened, &mapping, ids)?;
    drop(mapping);
    // @invariant: Release rewrite scratch after remapping, while output capacity stays charged.
    let retained_limit = workspace - output.capacity();
    reopened.release_page_retained(retained_limit - reopened.limits.max_retained_bytes);
    reopened.limits.max_retained_bytes = retained_limit;
    Ok(RemappedRewrite {
        output,
        reopened,
        ids,
    })
}

fn preserve_and_remap(
    staged: &NativePdfGraph,
    reopened: &mut NativePdfGraph,
    mapping: &BTreeMap<PdfObjectRef, PdfObjectRef>,
    ids: &BTreeMap<PdfObjectRef, NativeNoteId>,
) -> Result<BTreeMap<PdfObjectRef, NativeNoteId>, NativeSessionError> {
    let work_budget = reopened.remaining_work();
    let mut work = work_budget;
    spend(&mut work, staged.pages.len())?;
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
    {
        return Err(NativeSessionError::Postcondition);
    }
    for ((left, a), (right, b)) in staged.trailer.iter().zip(&reopened.trailer) {
        if !same_bytes(left.as_bytes(), right.as_bytes(), &mut work)?
            || (left != "Size" && !equivalent(a, b, mapping, &mut work)?)
        {
            return Err(NativeSessionError::Postcondition);
        }
    }

    let mut remapped = BTreeMap::new();
    for (before, after) in mapping {
        spend(&mut work, 1)?;
        let value = &staged.objects[before];
        let reopened_value = reopened
            .objects
            .get(after)
            .ok_or(NativeSessionError::Postcondition)?;
        if !equivalent(value, reopened_value, mapping, &mut work)? {
            return Err(NativeSessionError::Postcondition);
        }
        let id = ids.get(before).ok_or(NativeSessionError::Postcondition)?;
        remapped.insert(*after, *id);
    }
    reopened.charge_page_work(work_budget - work)?;
    Ok(remapped)
}

fn same_bytes(lhs: &[u8], rhs: &[u8], work: &mut usize) -> Result<bool, NativeEditError> {
    if lhs.len() != rhs.len() {
        return Ok(false);
    }
    spend(work, lhs.len())?;
    Ok(lhs == rhs)
}

fn same_dict(
    lhs: &BTreeMap<String, PdfValue>,
    rhs: &BTreeMap<String, PdfValue>,
    mapping: &BTreeMap<PdfObjectRef, PdfObjectRef>,
    work: &mut usize,
) -> Result<bool, NativeEditError> {
    if lhs.len() != rhs.len() {
        return Ok(false);
    }
    for ((left, a), (right, b)) in lhs.iter().zip(rhs) {
        if !same_bytes(left.as_bytes(), right.as_bytes(), work)?
            || !equivalent(a, b, mapping, work)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn equivalent(
    lhs: &PdfValue,
    rhs: &PdfValue,
    mapping: &BTreeMap<PdfObjectRef, PdfObjectRef>,
    work: &mut usize,
) -> Result<bool, NativeEditError> {
    spend(work, 1)?;
    Ok(match (lhs, rhs) {
        (PdfValue::Null, PdfValue::Null) => true,
        (PdfValue::Boolean(a), PdfValue::Boolean(b)) => a == b,
        (PdfValue::Integer(a), PdfValue::Integer(b)) => a == b,
        (PdfValue::ExactReal(a), PdfValue::ExactReal(b))
        | (PdfValue::Name(a), PdfValue::Name(b)) => same_bytes(a.as_bytes(), b.as_bytes(), work)?,
        (PdfValue::String(a), PdfValue::String(b)) => same_bytes(a, b, work)?,
        (PdfValue::Reference(a), PdfValue::Reference(b)) => mapping.get(a) == Some(b),
        (PdfValue::Array(a), PdfValue::Array(b)) => {
            if a.len() != b.len() {
                return Ok(false);
            }
            for (lhs, rhs) in a.iter().zip(b) {
                if !equivalent(lhs, rhs, mapping, work)? {
                    return Ok(false);
                }
            }
            true
        }
        (PdfValue::Dictionary(a), PdfValue::Dictionary(b)) => same_dict(a, b, mapping, work)?,
        (PdfValue::Stream(a), PdfValue::Stream(b)) => {
            // @invariant: Debit each stream buffer before its exact comparison.
            spend(work, a.encoded.len().div_ceil(16))?;
            if a.encoded != b.encoded {
                return Ok(false);
            }
            spend(work, a.data.len().div_ceil(16))?;
            a.data == b.data && same_dict(&a.dict, &b.dict, mapping, work)?
        }
        _ => false,
    })
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum FailurePoint {
    Serialize,
    Reopen,
    PageVerificationBudget(usize),
}

#[cfg(test)]
mod tests {
    use super::super::notes::NativePageId;
    use super::*;

    #[test]
    fn detached_note_identity_checks_reserve_exact_live_scratch() {
        let source = super::super::tests::redaction_fixture("q Q");
        let limits = NativeEditLimits::default();
        let mut session = NativePdfSession::open_scoped(&source, limits, [7; 16]).unwrap();
        for (request_id, rect) in [(1, [20, 30, 40, 50]), (2, [60, 70, 80, 90])] {
            session
                .apply(
                    session.snapshot.binding,
                    request_id,
                    &NativeNoteCommand::Add {
                        page: NativePageId(0),
                        rect,
                        contents: format!("Review note {request_id}"),
                    },
                )
                .unwrap();
        }
        let snapshot = session.snapshot();
        let mut notes = projected_notes(&snapshot.scene).collect::<Vec<_>>();
        assert_eq!(notes.len(), 2);
        let verify = |notes: &[_]| {
            NativePdfSession::verify_page_and_field_projection_with_objects(
                session.output_bytes(),
                limits,
                snapshot.binding,
                snapshot.scene.scene_hash,
                None,
                &snapshot.page_state,
                &snapshot.fields,
                notes,
                session.redaction_admitted(),
                None,
            )
        };
        assert_eq!(verify(&notes), Ok(true));

        let peak = notes.len() * (128 + 12 * std::mem::size_of::<u64>());
        assert_eq!(
            detached_notes_match(
                &snapshot.scene,
                snapshot.binding,
                notes.iter().copied(),
                peak
            ),
            Ok(true)
        );
        assert_eq!(
            detached_notes_match(
                &snapshot.scene,
                snapshot.binding,
                notes.iter().copied(),
                peak - 1
            ),
            Err(NativeSessionError::Budget)
        );

        notes[1].2 = notes[0].2;
        assert_eq!(verify(&notes), Ok(false));
        assert_eq!(
            detached_notes_match(
                &snapshot.scene,
                snapshot.binding,
                notes.iter().copied(),
                peak
            ),
            Ok(false)
        );
    }

    #[test]
    fn copied_annotations_require_all_staged_ids_before_insertion() {
        let bytes = super::super::tests::redaction_fixture("q Q");
        let limits = NativeEditLimits::default();
        let mut session = NativePdfSession::open_scoped(&bytes, limits, [7; 16]).unwrap();
        session
            .apply(
                session.snapshot.binding,
                1,
                &NativeNoteCommand::Add {
                    page: NativePageId(0),
                    rect: [10, 10, 30, 30],
                    contents: "Delivery note".into(),
                },
            )
            .unwrap();
        let mut staged = NativePdfGraph::open(session.output_bytes(), limits).unwrap();
        let duplicate = NativePageCommand::DuplicatePage {
            target: pages::NativePageTarget {
                page: 0,
                stable_id: session.snapshot.page_state.order[0],
            },
        };
        let copied =
            pages::stage(&mut staged, duplicate, 0, &session.snapshot.scene.pages).unwrap();
        assert_eq!(staged.objects.len(), session.ids.len() + 2);
        let required = 2 * OBJECT_ID_BYTES;
        for short_by in [0, 1] {
            let mut remaining = required - short_by;
            let mut ids = session.ids.clone();
            let result =
                reconcile_staged_ids(&staged, &mut ids, &mut remaining, || Ok(NativeNoteId(99)));
            if short_by == 0 {
                assert_eq!(result, Ok(required));
                assert_eq!(remaining, 0);
                assert_eq!(ids[&copied], NativeNoteId(99));
                assert_eq!(ids.len(), staged.objects.len());
            } else {
                assert_eq!(result, Err(NativeSessionError::Budget));
                assert_eq!(ids, session.ids);
                assert_eq!(remaining, required - 1);
            }
        }
    }

    #[test]
    fn page_postconditions_reserve_and_refund_live_scratch() {
        use pages::NativePageTarget;

        let source = super::super::tests::redaction_fixture("q Q");
        let session =
            NativePdfSession::open_scoped(&source, NativeEditLimits::default(), [7; 16]).unwrap();
        let before = session.snapshot();
        let command = NativePageCommand::ReorderPage {
            target: NativePageTarget {
                page: 0,
                stable_id: before.page_state.order[0],
            },
            position: 0,
        };
        let positions_bytes = 128 + 12 * std::mem::size_of::<(u64, &NativePdfNotePage)>();
        let unique_bytes = 128 + 12 * std::mem::size_of::<u64>();
        for short in [0, 1] {
            let workspace = Workspace::new(positions_bytes - short);
            let mut work = 2 * before.scene.pages.len();
            let result = session.verify_page_geometry(
                command,
                &before.scene,
                &before.page_state,
                &workspace,
                &mut work,
            );
            if short == 0 {
                assert_eq!(result, Ok(()));
                assert_eq!(work, 0);
            } else {
                assert_eq!(result, Err(NativeSessionError::Budget));
            }
            assert_eq!(workspace.remaining(), positions_bytes - short);
            let workspace = Workspace::new(unique_bytes - short);
            assert_eq!(
                before
                    .page_state
                    .valid_in_workspace(before.scene.pages.len(), &workspace),
                if short == 0 {
                    Ok(true)
                } else {
                    Err(NativeEditError::Budget)
                }
            );
            assert_eq!(workspace.remaining(), unique_bytes - short);
        }
    }

    #[test]
    fn page_postcondition_budget_refusal_preserves_accepted_session() {
        use pages::NativePageTarget;

        let source = super::super::tests::redaction_fixture("q Q");
        let mut session =
            NativePdfSession::open_scoped(&source, NativeEditLimits::default(), [7; 16]).unwrap();
        let before = session.snapshot().clone();
        let retained = session.remaining_retained;
        let command = NativePageCommand::RotatePage {
            target: NativePageTarget {
                page: 0,
                stable_id: before.page_state.order[0],
            },
            degrees: 90,
        };
        let peak = 128 + 12 * std::mem::size_of::<(u64, &NativePdfNotePage)>();
        let unique_bytes = 128 + 12 * std::mem::size_of::<u64>();
        for allowance in [unique_bytes - 1, peak - 1] {
            session.fail_at = Some(FailurePoint::PageVerificationBudget(allowance));
            assert_eq!(
                session.apply_page(before.binding, 1, &command),
                Err(NativeSessionError::Budget)
            );
            assert_eq!(session.output_bytes(), source);
            assert_eq!(session.remaining_retained, retained);
            let after = session.snapshot();
            assert_eq!(after.binding, before.binding);
            assert_eq!(after.consumed_request, before.consumed_request);
            assert_eq!(after.output_hash, before.output_hash);
            assert_eq!(after.scene.scene_hash, before.scene.scene_hash);
            assert_eq!(after.page_state, before.page_state);
            assert_eq!(
                session.lookup(before.binding, 1),
                Ok(NativeRequestLookup::Unseen)
            );
        }

        session.fail_at = Some(FailurePoint::PageVerificationBudget(peak));
        session.apply_page(before.binding, 1, &command).unwrap();
        assert_eq!(session.snapshot().page_state.order, before.page_state.order);
        assert_eq!(
            session.snapshot().scene.pages[0]
                .geometry
                .unwrap()
                .rotation(),
            90
        );
    }

    #[test]
    fn accepted_text_field_balance_matches_fresh_projection() {
        let source = super::super::tests::redaction_fixture("q Q");
        let limits = NativeEditLimits::default();
        let mut session = NativePdfSession::open_scoped(&source, limits, [7; 16]).unwrap();
        let value = "Approved";
        let receipt = session
            .apply_field(
                session.snapshot.binding,
                1,
                &NativeFieldCommand::AddTextField {
                    page: 0,
                    rect: [5, 5, 98, 24],
                    name: "decision".into(),
                    value: value.into(),
                },
            )
            .unwrap();
        assert_eq!(receipt.binding.revision, 1);
        assert_eq!(session.snapshot.fields[0].value.as_deref(), Some(value));

        let fresh = NativePdfGraph::open(session.output_bytes(), limits).unwrap();
        let (scene, mut remaining, mut work) = accepted_scene(
            &fresh,
            &session.ids,
            session.snapshot.output_hash,
            session.snapshot.binding,
            session.origin_bytes().len(),
            session.records.len(),
            fresh.remaining_retained(),
        )
        .unwrap();
        let fields = forms::project(
            &fresh,
            &scene.pages,
            session.snapshot.binding,
            &mut remaining,
            &mut work,
        )
        .unwrap();
        let state = page_state(
            &fresh,
            &session.ids,
            None,
            &scene,
            &mut remaining,
            &mut work,
        )
        .unwrap();
        assert_eq!(fields, session.snapshot.fields);
        assert_eq!(state, session.snapshot.page_state);
        assert_eq!(session.remaining_retained, remaining);
    }

    #[test]
    fn accepted_object_balance_matches_fresh_projection() {
        let source = super::super::tests::redaction_fixture("10 10 10 10 re f 40 40 10 10 re f");
        let limits = NativeEditLimits::default();
        let mut session = NativePdfSession::open_scoped(&source, limits, [7; 16]).unwrap();
        let page = &session.snapshot.scene.object_pages[0];
        assert_eq!(page.objects.len(), 2);
        let target = objects::NativeObjectTarget {
            page: page.page,
            id: page.objects[0].id,
            profile: page.objects[0].profile,
        };
        let kept_bounds = page.objects[1].bounds;
        let receipt = session
            .apply_object(
                session.snapshot.binding,
                1,
                &NativeObjectCommand::RemoveObject { target },
            )
            .unwrap();
        assert_eq!(receipt.binding.revision, 1);
        let objects = &session.snapshot.scene.object_pages[0].objects;
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].bounds, kept_bounds);

        let fresh = NativePdfGraph::open(session.output_bytes(), limits).unwrap();
        let (scene, mut remaining, mut work) = accepted_scene(
            &fresh,
            &session.ids,
            receipt.output_hash,
            receipt.binding,
            source.len(),
            1,
            fresh.remaining_retained(),
        )
        .unwrap();
        let fields = forms::project(
            &fresh,
            &scene.pages,
            receipt.binding,
            &mut remaining,
            &mut work,
        )
        .unwrap();
        let state = page_state(
            &fresh,
            &session.ids,
            None,
            &scene,
            &mut remaining,
            &mut work,
        )
        .unwrap();
        assert_eq!(scene.object_pages, session.snapshot.scene.object_pages);
        assert_eq!(fields, session.snapshot.fields);
        assert_eq!(state, session.snapshot.page_state);
        assert_eq!(session.remaining_retained, remaining);
    }

    #[test]
    fn object_edit_requires_work_for_reopened_scans_and_ordered_signatures() {
        let paint = format!("10 20 30 40 re f\n55 65 8 9 re f\n{}", " ".repeat(128));
        let source = super::super::tests::redaction_fixture(&paint);
        let limits = NativeEditLimits::default();
        let setup = || {
            let mut session = NativePdfSession::open_scoped(&source, limits, [7; 16]).unwrap();
            let initial = session.snapshot.binding;
            let note = session
                .apply(
                    initial,
                    1,
                    &NativeNoteCommand::Add {
                        page: NativePageId(0),
                        rect: [5, 5, 15, 15],
                        contents: "Accepted note".into(),
                    },
                )
                .unwrap();
            (session, initial, note)
        };
        let move_graphic = |session: &mut NativePdfSession| {
            let page = &session.snapshot.scene.object_pages[0];
            let target = objects::NativeObjectTarget {
                page: page.page,
                id: page.objects[0].id,
                profile: page.objects[0].profile,
            };
            session.apply_object(
                session.snapshot.binding,
                2,
                &NativeObjectCommand::MoveObject {
                    target,
                    transform: [1., 0., 0., 1., 10., 0.],
                },
            )
        };

        let (mut full, _, _) = setup();
        move_graphic(&mut full).unwrap();
        let (mut limited, initial, note) = setup();
        let reopened = &full.graph;
        let next_binding = NativeBinding {
            revision: note.binding.revision + 1,
            ..note.binding
        };
        let (scene, mut remaining, mut work) = accepted_scene(
            reopened,
            &full.ids,
            full.snapshot.output_hash,
            next_binding,
            full.origin_bytes().len(),
            full.records.len(),
            reopened.remaining_retained(),
        )
        .unwrap();
        forms::project(
            reopened,
            &scene.pages,
            next_binding,
            &mut remaining,
            &mut work,
        )
        .unwrap();
        let state =
            page_state(reopened, &full.ids, None, &scene, &mut remaining, &mut work).unwrap();
        objects::charge_verification_scan(reopened, &scene.pages[0], remaining, &mut work).unwrap();
        assert_eq!(scene.object_pages[0].objects.len(), 2);
        spend(&mut work, scene.object_pages[0].objects.len()).unwrap();
        let signatures_work = limits.max_work - work;
        spend(&mut work, state.order.len()).unwrap();
        let exact_work = limits.max_work - work;

        let accepted = limited.output_bytes().to_vec();
        let before = limited.snapshot.clone();
        for max_work in [signatures_work - 1, exact_work - 1] {
            limited.snapshot.limits.max_work = max_work;
            assert_eq!(move_graphic(&mut limited), Err(NativeSessionError::Budget));
            assert_eq!(limited.output_bytes(), accepted);
            assert_eq!(limited.snapshot.binding, before.binding);
            assert_eq!(limited.snapshot.output_hash, before.output_hash);
            assert_eq!(limited.snapshot.scene, before.scene);
            assert_eq!(limited.snapshot.consumed_request, before.consumed_request);
            assert_eq!(
                limited.lookup(initial, 1),
                Ok(NativeRequestLookup::Accepted(note))
            );
            assert_eq!(
                limited.lookup(note.binding, 2),
                Ok(NativeRequestLookup::Unseen)
            );
        }

        limited.snapshot.limits.max_work = exact_work;
        let receipt = move_graphic(&mut limited).unwrap();
        assert_eq!(receipt.binding, next_binding);
        assert_eq!(limited.output_bytes(), full.output_bytes());
        assert_eq!(
            limited.lookup(note.binding, 2),
            Ok(NativeRequestLookup::Accepted(receipt))
        );
    }

    #[test]
    fn rewrite_spare_capacity_stays_charged_during_reopen_and_remap() {
        let bytes = super::super::tests::redaction_fixture("q Q");
        let defaults = NativeEditLimits::default();
        let mut staged = NativePdfGraph::open(&bytes, defaults).unwrap();
        let ids = initial_ids(&staged);
        staged.limits.max_output_bytes = 1024;
        let roomy = staged.rewrite().unwrap();
        staged.limits.max_output_bytes = roomy.len();
        let exact = staged.rewrite().unwrap();
        assert_eq!(exact, roomy);
        assert!(exact.capacity() < roomy.capacity());

        let mut lower = 1;
        let mut upper = staged.remaining_retained();
        while lower < upper {
            let mid = lower + (upper - lower) / 2;
            if rewrite_and_remap(&staged, mid, &ids).is_ok() {
                upper = mid;
            } else {
                lower = mid + 1;
            }
        }
        let accepted = rewrite_and_remap(&staged, lower, &ids).unwrap();
        assert_eq!(accepted.reopened.original_bytes(), exact.as_slice());
        assert_eq!(accepted.ids, ids);
        assert_eq!(
            accepted.reopened.limits.max_retained_bytes,
            lower - exact.capacity()
        );

        staged.limits.max_output_bytes = 1024;
        let refusal = rewrite_and_remap(&staged, lower, &ids).err();
        assert!(
            matches!(
                refusal,
                Some(
                    NativeSessionError::Budget
                        | NativeSessionError::Graph(NativeEditError::Reader(
                            PdfReadError::InputSizeLimit
                        ))
                )
            ),
            "unexpected retained-capacity result: {refusal:?}"
        );
        let with_spare =
            rewrite_and_remap(&staged, lower + roomy.capacity() - exact.capacity(), &ids).unwrap();
        assert_eq!(with_spare.output.capacity(), roomy.capacity());
        assert_eq!(
            with_spare.reopened.remaining_retained(),
            accepted.reopened.remaining_retained()
        );
    }

    #[test]
    fn preservation_uses_only_work_left_by_serialization_and_reopen() {
        let bytes = super::super::tests::redaction_fixture("q Q");
        let mut staged = NativePdfGraph::open(&bytes, NativeEditLimits::default()).unwrap();
        let ids = initial_ids(&staged);
        let workspace = staged.remaining_retained();
        let full = rewrite_and_remap(&staged, workspace, &ids).unwrap();
        let needed = staged.remaining_work() - full.reopened.remaining_work();

        staged
            .charge_page_work(staged.remaining_work() - needed)
            .unwrap();
        let validated = rewrite_with_workspace(&staged, workspace).unwrap();
        assert_eq!(validated.bytes, full.output);
        let exact = rewrite_and_remap(&staged, workspace, &ids).unwrap();
        assert_eq!(exact.reopened.remaining_work(), 0);
        assert_eq!(exact.ids, full.ids);
        staged.charge_page_work(1).unwrap();
        assert!(rewrite_with_workspace(&staged, workspace).is_ok());
        assert!(matches!(
            rewrite_and_remap(&staged, workspace, &ids),
            Err(NativeSessionError::Budget)
        ));
    }

    #[test]
    fn redaction_mark_growth_charges_both_buffers_and_the_live_candidate() {
        let source = super::super::tests::redaction_fixture("10 10 10 10 re f 40 40 10 10 re f");
        let mut session =
            NativePdfSession::open_scoped(&source, NativeEditLimits::default(), [1; 16]).unwrap();
        let binding = session.snapshot.binding;
        let page_id = session.snapshot.page_state.order[0];
        let first = NativeRedactionMark {
            page_id,
            geometry: session.redaction_mark_geometry(page_id).unwrap(),
            rect: [12_000, 12_000, 13_000, 13_000],
        };
        let second = NativeRedactionMark {
            rect: [42_000, 42_000, 43_000, 43_000],
            ..first
        };
        session.add_redaction_mark(binding, 1, first).unwrap();
        assert_eq!(session.redaction_marks.capacity(), 1);
        let last_mark = session.last_redaction_mark();
        let staged = session.stage_redaction(binding, 2).unwrap();
        assert_eq!(session.check_redaction_admission(), Ok(()));
        let peak = session.candidate.as_ref().unwrap().retained()
            + 3 * std::mem::size_of::<NativeRedactionMark>();
        session.remaining_retained = peak - 1;

        session.add_redaction_mark(binding, 1, first).unwrap();
        assert_eq!(
            session.add_redaction_mark(binding, 3, second),
            Err(NativeSessionError::Budget)
        );
        assert_eq!(session.snapshot.binding, binding);
        assert_eq!(session.snapshot.output_hash, binding.origin_hash);
        assert_eq!(session.output_bytes(), source);
        assert_eq!(session.redaction_marks(), &[first]);
        assert_eq!(session.redaction_marks_binding(), Some(binding));
        assert_eq!(session.redaction_marks.capacity(), 1);
        assert_eq!(session.last_redaction_mark(), last_mark);
        assert_eq!(session.stage_redaction(binding, 2), Ok(staged));

        session.remaining_retained = peak;
        session.add_redaction_mark(binding, 3, second).unwrap();
        assert_eq!(session.redaction_marks(), &[first, second]);
        assert_eq!(session.last_redaction_mark().unwrap().request_id, 3);
        assert!(session.redaction_candidate_status().is_none());
    }

    #[test]
    fn redaction_publication_charges_spare_capacity_to_the_accepted_session() {
        let source = super::super::tests::redaction_fixture("10 10 10 10 re f 40 40 10 10 re f");
        let verified_session = |max_output_bytes| {
            let limits = NativeEditLimits {
                max_retained_bytes: 512 * 1024,
                max_output_bytes,
                ..NativeEditLimits::default()
            };
            let mut session = NativePdfSession::open_scoped(&source, limits, [1; 16]).unwrap();
            let binding = session.snapshot.binding;
            let page_id = session.snapshot.page_state.order[0];
            let mark = NativeRedactionMark {
                page_id,
                geometry: session.redaction_mark_geometry(page_id).unwrap(),
                rect: [12_000, 12_000, 13_000, 13_000],
            };
            session.add_redaction_mark(binding, 1, mark).unwrap();
            session.stage_redaction(binding, 2).unwrap();
            let before_verification = session.redaction_workspace().unwrap();
            assert_eq!(session.check_redaction_admission(), Ok(()));
            assert!(session.redaction_admission.get().is_some());
            let evidence = session.verify_redaction(binding, 2).unwrap();
            assert!(evidence.checks_passed(), "{evidence:?}");
            let evidence_bytes = evidence.retained();
            assert!(evidence_bytes > 0);
            assert!(session.redaction_admission.get().is_none());
            assert_eq!(
                session.redaction_workspace().unwrap() + evidence_bytes,
                before_verification
            );
            assert_eq!(session.check_redaction_admission(), Ok(()));
            assert_eq!(
                session.verify_redaction(binding, 2).unwrap().retained(),
                evidence_bytes
            );
            assert!(session.redaction_admission.get().is_some());
            session
        };

        let mut roomy = verified_session(1024);
        let binding = roomy.snapshot.binding;
        let identity = roomy.redaction_candidate_status().unwrap().identity;
        let candidate = roomy.candidate.as_ref().unwrap();
        let expected_bytes = candidate
            .verified_bytes(roomy.redaction_context(2), roomy.redaction_marks())
            .unwrap()
            .to_vec();
        let mark = roomy.redaction_marks()[0];
        let available = roomy.remaining_retained;
        let publish_reserve = roomy.mark_bytes() + candidate.retained();
        roomy.remaining_retained = publish_reserve - 1;
        assert_eq!(
            roomy.publish_redaction(binding, 2),
            Err(NativeSessionError::Budget)
        );
        assert_eq!(roomy.snapshot.binding, binding);
        assert_eq!(roomy.snapshot.output_hash, binding.origin_hash);
        assert_eq!(roomy.output_bytes(), source);
        assert_eq!(roomy.redaction_marks(), &[mark]);
        assert_eq!(
            roomy.redaction_candidate_status().unwrap().identity,
            identity
        );
        assert!(roomy.accepted_redaction_evidence().is_none());
        roomy.remaining_retained = available;

        let receipt = roomy.publish_redaction(binding, 2).unwrap();
        assert_eq!(receipt.output_hash, identity.candidate_hash);
        assert_eq!(roomy.output_bytes(), expected_bytes);
        assert_eq!(roomy.origin_bytes(), source);

        let mut exact = verified_session(expected_bytes.len());
        exact.publish_redaction(exact.snapshot.binding, 2).unwrap();
        assert_eq!(exact.output_bytes(), expected_bytes);
        assert_eq!(exact.graph.source.capacity(), expected_bytes.len());
        let spare = roomy.graph.source.capacity() - exact.graph.source.capacity();
        assert!(spare > 0);
        assert_eq!(roomy.remaining_retained + spare, exact.remaining_retained);
    }

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
    fn combined_projections_and_hashing_refuse_without_changing_the_accepted_revision() {
        let limits = NativeEditLimits::default();
        let paint = format!("%{}\n10 10 10 10 re f", " ".repeat(4096));
        let source = super::super::tests::redaction_fixture(&paint);
        let mut graph = NativePdfGraph::open(&source, limits).unwrap();
        let page = graph.pages[0];
        pages::dict_mut(&mut graph, page)
            .unwrap()
            .insert("Resources".into(), PdfValue::Dictionary(BTreeMap::new()));
        let source = graph.rewrite().unwrap();
        let setup = || {
            let mut session = NativePdfSession::open_scoped(&source, limits, [7; 16]).unwrap();
            let field = session
                .apply_field(
                    session.snapshot.binding,
                    1,
                    &NativeFieldCommand::AddCheckboxField {
                        page: 0,
                        rect: [30, 30, 45, 45],
                        name: "approved".into(),
                        checked: false,
                    },
                )
                .unwrap();
            let note = session
                .apply(
                    field.binding,
                    2,
                    &NativeNoteCommand::Add {
                        page: NativePageId(0),
                        rect: [60, 60, 80, 80],
                        contents: "Review".into(),
                    },
                )
                .unwrap();
            (session, field.binding, note)
        };
        let (mut full, _, full_note) = setup();
        let edit = NativeNoteCommand::Edit {
            note: full_note.note,
            contents: "Ї".repeat(1024),
        };
        let edited = full.apply(full_note.binding, 3, &edit).unwrap();
        let (mut tight, prior_binding, note) = setup();
        assert_eq!(note.note, full_note.note);
        let binding = NativeBinding {
            revision: edited.binding.revision,
            ..note.binding
        };

        let graph = &full.graph;
        let allowance = graph.remaining_work();
        let mut retained = graph.remaining_retained();
        let mut work = allowance;
        let pages = notes::project(graph, &full.ids, &mut retained, &mut work).unwrap();
        let notes_work = allowance - work;
        let hash = full.snapshot.output_hash;
        work = allowance;
        let text_pages = text::project(
            graph,
            &pages,
            hash,
            binding,
            &mut retained,
            &mut work,
            text::WorkExhaustion::BudgetError,
        )
        .unwrap();
        let text_work = allowance - work;
        work = allowance;
        let objects =
            objects::project(graph, &pages, hash, binding, &mut retained, &mut work, None).unwrap();
        assert_eq!(objects[0].objects.len(), 1);
        let objects_work = allowance - work;
        work = allowance;
        let fields = forms::project(graph, &pages, binding, &mut retained, &mut work).unwrap();
        assert_eq!(fields[0].checked, Some(false));
        let forms_work = allowance - work;
        work = allowance;
        pages::restrictions(graph, &pages, &Workspace::new(retained), &mut work).unwrap();
        let restrictions_work = allowance - work;
        work = allowance;
        let scene = NativePdfNoteScene::new(hash, pages, text_pages, objects, &mut work).unwrap();
        let hash_work = allowance - work;
        assert!(scene.text_pages.iter().all(|page| page.islands.is_empty()));

        let scene_work = notes_work + text_work + objects_work + hash_work;
        let (projected_scene, _, projected_work) = project_scene(
            graph,
            &full.ids,
            hash,
            binding,
            graph.remaining_retained(),
            None,
        )
        .unwrap();
        assert_eq!(projected_scene, scene);
        assert_eq!(allowance - projected_work, scene_work);
        let costs = [
            notes_work,
            text_work,
            objects_work,
            hash_work,
            forms_work,
            restrictions_work,
        ];
        let combined = costs.iter().sum::<usize>();
        assert!(costs.into_iter().all(|cost| (1..combined).contains(&cost)));

        let output = full.output_bytes();
        let detached = NativePdfGraph::open(output, limits).unwrap();
        let snapshot = full.snapshot();
        let verify_detached = |limits| {
            NativePdfSession::verify_page_and_field_projection_with_note_identities(
                output,
                limits,
                binding,
                scene.scene_hash,
                Some(&scene.object_pages),
                &snapshot.page_state,
                &fields,
                projected_notes(&scene),
                false,
                None,
            )
        };
        assert_eq!(verify_detached(limits), Ok(true));
        let detached_work = limits.max_work - detached.remaining_work();
        assert_eq!(
            verify_detached(NativeEditLimits {
                max_work: detached_work + combined,
                ..limits
            }),
            Ok(true)
        );
        let before = tight.snapshot.clone();
        let accepted = tight.output_bytes().to_vec();
        // @invariant: A fresh session's one 32-byte object ID may add or remove up to 64 JSON bytes.
        let hash_work_spread = (2 * size_of::<[u8; 32]>()).div_ceil(16);
        for projection_allowance in [scene_work - 1, combined - 1] {
            tight.snapshot.limits.max_work = limits.max_work - allowance + projection_allowance;
            assert_eq!(
                tight.apply(note.binding, 3, &edit),
                Err(NativeSessionError::Budget)
            );
            assert_eq!(tight.output_bytes(), accepted);
            assert_eq!(tight.snapshot.output_hash, before.output_hash);
            assert_eq!(tight.snapshot.scene, before.scene);
            assert_eq!(tight.snapshot.binding, before.binding);
            assert_eq!(tight.snapshot.consumed_request, before.consumed_request);
            assert_eq!(
                tight.lookup(note.binding, 3),
                Ok(NativeRequestLookup::Unseen)
            );
            assert_eq!(
                tight.lookup(prior_binding, 2),
                Ok(NativeRequestLookup::Accepted(note))
            );

            let limits = NativeEditLimits {
                max_work: detached_work + projection_allowance,
                ..limits
            };
            let fresh_limits = NativeEditLimits {
                max_work: limits.max_work - hash_work_spread,
                ..limits
            };
            assert!(matches!(
                NativePdfSession::open_scoped(output, fresh_limits, [7; 16]),
                Err(NativeSessionError::Budget)
            ));
            assert_eq!(verify_detached(limits), Err(NativeSessionError::Budget));
        }
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

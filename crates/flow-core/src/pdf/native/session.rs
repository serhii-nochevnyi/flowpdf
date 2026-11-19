use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::reader::{PdfObjectRef, PdfValue};
use super::super::scene::{NativePdfAnnotation, NativePdfNoteScene};
use super::candidate::{
    Candidate, NativeRedactionCandidateStatus, NativeRedactionStage, evidence_reserve,
};
use super::forms::{self, NativeFieldCommand, NativeFieldRefusal, NativeFieldView};
use super::graph::{NativeEditError, NativeEditLimits, NativePdfGraph, collect_refs, spend};
use super::notes::{self, ExpectedNote, NativeNoteCommand, NativeNoteId};

use super::objects::{self, NativeObjectCommand, NativeObjectPage, NativeObjectRefusal};
use super::pages::{self, NativePageCommand, NativePageState};
use super::redaction;
use super::text::{self, NativeTextCommand, NativeTextPage, NativeTextRefusal};
use super::verifier::{
    NativeRedactionContext, NativeRedactionMark, VersionedNativeRedactionEvidence,
};
use super::writer::rewrite_with_workspace;

pub const NATIVE_REQUEST_HISTORY: usize = 128;
pub const NATIVE_MAX_NOTE_BYTES: usize = 16 * 1024;
const REQUEST_RECORD_BYTES: usize = 512;
const SESSION_BYTES: usize = 1024;
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

pub struct NativePdfSession {
    origin: Option<Vec<u8>>,
    graph: NativePdfGraph,
    ids: BTreeMap<PdfObjectRef, NativeNoteId>,
    snapshot: NativeSnapshot,
    records: VecDeque<RequestRecord>,
    remaining_retained: usize,
    next_page_object_id: u64,
    redaction_admission: OnceLock<bool>,
    redaction_marks: Vec<NativeRedactionMark>,
    redaction_marks_binding: Option<NativeBinding>,
    last_redaction_request: u64,
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
        let mut remaining = graph.remaining_retained();
        spend(&mut remaining, graph.objects.len() * OBJECT_ID_BYTES)?;
        let ids = initial_ids(&graph);
        let binding = NativeBinding {
            epoch,
            session_id,
            origin_hash,
            revision: 0,
        };
        let (scene, mut remaining_retained) = accepted_scene(
            &graph,
            &ids,
            origin_hash,
            binding,
            0,
            0,
            graph.remaining_retained(),
        )?;
        let fields = forms::project(&graph, &scene.pages, binding, &mut remaining_retained)?;
        let page_state = page_state(&graph, &ids, None, &scene, &mut remaining_retained)?;
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
            remaining_retained,
            next_page_object_id: FIRST_PAGE_OBJECT_ID,
            redaction_admission: OnceLock::new(),
            redaction_marks: Vec::new(),
            redaction_marks_binding: None,
            last_redaction_request: 0,
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
        super::verify_raster::accepted_page(self.output_bytes(), index, geometry)
            .map_err(|_| NativeSessionError::TargetUnavailable)
    }

    pub fn redaction_admitted(&self) -> bool {
        *self.redaction_admission.get_or_init(|| {
            NativePdfGraph::open(self.output_bytes(), self.snapshot.limits)
                .is_ok_and(|graph| redaction::admitted(&graph, &self.snapshot.page_state.order))
        })
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

    pub fn add_redaction_mark(
        &mut self,
        binding: NativeBinding,
        mark: NativeRedactionMark,
    ) -> Result<(), NativeSessionError> {
        self.check_current_binding(binding)?;
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
        if self.redaction_marks.contains(&mark) {
            return Ok(());
        }
        redaction::check_raster_budget(&self.graph)?;
        if self.redaction_marks.len() == redaction::MAX_MARKS {
            return Err(NativeSessionError::Budget);
        }
        let mut remaining = self.remaining_retained;
        spend(
            &mut remaining,
            self.candidate.as_ref().map_or(0, Candidate::retained),
        )?;
        spend(
            &mut remaining,
            (self
                .redaction_marks
                .capacity()
                .max(self.redaction_marks.len() + 1))
                * std::mem::size_of::<NativeRedactionMark>(),
        )?;
        self.redaction_marks
            .try_reserve_exact(1)
            .map_err(|_| NativeSessionError::Budget)?;
        self.candidate = None;
        self.redaction_marks.push(mark);
        self.redaction_marks_binding = Some(binding);
        Ok(())
    }

    pub fn remove_redaction_mark(
        &mut self,
        binding: NativeBinding,
        mark: NativeRedactionMark,
    ) -> Result<(), NativeSessionError> {
        self.check_current_binding(binding)?;
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
        Ok(())
    }

    pub fn clear_redaction_marks(
        &mut self,
        binding: NativeBinding,
    ) -> Result<(), NativeSessionError> {
        self.check_current_binding(binding)?;
        self.candidate = None;
        self.reset_marks();
        Ok(())
    }

    fn reset_marks(&mut self) {
        self.redaction_marks = Vec::new();
        self.redaction_marks_binding = None;
    }

    fn mark_bytes(&self) -> usize {
        self.redaction_marks.capacity() * std::mem::size_of::<NativeRedactionMark>()
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
        if request_id <= self.last_redaction_request {
            return Err(NativeSessionError::ReconciliationRequired);
        }
        redaction::check_raster_budget(&self.graph)?;
        if self.redaction_marks_binding != Some(binding) {
            return Err(NativeSessionError::InvalidRedactionMark);
        }
        self.last_redaction_request = request_id;
        let max_input = super::verify_structure::MAX_INPUT;
        let pages = self.snapshot.page_state.order.len();
        if self.origin_bytes().len().max(self.output_bytes().len()) > max_input {
            return Err(NativeSessionError::Budget);
        }
        let mut remaining = self.remaining_retained;
        spend(
            &mut remaining,
            self.mark_bytes() + evidence_reserve(pages) + pages * 128,
        )?;
        spend(&mut remaining, self.ids.len() * OBJECT_ID_BYTES)?;
        let mut limits = self.snapshot.limits;
        if binding.revision != 0 {
            limits.max_source_bytes = limits.max_output_bytes;
        }
        let output_budget = limits.max_output_bytes.min(max_input).min(remaining / 5);
        if output_budget == 0 {
            return Err(NativeSessionError::Budget);
        }
        limits.max_output_bytes = output_budget;
        limits.max_retained_bytes = remaining - output_budget;
        let mut staged = NativePdfGraph::open(self.output_bytes(), limits)?;
        redaction::scrub(
            &mut staged,
            &self.snapshot.page_state.order,
            &self.redaction_marks,
        )?;
        staged.limits.max_source_bytes = output_budget;
        let bytes = rewrite_with_workspace(&staged, staged.remaining_retained() + output_budget)?;

        let mut reopen_limits = staged.limits;
        reopen_limits.max_retained_bytes = staged.remaining_retained();
        reopen_limits.max_total_decoded_bytes = staged.remaining_decoded();
        if reopen_limits.max_retained_bytes == 0 || reopen_limits.max_total_decoded_bytes == 0 {
            return Err(NativeSessionError::Budget);
        }
        let reopened = NativePdfGraph::open(&bytes, reopen_limits)?;
        let ids = preserve_and_remap(&staged, &reopened, &self.ids)?;
        let candidate = Candidate::new(
            bytes,
            ids,
            self.redaction_context(request_id),
            &self.redaction_marks,
        );
        let mut remaining = self.remaining_retained;
        spend(&mut remaining, self.mark_bytes() + candidate.retained())?;
        let result = NativeRedactionStage::Pending(candidate.status().identity);
        self.candidate = Some(candidate);
        Ok(result)
    }

    pub fn verify_redaction(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<&VersionedNativeRedactionEvidence, NativeSessionError> {
        self.check_candidate(binding, request_id)?;
        let accepted = self.graph.original_bytes();
        let origin = self.origin.as_deref().unwrap_or(accepted);
        let mut remaining = self.remaining_retained;
        spend(&mut remaining, self.mark_bytes())?;
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
        // @invariant: Candidate retention includes the evidence accumulated across lanes.
        spend(&mut remaining, candidate.retained())?;
        Ok(candidate.verify(origin, accepted, context, &self.redaction_marks, remaining))
    }

    pub fn cancel_redaction(
        &mut self,
        binding: NativeBinding,
        request_id: u64,
    ) -> Result<(), NativeSessionError> {
        self.check_candidate(binding, request_id)?;
        self.candidate = None;
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
        let evidence_bytes = evidence_reserve(candidate.status().affected_pages.len());
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
        let (scene, mut scene_remaining) = accepted_scene(
            &reopened,
            candidate.ids(),
            output_hash,
            next_binding,
            self.origin_bytes().len(),
            record_count,
            reopened.remaining_retained(),
        )?;
        spend(&mut scene_remaining, evidence_bytes)?;
        let fields = forms::project(&reopened, &scene.pages, next_binding, &mut scene_remaining)?;
        let state = page_state(
            &reopened,
            candidate.ids(),
            self.snapshot.page_state.selected,
            &scene,
            &mut scene_remaining,
        )?;
        if !state.valid_for_count(scene.pages.len())
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
        let retained_refund =
            self.snapshot.limits.max_retained_bytes - limits.max_retained_bytes - source_spare;
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
        if request_id <= self.snapshot.consumed_request || request_id <= self.last_redaction_request
        {
            return Err(NativeSessionError::InvalidRequest);
        }
        if self.candidate.is_some() {
            return Err(NativeSessionError::ReconciliationRequired);
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
        let hash = *blake3::hash(bytes).as_bytes();
        if live.is_some_and(|session| {
            session.snapshot.binding != binding || session.snapshot.output_hash != hash
        }) {
            return Ok(false);
        }
        let graph = NativePdfGraph::open(bytes, limits)?;
        let initial;
        let ids = if let Some(session) = live {
            &session.ids
        } else {
            initial = initial_ids(&graph);
            &initial
        };
        let (scene, mut remaining) =
            accepted_scene(&graph, ids, hash, binding, 0, 0, graph.remaining_retained())?;
        let mut projected_notes = scene.pages.iter().flat_map(|page| {
            page.annotations
                .iter()
                .filter_map(move |annotation| match annotation {
                    NativePdfAnnotation::PlainNote(note) => {
                        Some((page.id.0, note.object, note.id.0))
                    }
                    _ => None,
                })
        });
        let mut seen = BTreeSet::new();
        let notes_match = claimed_notes
            .iter()
            .all(|&(claimed_page, claimed_object, id)| {
                let Some((page, object, projected_id)) = projected_notes.next() else {
                    return false;
                };
                let valid_id = if live.is_some() || binding.revision == 0 {
                    id == projected_id
                } else {
                    id != 0
                        && (id < FIRST_ADDED_NOTE_ID
                            || (id > FIRST_ADDED_NOTE_ID
                                && id - FIRST_ADDED_NOTE_ID <= binding.revision)
                            || (FIRST_PAGE_OBJECT_ID..PAGE_OBJECT_ID_LIMIT).contains(&id))
                };
                page == claimed_page && object == claimed_object && valid_id && seen.insert(id)
            })
            && projected_notes.next().is_none();
        let page_state_matches = if let Some(session) = live {
            *state == session.snapshot.page_state
        } else {
            let projected = page_state(&graph, ids, None, &scene, &mut remaining)?;
            state.restrictions == projected.restrictions
                && (binding.revision != 0 || *state == projected)
        };
        Ok(scene.scene_hash == claimed_scene
            && notes_match
            && state.valid_for_count(scene.pages.len())
            && page_state_matches
            && forms::project(&graph, &scene.pages, binding, &mut remaining)? == claimed_fields
            && redaction::admitted(&graph, &state.order) == redaction_admitted)
    }

    pub fn verify_page_projection(
        bytes: &[u8],
        limits: NativeEditLimits,
        binding: NativeBinding,
        claimed_scene: [u8; 32],
        state: &NativePageState,
    ) -> Result<bool, NativeSessionError> {
        let graph = NativePdfGraph::open(bytes, limits)?;
        let ids = initial_ids(&graph);
        let hash = *blake3::hash(bytes).as_bytes();
        let (scene, mut remaining) = accepted_scene(
            &graph,
            &ids,
            hash,
            binding,
            0,
            0,
            graph.remaining_retained(),
        )?;
        let projected = page_state(&graph, &ids, None, &scene, &mut remaining)?;
        Ok(scene.scene_hash == claimed_scene
            && state.valid_for_count(scene.pages.len())
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
        let graph = NativePdfGraph::open(bytes, limits)?;
        let ids = initial_ids(&graph);
        let hash = *blake3::hash(bytes).as_bytes();
        let (scene, _) = accepted_scene(
            &graph,
            &ids,
            hash,
            binding,
            0,
            0,
            graph.remaining_retained(),
        )?;
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
        let (scene, _) = accepted_scene(
            &graph,
            &ids,
            hash,
            binding,
            0,
            0,
            graph.remaining_retained(),
        )?;
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
            if command_digest(command).map(Some) != Ok(record.digest) {
                return Err(NativeSessionError::RequestConflict);
            }
            return Ok(record.receipt);
        }
        if binding.revision != self.snapshot.binding.revision {
            return Err(NativeSessionError::RevisionMismatch);
        }
        if request_id <= self.last_redaction_request {
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
        spend(
            &mut remaining,
            4096 + content_cost + annotation_cost + (self.ids.len() + 1) * OBJECT_ID_BYTES,
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
            Command::Field(command) => {
                let expected = forms::stage(
                    &mut staged,
                    &self.snapshot.scene.pages,
                    &self.snapshot.fields,
                    command,
                )?;
                ids.retain(|reference, _| staged.objects.contains_key(reference));
                for reference in staged.objects.keys() {
                    ids.entry(*reference).or_insert(NativeNoteId(0));
                }
                Expected::Field(expected)
            }
            Command::Page(command) => {
                let target = command.target();
                let index = target.page as usize;
                let selected =
                    pages::stage(&mut staged, command, index, &self.snapshot.scene.pages)?;
                ids.retain(|reference, _| staged.objects.contains_key(reference));
                for reference in staged.objects.keys() {
                    if let std::collections::btree_map::Entry::Vacant(entry) = ids.entry(*reference)
                    {
                        if next_page_object_id >= PAGE_OBJECT_ID_LIMIT {
                            return Err(NativeSessionError::Budget);
                        }
                        entry.insert(NativeNoteId(next_page_object_id));
                        next_page_object_id += 1;
                    }
                }
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
        let staged_remaining = staged_remaining.unwrap_or_else(|| staged.remaining_retained());
        let output = rewrite_with_workspace(&staged, staged_remaining + output_budget)?;
        #[cfg(test)]
        let output = if self.fail_at == Some(FailurePoint::Reopen) {
            Vec::new()
        } else {
            output
        };
        let mut reopen_limits = self.snapshot.limits;
        reopen_limits.max_source_bytes = reopen_limits.max_output_bytes;
        reopen_limits.max_total_decoded_bytes = staged.remaining_decoded();
        reopen_limits.max_retained_bytes = staged_remaining;
        let (reopened, ids) = reopen_rewrite(&staged, &output, output_budget, reopen_limits, &ids)?;
        let output_hash = *blake3::hash(&output).as_bytes();
        let output_capacity = output.capacity();
        drop(staged);
        drop(output);
        let scene_limit = reopened.limits.max_retained_bytes + output_capacity;
        let next_binding = NativeBinding {
            revision,
            ..binding
        };
        let (scene, mut scene_remaining) = accepted_scene(
            &reopened,
            &ids,
            output_hash,
            next_binding,
            self.origin_bytes().len(),
            record_count,
            reopened.remaining_retained() + output_capacity,
        )?;
        let fields = forms::project(&reopened, &scene.pages, next_binding, &mut scene_remaining)?;
        let selected_page = match &expected {
            Expected::Page(id) => Some(*id),
            _ => None,
        };
        let (note, text_island, object) = match expected {
            Expected::Page(_) => (NativeNoteId(0), None, None),
            Expected::Field(expected) => {
                expected.verify(&fields)?;
                (NativeNoteId(0), None, None)
            }
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
            selected_page,
            output_hash,
            scene_hash: scene.scene_hash,
        };
        self.records
            .try_reserve_exact(usize::from(self.records.len() < NATIVE_REQUEST_HISTORY))
            .map_err(|_| NativeSessionError::Budget)?;

        let state = page_state(
            &reopened,
            &ids,
            selected_page.or(self.snapshot.page_state.selected),
            &scene,
            &mut scene_remaining,
        )?;
        if !state.valid_for_count(scene.pages.len()) {
            return Err(NativeSessionError::Postcondition);
        }
        if let Command::Page(command) = command {
            self.verify_page_geometry(command, &scene, &state)?;
        }
        let previous = std::mem::replace(&mut self.graph, reopened);
        self.redaction_admission.take();
        if self.origin.is_none() {
            self.origin = Some(previous.source);
        }
        self.remaining_retained =
            self.snapshot.limits.max_retained_bytes - scene_limit + scene_remaining;
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
    ) -> Result<(), NativeSessionError> {
        let target = command.target();
        let prior = &self.snapshot;
        let positions: BTreeMap<_, _> = prior
            .page_state
            .order
            .iter()
            .copied()
            .zip(&prior.scene.pages)
            .collect();
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
) -> Result<NativePageState, NativeSessionError> {
    use pages::NativePageCapability;
    let capability_bytes = graph.pages.len() * std::mem::size_of::<NativePageCapability>();
    spend(remaining, capability_bytes)?;
    let restrictions = pages::restrictions(graph, &scene.pages)?;
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

fn accepted_scene(
    graph: &NativePdfGraph,
    ids: &BTreeMap<PdfObjectRef, NativeNoteId>,
    output_hash: [u8; 32],
    binding: NativeBinding,
    separate_origin: usize,
    request_count: usize,
    mut remaining: usize,
) -> Result<(NativePdfNoteScene, usize), NativeSessionError> {
    spend(&mut remaining, separate_origin)?;
    spend(
        &mut remaining,
        SESSION_BYTES
            + ids.len() * OBJECT_ID_BYTES
            + request_count * REQUEST_RECORD_BYTES
            + graph.pages.len() * std::mem::size_of::<u64>(),
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

fn reopen_rewrite(
    staged: &NativePdfGraph,
    output: &Vec<u8>,
    output_budget: usize,
    mut limits: NativeEditLimits,
    ids: &BTreeMap<PdfObjectRef, NativeNoteId>,
) -> Result<(NativePdfGraph, BTreeMap<PdfObjectRef, NativeNoteId>), NativeSessionError> {
    let refund = output_budget
        .checked_sub(output.capacity())
        .ok_or(NativeSessionError::Budget)?;
    limits.max_retained_bytes += refund;
    if limits.max_retained_bytes == 0 {
        return Err(NativeSessionError::Budget);
    }
    let reopened = NativePdfGraph::open(output, limits)?;
    let ids = preserve_and_remap(staged, &reopened, ids)?;
    Ok((reopened, ids))
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
    fn rewrite_spare_capacity_stays_charged_during_reopen_and_remap() {
        let encoded = miniz_oxide::deflate::compress_to_vec_zlib(b"q Q", 6);
        let bytes = super::super::tests::fixture("", &encoded);
        let defaults = NativeEditLimits::default();
        let mut staged = NativePdfGraph::open(&bytes, defaults).unwrap();
        staged.limits.max_output_bytes = 1024;
        let roomy = staged.rewrite().unwrap();
        staged.limits.max_output_bytes = roomy.len();
        let exact = staged.rewrite().unwrap();
        assert_eq!(exact, roomy);
        assert!(exact.capacity() < roomy.capacity());

        let ids = initial_ids(&staged);
        let reopened = NativePdfGraph::open(&exact, defaults).unwrap();
        let open_cost = defaults.max_retained_bytes - reopened.remaining_retained();
        let remap_cost = staged.objects.len() * 512 + 7 * 128;
        let output_budget = roomy.capacity();
        let spare = output_budget - exact.capacity();
        let limits = NativeEditLimits {
            max_retained_bytes: open_cost + remap_cost - spare,
            ..defaults
        };
        assert!(NativePdfGraph::open(&roomy, limits).is_ok());
        assert!(matches!(
            reopen_rewrite(&staged, &roomy, output_budget, limits, &ids),
            Err(NativeSessionError::Budget)
        ));
        let (reopened, remapped) =
            reopen_rewrite(&staged, &exact, output_budget, limits, &ids).unwrap();
        assert_eq!(reopened.original_bytes(), exact.as_slice());
        assert_eq!(remapped.len(), ids.len());
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
            session.add_redaction_mark(binding, mark).unwrap();
            session.stage_redaction(binding, 1).unwrap();
            let evidence = session.verify_redaction(binding, 1).unwrap();
            assert!(evidence.checks_passed(), "{evidence:?}");
            session
        };

        let mut roomy = verified_session(1024);
        let binding = roomy.snapshot.binding;
        let identity = roomy.redaction_candidate_status().unwrap().identity;
        let candidate = roomy.candidate.as_ref().unwrap();
        let expected_bytes = candidate
            .verified_bytes(roomy.redaction_context(1), roomy.redaction_marks())
            .unwrap()
            .to_vec();
        let mark = roomy.redaction_marks()[0];
        let available = roomy.remaining_retained;
        let publish_reserve = roomy.mark_bytes() + candidate.retained();
        roomy.remaining_retained = publish_reserve - 1;
        assert_eq!(
            roomy.publish_redaction(binding, 1),
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

        let receipt = roomy.publish_redaction(binding, 1).unwrap();
        assert_eq!(receipt.output_hash, identity.candidate_hash);
        assert_eq!(roomy.output_bytes(), expected_bytes);
        assert_eq!(roomy.origin_bytes(), source);

        let mut exact = verified_session(expected_bytes.len());
        exact.publish_redaction(exact.snapshot.binding, 1).unwrap();
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

use std::collections::BTreeMap;
use std::mem::size_of;

use serde::Serialize;

use super::super::reader::PdfObjectRef;
use super::session::OBJECT_ID_BYTES;
use super::verifier::verify_with_workspace;
use super::{
    NATIVE_REDACTION_VALIDATOR_PROFILE, NATIVE_REDACTION_VALIDATOR_VERSION, NativeBinding,
    NativeEditReceipt, NativeNoteId, NativeRedactionContext, NativeRedactionMark,
    NativeRedactionPageEvidence, NativeRedactionPolicy, NativeVerificationStatus,
    VersionedNativeRedactionEvidence,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeRedactionCandidateIdentity {
    pub binding: NativeBinding,
    pub request_id: u64,
    pub accepted_hash: [u8; 32],
    pub candidate_hash: [u8; 32],
    pub validator_version: u32,
    pub validator_profile: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRedactionStage {
    Pending(NativeRedactionCandidateIdentity),
    Accepted(NativeEditReceipt),
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeRedactionCandidateStatus<'a> {
    pub identity: NativeRedactionCandidateIdentity,
    pub affected_pages: &'a [NativeRedactionPageEvidence],
    pub evidence: Option<&'a VersionedNativeRedactionEvidence>,
}

impl NativeRedactionCandidateStatus<'_> {
    pub fn status(self) -> NativeVerificationStatus {
        use NativeVerificationStatus::*;
        let Some(evidence) = self.evidence else {
            return Pending;
        };
        if evidence.checks_passed() {
            return if evidence.identity().is_some_and(|identity| {
                identity.binding() == self.identity.binding
                    && identity.request_id() == self.identity.request_id
                    && identity.accepted_hash() == self.identity.accepted_hash
                    && identity.candidate_hash() == self.identity.candidate_hash
                    && identity.validator_version() == self.identity.validator_version
                    && identity.validator_profile() == self.identity.validator_profile
                    && identity.affected_pages() == self.affected_pages
            }) {
                Passed
            } else {
                Failed
            };
        }
        let lanes = [
            evidence.structure().status(),
            evidence.text().status(),
            evidence.raster().status(),
        ];
        if evidence.identity_failure().is_some() || lanes.contains(&Failed) {
            Failed
        } else if lanes.contains(&Unavailable) {
            Unavailable
        } else {
            Inconclusive
        }
    }
}

pub(super) struct Candidate {
    identity: NativeRedactionCandidateIdentity,
    affected: Vec<NativeRedactionPageEvidence>,
    bytes: Vec<u8>,
    ids: BTreeMap<PdfObjectRef, NativeNoteId>,
    evidence: Option<VersionedNativeRedactionEvidence>,
}

impl Candidate {
    // @invariant: Construct only after the scrubbed closure and exact rewrite agree.
    pub(super) fn new(
        bytes: Vec<u8>,
        ids: BTreeMap<PdfObjectRef, NativeNoteId>,
        context: NativeRedactionContext<'_>,
        affected: Vec<NativeRedactionPageEvidence>,
    ) -> Self {
        Self {
            identity: NativeRedactionCandidateIdentity {
                binding: context.binding,
                request_id: context.request_id,
                accepted_hash: context.accepted_hash,
                candidate_hash: *blake3::hash(&bytes).as_bytes(),
                validator_version: NATIVE_REDACTION_VALIDATOR_VERSION,
                validator_profile: NATIVE_REDACTION_VALIDATOR_PROFILE,
            },
            affected,
            bytes,
            ids,
            evidence: None,
        }
    }

    pub(super) fn status(&self) -> NativeRedactionCandidateStatus<'_> {
        NativeRedactionCandidateStatus {
            identity: self.identity,
            affected_pages: &self.affected,
            evidence: self.evidence.as_ref(),
        }
    }

    pub(super) fn retained(&self) -> usize {
        self.bytes.capacity()
            + self.ids.len() * OBJECT_ID_BYTES
            + self.affected.capacity() * size_of::<NativeRedactionPageEvidence>()
            + self
                .evidence
                .as_ref()
                .map_or(0, VersionedNativeRedactionEvidence::retained)
    }

    fn policy<'a>(&self, marks: &'a [NativeRedactionMark]) -> NativeRedactionPolicy<'a> {
        NativeRedactionPolicy {
            binding: self.identity.binding,
            request_id: self.identity.request_id,
            candidate_hash: self.identity.candidate_hash,
            marks,
        }
    }

    pub(super) fn verify(
        &mut self,
        origin: &[u8],
        accepted: &[u8],
        context: NativeRedactionContext<'_>,
        marks: &[NativeRedactionMark],
        allowance: usize,
    ) -> &VersionedNativeRedactionEvidence {
        let policy = self.policy(marks);
        self.evidence.get_or_insert_with(|| {
            verify_with_workspace(origin, accepted, &self.bytes, context, policy, allowance)
        })
    }

    // @security: Only the internally acquired verdict can authorize access to quarantined bytes.
    pub(super) fn verified_bytes(
        &self,
        context: NativeRedactionContext<'_>,
        marks: &[NativeRedactionMark],
    ) -> Option<&[u8]> {
        let evidence = self.evidence.as_ref()?;
        if self.status().status() != NativeVerificationStatus::Passed
            || !evidence.eligible_for(&self.bytes, context, self.policy(marks))
        {
            return None;
        }
        Some(&self.bytes)
    }

    pub(super) fn into_verified(
        self,
        context: NativeRedactionContext<'_>,
        marks: &[NativeRedactionMark],
    ) -> Option<PublishedCandidate> {
        self.verified_bytes(context, marks)?;
        Some(PublishedCandidate {
            bytes: self.bytes,
            ids: self.ids,
            evidence: self.evidence?,
        })
    }

    pub(super) fn ids(&self) -> &BTreeMap<PdfObjectRef, NativeNoteId> {
        &self.ids
    }
}

pub(super) struct PublishedCandidate {
    pub bytes: Vec<u8>,
    pub ids: BTreeMap<PdfObjectRef, NativeNoteId>,
    pub evidence: VersionedNativeRedactionEvidence,
}

#[cfg(test)]
mod tests {
    use super::super::verifier::{STANDALONE_WORKSPACE, Workspace};
    use super::super::{NativeRedactionGeometry, tests::redaction_fixture};
    use super::*;

    #[test]
    fn quarantine_requires_matching_verdict_identity_coverage_and_bytes() {
        let source = redaction_fixture("10 10 10 10 re f 40 40 10 10 re f");
        let exact = redaction_fixture("40 40 10 10 re f");
        let other = redaction_fixture("55 55 10 10 re f");
        let context = NativeRedactionContext {
            binding: NativeBinding {
                epoch: [5; 16],
                session_id: 1,
                revision: 0,
                origin_hash: *blake3::hash(&source).as_bytes(),
            },
            request_id: 2,
            accepted_hash: *blake3::hash(&source).as_bytes(),
            page_order: &[3],
        };
        let marks = [NativeRedactionMark {
            page_id: 3,
            geometry: NativeRedactionGeometry {
                media_box: [0, 0, 100_000, 100_000],
                crop_box: [0, 0, 100_000, 100_000],
                rotation: 0,
            },
            rect: [12_000, 12_000, 13_000, 13_000],
        }];
        let workspace = Workspace::new(STANDALONE_WORKSPACE);
        let mut page_bytes = workspace.scope();
        let mut new_candidate = |bytes| {
            let affected = page_bytes
                .collect_affected_pages(context.page_order, &marks)
                .unwrap();
            Candidate::new(bytes, BTreeMap::new(), context, affected)
        };
        let mut candidate = new_candidate(exact.clone());
        assert_eq!(
            candidate.status().status(),
            NativeVerificationStatus::Pending
        );
        assert!(candidate.verified_bytes(context, &marks).is_none());

        let mut mismatch = new_candidate(other.clone());
        assert!(
            !mismatch
                .verify(&source, &source, context, &marks, STANDALONE_WORKSPACE)
                .checks_passed()
        );
        assert_eq!(mismatch.status().status(), NativeVerificationStatus::Failed);
        assert!(mismatch.into_verified(context, &marks).is_none());

        assert!(
            candidate
                .verify(&source, &source, context, &marks, STANDALONE_WORKSPACE)
                .checks_passed()
        );
        assert_eq!(
            candidate.status().status(),
            NativeVerificationStatus::Passed
        );
        assert_eq!(
            candidate.verified_bytes(context, &marks),
            Some(exact.as_slice())
        );
        let identity = candidate.identity;
        candidate.identity.validator_version += 1;
        assert_eq!(
            candidate.status().status(),
            NativeVerificationStatus::Failed
        );
        assert!(candidate.verified_bytes(context, &marks).is_none());
        candidate.identity = identity;
        candidate.identity.validator_profile = "different-profile";
        assert!(candidate.verified_bytes(context, &marks).is_none());
        candidate.identity = identity;
        candidate.identity.candidate_hash = *blake3::hash(&other).as_bytes();
        assert!(candidate.verified_bytes(context, &marks).is_none());
        candidate.identity = identity;
        let coverage = std::mem::take(&mut candidate.affected);
        assert!(candidate.verified_bytes(context, &marks).is_none());
        candidate.affected = coverage;
        assert!(
            candidate
                .verified_bytes(
                    NativeRedactionContext {
                        request_id: 3,
                        ..context
                    },
                    &marks
                )
                .is_none()
        );
        assert!(candidate.verified_bytes(context, &[]).is_none());
        candidate.bytes = other;
        assert!(candidate.into_verified(context, &marks).is_none());
    }
}

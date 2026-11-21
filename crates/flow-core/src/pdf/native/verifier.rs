// @contract: Local readback is independent of rewriting and shares the bounded PDF parser, codecs and font primitives.
// @contract: Independent release differentials cover shared defects and PDF consumers outside this profile.
use std::cell::Cell;
use std::collections::BTreeSet;
use std::mem::size_of;

use serde::Serialize;

use super::{NativeBinding, verify_raster, verify_structure, verify_text};

pub const NATIVE_REDACTION_VALIDATOR_VERSION: u32 = 1;
pub const NATIVE_REDACTION_VALIDATOR_PROFILE: &str = "classic-xref-owned-144dpi-v1";
pub(super) const POINT: i64 = 1_000;
pub(super) const MAX_COORD: i64 = 1_000_000 * POINT;
pub(super) const MAX_PAGES: usize = 256;
const MAX_MARKS: usize = 1024;
pub(super) const STANDALONE_WORKSPACE: usize = 512 * 1024 * 1024;

pub(super) type Rect = [i64; 4];
pub(super) type VerifyResult<T> = Result<T, NativeVerificationFailure>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeRedactionGeometry {
    pub media_box: [i64; 4],
    pub crop_box: [i64; 4],
    pub rotation: u16,
}

// @contract: Mark rectangles use unrotated PDF coordinates in thousandths of a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeRedactionMark {
    pub page_id: u64,
    pub geometry: NativeRedactionGeometry,
    pub rect: [i64; 4],
}

#[derive(Clone, Copy)]
pub struct NativeRedactionContext<'a> {
    pub binding: NativeBinding,
    pub request_id: u64,
    pub accepted_hash: [u8; 32],
    pub page_order: &'a [u64],
}

#[derive(Clone, Copy)]
pub struct NativeRedactionPolicy<'a> {
    pub binding: NativeBinding,
    pub request_id: u64,
    pub candidate_hash: [u8; 32],
    pub marks: &'a [NativeRedactionMark],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeRedactionPageEvidence {
    pub page_id: u64,
    pub geometry: NativeRedactionGeometry,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeRedactionEvidenceIdentity {
    binding: NativeBinding,
    request_id: u64,
    accepted_hash: [u8; 32],
    candidate_hash: [u8; 32],
    validator_version: u32,
    validator_profile: &'static str,
    policy_hash: [u8; 32],
    affected_pages: Vec<NativeRedactionPageEvidence>,
}

impl NativeRedactionEvidenceIdentity {
    pub fn binding(&self) -> NativeBinding {
        self.binding
    }

    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    pub fn accepted_hash(&self) -> [u8; 32] {
        self.accepted_hash
    }

    pub fn candidate_hash(&self) -> [u8; 32] {
        self.candidate_hash
    }

    pub fn validator_version(&self) -> u32 {
        self.validator_version
    }

    pub fn validator_profile(&self) -> &'static str {
        self.validator_profile
    }

    pub fn affected_pages(&self) -> &[NativeRedactionPageEvidence] {
        &self.affected_pages
    }

    fn matches(
        &self,
        context: NativeRedactionContext<'_>,
        policy: NativeRedactionPolicy<'_>,
    ) -> bool {
        self.binding == context.binding
            && self.binding == policy.binding
            && self.request_id == context.request_id
            && self.request_id == policy.request_id
            && self.accepted_hash == context.accepted_hash
            && self.candidate_hash == policy.candidate_hash
            && self.validator_version == NATIVE_REDACTION_VALIDATOR_VERSION
            && self.validator_profile == NATIVE_REDACTION_VALIDATOR_PROFILE
            && context.page_order.len() <= MAX_PAGES
            && policy.marks.len() <= MAX_MARKS
            && self.policy_hash == policy_hash(context.page_order, policy.marks)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeVerificationStatus {
    Pending,
    Passed,
    Failed,
    Unavailable,
    Inconclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeVerificationFailure {
    Identity,
    Coverage,
    Structure,
    UnsupportedProfile,
    Budget,
    TextMismatch,
    RasterMismatch,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeVerificationLane {
    status: NativeVerificationStatus,
    failure: Option<NativeVerificationFailure>,
    covered_pages: Vec<NativeRedactionPageEvidence>,
}

impl NativeVerificationLane {
    pub fn status(&self) -> NativeVerificationStatus {
        self.status
    }

    pub fn failure(&self) -> Option<NativeVerificationFailure> {
        self.failure
    }

    pub fn covered_pages(&self) -> &[NativeRedactionPageEvidence] {
        &self.covered_pages
    }

    fn pending() -> Self {
        Self {
            status: NativeVerificationStatus::Pending,
            failure: None,
            covered_pages: Vec::new(),
        }
    }

    fn from_result(
        result: VerifyResult<Vec<NativeRedactionPageEvidence>>,
        expected: &[NativeRedactionPageEvidence],
    ) -> Self {
        use NativeVerificationFailure::{Budget, Coverage, UnsupportedProfile};
        use NativeVerificationStatus::{Failed, Inconclusive, Passed};
        let (status, failure, covered_pages) = match result {
            Ok(pages) if pages == expected => (Passed, None, pages),
            Ok(_) => (Inconclusive, Some(Coverage), Vec::new()),
            Err(error @ (Budget | Coverage | UnsupportedProfile)) => {
                (Inconclusive, Some(error), Vec::new())
            }
            Err(error) => (Failed, Some(error), Vec::new()),
        };
        Self {
            status,
            failure,
            covered_pages,
        }
    }

    fn verify<'w>(
        retained: &mut Reservation<'w>,
        expected: &[NativeRedactionPageEvidence],
        check: impl FnOnce(&mut Reservation<'w>) -> VerifyResult<Vec<NativeRedactionPageEvidence>>,
    ) -> Self {
        let mut lane_bytes = retained.workspace().scope();
        let lane = Self::from_result(check(&mut lane_bytes), expected);
        if lane.status == NativeVerificationStatus::Passed {
            // @invariant: The next lane shares the workspace with this result.
            retained.bytes += std::mem::take(&mut lane_bytes.bytes);
        }
        lane
    }

    fn covers(&self, pages: &[NativeRedactionPageEvidence]) -> bool {
        self.status == NativeVerificationStatus::Passed
            && self.failure.is_none()
            && self.covered_pages == pages
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionedNativeRedactionEvidence {
    identity: Option<NativeRedactionEvidenceIdentity>,
    identity_failure: Option<NativeVerificationFailure>,
    structure: NativeVerificationLane,
    text: NativeVerificationLane,
    raster: NativeVerificationLane,
}

impl VersionedNativeRedactionEvidence {
    pub(super) fn retained(&self) -> usize {
        let identity = self
            .identity
            .as_ref()
            .map_or(0, |identity| identity.affected_pages.capacity());
        (identity
            + self.structure.covered_pages.capacity()
            + self.text.covered_pages.capacity()
            + self.raster.covered_pages.capacity())
            * size_of::<NativeRedactionPageEvidence>()
    }

    pub fn identity(&self) -> Option<&NativeRedactionEvidenceIdentity> {
        self.identity.as_ref()
    }

    pub fn identity_failure(&self) -> Option<NativeVerificationFailure> {
        self.identity_failure
    }

    pub fn structure(&self) -> &NativeVerificationLane {
        &self.structure
    }

    pub fn text(&self) -> &NativeVerificationLane {
        &self.text
    }

    pub fn raster(&self) -> &NativeVerificationLane {
        &self.raster
    }

    pub fn checks_passed(&self) -> bool {
        self.identity.as_ref().is_some_and(|identity| {
            self.identity_failure.is_none()
                && !identity.affected_pages.is_empty()
                && self.structure.covers(&identity.affected_pages)
                && self.text.covers(&identity.affected_pages)
                && self.raster.covers(&identity.affected_pages)
        })
    }

    // @security: Recheck the live trusted context, mark policy and bytes at the publication boundary.
    pub fn eligible_for(
        &self,
        candidate: &[u8],
        context: NativeRedactionContext<'_>,
        policy: NativeRedactionPolicy<'_>,
    ) -> bool {
        self.checks_passed()
            && candidate.len() <= verify_structure::MAX_INPUT
            && self.identity.as_ref().is_some_and(|identity| {
                identity.matches(context, policy)
                    && blake3::hash(candidate).as_bytes() == &identity.candidate_hash
            })
    }
}

pub fn verify_native_redaction(
    origin: &[u8],
    accepted: &[u8],
    candidate: &[u8],
    context: NativeRedactionContext<'_>,
    policy: NativeRedactionPolicy<'_>,
) -> VersionedNativeRedactionEvidence {
    verify_with_workspace(
        origin,
        accepted,
        candidate,
        context,
        policy,
        STANDALONE_WORKSPACE,
    )
}

pub(super) fn verify_with_workspace(
    origin: &[u8],
    accepted: &[u8],
    candidate: &[u8],
    context: NativeRedactionContext<'_>,
    policy: NativeRedactionPolicy<'_>,
    allowance: usize,
) -> VersionedNativeRedactionEvidence {
    let mut evidence = VersionedNativeRedactionEvidence {
        identity: None,
        identity_failure: None,
        structure: NativeVerificationLane::pending(),
        text: NativeVerificationLane::pending(),
        raster: NativeVerificationLane::pending(),
    };
    let workspace = Workspace::new(allowance);
    let mut retained = workspace.scope();
    let identity = match bind(origin, accepted, candidate, context, policy, &mut retained) {
        Ok(identity) => evidence.identity.insert(identity),
        Err(error) => {
            evidence.identity_failure = Some(error);
            return evidence;
        }
    };
    let expected = &identity.affected_pages;
    evidence.structure = NativeVerificationLane::verify(&mut retained, expected, |pages| {
        verify_structure::verify(accepted, candidate, context.page_order, policy.marks, pages)
    });
    if evidence.structure.status == NativeVerificationStatus::Passed {
        evidence.text = NativeVerificationLane::verify(&mut retained, expected, |pages| {
            verify_text::verify_with_workspace(
                accepted,
                candidate,
                context.page_order,
                policy.marks,
                pages,
            )
        });
        evidence.raster = NativeVerificationLane::verify(&mut retained, expected, |pages| {
            verify_raster::verify_with_workspace(
                accepted,
                candidate,
                context.page_order,
                policy.marks,
                pages,
            )
        });
    }
    evidence
}

fn policy_hash(order: &[u64], marks: &[NativeRedactionMark]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"flowpdf/native/redaction/policy/v1");
    hasher.update(&(order.len() as u64).to_be_bytes());
    for page in order {
        hasher.update(&page.to_be_bytes());
    }
    hasher.update(&(marks.len() as u64).to_be_bytes());
    for mark in marks {
        hasher.update(&mark.page_id.to_be_bytes());
        for coordinate in mark
            .geometry
            .media_box
            .into_iter()
            .chain(mark.geometry.crop_box)
            .chain(mark.rect)
        {
            hasher.update(&coordinate.to_be_bytes());
        }
        hasher.update(&mark.geometry.rotation.to_be_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn bind(
    origin: &[u8],
    accepted: &[u8],
    candidate: &[u8],
    context: NativeRedactionContext<'_>,
    policy: NativeRedactionPolicy<'_>,
    retained: &mut Reservation<'_>,
) -> VerifyResult<NativeRedactionEvidenceIdentity> {
    use NativeVerificationFailure::{Budget, Coverage, Identity};
    if origin.len().max(accepted.len()).max(candidate.len()) > verify_structure::MAX_INPUT
        || context.page_order.len() > MAX_PAGES
        || policy.marks.len() > MAX_MARKS
    {
        return Err(Budget);
    }
    let origin_hash = *blake3::hash(origin).as_bytes();
    let accepted_hash = *blake3::hash(accepted).as_bytes();
    let candidate_hash = *blake3::hash(candidate).as_bytes();
    if context.binding != policy.binding
        || context.binding.session_id == 0
        || context.request_id == 0
        || context.request_id != policy.request_id
        || origin_hash != context.binding.origin_hash
        || accepted_hash != context.accepted_hash
        || candidate_hash != policy.candidate_hash
        || (context.binding.revision == 0 && accepted_hash != origin_hash)
    {
        return Err(Identity);
    }
    let order = context.page_order;
    if policy.marks.is_empty()
        || order
            .iter()
            .enumerate()
            .any(|(index, page)| order[..index].contains(page))
    {
        return Err(Coverage);
    }
    for mark in policy.marks {
        if !order.contains(&mark.page_id)
            || !valid_rect(mark.geometry.media_box)
            || !valid_rect(mark.geometry.crop_box)
            || !valid_rect(mark.rect)
            || !contains(mark.geometry.media_box, mark.geometry.crop_box)
            || !contains(mark.geometry.crop_box, mark.rect)
            || !matches!(mark.geometry.rotation, 0 | 90 | 180 | 270)
        {
            return Err(Coverage);
        }
    }
    let affected_pages = retained.collect_affected_pages(order, policy.marks)?;
    Ok(NativeRedactionEvidenceIdentity {
        binding: context.binding,
        request_id: context.request_id,
        accepted_hash,
        candidate_hash,
        validator_version: NATIVE_REDACTION_VALIDATOR_VERSION,
        validator_profile: NATIVE_REDACTION_VALIDATOR_PROFILE,
        policy_hash: policy_hash(order, policy.marks),
        affected_pages,
    })
}

pub(super) fn affected_pages<'a>(
    order: &'a [u64],
    marks: &'a [NativeRedactionMark],
) -> impl Iterator<Item = NativeRedactionPageEvidence> + Clone + 'a {
    order.iter().filter_map(|&page_id| {
        marks
            .iter()
            .find(|mark| mark.page_id == page_id)
            .map(|mark| NativeRedactionPageEvidence {
                page_id,
                geometry: mark.geometry,
            })
    })
}

pub(super) fn valid_rect([left, bottom, right, top]: Rect) -> bool {
    left < right
        && bottom < top
        && [left, bottom, right, top]
            .into_iter()
            .all(|value| (-MAX_COORD..=MAX_COORD).contains(&value))
}

pub(super) fn contains(outer: Rect, inner: Rect) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && outer[2] >= inner[2] && outer[3] >= inner[3]
}

pub(super) fn intersects(a: Rect, b: Rect) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

pub(super) fn spend(remaining: &mut usize, cost: usize) -> VerifyResult<()> {
    *remaining = remaining
        .checked_sub(cost)
        .ok_or(NativeVerificationFailure::Budget)?;
    Ok(())
}

// @invariant: Each lane holds its reservations for the lifetime of its live workspace.
pub(super) struct Workspace(Cell<usize>);

impl Workspace {
    pub fn new(allowance: usize) -> Self {
        Self(Cell::new(allowance))
    }

    pub fn remaining(&self) -> usize {
        self.0.get()
    }

    pub fn reserve(&self, bytes: usize) -> VerifyResult<Reservation<'_>> {
        let mut reservation = self.scope();
        reservation.grow(bytes)?;
        Ok(reservation)
    }

    pub fn credit(&self, bytes: usize) {
        self.0.set(self.remaining() + bytes);
    }

    pub fn scope(&self) -> Reservation<'_> {
        Reservation {
            workspace: self,
            bytes: 0,
        }
    }
}

pub(super) struct Reservation<'a> {
    workspace: &'a Workspace,
    bytes: usize,
}

impl<'a> Reservation<'a> {
    pub fn into_charge(mut self) -> usize {
        std::mem::take(&mut self.bytes)
    }

    pub fn workspace(&self) -> &'a Workspace {
        self.workspace
    }

    pub fn page_evidence(
        &mut self,
        count: usize,
    ) -> VerifyResult<Vec<NativeRedactionPageEvidence>> {
        self.grow(count * size_of::<NativeRedactionPageEvidence>())?;
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(count)
            .map_err(|_| NativeVerificationFailure::Budget)?;
        Ok(pages)
    }

    pub fn collect_affected_pages(
        &mut self,
        order: &[u64],
        marks: &[NativeRedactionMark],
    ) -> VerifyResult<Vec<NativeRedactionPageEvidence>> {
        let affected = affected_pages(order, marks);
        let mut pages = self.page_evidence(affected.clone().count())?;
        pages.extend(affected);
        Ok(pages)
    }

    pub fn grow(&mut self, bytes: usize) -> VerifyResult<()> {
        let mut remaining = self.workspace.remaining();
        spend(&mut remaining, bytes)?;
        self.workspace.0.set(remaining);
        self.bytes += bytes;
        Ok(())
    }

    pub fn push<T>(&mut self, items: &mut Vec<T>, item: T) -> VerifyResult<()> {
        if items.len() == items.capacity() {
            let extra = items.capacity().max(4);
            let _previous = self.workspace.reserve(items.capacity() * size_of::<T>())?;
            self.grow(
                extra
                    .checked_mul(size_of::<T>())
                    .ok_or(NativeVerificationFailure::Budget)?,
            )?;
            items
                .try_reserve_exact(extra)
                .map_err(|_| NativeVerificationFailure::Budget)?;
        }
        items.push(item);
        Ok(())
    }

    pub fn tree_entry<T>(&mut self) -> VerifyResult<()> {
        // @invariant: Allow for a new B-tree node and its slots before insertion.
        self.grow(128 + 12 * size_of::<T>())
    }

    pub fn insert<T: Ord>(&mut self, set: &mut BTreeSet<T>, item: T) -> VerifyResult<bool> {
        if set.contains(&item) {
            return Ok(false);
        }
        self.tree_entry::<T>()?;
        Ok(set.insert(item))
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        self.workspace
            .0
            .set(self.workspace.remaining() + self.bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_pending_warning_and_partial_lanes_cannot_complete_evidence() {
        let geometry = NativeRedactionGeometry {
            media_box: [0, 0, 1000, 1000],
            crop_box: [0, 0, 1000, 1000],
            rotation: 0,
        };
        let page = NativeRedactionPageEvidence {
            page_id: 6,
            geometry,
        };
        let expected = [page];
        for status in [
            NativeVerificationStatus::Pending,
            NativeVerificationStatus::Unavailable,
            NativeVerificationStatus::Inconclusive,
            NativeVerificationStatus::Failed,
        ] {
            let mut lane = NativeVerificationLane::from_result(Ok(vec![page]), &expected);
            lane.status = status;
            assert!(!lane.covers(&expected));
        }
        let mut warning = NativeVerificationLane::from_result(Ok(vec![page]), &expected);
        warning.failure = Some(NativeVerificationFailure::UnsupportedProfile);
        assert!(!warning.covers(&expected));
        assert!(!NativeVerificationLane::from_result(Ok(Vec::new()), &expected).covers(&expected));
        assert!(
            !NativeVerificationLane::from_result(
                Ok(vec![NativeRedactionPageEvidence {
                    page_id: 7,
                    geometry
                }]),
                &expected
            )
            .covers(&expected)
        );
    }

    #[test]
    fn page_evidence_shares_the_workspace_with_later_lanes_and_scratch() {
        use NativeVerificationFailure::{Budget, Coverage};
        use NativeVerificationStatus::{Inconclusive, Passed};

        let geometry = NativeRedactionGeometry {
            media_box: [0, 0, 100_000, 100_000],
            crop_box: [0, 0, 100_000, 100_000],
            rotation: 0,
        };
        let marks = [NativeRedactionMark {
            page_id: 6,
            geometry,
            rect: [10_000, 10_000, 20_000, 20_000],
        }];
        let page_bytes = size_of::<NativeRedactionPageEvidence>();
        let too_small = Workspace::new(page_bytes - 1);
        assert_eq!(
            too_small.scope().collect_affected_pages(&[6], &marks),
            Err(Budget)
        );

        let workspace = Workspace::new(3 * page_bytes);
        let mut retained = workspace.scope();
        let expected = retained.collect_affected_pages(&[6], &marks).unwrap();
        let structure = NativeVerificationLane::verify(&mut retained, &expected, |bytes| {
            bytes.collect_affected_pages(&[6], &marks)
        });
        assert_eq!(structure.status(), Passed);
        assert_eq!(workspace.remaining(), page_bytes);

        let text = NativeVerificationLane::verify(&mut retained, &expected, |bytes| {
            let covered = bytes.collect_affected_pages(&[6], &marks)?;
            let _scratch = bytes.workspace().reserve(1)?;
            Ok(covered)
        });
        assert_eq!(text.status(), Inconclusive);
        assert_eq!(text.failure(), Some(Budget));
        assert!(text.covered_pages().is_empty());
        assert_eq!(workspace.remaining(), page_bytes);

        let mismatch = NativeVerificationLane::verify(&mut retained, &expected, |bytes| {
            bytes.page_evidence(1)
        });
        assert_eq!(mismatch.failure(), Some(Coverage));
        assert_eq!(workspace.remaining(), page_bytes);

        let raster = NativeVerificationLane::verify(&mut retained, &expected, |bytes| {
            bytes.collect_affected_pages(&[6], &marks)
        });
        assert_eq!(raster.status(), Passed);
        assert_eq!(raster.covered_pages(), expected.as_slice());
        assert_eq!(workspace.remaining(), 0);
    }
}

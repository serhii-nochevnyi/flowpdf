//! Derived invalidation and carry-signature contracts for layout reuse.
//!
//! This module deliberately owns no mutable cache and is not part of the
//! canonical document. It answers one narrow question: where is the earliest
//! safe boundary at which a derived paginator may attempt reuse? The full
//! paginator remains the reference implementation whenever the answer is
//! ambiguous or any identity differs.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    layout::{FontCatalog, PaginationRequest, PaginationResult},
    model::{BlockKind, ContentNode, FlowDocument, NodeId},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Text,
    Structure,
    Style,
    Asset,
    Section,
    Geometry,
    FontData,
    Constraint,
    Revision,
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeSet {
    pub node_ids: BTreeSet<NodeId>,
    pub kinds: BTreeSet<ChangeKind>,
    pub ambiguous: bool,
}

impl ChangeSet {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn node(node_id: NodeId, kind: ChangeKind) -> Self {
        let mut changes = Self::default();
        changes.node_ids.insert(node_id);
        changes.kinds.insert(kind);
        changes
    }

    #[must_use]
    pub fn global(kind: ChangeKind) -> Self {
        let mut changes = Self::default();
        changes.kinds.insert(kind);
        changes
    }

    #[must_use]
    pub fn ambiguous() -> Self {
        Self {
            ambiguous: true,
            ..Self::default()
        }
    }

    pub fn add_node(&mut self, node_id: NodeId, kind: ChangeKind) {
        self.node_ids.insert(node_id);
        self.kinds.insert(kind);
    }

    pub fn add_global(&mut self, kind: ChangeKind) {
        self.kinds.insert(kind);
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum InvalidationReason {
    NoChanges,
    NodeChanged,
    GlobalLayoutInput,
    Ambiguous,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvalidationPlan {
    pub earliest_boundary: usize,
    pub reusable_prefix: bool,
    pub reason: InvalidationReason,
}

impl InvalidationPlan {
    #[must_use]
    pub const fn no_op(document_nodes: usize) -> Self {
        Self {
            earliest_boundary: document_nodes,
            reusable_prefix: true,
            reason: InvalidationReason::NoChanges,
        }
    }

    #[must_use]
    pub const fn requires_full_reflow(reason: InvalidationReason) -> Self {
        Self {
            earliest_boundary: 0,
            reusable_prefix: false,
            reason,
        }
    }
}

/// Computes the earliest safe top-level boundary for a declared change set.
/// Unknown or unresolvable changes deliberately disable reuse from the start
/// rather than guessing at a dependency path.
pub fn plan_for_document(document: &FlowDocument, changes: &ChangeSet) -> InvalidationPlan {
    if changes.ambiguous || changes.kinds.contains(&ChangeKind::Unknown) {
        return InvalidationPlan::requires_full_reflow(InvalidationReason::Ambiguous);
    }
    if changes.node_ids.is_empty() && changes.kinds.is_empty() {
        return InvalidationPlan::no_op(document.content.len());
    }
    if changes.kinds.iter().any(|kind| {
        matches!(
            kind,
            ChangeKind::Section
                | ChangeKind::Geometry
                | ChangeKind::FontData
                | ChangeKind::Constraint
                | ChangeKind::Revision
        )
    }) {
        return InvalidationPlan::requires_full_reflow(InvalidationReason::GlobalLayoutInput);
    }
    if changes.node_ids.is_empty() {
        return InvalidationPlan::requires_full_reflow(InvalidationReason::Ambiguous);
    }
    let mut earliest = document.content.len();
    for node_id in &changes.node_ids {
        let Some(index) = top_level_owner_index(&document.content, node_id) else {
            return InvalidationPlan::requires_full_reflow(InvalidationReason::Ambiguous);
        };
        earliest = earliest.min(index);
    }
    InvalidationPlan {
        earliest_boundary: earliest,
        reusable_prefix: earliest > 0,
        reason: InvalidationReason::NodeChanged,
    }
}

/// Compares two canonical documents and emits a conservative derived change
/// set. It is intentionally independent of transaction internals so recovered
/// or API-provided snapshots receive the same invalidation semantics.
pub fn diff_documents(before: &FlowDocument, after: &FlowDocument) -> ChangeSet {
    let mut changes = ChangeSet::default();
    let revision_changed = before.revision != after.revision;
    if before.page_settings != after.page_settings {
        changes.add_global(ChangeKind::Geometry);
    }
    if before.sections != after.sections {
        changes.add_global(ChangeKind::Section);
    }
    if before.styles != after.styles {
        let changed_style_ids = changed_style_ids(before, after);
        let matched = add_style_owners(&after.content, &changed_style_ids, &mut changes);
        if changed_style_ids.is_empty() || !matched {
            changes.add_global(ChangeKind::Style);
        }
    }
    if before.assets != after.assets {
        let changed_asset_ids = changed_asset_ids(before, after);
        let matched = add_asset_owners(&after.content, &changed_asset_ids, &mut changes);
        if changed_asset_ids.is_empty() || !matched {
            changes.add_global(ChangeKind::Asset);
        }
    }
    compare_nodes(&before.content, &after.content, &mut changes);
    if revision_changed && changes.kinds.is_empty() && changes.node_ids.is_empty() {
        changes.add_global(ChangeKind::Revision);
    }
    changes
}

fn compare_nodes(before: &[ContentNode], after: &[ContentNode], changes: &mut ChangeSet) {
    let length = before.len().max(after.len());
    for index in 0..length {
        match (before.get(index), after.get(index)) {
            (Some(left), Some(right)) if left.id == right.id => {
                if left.style_id != right.style_id {
                    changes.add_node(right.id.clone(), ChangeKind::Style);
                }
                if left.body != right.body {
                    let kind = if matches!(
                        (&left.body, &right.body),
                        (
                            BlockKind::Paragraph { .. } | BlockKind::Heading { .. },
                            BlockKind::Paragraph { .. } | BlockKind::Heading { .. }
                        )
                    ) {
                        ChangeKind::Text
                    } else {
                        ChangeKind::Structure
                    };
                    changes.add_node(right.id.clone(), kind);
                }
                compare_nodes(left.children(), right.children(), changes);
            }
            (Some(left), Some(right)) => {
                changes.add_node(left.id.clone(), ChangeKind::Structure);
                changes.add_node(right.id.clone(), ChangeKind::Structure);
            }
            (Some(left), None) => changes.add_node(left.id.clone(), ChangeKind::Structure),
            (None, Some(right)) => changes.add_node(right.id.clone(), ChangeKind::Structure),
            (None, None) => {}
        }
    }
}

fn changed_style_ids(before: &FlowDocument, after: &FlowDocument) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    for style in before.styles.iter().chain(after.styles.iter()) {
        let before_style = before
            .styles
            .iter()
            .find(|candidate| candidate.id == style.id);
        let after_style = after
            .styles
            .iter()
            .find(|candidate| candidate.id == style.id);
        if before_style != after_style {
            changed.insert(style.id.as_str().to_owned());
        }
    }
    changed
}

fn add_style_owners(
    nodes: &[ContentNode],
    style_ids: &BTreeSet<String>,
    changes: &mut ChangeSet,
) -> bool {
    let mut matched = false;
    for node in nodes {
        if node
            .style_id
            .as_ref()
            .is_some_and(|style_id| style_ids.contains(style_id.as_str()))
        {
            changes.add_node(node.id.clone(), ChangeKind::Style);
            matched = true;
        }
        matched |= add_style_owners(node.children(), style_ids, changes);
    }
    matched
}

fn changed_asset_ids(before: &FlowDocument, after: &FlowDocument) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    for asset in before.assets.iter().chain(after.assets.iter()) {
        let before_asset = before
            .assets
            .iter()
            .find(|candidate| candidate.id == asset.id);
        let after_asset = after
            .assets
            .iter()
            .find(|candidate| candidate.id == asset.id);
        if before_asset != after_asset {
            changed.insert(asset.id.as_str().to_owned());
        }
    }
    changed
}

fn add_asset_owners(
    nodes: &[ContentNode],
    asset_ids: &BTreeSet<String>,
    changes: &mut ChangeSet,
) -> bool {
    let mut matched = false;
    for node in nodes {
        if let BlockKind::Image { asset_id, .. } = &node.body
            && asset_ids.contains(asset_id.as_str())
        {
            changes.add_node(node.id.clone(), ChangeKind::Asset);
            matched = true;
        }
        matched |= add_asset_owners(node.children(), asset_ids, changes);
    }
    matched
}

fn top_level_owner_index(nodes: &[ContentNode], target: &NodeId) -> Option<usize> {
    nodes
        .iter()
        .enumerate()
        .find_map(|(index, node)| contains_node(node, target).then_some(index))
}

fn contains_node(node: &ContentNode, target: &NodeId) -> bool {
    &node.id == target
        || node
            .children()
            .iter()
            .any(|child| contains_node(child, target))
}

/// The complete state that crosses an incremental reuse boundary. The hash
/// binds the identity fields and the already accepted derived result, so a
/// cache cannot be reused across a stale revision or different font/data set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CarrySignature {
    pub source_revision: u32,
    pub source_hash: String,
    pub layout_settings_fingerprint: String,
    pub font_catalog_identity: String,
    pub hyphenation_data_identity: Option<String>,
    pub boundary: usize,
    pub carry_hash: String,
}

impl CarrySignature {
    pub fn from_result(
        result: &PaginationResult,
        boundary: usize,
    ) -> Result<Self, InvalidationError> {
        let boundary = boundary.min(result.pages.len());
        let mut signature = Self {
            source_revision: result.source_revision,
            source_hash: result.source_hash.clone(),
            layout_settings_fingerprint: result.layout_settings_fingerprint.clone(),
            font_catalog_identity: result.font_catalog_identity.clone(),
            hyphenation_data_identity: result.hyphenation_data_identity.clone(),
            boundary,
            carry_hash: String::new(),
        };
        signature.carry_hash = carry_hash(&signature, &result.pages[..boundary])?;
        Ok(signature)
    }

    #[must_use]
    pub fn matches_request(
        &self,
        request: &PaginationRequest,
        catalog: &FontCatalog,
        ukrainian_hyphenation: Option<&crate::layout::UkrainianHyphenation>,
    ) -> bool {
        self.source_revision == request.source_revision
            && self.source_hash == request.source_hash
            && self.font_catalog_identity == catalog.identity()
            && self.hyphenation_data_identity
                == ukrainian_hyphenation.map(|data| data.identity().to_owned())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IncrementalLayoutCache {
    pub result: PaginationResult,
    pub carry: CarrySignature,
}

impl IncrementalLayoutCache {
    pub fn from_result(
        result: PaginationResult,
        boundary: usize,
    ) -> Result<Self, InvalidationError> {
        let carry = CarrySignature::from_result(&result, boundary)?;
        Ok(Self { result, carry })
    }

    #[must_use]
    pub fn matches(
        &self,
        request: &PaginationRequest,
        catalog: &FontCatalog,
        ukrainian_hyphenation: Option<&crate::layout::UkrainianHyphenation>,
    ) -> bool {
        self.carry
            .matches_request(request, catalog, ukrainian_hyphenation)
            && self.carry.matches_result(&self.result)
    }
}

impl CarrySignature {
    #[must_use]
    pub fn matches_result(&self, result: &PaginationResult) -> bool {
        if self.source_revision != result.source_revision
            || self.source_hash != result.source_hash
            || self.layout_settings_fingerprint != result.layout_settings_fingerprint
            || self.font_catalog_identity != result.font_catalog_identity
            || self.hyphenation_data_identity != result.hyphenation_data_identity
        {
            return false;
        }
        let Ok(result_hash) = crate::layout::pagination_result_hash(result) else {
            return false;
        };
        if result_hash != result.result_hash {
            return false;
        }
        let boundary = self.boundary.min(result.pages.len());
        carry_hash(self, &result.pages[..boundary])
            .ok()
            .is_some_and(|hash| hash == self.carry_hash)
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum InvalidationError {
    #[error("The carry signature could not be serialized")]
    Serialization,
}

fn carry_hash(
    signature: &CarrySignature,
    pages: &[crate::layout::LayoutPage],
) -> Result<String, InvalidationError> {
    serde_json::to_vec(&(
        signature.source_revision,
        &signature.source_hash,
        &signature.layout_settings_fingerprint,
        &signature.font_catalog_identity,
        &signature.hyphenation_data_identity,
        signature.boundary,
        pages,
    ))
    .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
    .map_err(|_| InvalidationError::Serialization)
}

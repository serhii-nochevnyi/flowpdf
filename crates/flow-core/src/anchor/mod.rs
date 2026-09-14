//! Stable logical positions and deterministic transformation mappings.
//!
//! The public contract deliberately contains no DOM path, absolute document
//! index, page rectangle, or PDF coordinate. Public offsets use UTF-16 code
//! units because browser selection APIs do; internal string access uses a
//! distinct byte-offset type.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{Affinity, LogicalPosition, NodeId, TombstoneToken};
use crate::model::{BlockKind, ContentNode};
use crate::schema::{DocumentLimits, MAX_INLINE_RUNS};

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(transparent)]
pub struct Utf16Offset(u32);

impl Utf16Offset {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    pub fn checked_add(self, amount: u32) -> Result<Self, AnchorError> {
        self.0
            .checked_add(amount)
            .map(Self)
            .ok_or(AnchorError::OutOfRange)
    }
}

impl From<u32> for Utf16Offset {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeByteOffset(usize);

impl NativeByteOffset {
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphemeBoundary {
    pub byte_offset: NativeByteOffset,
    pub utf16_offset: Utf16Offset,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum AnchorError {
    #[error("The logical position is outside the target text")]
    OutOfRange,
    #[error("The UTF-16 position splits a surrogate pair")]
    InvalidUtf16Boundary,
}

impl AnchorError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::OutOfRange => "FLOW_INVALID_RANGE",
            Self::InvalidUtf16Boundary => "FLOW_INVALID_UTF16_BOUNDARY",
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum EditorPositionError {
    #[error(transparent)]
    Scalar(#[from] AnchorError),
    #[error("The UTF-16 position is inside an extended grapheme cluster")]
    InvalidGraphemeBoundary,
    #[error("The atomic node position must be an exact directional edge")]
    InvalidAtomicPosition,
    #[error("The node kind cannot carry a public logical position")]
    UnsupportedNodeKind,
    #[error("The logical position targets a different node")]
    WrongNode,
    #[error("The logical position is bound to a stale document revision")]
    StaleRevision,
    #[error("The text block exceeds the editor boundary budget")]
    TextLimit,
}

impl EditorPositionError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Scalar(error) => error.code(),
            Self::InvalidGraphemeBoundary => "FLOW_INVALID_GRAPHEME_BOUNDARY",
            Self::InvalidAtomicPosition => "FLOW_INVALID_ATOMIC_POSITION",
            Self::UnsupportedNodeKind => "FLOW_INVALID_POSITION_NODE_KIND",
            Self::WrongNode => "FLOW_POSITION_NODE_MISMATCH",
            Self::StaleRevision => "FLOW_STALE_REVISION",
            Self::TextLimit => "FLOW_POSITION_TEXT_LIMIT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphemeBoundaryMap {
    text: String,
    boundaries: Vec<GraphemeBoundary>,
}

impl GraphemeBoundaryMap {
    pub fn new(value: &str) -> Result<Self, EditorPositionError> {
        if value.len() > DocumentLimits::V1.text_node_bytes {
            return Err(EditorPositionError::TextLimit);
        }

        let segmenter = icu_segmenter::GraphemeClusterSegmenter::new();
        let mut chars = value.char_indices().peekable();
        let mut utf16_offset = 0_u32;
        let mut boundaries = Vec::new();
        for byte_offset in segmenter.segment_str(value) {
            while chars.peek().is_some_and(|(index, _)| *index < byte_offset) {
                let (_, character) = chars.next().expect("peeked character exists");
                let width = u32::try_from(character.len_utf16())
                    .map_err(|_| EditorPositionError::Scalar(AnchorError::OutOfRange))?;
                utf16_offset = utf16_offset
                    .checked_add(width)
                    .ok_or(EditorPositionError::Scalar(AnchorError::OutOfRange))?;
            }
            let byte_offset = NativeByteOffset::new(byte_offset);
            boundaries.push(GraphemeBoundary {
                byte_offset,
                utf16_offset: Utf16Offset::new(utf16_offset),
            });
        }

        Ok(Self {
            text: value.to_owned(),
            boundaries,
        })
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn boundaries(&self) -> &[GraphemeBoundary] {
        &self.boundaries
    }

    #[must_use]
    pub fn contains(&self, offset: Utf16Offset) -> bool {
        self.boundaries
            .binary_search_by_key(&offset, |boundary| boundary.utf16_offset)
            .is_ok()
    }

    pub fn resolve(&self, offset: Utf16Offset) -> Result<NativeByteOffset, EditorPositionError> {
        resolve_utf16_offset(&self.text, offset).map_err(EditorPositionError::Scalar)?;
        self.boundaries
            .binary_search_by_key(&offset, |boundary| boundary.utf16_offset)
            .map(|index| self.boundaries[index].byte_offset)
            .map_err(|_| EditorPositionError::InvalidGraphemeBoundary)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPosition {
    Text(NativeByteOffset),
    BeforeAtom,
    AfterAtom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodePositionMap {
    node_id: NodeId,
    revision: u32,
    graphemes: Option<GraphemeBoundaryMap>,
}

impl NodePositionMap {
    pub fn new(node: &ContentNode, revision: u32) -> Result<Self, EditorPositionError> {
        let graphemes = match &node.body {
            BlockKind::Paragraph { runs, .. } | BlockKind::Heading { runs, .. } => {
                if runs.len() > MAX_INLINE_RUNS {
                    return Err(EditorPositionError::TextLimit);
                }
                Some(GraphemeBoundaryMap::new(&node.text())?)
            }
            BlockKind::Image { .. } | BlockKind::Table { .. } | BlockKind::PageBreak => None,
            BlockKind::OrderedList { .. }
            | BlockKind::UnorderedList { .. }
            | BlockKind::ListItem { .. }
            | BlockKind::TableRow { .. }
            | BlockKind::TableCell { .. } => {
                return Err(EditorPositionError::UnsupportedNodeKind);
            }
        };

        Ok(Self {
            node_id: node.id.clone(),
            revision,
            graphemes,
        })
    }

    #[must_use]
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    #[must_use]
    pub const fn revision(&self) -> u32 {
        self.revision
    }

    #[must_use]
    pub fn graphemes(&self) -> Option<&GraphemeBoundaryMap> {
        self.graphemes.as_ref()
    }

    pub fn validate(
        &self,
        position: &LogicalPosition,
        revision: u32,
    ) -> Result<ResolvedPosition, EditorPositionError> {
        if position.node_id != self.node_id {
            return Err(EditorPositionError::WrongNode);
        }
        if revision != self.revision {
            return Err(EditorPositionError::StaleRevision);
        }

        match &self.graphemes {
            Some(graphemes) => graphemes
                .resolve(position.utf16_offset)
                .map(ResolvedPosition::Text),
            None => match (position.utf16_offset.get(), &position.affinity) {
                (0, Affinity::Forward) => Ok(ResolvedPosition::BeforeAtom),
                (1, Affinity::Backward) => Ok(ResolvedPosition::AfterAtom),
                _ => Err(EditorPositionError::InvalidAtomicPosition),
            },
        }
    }
}

/// Converts a browser/public UTF-16 offset to a native UTF-8 byte boundary.
///
/// This validates Unicode scalar boundaries only. Grapheme-safe editing is a
/// later editor-layer responsibility.
pub fn resolve_utf16_offset(
    value: &str,
    offset: Utf16Offset,
) -> Result<NativeByteOffset, AnchorError> {
    let requested = usize::try_from(offset.get()).map_err(|_| AnchorError::OutOfRange)?;
    if requested == 0 {
        return Ok(NativeByteOffset(0));
    }

    let mut consumed = 0_usize;
    for (byte_index, character) in value.char_indices() {
        if consumed == requested {
            return Ok(NativeByteOffset(byte_index));
        }
        let next = consumed
            .checked_add(character.len_utf16())
            .ok_or(AnchorError::OutOfRange)?;
        if requested < next {
            return Err(AnchorError::InvalidUtf16Boundary);
        }
        consumed = next;
    }
    if consumed == requested {
        Ok(NativeByteOffset(value.len()))
    } else {
        Err(AnchorError::OutOfRange)
    }
}

pub(crate) fn utf16_length(value: &str) -> Result<u32, AnchorError> {
    u32::try_from(value.encode_utf16().count()).map_err(|_| AnchorError::OutOfRange)
}

pub fn byte_to_utf16_offset(
    value: &str,
    byte_offset: NativeByteOffset,
) -> Result<Utf16Offset, AnchorError> {
    if byte_offset.get() > value.len() || !value.is_char_boundary(byte_offset.get()) {
        return Err(AnchorError::OutOfRange);
    }
    let units = value[..byte_offset.get()].encode_utf16().count();
    u32::try_from(units)
        .map(Utf16Offset::new)
        .map_err(|_| AnchorError::OutOfRange)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnchorMapping {
    pub transformations: Vec<AnchorTransformation>,
}

impl AnchorMapping {
    #[must_use]
    pub fn identity() -> Self {
        Self::default()
    }

    pub fn map(&self, position: &LogicalPosition) -> AnchorMapResult {
        let mut result = AnchorMapResult::Mapped(position.clone());
        for transformation in &self.transformations {
            let AnchorMapResult::Mapped(position) = result else {
                break;
            };
            result = transformation.map(&position);
        }
        result
    }

    pub(crate) fn push(&mut self, transformation: AnchorTransformation) {
        self.transformations.push(transformation);
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.transformations.extend(other.transformations);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AnchorTransformation {
    TextEdit {
        node_id: NodeId,
        start: Utf16Offset,
        removed_utf16_length: u32,
        inserted_utf16_length: u32,
    },
    NodeInvalidated {
        node_id: NodeId,
    },
    /// A text block was split without changing the retained block's identity.
    /// The generated block is intentionally named in the mapping so an anchor
    /// is never relocated by guessing which surviving node is nearby.
    SplitTextBlock {
        node_id: NodeId,
        new_node_id: NodeId,
        split_utf16: Utf16Offset,
    },
    /// The second adjacent text block was appended to the first and removed
    /// from the container. Its original identity remains available to the
    /// exact inverse operation.
    MergeTextBlocks {
        first_node_id: NodeId,
        second_node_id: NodeId,
        first_utf16_length: u32,
    },
    /// A compatible multi-block replacement retained the first block and
    /// removed the selected suffix/intermediate blocks. The opaque token is
    /// the only public result for anchors inside removed material.
    CrossBlockReplace {
        first_node_id: NodeId,
        last_node_id: NodeId,
        removed_node_ids: Vec<NodeId>,
        first_start: Utf16Offset,
        first_end: Utf16Offset,
        last_end: Utf16Offset,
        inserted_utf16_length: u32,
        tombstone: TombstoneToken,
    },
    /// A whole subtree was explicitly removed. The token does not authorize
    /// retargeting; it only records that the old position is deleted.
    NodeDeleted {
        node_id: NodeId,
        tombstone: TombstoneToken,
    },
}

impl AnchorTransformation {
    fn map(&self, position: &LogicalPosition) -> AnchorMapResult {
        match self {
            Self::NodeInvalidated { node_id } if position.node_id == *node_id => {
                AnchorMapResult::Invalid(AnchorInvalidation::DeletedNode)
            }
            Self::NodeInvalidated { .. } => AnchorMapResult::Mapped(position.clone()),
            Self::NodeDeleted { node_id, tombstone } if position.node_id == *node_id => {
                AnchorMapResult::Deleted {
                    tombstone: tombstone.clone(),
                }
            }
            Self::NodeDeleted { .. } => AnchorMapResult::Mapped(position.clone()),
            Self::SplitTextBlock {
                node_id,
                new_node_id,
                split_utf16,
            } => map_split(position, node_id, new_node_id, *split_utf16),
            Self::MergeTextBlocks {
                first_node_id,
                second_node_id,
                first_utf16_length,
            } => map_merge(position, first_node_id, second_node_id, *first_utf16_length),
            Self::CrossBlockReplace {
                first_node_id,
                last_node_id,
                removed_node_ids,
                first_start,
                first_end,
                last_end,
                inserted_utf16_length,
                tombstone,
            } => map_cross_block_replace(
                position,
                first_node_id,
                last_node_id,
                removed_node_ids,
                *first_start,
                *first_end,
                *last_end,
                *inserted_utf16_length,
                tombstone,
            ),
            Self::TextEdit {
                node_id,
                start,
                removed_utf16_length,
                inserted_utf16_length,
            } if position.node_id == *node_id => map_text_edit(
                position,
                *start,
                *removed_utf16_length,
                *inserted_utf16_length,
            ),
            Self::TextEdit { .. } => AnchorMapResult::Mapped(position.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "status",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum AnchorMapResult {
    Mapped(LogicalPosition),
    Deleted { tombstone: TombstoneToken },
    Invalid(AnchorInvalidation),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AnchorInvalidation {
    DeletedText,
    DeletedNode,
}

fn map_text_edit(
    position: &LogicalPosition,
    start: Utf16Offset,
    removed: u32,
    inserted: u32,
) -> AnchorMapResult {
    let point = position.utf16_offset.get();
    let start = start.get();
    let Some(end) = start.checked_add(removed) else {
        return AnchorMapResult::Invalid(AnchorInvalidation::DeletedText);
    };

    let (mapped, affinity) = if removed == 0 {
        if point < start {
            (point, position.affinity.clone())
        } else if point > start || position.affinity == Affinity::Forward {
            let Some(value) = point.checked_add(inserted) else {
                return AnchorMapResult::Invalid(AnchorInvalidation::DeletedText);
            };
            (value, position.affinity.clone())
        } else {
            (point, position.affinity.clone())
        }
    } else if point < start {
        (point, position.affinity.clone())
    } else if point > end {
        let Some(with_insert) = point.checked_add(inserted) else {
            return AnchorMapResult::Invalid(AnchorInvalidation::DeletedText);
        };
        (with_insert - removed, position.affinity.clone())
    } else if point == start {
        match (&position.affinity, inserted) {
            (Affinity::Backward, _) => (start, Affinity::Backward),
            (Affinity::Forward, 0) => {
                return AnchorMapResult::Invalid(AnchorInvalidation::DeletedText);
            }
            (Affinity::Forward, _) => {
                let Some(value) = start.checked_add(inserted) else {
                    return AnchorMapResult::Invalid(AnchorInvalidation::DeletedText);
                };
                (value, Affinity::Backward)
            }
        }
    } else if point == end {
        match (&position.affinity, inserted) {
            (Affinity::Backward, 0) => {
                return AnchorMapResult::Invalid(AnchorInvalidation::DeletedText);
            }
            (Affinity::Backward, _) => (start, Affinity::Forward),
            (Affinity::Forward, _) => {
                let Some(value) = start.checked_add(inserted) else {
                    return AnchorMapResult::Invalid(AnchorInvalidation::DeletedText);
                };
                (value, Affinity::Forward)
            }
        }
    } else {
        return AnchorMapResult::Invalid(AnchorInvalidation::DeletedText);
    };

    AnchorMapResult::Mapped(LogicalPosition {
        node_id: position.node_id.clone(),
        utf16_offset: Utf16Offset::new(mapped),
        affinity,
    })
}

fn map_split(
    position: &LogicalPosition,
    node_id: &NodeId,
    new_node_id: &NodeId,
    split_utf16: Utf16Offset,
) -> AnchorMapResult {
    if position.node_id != *node_id {
        return AnchorMapResult::Mapped(position.clone());
    }
    let offset = position.utf16_offset.get();
    let split = split_utf16.get();
    if offset < split || (offset == split && position.affinity == Affinity::Backward) {
        return AnchorMapResult::Mapped(position.clone());
    }
    let mapped_offset = offset.saturating_sub(split);
    AnchorMapResult::Mapped(LogicalPosition {
        node_id: new_node_id.clone(),
        utf16_offset: mapped_offset.into(),
        affinity: position.affinity.clone(),
    })
}

fn map_merge(
    position: &LogicalPosition,
    first_node_id: &NodeId,
    second_node_id: &NodeId,
    first_utf16_length: u32,
) -> AnchorMapResult {
    if position.node_id == *second_node_id {
        return AnchorMapResult::Mapped(LogicalPosition {
            node_id: first_node_id.clone(),
            utf16_offset: first_utf16_length
                .saturating_add(position.utf16_offset.get())
                .into(),
            affinity: position.affinity.clone(),
        });
    }
    AnchorMapResult::Mapped(position.clone())
}

#[allow(clippy::too_many_arguments)]
fn map_cross_block_replace(
    position: &LogicalPosition,
    first_node_id: &NodeId,
    last_node_id: &NodeId,
    removed_node_ids: &[NodeId],
    first_start: Utf16Offset,
    first_end: Utf16Offset,
    last_end: Utf16Offset,
    inserted_utf16_length: u32,
    tombstone: &TombstoneToken,
) -> AnchorMapResult {
    if removed_node_ids
        .iter()
        .any(|node_id| node_id == &position.node_id)
    {
        return AnchorMapResult::Deleted {
            tombstone: tombstone.clone(),
        };
    }
    if position.node_id == *first_node_id {
        let point = position.utf16_offset.get();
        let start = first_start.get();
        let end = first_end.get();
        if point < start {
            return AnchorMapResult::Mapped(position.clone());
        }
        if point == start {
            let offset = if position.affinity == Affinity::Backward {
                start
            } else {
                start.saturating_add(inserted_utf16_length)
            };
            return AnchorMapResult::Mapped(LogicalPosition {
                node_id: first_node_id.clone(),
                utf16_offset: offset.into(),
                affinity: position.affinity.clone(),
            });
        }
        if point < end {
            return AnchorMapResult::Deleted {
                tombstone: tombstone.clone(),
            };
        }
        return AnchorMapResult::Mapped(LogicalPosition {
            node_id: first_node_id.clone(),
            utf16_offset: start
                .saturating_add(inserted_utf16_length)
                .saturating_add(point.saturating_sub(end))
                .into(),
            affinity: position.affinity.clone(),
        });
    }
    if position.node_id == *last_node_id {
        let point = position.utf16_offset.get();
        let end = last_end.get();
        if point < end {
            return AnchorMapResult::Deleted {
                tombstone: tombstone.clone(),
            };
        }
        return AnchorMapResult::Mapped(LogicalPosition {
            node_id: first_node_id.clone(),
            utf16_offset: first_start
                .get()
                .saturating_add(inserted_utf16_length)
                .saturating_add(point.saturating_sub(end))
                .into(),
            affinity: position.affinity.clone(),
        });
    }
    AnchorMapResult::Mapped(position.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_and_utf16_offsets_round_trip_only_at_scalar_boundaries() {
        let value = "Aи\u{0306}😀𝄞Б";
        for byte in value
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(value.len()))
        {
            let byte = NativeByteOffset(byte);
            let utf16 = byte_to_utf16_offset(value, byte).expect("valid boundary");
            assert_eq!(resolve_utf16_offset(value, utf16).expect("resolve"), byte);
        }
    }
}

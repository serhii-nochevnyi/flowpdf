//! Stable logical positions and deterministic transformation mappings.
//!
//! The public contract deliberately contains no DOM path, absolute document
//! index, page rectangle, or PDF coordinate. Public offsets use UTF-16 code
//! units because browser selection APIs do; internal string access uses a
//! distinct byte-offset type.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{Affinity, LogicalPosition, NodeId};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl AnchorTransformation {
    fn map(&self, position: &LogicalPosition) -> AnchorMapResult {
        match self {
            Self::NodeInvalidated { node_id } if position.node_id == *node_id => {
                AnchorMapResult::Invalid(AnchorInvalidation::DeletedNode)
            }
            Self::NodeInvalidated { .. } => AnchorMapResult::Mapped(position.clone()),
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

//! The only canonical byte and hash path for FlowDocument.
//!
//! The versioned BLAKE3 envelope is equality/integrity evidence. It is not an
//! authentication mechanism and does not authorize untrusted stored content.

use crate::{
    model::{AssetDescriptor, FlowDocument},
    schema::{DocumentLimits, LimitKind, SchemaError, validate_document},
};

pub fn canonical_bytes(document: &FlowDocument) -> Result<Vec<u8>, SchemaError> {
    validate_document(document)?;
    let bytes = serde_json::to_vec(document).map_err(|_| SchemaError::serialization())?;
    DocumentLimits::V1.check(LimitKind::CanonicalBytes, bytes.len())?;
    Ok(bytes)
}

pub fn decode_canonical(bytes: &[u8]) -> Result<FlowDocument, SchemaError> {
    let limits = DocumentLimits::V1;
    limits.check(LimitKind::CanonicalBytes, bytes.len())?;
    limits.check(LimitKind::TreeDepth, json_nesting_depth(bytes)?)?;
    let document: FlowDocument =
        serde_json::from_slice(bytes).map_err(|_| SchemaError::decode())?;
    validate_document(&document)?;
    let encoded = canonical_bytes(&document)?;
    if encoded != bytes {
        return Err(SchemaError::non_canonical());
    }
    Ok(document)
}

#[must_use]
pub fn canonical_hash(bytes: &[u8]) -> String {
    format!("flowpdf:blake3:v1:{}", blake3::hash(bytes).to_hex())
}

pub fn verify_asset_bytes(descriptor: &AssetDescriptor, bytes: &[u8]) -> Result<(), SchemaError> {
    let length_matches = usize::try_from(descriptor.byte_length)
        .is_ok_and(|expected_length| expected_length == bytes.len());
    let hash_matches = descriptor.content_hash == asset_hash(bytes);
    if !length_matches || !hash_matches {
        return Err(SchemaError::asset_hash_mismatch());
    }
    Ok(())
}

#[must_use]
pub fn asset_hash(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

fn json_nesting_depth(bytes: &[u8]) -> Result<usize, SchemaError> {
    let mut depth = 0_usize;
    let mut maximum = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth = depth
                    .checked_add(1)
                    .ok_or_else(SchemaError::invalid_document)?;
                maximum = maximum.max(depth);
                if maximum > DocumentLimits::V1.tree_depth {
                    return Err(SchemaError::limit(
                        LimitKind::TreeDepth,
                        maximum,
                        DocumentLimits::V1.tree_depth,
                    ));
                }
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1).ok_or_else(SchemaError::decode)?;
            }
            _ => {}
        }
    }
    if in_string || depth != 0 {
        return Err(SchemaError::decode());
    }
    Ok(maximum)
}

//! Bounded image resources derived from revision-owned asset records.
//!
//! The adapter accepts only the PNG/JPEG formats admitted by `asset`. PNG is
//! decoded into bounded RGBA samples because a PNG byte stream is not itself a
//! PDF image filter; JPEG bytes remain in their validated DCT form. Resource
//! ordering and deduplication use the canonical content identity, never host
//! paths or hash-map iteration order.

use std::{
    collections::BTreeMap,
    io::{BufReader, Cursor},
};

use image::{ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    asset::{self, MAX_IMAGE_DECODED_BYTES, validate_image_bytes_for_export},
    canonical::asset_hash,
    model::{AssetDescriptor, AssetId, FlowDocument},
    schema::validate_document,
    store::AssetRecord,
};

const MAX_PDF_IMAGE_RESOURCES: usize = 2_048;

/// Encoding carried by one derived PDF image resource.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PdfImageEncoding {
    /// Validated JPEG bytes can be emitted through PDF's DCTDecode filter.
    JpegDct,
    /// PNG pixels are decoded to RGBA samples for a later bounded PDF stream.
    RawRgba,
}

/// One deduplicated, validated image resource. The `bytes` field is derived
/// export data and is never included in semantic document or diagnostic DTOs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfImageResource {
    pub resource_name: String,
    pub resource_identity: String,
    pub content_hash: String,
    pub asset_ids: Vec<AssetId>,
    pub media_type: String,
    pub encoded_byte_length: u32,
    pub width: u32,
    pub height: u32,
    pub encoding: PdfImageEncoding,
    pub bytes: Vec<u8>,
}

/// Stable failure taxonomy for owned image-resource preparation.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PdfImageError {
    #[error("the canonical document is invalid")]
    InvalidDocument,
    #[error("a reachable image has no matching asset descriptor")]
    DescriptorMissing,
    #[error("a reachable image has no matching durable asset record")]
    RecordMissing,
    #[error("more than one durable record claims the same asset identity")]
    DuplicateRecord,
    #[error("the asset descriptor and durable record disagree")]
    DescriptorMismatch,
    #[error("the durable asset bytes do not match their content identity")]
    ContentHashMismatch,
    #[error("the durable asset record format is unsupported")]
    RecordFormat,
    #[error("the admitted asset failed bounded image validation: {0}")]
    Asset(#[from] asset::AssetError),
    #[error("the decoded PDF image samples exceed the limit")]
    SampleBytesLimit,
    #[error("the PDF image resource count exceeds the limit")]
    ResourceLimit,
}

impl PdfImageError {
    /// Stable diagnostic code for export/worker callers.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidDocument => "FLOW_PDF_IMAGE_DOCUMENT_INVALID",
            Self::DescriptorMissing => "FLOW_PDF_IMAGE_DESCRIPTOR_MISSING",
            Self::RecordMissing => "FLOW_PDF_IMAGE_RECORD_MISSING",
            Self::DuplicateRecord => "FLOW_PDF_IMAGE_RECORD_DUPLICATE",
            Self::DescriptorMismatch => "FLOW_PDF_IMAGE_DESCRIPTOR_MISMATCH",
            Self::ContentHashMismatch => "FLOW_PDF_IMAGE_HASH_MISMATCH",
            Self::RecordFormat => "FLOW_PDF_IMAGE_RECORD_FORMAT",
            Self::Asset(error) => error.code(),
            Self::SampleBytesLimit => "FLOW_PDF_IMAGE_SAMPLE_LIMIT",
            Self::ResourceLimit => "FLOW_PDF_IMAGE_RESOURCE_LIMIT",
        }
    }
}

/// Validates all reachable image records and builds one resource per content
/// identity. Unreachable durable records are not copied into the export.
pub fn build_image_resources(
    document: &FlowDocument,
    records: &[AssetRecord],
) -> Result<Vec<PdfImageResource>, PdfImageError> {
    validate_document(document).map_err(|_| PdfImageError::InvalidDocument)?;
    let reachable = asset::reachable_asset_ids(document);
    let mut descriptors = BTreeMap::<String, Vec<&AssetDescriptor>>::new();
    for asset_id in reachable {
        let descriptor = document
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .ok_or(PdfImageError::DescriptorMissing)?;
        descriptors
            .entry(descriptor.content_hash.clone())
            .or_default()
            .push(descriptor);
    }
    if descriptors.len() > MAX_PDF_IMAGE_RESOURCES {
        return Err(PdfImageError::ResourceLimit);
    }

    let mut resources = Vec::with_capacity(descriptors.len());
    for (content_hash, descriptors) in descriptors {
        let record = matching_record(&content_hash, records)?;
        if record.record_format_version != crate::RECORD_FORMAT_VERSION {
            return Err(PdfImageError::RecordFormat);
        }
        if record.content_hash != content_hash {
            return Err(PdfImageError::DescriptorMismatch);
        }
        if asset_hash(&record.bytes) != content_hash {
            return Err(PdfImageError::ContentHashMismatch);
        }
        let (width, height, media_type) = validate_image_bytes_for_export(&record.bytes)?;
        let mut asset_ids = Vec::with_capacity(descriptors.len());
        for descriptor in descriptors {
            if descriptor.content_hash != content_hash
                || descriptor.byte_length != u32::try_from(record.bytes.len()).unwrap_or(u32::MAX)
                || descriptor.media_type != media_type
            {
                return Err(PdfImageError::DescriptorMismatch);
            }
            asset_ids.push(descriptor.id.clone());
        }
        let (encoding, bytes) = match media_type {
            "image/jpeg" => (PdfImageEncoding::JpegDct, record.bytes.clone()),
            "image/png" => (
                PdfImageEncoding::RawRgba,
                decode_png_rgba(&record.bytes, width, height)?,
            ),
            _ => return Err(PdfImageError::DescriptorMismatch),
        };
        let mut identity_bytes = Vec::new();
        identity_bytes.extend_from_slice(content_hash.as_bytes());
        identity_bytes.extend_from_slice(media_type.as_bytes());
        identity_bytes.extend_from_slice(&width.to_be_bytes());
        identity_bytes.extend_from_slice(&height.to_be_bytes());
        identity_bytes.push(match encoding {
            PdfImageEncoding::JpegDct => 1,
            PdfImageEncoding::RawRgba => 2,
        });
        identity_bytes.extend_from_slice(&bytes);
        resources.push(PdfImageResource {
            resource_name: format!("Im{}", resources.len() + 1),
            resource_identity: blake3::hash(&identity_bytes).to_hex().to_string(),
            content_hash,
            asset_ids,
            media_type: media_type.to_owned(),
            encoded_byte_length: u32::try_from(record.bytes.len()).unwrap_or(u32::MAX),
            width,
            height,
            encoding,
            bytes,
        });
    }
    Ok(resources)
}

fn matching_record<'a>(
    content_hash: &str,
    records: &'a [AssetRecord],
) -> Result<&'a AssetRecord, PdfImageError> {
    let mut matching = records
        .iter()
        .filter(|record| record.content_hash == content_hash);
    let record = matching.next().ok_or(PdfImageError::RecordMissing)?;
    if matching.next().is_some() {
        return Err(PdfImageError::DuplicateRecord);
    }
    Ok(record)
}

fn decode_png_rgba(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, PdfImageError> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(crate::MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(crate::MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_DECODED_BYTES);
    let mut reader = ImageReader::with_format(BufReader::new(Cursor::new(bytes)), ImageFormat::Png);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|_| asset::AssetError::DecodeFailed)?;
    if decoded.width() != width || decoded.height() != height {
        return Err(PdfImageError::DescriptorMismatch);
    }
    let samples = decoded.to_rgba8().into_raw();
    let expected = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(PdfImageError::SampleBytesLimit)?;
    if u64::try_from(samples.len()).unwrap_or(u64::MAX) != expected
        || expected > MAX_IMAGE_DECODED_BYTES
    {
        return Err(PdfImageError::SampleBytesLimit);
    }
    Ok(samples)
}

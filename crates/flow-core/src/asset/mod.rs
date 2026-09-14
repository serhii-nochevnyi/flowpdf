//! Bounded embedded-asset validation and ephemeral staging receipts.
//!
//! Image bytes cross into the core through this module only.  The semantic
//! document stores a canonical BLAKE3 identity and an explicit accessibility
//! decision; compressed bytes remain in the physical asset commit and are
//! never copied into commands, transactions, audits, or editor projections.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufReader, Cursor},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use image::{ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    canonical::asset_hash,
    model::{AssetDescriptor, AssetId, BlockKind, ContentNode, DocumentId, ImageAccessibility},
    store::AssetRecord,
};

pub const MAX_IMAGE_ENCODED_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGE_DIMENSION: u32 = 8_192;
pub const MAX_IMAGE_PIXELS: u64 = 24_000_000;
pub const MAX_IMAGE_DECODED_BYTES: u64 = 96 * 1024 * 1024;
pub const MAX_AUTHORED_ALT_BYTES: usize = 4_096;
pub const MAX_STAGING_RECEIPTS_PER_SESSION: usize = 2;
pub const MAX_STAGING_BYTES_PER_SESSION: usize = 16 * 1024 * 1024;
pub const STAGING_RECEIPT_TTL_SECONDS: u64 = 5 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetStageRequest {
    pub session_id: String,
    pub document_id: DocumentId,
    pub revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetStageResponse {
    pub receipt: String,
    pub content_hash: String,
    pub media_type: String,
    pub byte_length: u32,
    pub width: u32,
    pub height: u32,
    pub expires_at_unix_seconds: u64,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AssetError {
    #[error("The image format is not an admitted PNG or JPEG")]
    UnsupportedFormat,
    #[error("The encoded image exceeds the bounded upload size")]
    EncodedSizeLimit,
    #[error("The image dimensions exceed the bounded axis limit")]
    DimensionLimit,
    #[error("The image pixel count exceeds the bounded limit")]
    PixelLimit,
    #[error("The decoded image allocation exceeds the bounded limit")]
    DecodedAllocationLimit,
    #[error("The image could not be decoded")]
    DecodeFailed,
    #[error("The staging session is invalid")]
    InvalidSession,
    #[error("The staging session receipt limit was reached")]
    SessionReceiptLimit,
    #[error("The staging session byte limit was reached")]
    SessionByteLimit,
    #[error("The staging receipt is invalid")]
    InvalidReceipt,
    #[error("The staging receipt has expired")]
    ExpiredReceipt,
    #[error("The staging receipt was already redeemed")]
    ReplayedReceipt,
    #[error("The staging receipt does not belong to this revision")]
    ReceiptBindingMismatch,
    #[error("The image accessibility decision is invalid")]
    InvalidAccessibility,
    #[error("The staging registry is unavailable")]
    StagingUnavailable,
}

impl AssetError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedFormat => "FLOW_ASSET_FORMAT_UNSUPPORTED",
            Self::EncodedSizeLimit => "FLOW_LIMIT_ASSET_ENCODED_BYTES",
            Self::DimensionLimit => "FLOW_LIMIT_ASSET_DIMENSIONS",
            Self::PixelLimit => "FLOW_LIMIT_ASSET_PIXELS",
            Self::DecodedAllocationLimit => "FLOW_LIMIT_ASSET_DECODED_BYTES",
            Self::DecodeFailed => "FLOW_ASSET_DECODE_FAILED",
            Self::InvalidSession => "FLOW_ASSET_INVALID_SESSION",
            Self::SessionReceiptLimit => "FLOW_LIMIT_ASSET_RECEIPTS",
            Self::SessionByteLimit => "FLOW_LIMIT_ASSET_STAGING_BYTES",
            Self::InvalidReceipt => "FLOW_ASSET_INVALID_RECEIPT",
            Self::ExpiredReceipt => "FLOW_ASSET_RECEIPT_EXPIRED",
            Self::ReplayedReceipt => "FLOW_ASSET_RECEIPT_REPLAYED",
            Self::ReceiptBindingMismatch => "FLOW_ASSET_RECEIPT_BINDING",
            Self::InvalidAccessibility => "FLOW_ASSET_ACCESSIBILITY_REQUIRED",
            Self::StagingUnavailable => "FLOW_ASSET_STAGING_UNAVAILABLE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RedeemedAsset {
    descriptor: AssetDescriptor,
    record: AssetRecord,
}

impl RedeemedAsset {
    #[must_use]
    pub const fn descriptor(&self) -> &AssetDescriptor {
        &self.descriptor
    }

    #[must_use]
    pub fn into_record(self) -> AssetRecord {
        self.record
    }
}

#[derive(Debug, Clone)]
struct StagedAsset {
    session_id: String,
    document_id: DocumentId,
    revision: u32,
    receipt: String,
    staging_digest: String,
    descriptor: AssetDescriptor,
    bytes: Vec<u8>,
    expires_at: u64,
}

#[derive(Debug, Clone, Default)]
pub struct AssetStagingStore {
    next_receipt: u64,
    active: BTreeMap<String, StagedAsset>,
    spent: BTreeMap<String, u64>,
}

impl AssetStagingStore {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next_receipt: 0,
            active: BTreeMap::new(),
            spent: BTreeMap::new(),
        }
    }

    pub fn stage(
        &mut self,
        request: &AssetStageRequest,
        bytes: Vec<u8>,
    ) -> Result<AssetStageResponse, AssetError> {
        self.stage_at(request, bytes, unix_seconds())
    }

    pub fn stage_at(
        &mut self,
        request: &AssetStageRequest,
        bytes: Vec<u8>,
        now: u64,
    ) -> Result<AssetStageResponse, AssetError> {
        self.prune(now);
        validate_session_id(&request.session_id)?;
        let validated = validate_image_bytes(&bytes)?;
        let (width, height, media_type) = validated;
        let session_receipts = self
            .active
            .values()
            .filter(|asset| asset.session_id == request.session_id)
            .count();
        if session_receipts >= MAX_STAGING_RECEIPTS_PER_SESSION {
            return Err(AssetError::SessionReceiptLimit);
        }
        let staged_bytes = self
            .active
            .values()
            .filter(|asset| asset.session_id == request.session_id)
            .map(|asset| asset.bytes.len())
            .sum::<usize>();
        if staged_bytes
            .checked_add(bytes.len())
            .is_none_or(|total| total > MAX_STAGING_BYTES_PER_SESSION)
        {
            return Err(AssetError::SessionByteLimit);
        }
        let byte_length = u32::try_from(bytes.len()).map_err(|_| AssetError::EncodedSizeLimit)?;
        let content_hash = asset_hash(&bytes);
        let descriptor = AssetDescriptor {
            id: AssetId::new(deterministic_asset_uuid(&content_hash, self.next_receipt))
                .map_err(|_| AssetError::StagingUnavailable)?,
            content_hash: content_hash.clone(),
            media_type: media_type.to_owned(),
            byte_length,
            alt_text: String::new(),
        };
        let receipt = self.new_receipt(&request.session_id, &request.document_id, request.revision);
        let expires_at = now.saturating_add(STAGING_RECEIPT_TTL_SECONDS);
        let staging_digest = format!(
            "staging:{}",
            blake3::hash(
                format!(
                    "flowpdf:staging:v1:{}:{}",
                    content_hash,
                    request.document_id.as_str()
                )
                .as_bytes(),
            )
            .to_hex()
        );
        self.active.insert(
            receipt.clone(),
            StagedAsset {
                session_id: request.session_id.clone(),
                document_id: request.document_id.clone(),
                revision: request.revision,
                receipt: receipt.clone(),
                staging_digest,
                descriptor,
                bytes,
                expires_at,
            },
        );
        Ok(AssetStageResponse {
            receipt,
            content_hash,
            media_type: media_type.to_owned(),
            byte_length,
            width,
            height,
            expires_at_unix_seconds: expires_at,
        })
    }

    pub fn redeem(
        &mut self,
        receipt: &str,
        session_id: &str,
        document_id: &DocumentId,
        revision: u32,
    ) -> Result<RedeemedAsset, AssetError> {
        self.redeem_at(receipt, session_id, document_id, revision, unix_seconds())
    }

    pub fn redeem_at(
        &mut self,
        receipt: &str,
        session_id: &str,
        document_id: &DocumentId,
        revision: u32,
        now: u64,
    ) -> Result<RedeemedAsset, AssetError> {
        self.spent.retain(|_, expires_at| *expires_at > now);
        if self.spent.contains_key(receipt) {
            return Err(AssetError::ReplayedReceipt);
        }
        let staged = self.active.get(receipt).ok_or(AssetError::InvalidReceipt)?;
        if staged.expires_at <= now {
            return Err(AssetError::ExpiredReceipt);
        }
        if staged.receipt != receipt
            || staged.session_id != session_id
            || staged.document_id != *document_id
            || staged.revision != revision
        {
            return Err(AssetError::ReceiptBindingMismatch);
        }
        let staged = self
            .active
            .remove(receipt)
            .ok_or(AssetError::InvalidReceipt)?;
        self.spent.insert(receipt.to_owned(), staged.expires_at);
        // The staging digest is intentionally retained only in this private
        // value's lifetime. It is never part of a durable or public DTO.
        let _private_staging_digest = staged.staging_digest;
        Ok(RedeemedAsset {
            descriptor: staged.descriptor,
            record: AssetRecord {
                record_format_version: crate::RECORD_FORMAT_VERSION,
                content_hash: asset_hash(&staged.bytes),
                bytes: staged.bytes,
            },
        })
    }

    fn new_receipt(&mut self, session_id: &str, document_id: &DocumentId, revision: u32) -> String {
        let counter = self.next_receipt;
        self.next_receipt = self.next_receipt.saturating_add(1);
        let digest = blake3::hash(
            format!(
                "flowpdf:receipt:v1:{}:{}:{}:{}",
                counter,
                session_id,
                document_id.as_str(),
                revision
            )
            .as_bytes(),
        );
        format!("flowpdf:receipt:v1:{}", digest.to_hex())
    }

    fn prune(&mut self, now: u64) {
        self.active.retain(|_, asset| asset.expires_at > now);
        self.spent.retain(|_, expires_at| *expires_at > now);
    }
}

fn validate_session_id(session_id: &str) -> Result<(), AssetError> {
    if session_id.trim().is_empty() || session_id.len() > 128 {
        return Err(AssetError::InvalidSession);
    }
    Ok(())
}

fn validate_image_bytes(bytes: &[u8]) -> Result<(u32, u32, &'static str), AssetError> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_ENCODED_BYTES {
        return Err(AssetError::EncodedSizeLimit);
    }
    let (format, media_type) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        (ImageFormat::Png, "image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        (ImageFormat::Jpeg, "image/jpeg")
    } else {
        return Err(AssetError::UnsupportedFormat);
    };
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_DECODED_BYTES);
    let mut dimension_reader = ImageReader::with_format(BufReader::new(Cursor::new(bytes)), format);
    dimension_reader.limits(limits.clone());
    let dimensions = dimension_reader
        .into_dimensions()
        .map_err(|_| AssetError::DecodeFailed)?;
    check_dimensions(dimensions.0, dimensions.1)?;

    let mut decode_reader = ImageReader::with_format(BufReader::new(Cursor::new(bytes)), format);
    decode_reader.limits(limits);
    let decoded = decode_reader
        .decode()
        .map_err(|_| AssetError::DecodeFailed)?;
    if decoded.width() != dimensions.0 || decoded.height() != dimensions.1 {
        return Err(AssetError::DecodeFailed);
    }
    if u64::try_from(decoded.as_bytes().len()).unwrap_or(u64::MAX) > MAX_IMAGE_DECODED_BYTES {
        return Err(AssetError::DecodedAllocationLimit);
    }
    Ok((dimensions.0, dimensions.1, media_type))
}

fn check_dimensions(width: u32, height: u32) -> Result<(), AssetError> {
    if width == 0 || height == 0 || width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(AssetError::DimensionLimit);
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(AssetError::PixelLimit)?;
    if pixels > MAX_IMAGE_PIXELS {
        return Err(AssetError::PixelLimit);
    }
    let rgba_bytes = pixels
        .checked_mul(4)
        .ok_or(AssetError::DecodedAllocationLimit)?;
    if rgba_bytes > MAX_IMAGE_DECODED_BYTES {
        return Err(AssetError::DecodedAllocationLimit);
    }
    Ok(())
}

fn deterministic_asset_uuid(content_hash: &str, counter: u64) -> String {
    let digest = blake3::hash(format!("flowpdf:asset:{}:{}", content_hash, counter).as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).hyphenated().to_string()
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

static GLOBAL_STAGING: OnceLock<Mutex<AssetStagingStore>> = OnceLock::new();

fn global_staging() -> Result<&'static Mutex<AssetStagingStore>, AssetError> {
    Ok(GLOBAL_STAGING.get_or_init(|| Mutex::new(AssetStagingStore::new())))
}

pub fn stage_asset(
    request: AssetStageRequest,
    bytes: Vec<u8>,
) -> Result<AssetStageResponse, AssetError> {
    let mut store = global_staging()
        .map_err(|_| AssetError::StagingUnavailable)?
        .lock()
        .map_err(|_| AssetError::StagingUnavailable)?;
    store.stage(&request, bytes)
}

pub(crate) fn redeem_asset(
    receipt: &str,
    session_id: &str,
    document_id: &DocumentId,
    revision: u32,
) -> Result<RedeemedAsset, AssetError> {
    let mut store = global_staging()
        .map_err(|_| AssetError::StagingUnavailable)?
        .lock()
        .map_err(|_| AssetError::StagingUnavailable)?;
    store.redeem(receipt, session_id, document_id, revision)
}

#[must_use]
pub fn reachable_asset_ids(document: &crate::model::FlowDocument) -> BTreeSet<AssetId> {
    fn visit(nodes: &[ContentNode], ids: &mut BTreeSet<AssetId>) {
        for node in nodes {
            if let BlockKind::Image { asset_id, .. } = &node.body {
                ids.insert(asset_id.clone());
            }
            visit(node.children(), ids);
        }
    }
    let mut ids = BTreeSet::new();
    visit(&document.content, &mut ids);
    ids
}

#[must_use]
pub fn reachable_asset_hashes(document: &crate::model::FlowDocument) -> BTreeSet<String> {
    let ids = reachable_asset_ids(document);
    document
        .assets
        .iter()
        .filter(|asset| ids.contains(&asset.id))
        .map(|asset| asset.content_hash.clone())
        .collect()
}

pub fn validate_accessibility(accessibility: &ImageAccessibility) -> Result<(), AssetError> {
    match accessibility {
        ImageAccessibility::Described { text }
            if text.trim().is_empty() || text.len() > MAX_AUTHORED_ALT_BYTES =>
        {
            Err(AssetError::InvalidAccessibility)
        }
        ImageAccessibility::Described { .. } | ImageAccessibility::Decorative => Ok(()),
        ImageAccessibility::MissingLegacy => Err(AssetError::InvalidAccessibility),
    }
}

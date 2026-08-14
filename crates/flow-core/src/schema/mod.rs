//! Schema validation, fixed resource budgets, and sequential migrations.

use std::collections::BTreeSet;

use thiserror::Error;
use uuid::Uuid;

use crate::model::{
    ContentNodeKind, FieldDescriptor, FieldKind, FieldValue, FlowDocument, SCHEMA_VERSION,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    CanonicalBytes,
    TreeDepth,
    SemanticNodes,
    TotalTextBytes,
    TextNodeBytes,
    Styles,
    Assets,
    Fields,
    TransactionOperations,
    TransactionBytes,
    RecoveryRecords,
    RecoveryBytes,
}

impl LimitKind {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::CanonicalBytes => "FLOW_LIMIT_CANONICAL_BYTES",
            Self::TreeDepth => "FLOW_LIMIT_TREE_DEPTH",
            Self::SemanticNodes => "FLOW_LIMIT_SEMANTIC_NODES",
            Self::TotalTextBytes => "FLOW_LIMIT_TOTAL_TEXT_BYTES",
            Self::TextNodeBytes => "FLOW_LIMIT_TEXT_NODE_BYTES",
            Self::Styles => "FLOW_LIMIT_STYLES",
            Self::Assets => "FLOW_LIMIT_ASSETS",
            Self::Fields => "FLOW_LIMIT_FIELDS",
            Self::TransactionOperations => "FLOW_LIMIT_TRANSACTION_OPERATIONS",
            Self::TransactionBytes => "FLOW_LIMIT_TRANSACTION_BYTES",
            Self::RecoveryRecords => "FLOW_LIMIT_RECOVERY_RECORDS",
            Self::RecoveryBytes => "FLOW_LIMIT_RECOVERY_BYTES",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentLimits {
    pub canonical_bytes: usize,
    pub tree_depth: usize,
    pub semantic_nodes: usize,
    pub total_text_bytes: usize,
    pub text_node_bytes: usize,
    pub styles: usize,
    pub assets: usize,
    pub fields: usize,
    pub transaction_operations: usize,
    pub transaction_bytes: usize,
    pub recovery_records: usize,
    pub recovery_bytes: usize,
}

impl DocumentLimits {
    pub const V1: Self = Self {
        canonical_bytes: 64 * 1024 * 1024,
        tree_depth: 128,
        semantic_nodes: 200_000,
        total_text_bytes: 32 * 1024 * 1024,
        text_node_bytes: 4 * 1024 * 1024,
        styles: 4_096,
        assets: 10_000,
        fields: 10_000,
        transaction_operations: 10_000,
        transaction_bytes: 8 * 1024 * 1024,
        recovery_records: 10_000,
        recovery_bytes: 64 * 1024 * 1024,
    };

    pub fn check(self, kind: LimitKind, actual: usize) -> Result<(), SchemaError> {
        let maximum = match kind {
            LimitKind::CanonicalBytes => self.canonical_bytes,
            LimitKind::TreeDepth => self.tree_depth,
            LimitKind::SemanticNodes => self.semantic_nodes,
            LimitKind::TotalTextBytes => self.total_text_bytes,
            LimitKind::TextNodeBytes => self.text_node_bytes,
            LimitKind::Styles => self.styles,
            LimitKind::Assets => self.assets,
            LimitKind::Fields => self.fields,
            LimitKind::TransactionOperations => self.transaction_operations,
            LimitKind::TransactionBytes => self.transaction_bytes,
            LimitKind::RecoveryRecords => self.recovery_records,
            LimitKind::RecoveryBytes => self.recovery_bytes,
        };
        if actual > maximum {
            return Err(SchemaError::limit(kind, actual, maximum));
        }
        Ok(())
    }

    pub fn check_transaction(
        self,
        operation_count: usize,
        record_bytes: usize,
    ) -> Result<(), SchemaError> {
        self.check(LimitKind::TransactionOperations, operation_count)?;
        self.check(LimitKind::TransactionBytes, record_bytes)
    }

    pub fn check_recovery(
        self,
        record_count: usize,
        replay_bytes: usize,
    ) -> Result<(), SchemaError> {
        self.check(LimitKind::RecoveryRecords, record_count)?;
        self.check(LimitKind::RecoveryBytes, replay_bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct SchemaError {
    code: &'static str,
    message: String,
}

impl SchemaError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    pub(crate) fn decode() -> Self {
        Self::new(
            "FLOW_SCHEMA_DECODE",
            "The document JSON does not match the closed schema",
        )
    }

    pub(crate) fn non_canonical() -> Self {
        Self::new(
            "FLOW_NON_CANONICAL_PAYLOAD",
            "The document bytes are not the compact canonical representation",
        )
    }

    pub(crate) fn serialization() -> Self {
        Self::new(
            "FLOW_SCHEMA_SERIALIZE",
            "The typed document could not be serialized",
        )
    }

    pub(crate) fn unsupported_schema() -> Self {
        Self::new(
            "FLOW_UNSUPPORTED_SCHEMA",
            "The schema version is not supported",
        )
    }

    pub(crate) fn invalid_document() -> Self {
        Self::new("FLOW_INVALID_DOCUMENT", "A document invariant is invalid")
    }

    pub(crate) fn invalid_id() -> Self {
        Self::new(
            "FLOW_INVALID_ID",
            "A stable identity is not UUID-compatible",
        )
    }

    fn duplicate_id() -> Self {
        Self::new(
            "FLOW_DUPLICATE_ID",
            "Stable identities must be globally distinct",
        )
    }

    fn dangling_reference() -> Self {
        Self::new(
            "FLOW_DANGLING_REFERENCE",
            "A semantic reference has no target",
        )
    }

    fn invalid_utf16_position() -> Self {
        Self::new(
            "FLOW_INVALID_UTF16_POSITION",
            "A public position is not a valid UTF-16 boundary",
        )
    }

    fn field_value_invalid() -> Self {
        Self::new(
            "FLOW_FIELD_VALUE_INVALID",
            "The field default value is incompatible with its kind",
        )
    }

    fn field_options_invalid() -> Self {
        Self::new(
            "FLOW_FIELD_OPTIONS_INVALID",
            "The field options are incompatible, duplicated, or invalid",
        )
    }

    pub(crate) fn asset_hash_mismatch() -> Self {
        Self::new(
            "FLOW_ASSET_HASH_MISMATCH",
            "Resolved asset bytes do not match their descriptor",
        )
    }

    pub(crate) fn identity_conflict() -> Self {
        Self::new(
            "FLOW_IDENTITY_CONFLICT",
            "The same document identity already has different canonical bytes",
        )
    }

    pub(crate) fn future_schema() -> Self {
        Self::new(
            "FLOW_MIGRATION_FUTURE_VERSION",
            "A future schema version cannot be migrated by this registry",
        )
    }

    pub(crate) fn migration_hop_missing() -> Self {
        Self::new(
            "FLOW_MIGRATION_HOP_MISSING",
            "The sequential migration registry has no required hop",
        )
    }

    pub(crate) fn migration_intermediate_invalid() -> Self {
        Self::new(
            "FLOW_MIGRATION_INTERMEDIATE_INVALID",
            "A migration hop did not produce its declared validated version",
        )
    }

    pub(crate) fn limit(kind: LimitKind, actual: usize, maximum: usize) -> Self {
        Self::new(
            kind.code(),
            format!("Resource limit exceeded: actual {actual}, maximum {maximum}"),
        )
    }

    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub fn validate_document(document: &FlowDocument) -> Result<(), SchemaError> {
    if document.schema_version != SCHEMA_VERSION {
        return Err(SchemaError::unsupported_schema());
    }
    if document.revision == 0 || document.locale.is_empty() || document.locale.len() > 32 {
        return Err(SchemaError::invalid_document());
    }

    let limits = DocumentLimits::V1;
    limits.check(LimitKind::Styles, document.styles.len())?;
    limits.check(LimitKind::SemanticNodes, document.content.len())?;
    limits.check(LimitKind::Assets, document.assets.len())?;
    limits.check(LimitKind::Fields, document.fields.len())?;

    let mut all_ids = BTreeSet::new();
    insert_id(&mut all_ids, document.document_id.as_str())?;

    let mut style_ids = BTreeSet::new();
    let mut style_names = BTreeSet::new();
    for style in &document.styles {
        insert_id(&mut all_ids, style.id.as_str())?;
        style_ids.insert(style.id.as_str());
        if style.name.trim().is_empty()
            || style.name.len() > 256
            || style.font_family.trim().is_empty()
            || style.font_family.len() > 512
            || style.font_size_millipoints == 0
            || !style_names.insert(style.name.as_str())
        {
            return Err(SchemaError::invalid_document());
        }
    }

    let mut asset_ids = BTreeSet::new();
    for asset in &document.assets {
        insert_id(&mut all_ids, asset.id.as_str())?;
        asset_ids.insert(asset.id.as_str());
        if !is_asset_hash(&asset.content_hash)
            || asset.media_type.trim().is_empty()
            || asset.media_type.len() > 256
            || asset.alt_text.len() > 16_384
        {
            return Err(SchemaError::invalid_document());
        }
    }

    let mut node_ids = BTreeSet::new();
    let mut total_text_bytes = 0_usize;
    for node in &document.content {
        insert_id(&mut all_ids, node.id.as_str())?;
        node_ids.insert(node.id.as_str());
        limits.check(LimitKind::TextNodeBytes, node.text.len())?;
        total_text_bytes = total_text_bytes
            .checked_add(node.text.len())
            .ok_or_else(SchemaError::invalid_document)?;
        match node.kind {
            ContentNodeKind::Paragraph => {
                let style_id = node
                    .style_id
                    .as_ref()
                    .ok_or_else(SchemaError::dangling_reference)?;
                if !style_ids.contains(style_id.as_str()) || node.asset_id.is_some() {
                    return Err(SchemaError::dangling_reference());
                }
            }
            ContentNodeKind::Image => {
                let asset_id = node
                    .asset_id
                    .as_ref()
                    .ok_or_else(SchemaError::dangling_reference)?;
                if !asset_ids.contains(asset_id.as_str())
                    || node.style_id.is_some()
                    || !node.text.is_empty()
                {
                    return Err(SchemaError::dangling_reference());
                }
            }
        }
    }
    limits.check(LimitKind::TotalTextBytes, total_text_bytes)?;

    let mut field_names = BTreeSet::new();
    for field in &document.fields {
        insert_id(&mut all_ids, field.id.as_str())?;
        if field.name.trim().is_empty()
            || field.name.len() > 512
            || !field_names.insert(field.name.as_str())
            || field
                .label
                .as_ref()
                .is_some_and(|label| label.len() > 16_384)
        {
            return Err(SchemaError::invalid_document());
        }
        let node = document
            .content
            .iter()
            .find(|node| node.id == field.anchor.node_id)
            .ok_or_else(SchemaError::dangling_reference)?;
        if utf16_to_byte_offset(&node.text, field.anchor.utf16_offset).is_none() {
            return Err(SchemaError::invalid_utf16_position());
        }
        validate_field(field, &mut all_ids)?;
    }

    Ok(())
}

fn validate_field(
    field: &FieldDescriptor,
    all_ids: &mut BTreeSet<String>,
) -> Result<(), SchemaError> {
    let options_allowed = matches!(field.kind, FieldKind::RadioGroup | FieldKind::Select { .. });
    if !options_allowed && !field.options.is_empty() {
        return Err(SchemaError::field_options_invalid());
    }

    let mut option_ids = BTreeSet::new();
    let mut export_values = BTreeSet::new();
    for option in &field.options {
        insert_id(all_ids, option.id.as_str())?;
        if option.label.trim().is_empty()
            || option.label.len() > 16_384
            || option.export_value.trim().is_empty()
            || option.export_value.len() > 4_096
            || !option_ids.insert(option.id.as_str())
            || !export_values.insert(option.export_value.as_str())
        {
            return Err(SchemaError::field_options_invalid());
        }
    }

    match (&field.kind, &field.default_value) {
        (FieldKind::Text { .. }, FieldValue::Empty | FieldValue::Text { .. })
        | (FieldKind::Checkbox, FieldValue::Empty | FieldValue::Checked { .. })
        | (FieldKind::Signature | FieldKind::Button, FieldValue::Empty) => {}
        (FieldKind::RadioGroup, FieldValue::Empty)
        | (FieldKind::Select { .. }, FieldValue::Empty) => {}
        (
            FieldKind::RadioGroup,
            FieldValue::Selected {
                option_ids: selected,
            },
        ) => {
            validate_selected(selected, &option_ids, 1)?;
        }
        (
            FieldKind::Select { multiple },
            FieldValue::Selected {
                option_ids: selected,
            },
        ) => {
            let maximum = if *multiple { usize::MAX } else { 1 };
            validate_selected(selected, &option_ids, maximum)?;
        }
        _ => return Err(SchemaError::field_value_invalid()),
    }
    Ok(())
}

fn validate_selected(
    selected: &[crate::model::FieldOptionId],
    available: &BTreeSet<&str>,
    maximum: usize,
) -> Result<(), SchemaError> {
    if selected.len() > maximum {
        return Err(SchemaError::field_value_invalid());
    }
    let mut unique = BTreeSet::new();
    for selected_id in selected {
        if !available.contains(selected_id.as_str()) || !unique.insert(selected_id.as_str()) {
            return Err(SchemaError::field_value_invalid());
        }
    }
    Ok(())
}

fn insert_id(ids: &mut BTreeSet<String>, value: &str) -> Result<(), SchemaError> {
    if Uuid::parse_str(value).is_err() {
        return Err(SchemaError::invalid_id());
    }
    if !ids.insert(value.to_owned()) {
        return Err(SchemaError::duplicate_id());
    }
    Ok(())
}

fn is_asset_hash(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("blake3:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[must_use]
pub fn utf16_to_byte_offset(value: &str, utf16_offset: u32) -> Option<usize> {
    let requested = usize::try_from(utf16_offset).ok()?;
    if requested == 0 {
        return Some(0);
    }
    let mut consumed = 0;
    for (byte_index, character) in value.char_indices() {
        if consumed == requested {
            return Some(byte_index);
        }
        consumed += character.len_utf16();
        if consumed > requested {
            return None;
        }
    }
    (consumed == requested).then_some(value.len())
}

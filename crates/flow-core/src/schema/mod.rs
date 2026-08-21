//! Schema validation, fixed resource budgets, and sequential migrations.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::anchor::{Utf16Offset, resolve_utf16_offset};
use crate::model::{
    AssetDescriptor, ContentNode, ContentNodeKind, DocumentId, FieldDescriptor, FieldKind,
    FieldValue, FlowDocument, MigrationHop, PageSettings, Provenance, SCHEMA_VERSION,
    StyleDefinition,
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

    pub fn migration_aborted() -> Self {
        Self::new(
            "FLOW_MIGRATION_ABORTED",
            "The migration was interrupted before a current snapshot was published",
        )
    }

    fn migration_input_invalid() -> Self {
        Self::new(
            "FLOW_MIGRATION_INPUT_INVALID",
            "The source document is not a valid canonical supported schema",
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
        if resolve_utf16_offset(&node.text, field.anchor.utf16_offset).is_err() {
            return Err(SchemaError::invalid_utf16_position());
        }
        validate_field(field, &mut all_ids)?;
    }

    validate_provenance(document)?;

    Ok(())
}

fn validate_provenance(document: &FlowDocument) -> Result<(), SchemaError> {
    match &document.provenance {
        Provenance::LocalSample { created_at } => {
            if !is_compact_utc_timestamp(created_at) {
                return Err(SchemaError::invalid_document());
            }
        }
        Provenance::Migrated {
            source_schema_version,
            current_schema_version,
            source_created_at,
            hops,
        } => {
            if *source_schema_version >= *current_schema_version
                || *current_schema_version != document.schema_version
                || !is_compact_utc_timestamp(source_created_at)
                || hops.is_empty()
                || hops.len() > 32
            {
                return Err(SchemaError::invalid_document());
            }
            let mut expected_from = *source_schema_version;
            for hop in hops {
                let expected_to = expected_from
                    .checked_add(1)
                    .ok_or_else(SchemaError::invalid_document)?;
                if hop.from_version != expected_from || hop.to_version != expected_to {
                    return Err(SchemaError::invalid_document());
                }
                expected_from = hop.to_version;
            }
            if expected_from != *current_schema_version {
                return Err(SchemaError::invalid_document());
            }
        }
    }
    Ok(())
}

pub(crate) fn is_compact_utc_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    let punctuation = [
        (4, b'-'),
        (7, b'-'),
        (10, b'T'),
        (13, b':'),
        (16, b':'),
        (19, b'Z'),
    ];
    if bytes.len() != 20
        || punctuation
            .iter()
            .any(|(index, expected)| bytes[*index] != *expected)
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| ![4, 7, 10, 13, 16, 19].contains(&index) && !byte.is_ascii_digit())
    {
        return false;
    }
    let year = timestamp_decimal(&bytes[0..4]);
    let month = timestamp_decimal(&bytes[5..7]);
    let day = timestamp_decimal(&bytes[8..10]);
    let hour = timestamp_decimal(&bytes[11..13]);
    let minute = timestamp_decimal(&bytes[14..16]);
    let second = timestamp_decimal(&bytes[17..19]);
    let maximum_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => return false,
    };
    year != 0 && (1..=maximum_day).contains(&day) && hour <= 23 && minute <= 59 && second <= 59
}

fn timestamp_decimal(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0, |value, byte| value * 10 + u32::from(byte - b'0'))
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
    resolve_utf16_offset(value, Utf16Offset::new(utf16_offset))
        .ok()
        .map(|offset| offset.get())
}

pub type MigrationFunction = fn(&[u8]) -> Result<Vec<u8>, SchemaError>;

#[derive(Clone, Copy)]
pub struct MigrationStep {
    pub from_version: u32,
    pub to_version: u32,
    migrate: MigrationFunction,
}

impl MigrationStep {
    #[must_use]
    pub const fn new(from_version: u32, to_version: u32, migrate: MigrationFunction) -> Self {
        Self {
            from_version,
            to_version,
            migrate,
        }
    }
}

#[derive(Clone)]
pub struct MigrationRegistry {
    steps: Vec<MigrationStep>,
}

impl MigrationRegistry {
    #[must_use]
    pub fn current() -> Self {
        Self::new(vec![MigrationStep::new(0, 1, migrate_v0_to_v1)])
    }

    #[must_use]
    pub fn new(steps: Vec<MigrationStep>) -> Self {
        Self { steps }
    }

    pub fn migrate(&self, input: &[u8]) -> Result<MigrationOutcome, SchemaError> {
        crate::canonical::preflight_canonical_bytes(input)?;
        let source_schema_version = probe_schema_version(input)?;
        if source_schema_version > SCHEMA_VERSION {
            return Err(SchemaError::future_schema());
        }
        if source_schema_version == SCHEMA_VERSION {
            let document = crate::canonical::decode_canonical(input)?;
            return Ok(MigrationOutcome {
                canonical_hash: crate::canonical::canonical_hash(input),
                canonical_bytes: input.to_vec(),
                document,
                report: MigrationReport {
                    source_schema_version,
                    current_schema_version: SCHEMA_VERSION,
                    hops: Vec::new(),
                    requires_new_snapshot: false,
                    preserve_source_records: true,
                },
            });
        }

        let mut version = source_schema_version;
        let mut bytes = input.to_vec();
        let mut hops = Vec::new();
        while version < SCHEMA_VERSION {
            let expected_to = version
                .checked_add(1)
                .ok_or_else(SchemaError::migration_hop_missing)?;
            let step = self
                .steps
                .iter()
                .find(|step| step.from_version == version && step.to_version == expected_to)
                .ok_or_else(SchemaError::migration_hop_missing)?;
            let candidate = (step.migrate)(&bytes)?;
            let candidate_version = probe_schema_version(&candidate)
                .map_err(|_| SchemaError::migration_intermediate_invalid())?;
            if candidate_version != expected_to {
                return Err(SchemaError::migration_intermediate_invalid());
            }
            if candidate_version == SCHEMA_VERSION
                && crate::canonical::decode_canonical(&candidate).is_err()
            {
                return Err(SchemaError::migration_intermediate_invalid());
            }
            bytes = candidate;
            hops.push(MigrationHop {
                from_version: version,
                to_version: expected_to,
            });
            version = expected_to;
        }

        let document = crate::canonical::decode_canonical(&bytes)
            .map_err(|_| SchemaError::migration_intermediate_invalid())?;
        Ok(MigrationOutcome {
            canonical_hash: crate::canonical::canonical_hash(&bytes),
            canonical_bytes: bytes,
            document,
            report: MigrationReport {
                source_schema_version,
                current_schema_version: SCHEMA_VERSION,
                hops,
                requires_new_snapshot: true,
                preserve_source_records: true,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    pub document: FlowDocument,
    pub canonical_bytes: Vec<u8>,
    pub canonical_hash: String,
    pub report: MigrationReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationReport {
    pub source_schema_version: u32,
    pub current_schema_version: u32,
    pub hops: Vec<MigrationHop>,
    pub requires_new_snapshot: bool,
    pub preserve_source_records: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionProbe {
    schema_version: u32,
}

fn probe_schema_version(input: &[u8]) -> Result<u32, SchemaError> {
    serde_json::from_slice::<VersionProbe>(input)
        .map(|probe| probe.schema_version)
        .map_err(|_| SchemaError::decode())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyDocumentV0 {
    schema_version: u32,
    document_id: DocumentId,
    revision: u32,
    locale: String,
    page_settings: PageSettings,
    styles: Vec<StyleDefinition>,
    content: Vec<ContentNode>,
    assets: Vec<AssetDescriptor>,
    fields: Vec<FieldDescriptor>,
    provenance: LegacyProvenanceV0,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyProvenanceV0 {
    created_at: String,
}

fn migrate_v0_to_v1(input: &[u8]) -> Result<Vec<u8>, SchemaError> {
    let legacy: LegacyDocumentV0 =
        serde_json::from_slice(input).map_err(|_| SchemaError::migration_input_invalid())?;
    if legacy.schema_version != 0
        || serde_json::to_vec(&legacy).map_err(|_| SchemaError::serialization())? != input
    {
        return Err(SchemaError::migration_input_invalid());
    }
    let document = FlowDocument {
        schema_version: SCHEMA_VERSION,
        document_id: legacy.document_id,
        revision: legacy.revision,
        locale: legacy.locale,
        page_settings: legacy.page_settings,
        styles: legacy.styles,
        content: legacy.content,
        assets: legacy.assets,
        fields: legacy.fields,
        provenance: Provenance::Migrated {
            source_schema_version: 0,
            current_schema_version: SCHEMA_VERSION,
            source_created_at: legacy.provenance.created_at,
            hops: vec![MigrationHop {
                from_version: 0,
                to_version: 1,
            }],
        },
    };
    crate::canonical::canonical_bytes(&document)
}

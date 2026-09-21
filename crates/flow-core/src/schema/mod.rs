//! Schema validation, fixed resource budgets, and sequential migrations.

use std::collections::{BTreeMap, BTreeSet};

use icu_segmenter::GraphemeClusterSegmenter;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

mod legacy;
mod legacy_v2;

use crate::anchor::{Utf16Offset, resolve_utf16_offset};
use crate::model::*;

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

    fn layout_geometry_invalid() -> Self {
        Self::new(
            "FLOW_LAYOUT_GEOMETRY_INVALID",
            "Page geometry or header/footer distance is outside the supported bounds",
        )
    }

    fn section_invalid() -> Self {
        Self::new(
            "FLOW_LAYOUT_SECTION_INVALID",
            "Section boundaries or settings are not ordered and reference-safe",
        )
    }

    fn header_footer_limit() -> Self {
        Self::new(
            "FLOW_LAYOUT_HEADER_FOOTER_LIMIT",
            "Static header/footer content exceeds the supported bounds",
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
    validate_sections(document, &mut all_ids, false)?;

    let mut style_ids = BTreeSet::new();
    let mut style_names = BTreeSet::new();
    for style in &document.styles {
        insert_id(&mut all_ids, style.id.as_str())?;
        style_ids.insert(style.id.as_str());
        if style.name.trim().is_empty()
            || style.name.len() > 256
            || style.font_size_millipoints == 0
            || !style_names.insert(style.name.as_str())
        {
            return Err(SchemaError::invalid_document());
        }
        validate_font(&style.font_family, false)?;
    }

    let mut asset_ids = BTreeSet::new();
    let mut asset_lengths = BTreeMap::<&str, u32>::new();
    let mut encoded_asset_bytes = 0_usize;
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
        if let Some(existing_length) = asset_lengths.get(asset.content_hash.as_str()) {
            if *existing_length != asset.byte_length {
                return Err(SchemaError::invalid_document());
            }
        } else {
            asset_lengths.insert(asset.content_hash.as_str(), asset.byte_length);
            // JSON encodes each u8 as at most three digits plus a separator.
            // The fixed allowance covers the version/hash field names and delimiters.
            const ASSET_RECORD_JSON_OVERHEAD: usize = 128;
            let byte_length =
                usize::try_from(asset.byte_length).map_err(|_| SchemaError::invalid_document())?;
            let encoded_length = byte_length
                .checked_mul(4)
                .and_then(|length| length.checked_add(ASSET_RECORD_JSON_OVERHEAD))
                .ok_or_else(SchemaError::invalid_document)?;
            encoded_asset_bytes = encoded_asset_bytes
                .checked_add(encoded_length)
                .ok_or_else(SchemaError::invalid_document)?;
            limits.check(LimitKind::RecoveryBytes, encoded_asset_bytes)?;
        }
    }
    // Every recoverable document needs at least one checkpoint plus its
    // creation/migration audit and either a transaction or migration source.
    // Reserve that irreducible overhead so a schema-valid asset set can still
    // be represented by a valid recovery image.
    const MINIMUM_NON_ASSET_RECORDS: usize = 3;
    let minimum_recovery_records = asset_lengths
        .len()
        .checked_add(MINIMUM_NON_ASSET_RECORDS)
        .ok_or_else(SchemaError::invalid_document)?;
    limits.check(LimitKind::RecoveryRecords, minimum_recovery_records)?;

    let nodes = validate_tree(
        &document.content,
        &style_ids,
        &asset_ids,
        &mut all_ids,
        false,
    )?;
    let boundaries = field_boundaries(
        &nodes,
        document.fields.iter().map(|field| field.anchor.original()),
    );

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
        match &field.anchor {
            FieldAnchorState::GraphemeSafe { original } => {
                if !nodes.contains_key(original.node_id.as_str()) {
                    return Err(SchemaError::dangling_reference());
                }
                if !boundary_contains(&boundaries, original) {
                    return Err(SchemaError::invalid_utf16_position());
                }
            }
            FieldAnchorState::LegacyInvalid { .. } => {
                // This is a durable review state, never an instruction to snap
                // or promote an anchor after subsequent text edits.
            }
            FieldAnchorState::TargetDeleted { original, .. } => {
                if nodes.contains_key(original.node_id.as_str()) {
                    return Err(SchemaError::invalid_document());
                }
            }
        }
        validate_field(field, &mut all_ids)?;
    }

    validate_provenance(document)?;

    Ok(())
}

pub const MAX_TABLE_ROWS: usize = 50;
pub const MAX_TABLE_COLUMNS: usize = 20;
pub const MAX_TABLE_CELLS: usize = 1_000;
pub const MAX_LIST_DEPTH: usize = 8;
pub const MAX_INLINE_RUNS: usize = 4_096;
pub const MAX_AUTHORED_ALT_BYTES: usize = 4_096;
pub const MAX_SECTIONS: usize = 128;
pub const MAX_HEADER_FOOTER_RUNS: usize = 64;
pub const MAX_HEADER_FOOTER_BYTES: usize = 16 * 1024;
pub const MAX_HEADER_FOOTER_DISTANCE_MILLIMETRES: u16 = 100;
pub const MAX_PAGE_MARGIN_MILLIMETRES: u16 = 100;

fn validate_font(font: &FontFamily, authoring: bool) -> Result<(), SchemaError> {
    if let FontFamily::LegacyUnknown { original } = font
        && (authoring || original.trim().is_empty() || original.len() > 512)
    {
        return Err(SchemaError::invalid_document());
    }
    Ok(())
}

/// Validate a newly authored document. Loading and replay deliberately retain
/// migration-only states; command constructors must use this stricter boundary
/// for newly supplied values rather than treating a valid stored value as consent.
pub fn validate_new_document(document: &FlowDocument) -> Result<(), SchemaError> {
    validate_document(document)?;
    let mut all_ids = BTreeSet::new();
    insert_id(&mut all_ids, document.document_id.as_str())?;
    validate_sections(document, &mut all_ids, true)?;
    for style in &document.styles {
        validate_font(&style.font_family, true)?;
        if !(6_000..=288_000).contains(&style.font_size_millipoints) {
            return Err(SchemaError::invalid_document());
        }
    }
    if document
        .fields
        .iter()
        .any(|field| !matches!(field.anchor, FieldAnchorState::GraphemeSafe { .. }))
    {
        return Err(SchemaError::invalid_document());
    }
    let styles = document
        .styles
        .iter()
        .map(|style| style.id.as_str())
        .collect();
    let assets = document
        .assets
        .iter()
        .map(|asset| asset.id.as_str())
        .collect();
    let nodes = validate_tree(&document.content, &styles, &assets, &mut all_ids, true)?;
    if !nodes.values().any(|node| node.runs().is_some()) {
        return Err(SchemaError::invalid_document());
    }
    Ok(())
}

pub(crate) fn validate_new_node(
    document: &FlowDocument,
    node: &ContentNode,
) -> Result<(), SchemaError> {
    let styles = document
        .styles
        .iter()
        .map(|style| style.id.as_str())
        .collect();
    let assets = document
        .assets
        .iter()
        .map(|asset| asset.id.as_str())
        .collect();
    validate_tree(
        std::slice::from_ref(node),
        &styles,
        &assets,
        &mut BTreeSet::new(),
        true,
    )
    .map(|_| ())
}

fn validate_sections(
    document: &FlowDocument,
    all_ids: &mut BTreeSet<String>,
    authoring: bool,
) -> Result<(), SchemaError> {
    if document.sections.is_empty() || document.sections.len() > MAX_SECTIONS {
        return Err(SchemaError::section_invalid());
    }

    let top_level_ids = document
        .content
        .iter()
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut boundaries = BTreeSet::new();
    let mut previous_boundary_index = None;
    for (index, section) in document.sections.iter().enumerate() {
        insert_id(all_ids, section.id.as_str())?;
        if index == 0 {
            if section.start_node_id.is_some() || section.page_settings != document.page_settings {
                return Err(SchemaError::section_invalid());
            }
        } else {
            let Some(start_node_id) = section.start_node_id.as_ref() else {
                return Err(SchemaError::section_invalid());
            };
            let Some(start_index) = document
                .content
                .iter()
                .position(|node| node.id == *start_node_id)
            else {
                return Err(SchemaError::section_invalid());
            };
            if !top_level_ids.contains(start_node_id.as_str())
                || !boundaries.insert(start_node_id.as_str())
                || previous_boundary_index.is_some_and(|previous| start_index <= previous)
            {
                return Err(SchemaError::section_invalid());
            }
            previous_boundary_index = Some(start_index);
        }
        validate_page_settings(&section.page_settings)?;
        let page_height = page_dimensions(&section.page_settings).1;
        let header_extent = u32::from(section.page_settings.margins_millimetres.top)
            .checked_add(u32::from(section.header.distance_millimetres))
            .ok_or_else(SchemaError::layout_geometry_invalid)?;
        let footer_extent = u32::from(section.page_settings.margins_millimetres.bottom)
            .checked_add(u32::from(section.footer.distance_millimetres))
            .ok_or_else(SchemaError::layout_geometry_invalid)?;
        if header_extent >= page_height || footer_extent >= page_height {
            return Err(SchemaError::layout_geometry_invalid());
        }
        validate_header_footer(&section.header, authoring)?;
        validate_header_footer(&section.footer, authoring)?;
    }
    Ok(())
}

fn validate_page_settings(settings: &PageSettings) -> Result<(), SchemaError> {
    let (width, height) = page_dimensions(settings);
    let horizontal = u32::from(settings.margins_millimetres.left)
        .checked_add(u32::from(settings.margins_millimetres.right))
        .ok_or_else(SchemaError::layout_geometry_invalid)?;
    let vertical = u32::from(settings.margins_millimetres.top)
        .checked_add(u32::from(settings.margins_millimetres.bottom))
        .ok_or_else(SchemaError::layout_geometry_invalid)?;
    if [
        settings.margins_millimetres.top,
        settings.margins_millimetres.right,
        settings.margins_millimetres.bottom,
        settings.margins_millimetres.left,
    ]
    .into_iter()
    .any(|margin| margin > MAX_PAGE_MARGIN_MILLIMETRES)
        || horizontal >= width
        || vertical >= height
    {
        return Err(SchemaError::layout_geometry_invalid());
    }
    Ok(())
}

fn page_dimensions(settings: &PageSettings) -> (u32, u32) {
    match (&settings.page_size, &settings.orientation) {
        (PageSize::A4, PageOrientation::Portrait) => (210, 297),
        (PageSize::A4, PageOrientation::Landscape) => (297, 210),
        (PageSize::Letter, PageOrientation::Portrait) => (216, 279),
        (PageSize::Letter, PageOrientation::Landscape) => (279, 216),
    }
}

fn validate_header_footer(
    settings: &HeaderFooterSettings,
    authoring: bool,
) -> Result<(), SchemaError> {
    if settings.locale.trim().is_empty()
        || settings.locale.len() > 32
        || settings.distance_millimetres > MAX_HEADER_FOOTER_DISTANCE_MILLIMETRES
        || settings.runs.len() > MAX_HEADER_FOOTER_RUNS
        || (!settings.enabled && !settings.runs.is_empty())
        || (settings.enabled && settings.runs.is_empty())
    {
        return Err(SchemaError::header_footer_limit());
    }
    let mut total_bytes = 0_usize;
    for run in &settings.runs {
        if run.text.is_empty() || run.text.contains(['\n', '\r']) {
            return Err(SchemaError::section_invalid());
        }
        total_bytes = total_bytes
            .checked_add(run.text.len())
            .ok_or_else(SchemaError::header_footer_limit)?;
        if total_bytes > MAX_HEADER_FOOTER_BYTES {
            return Err(SchemaError::header_footer_limit());
        }
        validate_font(&run.style.font_family, authoring)?;
        if !(6_000..=288_000).contains(&run.style.font_size_millipoints) {
            return Err(SchemaError::section_invalid());
        }
    }
    Ok(())
}

fn validate_tree<'a>(
    content: &'a [ContentNode],
    styles: &BTreeSet<&str>,
    assets: &BTreeSet<&str>,
    all_ids: &mut BTreeSet<String>,
    authoring: bool,
) -> Result<BTreeMap<&'a str, &'a ContentNode>, SchemaError> {
    let limits = DocumentLimits::V1;
    let mut stack = content
        .iter()
        .map(|node| (node, None::<&BlockKind>, 1_usize, 0_usize, false))
        .collect::<Vec<_>>();
    let mut nodes = BTreeMap::new();
    let mut total_text = 0_usize;
    while let Some((node, parent, depth, list_depth, in_table)) = stack.pop() {
        limits.check(LimitKind::TreeDepth, depth)?;
        limits.check(LimitKind::SemanticNodes, nodes.len() + 1)?;
        insert_id(all_ids, node.id.as_str())?;
        nodes.insert(node.id.as_str(), node);
        if node
            .style_id
            .as_ref()
            .is_some_and(|id| !styles.contains(id.as_str()))
        {
            return Err(SchemaError::dangling_reference());
        }
        let nesting_ok = match (&node.body, parent) {
            (
                BlockKind::ListItem { .. },
                Some(BlockKind::OrderedList { .. } | BlockKind::UnorderedList { .. }),
            )
            | (BlockKind::TableRow { .. }, Some(BlockKind::Table { .. }))
            | (BlockKind::TableCell { .. }, Some(BlockKind::TableRow { .. })) => true,
            (
                BlockKind::ListItem { .. }
                | BlockKind::TableRow { .. }
                | BlockKind::TableCell { .. },
                _,
            ) => false,
            (
                _,
                Some(
                    BlockKind::OrderedList { .. }
                    | BlockKind::UnorderedList { .. }
                    | BlockKind::Table { .. }
                    | BlockKind::TableRow { .. },
                ),
            ) => false,
            _ => true,
        };
        if !nesting_ok {
            return Err(SchemaError::invalid_document());
        }
        let mut child_list_depth = list_depth;
        let mut child_in_table = in_table;
        match &node.body {
            BlockKind::Paragraph { attrs, runs } | BlockKind::Heading { attrs, runs, .. } => {
                if attrs.spacing_before_millipoints > 144_000
                    || attrs.spacing_after_millipoints > 144_000
                {
                    return Err(SchemaError::invalid_document());
                }
                if matches!(node.body, BlockKind::Heading { level, .. } if !(1..=6).contains(&level))
                {
                    return Err(SchemaError::invalid_document());
                }
                if runs.len() > MAX_INLINE_RUNS {
                    return Err(SchemaError::new(
                        "FLOW_LIMIT_INLINE_RUNS",
                        "Too many inline runs",
                    ));
                }
                let mut previous = None;
                let mut text_bytes = 0_usize;
                for run in runs {
                    if run.text.is_empty() || previous == Some(&run.marks) {
                        return Err(SchemaError::invalid_document());
                    }
                    previous = Some(&run.marks);
                    if let Some(font) = &run.marks.font_family {
                        validate_font(font, authoring)?;
                    }
                    if run
                        .marks
                        .font_size_millipoints
                        .is_some_and(|size| !(6_000..=288_000).contains(&size))
                    {
                        return Err(SchemaError::invalid_document());
                    }
                    text_bytes = text_bytes
                        .checked_add(run.text.len())
                        .ok_or_else(SchemaError::invalid_document)?;
                    limits.check(LimitKind::TextNodeBytes, text_bytes)?;
                }
                total_text = total_text
                    .checked_add(text_bytes)
                    .ok_or_else(SchemaError::invalid_document)?;
                limits.check(LimitKind::TotalTextBytes, total_text)?;
            }
            BlockKind::Image {
                asset_id,
                accessibility,
            } => {
                if !assets.contains(asset_id.as_str()) {
                    return Err(SchemaError::dangling_reference());
                }
                match accessibility {
                    ImageAccessibility::MissingLegacy if authoring => {
                        return Err(SchemaError::invalid_document());
                    }
                    ImageAccessibility::Described { text }
                        if text.is_empty()
                            || text.len()
                                > if authoring {
                                    MAX_AUTHORED_ALT_BYTES
                                } else {
                                    16_384
                                }
                            || (authoring && text.trim().is_empty()) =>
                    {
                        return Err(SchemaError::invalid_document());
                    }
                    _ => {}
                }
            }
            BlockKind::OrderedList { items } | BlockKind::UnorderedList { items } => {
                child_list_depth += 1;
                if child_list_depth > MAX_LIST_DEPTH {
                    return Err(SchemaError::new(
                        "FLOW_LIMIT_LIST_DEPTH",
                        "List nesting limit exceeded",
                    ));
                }
                if items.is_empty() {
                    return Err(SchemaError::invalid_document());
                }
            }
            BlockKind::ListItem { children } | BlockKind::TableCell { children } => {
                if children.is_empty() || !children.iter().any(|child| child.runs().is_some()) {
                    return Err(SchemaError::invalid_document());
                }
            }
            BlockKind::Table { header_rows, rows } => {
                if in_table || *header_rows > 1 || rows.is_empty() {
                    return Err(SchemaError::invalid_document());
                }
                if rows.len() > MAX_TABLE_ROWS {
                    return Err(SchemaError::new(
                        "FLOW_LIMIT_TABLE",
                        "Table dimensions exceeded",
                    ));
                }
                let columns = rows[0].children().len();
                if columns == 0 || columns > MAX_TABLE_COLUMNS {
                    return Err(SchemaError::new(
                        "FLOW_LIMIT_TABLE",
                        "Table dimensions exceeded",
                    ));
                }
                if rows.iter().any(|row| row.children().len() != columns) {
                    return Err(SchemaError::invalid_document());
                }
                if rows
                    .len()
                    .checked_mul(columns)
                    .is_none_or(|cells| cells > MAX_TABLE_CELLS)
                {
                    return Err(SchemaError::new(
                        "FLOW_LIMIT_TABLE",
                        "Table dimensions exceeded",
                    ));
                }
                child_in_table = true;
            }
            BlockKind::TableRow { .. } | BlockKind::PageBreak => {}
        }
        limits.check(
            LimitKind::SemanticNodes,
            nodes
                .len()
                .saturating_add(stack.len())
                .saturating_add(node.children().len()),
        )?;
        stack.extend(node.children().iter().map(|child| {
            (
                child,
                Some(&node.body),
                depth + 1,
                child_list_depth,
                child_in_table,
            )
        }));
    }
    Ok(nodes)
}

// Scan each referenced text block only through its last requested field offset
// and retain only requested boundaries, never a full-document grapheme cache.
fn field_boundaries<'a>(
    nodes: &BTreeMap<&str, &ContentNode>,
    positions: impl Iterator<Item = &'a LogicalPosition>,
) -> BTreeMap<String, BTreeSet<u32>> {
    let mut requested = BTreeMap::<String, BTreeSet<u32>>::new();
    for position in positions {
        requested
            .entry(position.node_id.to_string())
            .or_default()
            .insert(position.utf16_offset.get());
    }
    for (id, offsets) in &mut requested {
        let wanted = std::mem::take(offsets);
        let Some(node) = nodes.get(id.as_str()).filter(|node| node.runs().is_some()) else {
            continue;
        };
        let text = match node.runs().unwrap_or(&[]) {
            [] => std::borrow::Cow::Borrowed(""),
            [run] => std::borrow::Cow::Borrowed(run.text.as_str()),
            _ => std::borrow::Cow::Owned(node.text()),
        };
        let last_requested = wanted.last().copied().unwrap_or(0);
        let mut previous = 0;
        let mut utf16 = 0;
        for boundary in GraphemeClusterSegmenter::new().segment_str(&text) {
            utf16 += text[previous..boundary].encode_utf16().count() as u32;
            previous = boundary;
            if wanted.contains(&utf16) {
                offsets.insert(utf16);
            }
            if utf16 >= last_requested {
                break;
            }
        }
    }
    requested
}

fn boundary_contains(
    boundaries: &BTreeMap<String, BTreeSet<u32>>,
    position: &LogicalPosition,
) -> bool {
    boundaries
        .get(position.node_id.as_str())
        .is_some_and(|offsets| offsets.contains(&position.utf16_offset.get()))
}

fn validate_provenance(document: &FlowDocument) -> Result<(), SchemaError> {
    validate_provenance_version(&document.provenance, document.schema_version)
}

fn validate_provenance_version(provenance: &Provenance, version: u32) -> Result<(), SchemaError> {
    match provenance {
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
                || *current_schema_version != version
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
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[must_use]
pub fn utf16_to_byte_offset(value: &str, utf16_offset: u32) -> Option<usize> {
    resolve_utf16_offset(value, Utf16Offset::new(utf16_offset))
        .ok()
        .map(|offset| offset.get())
}

pub type MigrationFunction = fn(&[u8]) -> Result<Vec<u8>, SchemaError>;
pub type MigrationValidator = fn(&[u8]) -> Result<(), SchemaError>;

#[derive(Clone, Copy)]
pub struct MigrationStep {
    pub from_version: u32,
    pub to_version: u32,
    migrate: MigrationFunction,
    validate_output: MigrationValidator,
}

impl MigrationStep {
    #[must_use]
    pub const fn new(
        from_version: u32,
        to_version: u32,
        migrate: MigrationFunction,
        validate_output: MigrationValidator,
    ) -> Self {
        Self {
            from_version,
            to_version,
            migrate,
            validate_output,
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
        Self::new(vec![
            MigrationStep::new(0, 1, migrate_v0_to_v1, validate_v1_migration_output),
            MigrationStep::new(1, 2, migrate_v1_to_v2, validate_v2_migration_output),
            MigrationStep::new(2, 3, migrate_v2_to_v3, validate_v3_migration_output),
        ])
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
            crate::canonical::preflight_canonical_bytes(&candidate)?;
            let candidate_version = probe_schema_version(&candidate)
                .map_err(|_| SchemaError::migration_intermediate_invalid())?;
            if candidate_version != expected_to {
                return Err(SchemaError::migration_intermediate_invalid());
            }
            (step.validate_output)(&candidate)
                .map_err(|_| SchemaError::migration_intermediate_invalid())?;
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

fn validate_v1_migration_output(input: &[u8]) -> Result<(), SchemaError> {
    decode_legacy_validated(input, 1).map(|_| ())
}

fn validate_v2_migration_output(input: &[u8]) -> Result<(), SchemaError> {
    let document = legacy_v2::decode(input)?;
    validate_v2_document(&document)
}

fn validate_v3_migration_output(input: &[u8]) -> Result<(), SchemaError> {
    crate::canonical::decode_canonical(input).map(|_| ())
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

fn migrate_v0_to_v1(input: &[u8]) -> Result<Vec<u8>, SchemaError> {
    decode_legacy_validated(input, 0)?
        .canonical_bytes()
        .map_err(|_| SchemaError::serialization())
}

fn migrate_v1_to_v2(input: &[u8]) -> Result<Vec<u8>, SchemaError> {
    legacy_v2::encode(&convert_v1(&decode_legacy_validated(input, 1)?)?)
}

fn migrate_v2_to_v3(input: &[u8]) -> Result<Vec<u8>, SchemaError> {
    let source = legacy_v2::decode(input)?;
    let section = SectionSettings::default_for(&source.locale, source.page_settings.clone())?;
    let provenance = migrate_provenance_v2_to_v3(source.provenance)?;
    let document = FlowDocument {
        schema_version: SCHEMA_VERSION,
        document_id: source.document_id,
        revision: source.revision,
        locale: source.locale,
        page_settings: source.page_settings,
        sections: vec![section],
        styles: source.styles,
        content: source.content,
        assets: source.assets,
        fields: source.fields,
        provenance,
    };
    crate::canonical::canonical_bytes(&document)
}

fn decode_legacy_validated(
    input: &[u8],
    version: u32,
) -> Result<legacy::LegacyFlowDocumentV1, SchemaError> {
    crate::canonical::preflight_canonical_bytes(input)?;
    let source = legacy::decode(input, version)
        .map_err(|_| SchemaError::migration_input_invalid())?
        .into_v1();
    // convert_v1 validates every retained semantic/noncontent invariant before
    // even an intermediate v0->v1 result can leave this module.
    convert_v1(&source)?;
    Ok(source)
}

/// Unchanged closed wire records cross from frozen types into current types.
/// This helper cannot invent members or accept unknown current-schema values.
fn retained<T: Serialize, U: serde::de::DeserializeOwned>(value: &T) -> Result<U, SchemaError> {
    let bytes = serde_json::to_vec(value).map_err(|_| SchemaError::serialization())?;
    serde_json::from_slice(&bytes).map_err(|_| SchemaError::migration_input_invalid())
}

fn convert_v1(source: &legacy::LegacyFlowDocumentV1) -> Result<FlowDocument, SchemaError> {
    let source_provenance: Provenance = retained(&source.provenance)?;
    validate_provenance_version(&source_provenance, 1)?;
    DocumentLimits::V1.check(LimitKind::SemanticNodes, source.content.len())?;
    DocumentLimits::V1.check(LimitKind::Fields, source.fields.len())?;
    DocumentLimits::V1.check(LimitKind::Assets, source.assets.len())?;
    DocumentLimits::V1.check(LimitKind::Styles, source.styles.len())?;
    let mut total_text_bytes = 0_usize;
    for node in &source.content {
        DocumentLimits::V1.check(LimitKind::TextNodeBytes, node.text.len())?;
        total_text_bytes = total_text_bytes
            .checked_add(node.text.len())
            .ok_or_else(SchemaError::invalid_document)?;
        DocumentLimits::V1.check(LimitKind::TotalTextBytes, total_text_bytes)?;
    }
    let mut content = Vec::with_capacity(source.content.len());
    let assets = source
        .assets
        .iter()
        .map(|asset| (asset.id.as_str(), asset))
        .collect::<BTreeMap<_, _>>();
    for node in &source.content {
        let id = NodeId::new(node.id.as_str())?;
        let style_id = node
            .style_id
            .as_ref()
            .map(|id| StyleId::new(id.as_str()))
            .transpose()?;
        let migrated = match node.kind {
            legacy::ContentNodeKind::Paragraph => {
                if style_id.is_none() || node.asset_id.is_some() {
                    return Err(SchemaError::dangling_reference());
                }
                ContentNode::paragraph(id, style_id, node.text.clone())
            }
            legacy::ContentNodeKind::Image => {
                if style_id.is_some() || !node.text.is_empty() {
                    return Err(SchemaError::dangling_reference());
                }
                let asset_id = node
                    .asset_id
                    .as_ref()
                    .ok_or_else(SchemaError::dangling_reference)?;
                let asset = assets
                    .get(asset_id.as_str())
                    .ok_or_else(SchemaError::dangling_reference)?;
                let accessibility = if asset.alt_text.is_empty() {
                    ImageAccessibility::MissingLegacy
                } else {
                    ImageAccessibility::Described {
                        text: asset.alt_text.clone(),
                    }
                };
                ContentNode {
                    id,
                    style_id,
                    body: BlockKind::Image {
                        asset_id: AssetId::new(asset_id.as_str())?,
                        accessibility,
                    },
                }
            }
        };
        content.push(migrated);
    }
    let nodes = content
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let positions = source
        .fields
        .iter()
        .map(|field| retained(&field.anchor))
        .collect::<Result<Vec<LogicalPosition>, SchemaError>>()?;
    let boundaries = field_boundaries(&nodes, positions.iter());
    let mut fields = Vec::with_capacity(source.fields.len());
    for (field, original) in source.fields.iter().zip(positions) {
        let target = nodes.get(original.node_id.as_str());
        let anchor = match target {
            Some(_) if boundary_contains(&boundaries, &original) => {
                FieldAnchorState::GraphemeSafe { original }
            }
            Some(_) => FieldAnchorState::LegacyInvalid {
                original,
                reason: LegacyAnchorReason::NonGraphemeBoundary,
            },
            None => FieldAnchorState::LegacyInvalid {
                original,
                reason: LegacyAnchorReason::MissingNode,
            },
        };
        fields.push(FieldDescriptor {
            id: FieldId::new(field.id.as_str())?,
            name: field.name.clone(),
            label: field.label.clone(),
            anchor,
            kind: retained(&field.kind)?,
            required: field.required,
            read_only: field.read_only,
            default_value: retained(&field.default_value)?,
            options: retained(&field.options)?,
        });
    }
    let hop = MigrationHop {
        from_version: 1,
        to_version: 2,
    };
    let provenance = match source_provenance {
        Provenance::LocalSample { created_at } => Provenance::Migrated {
            source_schema_version: 1,
            current_schema_version: 2,
            source_created_at: created_at,
            hops: vec![hop],
        },
        Provenance::Migrated {
            source_schema_version,
            source_created_at,
            mut hops,
            ..
        } => {
            hops.push(hop);
            Provenance::Migrated {
                source_schema_version,
                current_schema_version: 2,
                source_created_at,
                hops,
            }
        }
    };
    let document = FlowDocument {
        schema_version: 2,
        document_id: DocumentId::new(source.document_id.as_str())?,
        revision: source.revision,
        locale: source.locale.clone(),
        page_settings: retained(&source.page_settings)?,
        sections: Vec::new(),
        styles: source
            .styles
            .iter()
            .map(|style| {
                Ok(StyleDefinition {
                    id: StyleId::new(style.id.as_str())?,
                    name: style.name.clone(),
                    font_family: if style.font_family == "Noto Sans" {
                        FontFamily::noto_sans()
                    } else {
                        FontFamily::LegacyUnknown {
                            original: style.font_family.clone(),
                        }
                    },
                    font_size_millipoints: style.font_size_millipoints,
                })
            })
            .collect::<Result<_, SchemaError>>()?,
        content,
        assets: retained(&source.assets)?,
        fields,
        provenance,
    };
    validate_v2_document(&document)?;
    Ok(document)
}

fn validate_v2_document(document: &FlowDocument) -> Result<(), SchemaError> {
    if document.schema_version != 2 || !document.sections.is_empty() {
        return Err(SchemaError::unsupported_schema());
    }
    validate_provenance_version(&document.provenance, 2)?;
    let section = SectionSettings::default_for(&document.locale, document.page_settings.clone())?;
    let promoted = FlowDocument {
        schema_version: SCHEMA_VERSION,
        document_id: document.document_id.clone(),
        revision: document.revision,
        locale: document.locale.clone(),
        page_settings: document.page_settings.clone(),
        sections: vec![section],
        styles: document.styles.clone(),
        content: document.content.clone(),
        assets: document.assets.clone(),
        fields: document.fields.clone(),
        provenance: migrate_provenance_v2_to_v3(document.provenance.clone())?,
    };
    validate_document(&promoted)
}

fn migrate_provenance_v2_to_v3(provenance: Provenance) -> Result<Provenance, SchemaError> {
    validate_provenance_version(&provenance, 2)?;
    let hop = MigrationHop {
        from_version: 2,
        to_version: 3,
    };
    Ok(match provenance {
        Provenance::LocalSample { created_at } => Provenance::Migrated {
            source_schema_version: 2,
            current_schema_version: 3,
            source_created_at: created_at,
            hops: vec![hop],
        },
        Provenance::Migrated {
            source_schema_version,
            source_created_at,
            mut hops,
            ..
        } => {
            hops.push(hop);
            Provenance::Migrated {
                source_schema_version,
                current_schema_version: 3,
                source_created_at,
                hops,
            }
        }
    })
}

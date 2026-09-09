//! Typed semantic FlowDocument schema.
//!
//! The types here deliberately exclude DOM paths, page/PDF coordinates,
//! floating-point values, widget appearances, scripts, and arbitrary property
//! bags. Every durable field is explicit and ordered.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::anchor::Utf16Offset;
use crate::schema::{SchemaError, validate_document};

pub const SCHEMA_VERSION: u32 = 2;

macro_rules! stable_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, SchemaError> {
                let value = value.into();
                let parsed = Uuid::parse_str(&value).map_err(|_| SchemaError::invalid_id())?;
                Ok(Self(parsed.hyphenated().to_string()))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(|_| serde::de::Error::custom("FLOW_INVALID_ID"))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

stable_id!(DocumentId);
stable_id!(StyleId);
stable_id!(NodeId);
stable_id!(AssetId);
stable_id!(FieldId);
stable_id!(FieldOptionId);
stable_id!(CommandId);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlowDocument {
    pub schema_version: u32,
    pub document_id: DocumentId,
    pub revision: u32,
    pub locale: String,
    pub page_settings: PageSettings,
    pub styles: Vec<StyleDefinition>,
    pub content: Vec<ContentNode>,
    pub assets: Vec<AssetDescriptor>,
    pub fields: Vec<FieldDescriptor>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageSettings {
    pub page_size: PageSize,
    pub orientation: PageOrientation,
    pub margins_millimetres: PageMargins,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PageSize {
    A4,
    Letter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PageOrientation {
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageMargins {
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
    pub left: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StyleDefinition {
    pub id: StyleId,
    pub name: String,
    pub font_family: FontFamily,
    pub font_size_millipoints: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentNode {
    pub id: NodeId,
    pub style_id: Option<StyleId>,
    pub body: BlockKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BlockKind {
    Paragraph {
        attrs: ParagraphAttrs,
        runs: Vec<InlineRun>,
    },
    Heading {
        level: u8,
        attrs: ParagraphAttrs,
        runs: Vec<InlineRun>,
    },
    OrderedList {
        items: Vec<ContentNode>,
    },
    UnorderedList {
        items: Vec<ContentNode>,
    },
    ListItem {
        children: Vec<ContentNode>,
    },
    Image {
        asset_id: AssetId,
        accessibility: ImageAccessibility,
    },
    Table {
        header_rows: u8,
        rows: Vec<ContentNode>,
    },
    TableRow {
        cells: Vec<ContentNode>,
    },
    TableCell {
        children: Vec<ContentNode>,
    },
    PageBreak,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParagraphAttrs {
    pub alignment: Alignment,
    pub spacing_before_millipoints: u32,
    pub spacing_after_millipoints: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Alignment {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InlineRun {
    pub text: String,
    pub marks: MarkSet,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkSet {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub font_family: Option<FontFamily>,
    pub font_size_millipoints: Option<u32>,
    pub color: Option<[u8; 3]>,
    pub language: Option<RunLanguage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunLanguage {
    #[serde(rename = "uk-UA")]
    Ukrainian,
    #[serde(rename = "en-US")]
    English,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FontFamily {
    Known { id: FontFamilyId },
    LegacyUnknown { original: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FontFamilyId {
    NotoSans,
    NotoSerif,
    NotoSansMono,
}

impl FontFamily {
    #[must_use]
    pub fn noto_sans() -> Self {
        Self::Known {
            id: FontFamilyId::NotoSans,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ImageAccessibility {
    Described { text: String },
    Decorative,
    MissingLegacy,
}

impl ContentNode {
    #[must_use]
    pub fn paragraph(id: NodeId, style_id: Option<StyleId>, text: String) -> Self {
        Self {
            id,
            style_id,
            body: BlockKind::Paragraph {
                attrs: ParagraphAttrs::default(),
                runs: plain_runs(text),
            },
        }
    }

    #[must_use]
    pub fn runs(&self) -> Option<&[InlineRun]> {
        match &self.body {
            BlockKind::Paragraph { runs, .. } | BlockKind::Heading { runs, .. } => Some(runs),
            _ => None,
        }
    }

    #[must_use]
    pub fn text(&self) -> String {
        self.runs()
            .unwrap_or(&[])
            .iter()
            .map(|run| run.text.as_str())
            .collect()
    }

    #[must_use]
    pub fn children(&self) -> &[Self] {
        match &self.body {
            BlockKind::OrderedList { items } | BlockKind::UnorderedList { items } => items,
            BlockKind::ListItem { children } | BlockKind::TableCell { children } => children,
            BlockKind::Table { rows, .. } => rows,
            BlockKind::TableRow { cells } => cells,
            _ => &[],
        }
    }

    /// Compatibility for the Phase 1 text command. Rich run algebra belongs to
    /// the subsequent editor-command plans; rejecting it preserves all marks.
    pub(crate) fn legacy_text(&self) -> Option<&str> {
        match &self.body {
            BlockKind::Paragraph { runs, .. } if runs.is_empty() => Some(""),
            BlockKind::Paragraph { runs, .. }
                if runs.len() == 1 && runs[0].marks == MarkSet::default() =>
            {
                Some(&runs[0].text)
            }
            _ => None,
        }
    }

    pub fn set_plain_text(&mut self, text: String) -> Result<(), SchemaError> {
        if self.legacy_text().is_none() {
            return Err(SchemaError::invalid_document());
        }
        if let BlockKind::Paragraph { runs, .. } = &mut self.body {
            *runs = plain_runs(text);
        }
        Ok(())
    }

    #[must_use]
    pub fn asset_id(&self) -> Option<&AssetId> {
        if let BlockKind::Image { asset_id, .. } = &self.body {
            Some(asset_id)
        } else {
            None
        }
    }
}

fn plain_runs(text: String) -> Vec<InlineRun> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![InlineRun {
            text,
            marks: MarkSet::default(),
        }]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "camelCase", deny_unknown_fields)]
pub enum FieldAnchorState {
    GraphemeSafe {
        original: LogicalPosition,
    },
    LegacyInvalid {
        original: LogicalPosition,
        reason: LegacyAnchorReason,
    },
    TargetDeleted {
        original: LogicalPosition,
        tombstone: TombstoneToken,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LegacyAnchorReason {
    NonGraphemeBoundary,
    MissingNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TombstoneToken {
    pub command_id: CommandId,
    pub slot: u32,
}

impl FieldAnchorState {
    #[must_use]
    pub fn original(&self) -> &LogicalPosition {
        match self {
            Self::GraphemeSafe { original }
            | Self::LegacyInvalid { original, .. }
            | Self::TargetDeleted { original, .. } => original,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetDescriptor {
    pub id: AssetId,
    pub content_hash: String,
    pub media_type: String,
    pub byte_length: u32,
    pub alt_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldDescriptor {
    pub id: FieldId,
    pub name: String,
    pub label: Option<String>,
    pub anchor: FieldAnchorState,
    pub kind: FieldKind,
    pub required: bool,
    pub read_only: bool,
    pub default_value: FieldValue,
    pub options: Vec<FieldOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FieldKind {
    Text {
        multiline: bool,
        input_hint: TextInputHint,
    },
    Checkbox,
    RadioGroup,
    Select {
        multiple: bool,
    },
    Signature,
    Button,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TextInputHint {
    Plain,
    Date,
    Number,
    Email,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FieldValue {
    Empty,
    Text { value: String },
    Checked { value: bool },
    Selected { option_ids: Vec<FieldOptionId> },
}

impl FieldValue {
    #[must_use]
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text {
            value: value.into(),
        }
    }

    #[must_use]
    pub fn checked(value: bool) -> Self {
        Self::Checked { value }
    }

    #[must_use]
    pub fn selected(option_ids: Vec<FieldOptionId>) -> Self {
        Self::Selected { option_ids }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldOption {
    pub id: FieldOptionId,
    pub label: String,
    pub export_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogicalPosition {
    pub node_id: NodeId,
    pub utf16_offset: Utf16Offset,
    pub affinity: Affinity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Affinity {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Provenance {
    LocalSample {
        created_at: String,
    },
    Migrated {
        source_schema_version: u32,
        current_schema_version: u32,
        source_created_at: String,
        hops: Vec<MigrationHop>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationHop {
    pub from_version: u32,
    pub to_version: u32,
}

impl FlowDocument {
    pub fn deterministic_sample(locale: &str) -> Result<Self, SchemaError> {
        Self::deterministic_sample_at(locale, "2026-08-14T00:00:00Z")
    }

    pub fn deterministic_sample_at(locale: &str, created_at: &str) -> Result<Self, SchemaError> {
        if locale != "uk-UA" && locale != "en-US" {
            return Err(SchemaError::invalid_document());
        }

        let body_style_id = StyleId::new("00000000-0000-4000-8000-000000000301")?;
        let heading_style_id = StyleId::new("00000000-0000-4000-8000-000000000302")?;
        let first_node_id = NodeId::new("00000000-0000-4000-8000-000000000101")?;
        let asset_id = AssetId::new("00000000-0000-4000-8000-000000000401")?;

        let radio_first = FieldOptionId::new("00000000-0000-4000-8000-000000000611")?;
        let radio_second = FieldOptionId::new("00000000-0000-4000-8000-000000000612")?;
        let select_first = FieldOptionId::new("00000000-0000-4000-8000-000000000621")?;
        let select_second = FieldOptionId::new("00000000-0000-4000-8000-000000000622")?;

        let document = Self {
            schema_version: SCHEMA_VERSION,
            document_id: DocumentId::new("00000000-0000-4000-8000-000000000001")?,
            revision: 1,
            locale: locale.to_owned(),
            page_settings: PageSettings {
                page_size: PageSize::A4,
                orientation: PageOrientation::Portrait,
                margins_millimetres: PageMargins {
                    top: 20,
                    right: 20,
                    bottom: 20,
                    left: 20,
                },
            },
            styles: vec![
                StyleDefinition {
                    id: body_style_id.clone(),
                    name: "Body".to_owned(),
                    font_family: FontFamily::noto_sans(),
                    font_size_millipoints: 12_000,
                },
                StyleDefinition {
                    id: heading_style_id.clone(),
                    name: "Heading".to_owned(),
                    font_family: FontFamily::noto_sans(),
                    font_size_millipoints: 18_000,
                },
            ],
            content: vec![
                ContentNode::paragraph(
                    first_node_id.clone(),
                    Some(body_style_id),
                    "Український текст: и\u{0306}, апостроф ’, emoji 😀, non-BMP 𝄞.".to_owned(),
                ),
                ContentNode::paragraph(
                    NodeId::new("00000000-0000-4000-8000-000000000102")?,
                    Some(heading_style_id),
                    "English sample document.".to_owned(),
                ),
                ContentNode {
                    id: NodeId::new("00000000-0000-4000-8000-000000000103")?,
                    style_id: None,
                    body: BlockKind::Image {
                        asset_id: asset_id.clone(),
                        accessibility: ImageAccessibility::Described {
                            text: "Порожній тестовий ресурс".to_owned(),
                        },
                    },
                },
            ],
            assets: vec![AssetDescriptor {
                id: asset_id,
                content_hash: format!("blake3:{}", blake3::hash(&[]).to_hex()),
                media_type: "image/png".to_owned(),
                byte_length: 0,
                alt_text: "Порожній тестовий ресурс".to_owned(),
            }],
            fields: vec![
                FieldDescriptor {
                    id: FieldId::new("00000000-0000-4000-8000-000000000501")?,
                    name: "sample-name".to_owned(),
                    label: Some("Ім’я / Name".to_owned()),
                    anchor: FieldAnchorState::GraphemeSafe {
                        original: LogicalPosition {
                            node_id: first_node_id.clone(),
                            utf16_offset: Utf16Offset::new(0),
                            affinity: Affinity::Forward,
                        },
                    },
                    kind: FieldKind::Text {
                        multiline: false,
                        input_hint: TextInputHint::Plain,
                    },
                    required: true,
                    read_only: false,
                    default_value: FieldValue::text("Тест"),
                    options: Vec::new(),
                },
                FieldDescriptor {
                    id: FieldId::new("00000000-0000-4000-8000-000000000502")?,
                    name: "sample-confirmed".to_owned(),
                    label: Some("Підтверджено".to_owned()),
                    anchor: FieldAnchorState::GraphemeSafe {
                        original: LogicalPosition {
                            node_id: first_node_id.clone(),
                            utf16_offset: Utf16Offset::new(0),
                            affinity: Affinity::Forward,
                        },
                    },
                    kind: FieldKind::Checkbox,
                    required: false,
                    read_only: false,
                    default_value: FieldValue::checked(false),
                    options: Vec::new(),
                },
                FieldDescriptor {
                    id: FieldId::new("00000000-0000-4000-8000-000000000503")?,
                    name: "sample-choice".to_owned(),
                    label: Some("Один варіант".to_owned()),
                    anchor: FieldAnchorState::GraphemeSafe {
                        original: LogicalPosition {
                            node_id: first_node_id.clone(),
                            utf16_offset: Utf16Offset::new(0),
                            affinity: Affinity::Forward,
                        },
                    },
                    kind: FieldKind::RadioGroup,
                    required: false,
                    read_only: false,
                    default_value: FieldValue::selected(vec![radio_first.clone()]),
                    options: vec![
                        FieldOption {
                            id: radio_first,
                            label: "Так".to_owned(),
                            export_value: "yes".to_owned(),
                        },
                        FieldOption {
                            id: radio_second,
                            label: "Ні".to_owned(),
                            export_value: "no".to_owned(),
                        },
                    ],
                },
                FieldDescriptor {
                    id: FieldId::new("00000000-0000-4000-8000-000000000504")?,
                    name: "sample-multiple".to_owned(),
                    label: Some("Кілька варіантів".to_owned()),
                    anchor: FieldAnchorState::GraphemeSafe {
                        original: LogicalPosition {
                            node_id: first_node_id.clone(),
                            utf16_offset: Utf16Offset::new(0),
                            affinity: Affinity::Forward,
                        },
                    },
                    kind: FieldKind::Select { multiple: true },
                    required: false,
                    read_only: false,
                    default_value: FieldValue::selected(vec![
                        select_first.clone(),
                        select_second.clone(),
                    ]),
                    options: vec![
                        FieldOption {
                            id: select_first,
                            label: "Перший".to_owned(),
                            export_value: "first".to_owned(),
                        },
                        FieldOption {
                            id: select_second,
                            label: "Другий".to_owned(),
                            export_value: "second".to_owned(),
                        },
                    ],
                },
                FieldDescriptor {
                    id: FieldId::new("00000000-0000-4000-8000-000000000505")?,
                    name: "sample-signature".to_owned(),
                    label: Some("Підпис".to_owned()),
                    anchor: FieldAnchorState::GraphemeSafe {
                        original: LogicalPosition {
                            node_id: first_node_id.clone(),
                            utf16_offset: Utf16Offset::new(0),
                            affinity: Affinity::Forward,
                        },
                    },
                    kind: FieldKind::Signature,
                    required: false,
                    read_only: false,
                    default_value: FieldValue::Empty,
                    options: Vec::new(),
                },
                FieldDescriptor {
                    id: FieldId::new("00000000-0000-4000-8000-000000000506")?,
                    name: "sample-button".to_owned(),
                    label: Some("Дія".to_owned()),
                    anchor: FieldAnchorState::GraphemeSafe {
                        original: LogicalPosition {
                            node_id: first_node_id,
                            utf16_offset: Utf16Offset::new(0),
                            affinity: Affinity::Forward,
                        },
                    },
                    kind: FieldKind::Button,
                    required: false,
                    read_only: true,
                    default_value: FieldValue::Empty,
                    options: Vec::new(),
                },
            ],
            provenance: Provenance::LocalSample {
                created_at: created_at.to_owned(),
            },
        };
        validate_document(&document)?;
        Ok(document)
    }
}

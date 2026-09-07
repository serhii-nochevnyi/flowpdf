//! Frozen schema 0/1 wire types. Never depend on the current document model.
//! This module is fingerprinted by the executable legacy compatibility gate.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! stable_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, String> {
                let value = value.into();
                let parsed = Uuid::parse_str(&value).map_err(|_| "FLOW_INVALID_ID".to_owned())?;
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
pub struct LegacyFlowDocumentV1 {
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
    pub font_family: String,
    pub font_size_millipoints: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentNode {
    pub id: NodeId,
    pub kind: ContentNodeKind,
    pub style_id: Option<StyleId>,
    pub text: String,
    pub asset_id: Option<AssetId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ContentNodeKind {
    Paragraph,
    Image,
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
    pub anchor: LogicalPosition,
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
    pub utf16_offset: u32,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyFlowDocumentV0 {
    pub schema_version: u32,
    pub document_id: DocumentId,
    pub revision: u32,
    pub locale: String,
    pub page_settings: PageSettings,
    pub styles: Vec<StyleDefinition>,
    pub content: Vec<ContentNode>,
    pub assets: Vec<AssetDescriptor>,
    pub fields: Vec<FieldDescriptor>,
    pub provenance: LegacyProvenanceV0,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyProvenanceV0 {
    pub created_at: String,
}

pub enum Decoded {
    V0(Box<LegacyFlowDocumentV0>),
    V1(Box<LegacyFlowDocumentV1>),
}

impl Decoded {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        match self {
            Self::V0(doc) => serde_json::to_vec(doc),
            Self::V1(doc) => serde_json::to_vec(doc),
        }
    }
    pub fn into_v1(self) -> LegacyFlowDocumentV1 {
        match self {
            Self::V1(doc) => *doc,
            Self::V0(doc) => LegacyFlowDocumentV1 {
                schema_version: 1,
                document_id: doc.document_id,
                revision: doc.revision,
                locale: doc.locale,
                page_settings: doc.page_settings,
                styles: doc.styles,
                content: doc.content,
                assets: doc.assets,
                fields: doc.fields,
                provenance: Provenance::Migrated {
                    source_schema_version: 0,
                    current_schema_version: 1,
                    source_created_at: doc.provenance.created_at,
                    hops: vec![MigrationHop {
                        from_version: 0,
                        to_version: 1,
                    }],
                },
            },
        }
    }
}
impl LegacyFlowDocumentV1 {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

pub fn decode(bytes: &[u8], version: u32) -> Result<Decoded, String> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("FLOW_LIMIT_CANONICAL_BYTES".to_owned());
    }
    let decoded = match version {
        0 => {
            let doc: LegacyFlowDocumentV0 =
                serde_json::from_slice(bytes).map_err(|_| "FLOW_MIGRATION_INPUT_INVALID")?;
            if doc.schema_version != 0 {
                return Err("FLOW_MIGRATION_INPUT_INVALID".to_owned());
            }
            Decoded::V0(Box::new(doc))
        }
        1 => {
            let doc: LegacyFlowDocumentV1 =
                serde_json::from_slice(bytes).map_err(|_| "FLOW_MIGRATION_INPUT_INVALID")?;
            if doc.schema_version != 1 {
                return Err("FLOW_MIGRATION_INPUT_INVALID".to_owned());
            }
            Decoded::V1(Box::new(doc))
        }
        _ => return Err("FLOW_UNSUPPORTED_SCHEMA".to_owned()),
    };
    if decoded
        .canonical_bytes()
        .map_err(|_| "FLOW_SCHEMA_SERIALIZE")?
        != bytes
    {
        return Err("FLOW_NON_CANONICAL_PAYLOAD".to_owned());
    }
    Ok(decoded)
}

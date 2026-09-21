//! Private schema-v2 envelope used only at the v2->v3 migration boundary.
//!
//! The v0/v1 decoder in `legacy.rs` remains independently frozen. This file
//! freezes the v2 envelope by omitting v3-only section settings and refusing
//! any unknown field before the current model is promoted.

use serde::{Deserialize, Serialize};

use crate::model::{
    AssetDescriptor, ContentNode, DocumentId, FieldDescriptor, FlowDocument, PageSettings,
    Provenance, StyleDefinition,
};

use super::SchemaError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyFlowDocumentV2 {
    schema_version: u32,
    document_id: DocumentId,
    revision: u32,
    locale: String,
    page_settings: PageSettings,
    styles: Vec<StyleDefinition>,
    content: Vec<ContentNode>,
    assets: Vec<AssetDescriptor>,
    fields: Vec<FieldDescriptor>,
    provenance: Provenance,
}

pub(crate) fn decode(input: &[u8]) -> Result<FlowDocument, SchemaError> {
    let wire: LegacyFlowDocumentV2 =
        serde_json::from_slice(input).map_err(|_| SchemaError::migration_input_invalid())?;
    if wire.schema_version != 2 {
        return Err(SchemaError::migration_input_invalid());
    }
    let encoded = serde_json::to_vec(&wire).map_err(|_| SchemaError::serialization())?;
    if encoded != input {
        return Err(SchemaError::non_canonical());
    }
    Ok(FlowDocument {
        schema_version: wire.schema_version,
        document_id: wire.document_id,
        revision: wire.revision,
        locale: wire.locale,
        page_settings: wire.page_settings,
        sections: Vec::new(),
        styles: wire.styles,
        content: wire.content,
        assets: wire.assets,
        fields: wire.fields,
        provenance: wire.provenance,
    })
}

pub(crate) fn encode(document: &FlowDocument) -> Result<Vec<u8>, SchemaError> {
    if document.schema_version != 2 || !document.sections.is_empty() {
        return Err(SchemaError::unsupported_schema());
    }
    let wire = LegacyFlowDocumentV2 {
        schema_version: document.schema_version,
        document_id: document.document_id.clone(),
        revision: document.revision,
        locale: document.locale.clone(),
        page_settings: document.page_settings.clone(),
        styles: document.styles.clone(),
        content: document.content.clone(),
        assets: document.assets.clone(),
        fields: document.fields.clone(),
        provenance: document.provenance.clone(),
    };
    serde_json::to_vec(&wire).map_err(|_| SchemaError::serialization())
}

//! Typed revision provenance without invented PDF preview/export lineage.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{DocumentId, FlowDocument, MigrationHop, Provenance};

const MAX_SOURCE_HASHES: usize = 16;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProvenanceError {
    #[error("Revision hash is not a versioned BLAKE3 envelope")]
    InvalidHash,
    #[error("Provenance timestamp is not compact UTC RFC 3339 seconds")]
    InvalidTimestamp,
    #[error("Source hashes are duplicated or exceed the bounded allowlist")]
    InvalidSourceHashes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "String", into = "String")]
pub struct RevisionHash(String);

impl RevisionHash {
    pub fn parse(value: impl Into<String>) -> Result<Self, ProvenanceError> {
        let value = value.into();
        let Some(hex) = value.strip_prefix("flowpdf:blake3:v1:") else {
            return Err(ProvenanceError::InvalidHash);
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ProvenanceError::InvalidHash);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RevisionHash {
    type Error = ProvenanceError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<RevisionHash> for String {
    fn from(value: RevisionHash) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "String", into = "String")]
pub struct ProvenanceTimestamp(String);

impl ProvenanceTimestamp {
    fn parse(value: impl Into<String>) -> Result<Self, ProvenanceError> {
        let value = value.into();
        let bytes = value.as_bytes();
        let punctuation = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':'), (19, b'Z')];
        if bytes.len() != 20
            || punctuation.iter().any(|(index, expected)| bytes[*index] != *expected)
            || bytes.iter().enumerate().any(|(index, byte)| {
                ![4, 7, 10, 13, 16, 19].contains(&index) && !byte.is_ascii_digit()
            })
        {
            return Err(ProvenanceError::InvalidTimestamp);
        }
        Ok(Self(value))
    }
}

impl TryFrom<String> for ProvenanceTimestamp {
    type Error = ProvenanceError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<ProvenanceTimestamp> for String {
    fn from(value: ProvenanceTimestamp) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineIdentity {
    engine: String,
    version: String,
}

impl EngineIdentity {
    #[must_use]
    pub fn current() -> Self {
        Self {
            engine: "flow-core".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum RevisionLineage {
    Created { created_at: ProvenanceTimestamp },
    Migrated {
        source_schema_version: u32,
        current_schema_version: u32,
        source_created_at: ProvenanceTimestamp,
        hops: Vec<MigrationHop>,
    },
}

/// Phase 1 owns no preview or export provenance. Serializing this absence is
/// deliberate: consumers must not infer an unavailable PDF lineage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PreviewExportProvenance {
    UnavailableInPhaseOne,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevisionProvenance {
    document_id: DocumentId,
    revision: u32,
    schema_version: u32,
    canonical_hash: RevisionHash,
    engine: EngineIdentity,
    lineage: RevisionLineage,
    source_hashes: Vec<RevisionHash>,
    preview_export_provenance: PreviewExportProvenance,
}

impl RevisionProvenance {
    pub fn from_document(
        document: &FlowDocument,
        canonical_hash: impl Into<String>,
    ) -> Result<Self, ProvenanceError> {
        let lineage = match &document.provenance {
            Provenance::LocalSample { created_at } => RevisionLineage::Created {
                created_at: ProvenanceTimestamp::parse(created_at)?,
            },
            Provenance::Migrated {
                source_schema_version,
                current_schema_version,
                source_created_at,
                hops,
            } => RevisionLineage::Migrated {
                source_schema_version: *source_schema_version,
                current_schema_version: *current_schema_version,
                source_created_at: ProvenanceTimestamp::parse(source_created_at)?,
                hops: hops.clone(),
            },
        };
        Ok(Self {
            document_id: document.document_id.clone(),
            revision: document.revision,
            schema_version: document.schema_version,
            canonical_hash: RevisionHash::parse(canonical_hash)?,
            engine: EngineIdentity::current(),
            lineage,
            source_hashes: Vec::new(),
            preview_export_provenance: PreviewExportProvenance::UnavailableInPhaseOne,
        })
    }

    pub fn with_source_hashes(
        mut self,
        source_hashes: Vec<RevisionHash>,
    ) -> Result<Self, ProvenanceError> {
        if source_hashes.len() > MAX_SOURCE_HASHES {
            return Err(ProvenanceError::InvalidSourceHashes);
        }
        let mut unique = BTreeSet::new();
        for hash in &source_hashes {
            if !unique.insert(hash) {
                return Err(ProvenanceError::InvalidSourceHashes);
            }
        }
        self.source_hashes = source_hashes;
        Ok(self)
    }

    #[must_use]
    pub fn document_id(&self) -> &DocumentId {
        &self.document_id
    }

    #[must_use]
    pub const fn revision(&self) -> u32 {
        self.revision
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub fn canonical_hash(&self) -> &RevisionHash {
        &self.canonical_hash
    }

    #[must_use]
    pub const fn preview_export_provenance(&self) -> &PreviewExportProvenance {
        &self.preview_export_provenance
    }
}

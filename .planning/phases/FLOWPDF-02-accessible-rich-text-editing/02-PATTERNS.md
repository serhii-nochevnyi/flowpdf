# Phase 2: Accessible Rich-Text Editing - Pattern Map

**Mapped:** 2026-08-26  
**Files classified:** 61 new/modified files or fixture groups  
**Existing analog families:** 5  
**Analogs found:** 58 / 61 (exact, direct-extension, role-match, or partial)

The paths below are inferred from the locked context, research project structure, and validation Wave 0. The planner may co-locate a small Rust module or React component when that reduces circular dependencies, but it must preserve the ownership and data-flow assignments in this map.

## File Classification

### Tooling, manifests, and entry points

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| <code>Cargo.toml</code> | config | build/dependency resolution | <code>Cargo.toml</code> | direct-extension |
| <code>Cargo.lock</code> | config | build/dependency resolution | <code>Cargo.lock</code> | generated-extension |
| <code>crates/flow-core/Cargo.toml</code> | config | build/dependency resolution | <code>crates/flow-core/Cargo.toml</code> | direct-extension |
| <code>package.json</code> | config | build/test orchestration | <code>package.json</code> | direct-extension |
| <code>package-lock.json</code> | config | build/dependency resolution | <code>package-lock.json</code> | generated-extension |
| <code>vite.config.ts</code> | config | build/request-response | <code>vitest.config.ts</code> | partial |
| <code>tsconfig.json</code> | config | build/type checking | <code>tsconfig.json</code> | direct-extension |
| <code>tsconfig.web.json</code> | config | build/emit | <code>tsconfig.web.json</code> | direct-extension |
| <code>vitest.config.ts</code> | config | test orchestration | <code>vitest.config.ts</code> | exact |
| <code>config/dependency-provenance.json</code> | config | batch/network verification | existing Phase 1 provenance config + <code>scripts/verify-dependency-provenance.mjs</code> | direct-extension |
| <code>web/index.html</code> | component | request-response/bootstrap | <code>web/index.html</code> | direct-extension |
| <code>web/src/main.ts</code> → <code>web/src/main.tsx</code> | route/component | request-response/bootstrap | <code>web/src/main.ts</code> | role-match |
| <code>scripts/build-web.mjs</code> | config/utility | file-I/O/build | <code>scripts/build-web.mjs</code> | direct-extension |

### Rust model, migration, editing, and durability

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| <code>crates/flow-core/src/model/mod.rs</code> | model | transform/serialization | same file: closed serde schema and stable IDs | exact |
| <code>crates/flow-core/src/schema/mod.rs</code> | service/utility | transform/migration | same file: sequential registry and bounded validation | exact |
| <code>crates/flow-core/src/anchor/mod.rs</code> | utility/model | transform | same file: typed UTF-16 offsets and anchor algebra | exact |
| <code>crates/flow-core/src/transaction/mod.rs</code> | service/model | event-driven/transform | same file: command → operation → inverse → transaction | exact |
| <code>crates/flow-core/src/lib.rs</code> | service/provider | request-response | same file: closed request/result/commit DTOs | exact |
| <code>crates/flow-core/src/store/mod.rs</code> | service/provider | CRUD/file-I/O | same file: Rust-planned atomic durability port | exact |
| <code>crates/flow-core/src/editor_view/mod.rs</code> | model/provider | request-response | <code>crates/flow-core/src/lib.rs</code> SessionDto/InspectorView/OperationResult | role-match |
| <code>crates/flow-core/src/asset/mod.rs</code> | service/utility | file-I/O/transform | <code>crates/flow-core/src/canonical/mod.rs</code> + <code>store/mod.rs</code> | partial |
| <code>crates/flow-wasm/src/lib.rs</code> | route/provider | request-response/binary ingress | same file: immutable serialized exports | exact |
| <code>web/persistence/indexeddb-store.ts</code> | service/store | CRUD/file-I/O | same file: one strict guarded IndexedDB transaction | exact |

### React editor and retained diagnostics

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| <code>web/src/foundation-inspector.ts</code> | component/controller | request-response | same file; retain as diagnostics and adapt only shared DTO/build seams | exact |
| <code>web/src/editor/editor-controller.ts</code> | service/controller | event-driven/request-response | <code>web/src/foundation-inspector.ts</code> | role-match |
| <code>web/src/editor/input-adapter.ts</code> | utility/controller | event-driven | no controlled IME analog; closest is inspector keyboard wiring | partial |
| <code>web/src/editor/selection-bridge.ts</code> | utility | event-driven/transform | Rust anchor DTO + inspector focus restoration | partial |
| <code>web/src/editor/editor-store.ts</code> | store/provider | event-driven/pub-sub | inspector accepted session/view fields | partial |
| <code>web/src/editor/semantic-document.tsx</code> | component | request-response projection | inspector native DOM construction | role-match |
| <code>web/src/editor/formatting-toolbar.tsx</code> | component | event-driven | inspector localized action groups and control availability | role-match |
| <code>web/src/editor/insert-menu.tsx</code> | component | event-driven | inspector localized native buttons/focus return | role-match |
| <code>web/src/editor/embedded-blocks.tsx</code> | component | event-driven/file-I/O | inspector native DOM + IndexedDB asset boundary | partial |
| <code>web/src/editor/editor-app.tsx</code> | component/provider | event-driven/request-response | inspector mount/controller lifecycle | role-match |
| <code>web/src/styles.css</code> | config/presentation | event-driven responsive states | same file: Phase 1 tokens, focus, live status, breakpoints | exact |
| <code>web/src/i18n/uk.ts</code> | config | request-response/localization | same file: authoritative key type | exact |
| <code>web/src/i18n/en.ts</code> | config | request-response/localization | same file: compile-time key parity | exact |

### Rust tests and corpora

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| <code>crates/flow-core/tests/grapheme_conformance.rs</code> | test | batch/transform | <code>crates/flow-core/tests/preconditions.rs</code> offset corpus pattern | role-match |
| <code>crates/flow-core/tests/schema_v2_migration.rs</code> | test | batch/transform | <code>crates/flow-core/tests/migration_golden.rs</code> | exact |
| <code>crates/flow-core/tests/rich_text_transactions.rs</code> | test | event-driven/transform | <code>crates/flow-core/tests/transaction_properties.rs</code> | exact |
| <code>crates/flow-core/tests/rich_text_properties.rs</code> | test | batch/transform | <code>crates/flow-core/tests/transaction_properties.rs</code> | exact |
| <code>crates/flow-core/tests/resource_limits.rs</code> | test | batch/transform | <code>crates/flow-core/tests/schema.rs</code> | exact |
| <code>crates/flow-core/tests/asset_staging.rs</code> | test | file-I/O/request-response | store atomicity + asset round-trip tests | partial |
| <code>fixtures/unicode/17.0.0/GraphemeBreakTest.txt</code> | test corpus | batch | no Unicode conformance corpus exists | none |
| <code>fixtures/unicode/17.0.0/provenance.json</code> | config/test corpus | batch | dependency provenance report shape | partial |
| <code>fixtures/flowdoc/schema-v1-rich-text-*.json</code> | test corpus | transform/migration | <code>fixtures/flowdoc/{older,current,migrated}.json</code> | exact |
| <code>fixtures/indexeddb/phase1-v3-*.json</code> | test corpus | file-I/O/recovery | IndexedDB unit-test record builders | partial |
| <code>fixtures/editor/semantic-scale.recipe.json</code> | test corpus | batch | <code>fixtures/recovery/benchmark-200-page.recipe.json</code> | role-match |

### Browser/unit tests and phase gates

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| <code>web/tests/editor-controller.test.ts</code> | test | event-driven/request-response | <code>web/tests/foundation-inspector.test.ts</code> | role-match |
| <code>web/tests/editor-input.browser.test.ts</code> | test | event-driven | <code>web/tests/accessibility.browser.test.ts</code> | role-match |
| <code>web/tests/editor-formatting.browser.test.ts</code> | test | event-driven | <code>web/tests/accessibility.browser.test.ts</code> | role-match |
| <code>web/tests/editor-parity.browser.test.ts</code> | test | event-driven/contract | Phase 1 boundary + accessibility enumeration patterns | partial |
| <code>web/tests/editor-accessibility.browser.test.ts</code> | test | request-response/accessibility | <code>web/tests/accessibility.browser.test.ts</code> | exact |
| <code>web/tests/editor-responsive.browser.test.ts</code> | test | request-response/responsive | <code>web/tests/accessibility.browser.test.ts</code> | exact |
| <code>web/tests/walking-skeleton.browser.test.ts</code> | test | request-response/file-I/O | same file: real WASM + IndexedDB lifecycle | exact |
| <code>web/tests/indexeddb-store.test.ts</code> | test | CRUD/file-I/O | same file: atomic failure and migration sets | exact |
| <code>scripts/verify-phase2-dependencies.mjs</code> | utility/test | batch/network | <code>scripts/verify-dependency-provenance.mjs</code> | exact |
| <code>scripts/verify-wasm-size.mjs</code> | utility/test | batch/file-I/O | <code>scripts/verify-recovery-benchmark.mjs</code> terminal artifact pattern | role-match |
| <code>scripts/verify-ime-chromium.mjs</code> | utility/test | event-driven/browser | no CDP/IME script exists | none |
| <code>scripts/check-phase2.mjs</code> | config/utility | batch/test orchestration | <code>scripts/check-phase1.mjs</code> | exact |
| <code>tests/contracts/phase2-boundary.test.mjs</code> | test | batch/static analysis | <code>tests/contracts/phase1-boundary.test.mjs</code> | exact |
| <code>tests/contracts/phase1-boundary.test.mjs</code> | test | batch/static analysis | same file, made phase-aware without weakening ownership checks | direct-extension |

## Pattern Assignments

### Canonical schema and migration

**Applies to:** <code>model/mod.rs</code>, <code>schema/mod.rs</code>, schema-v1 fixtures, and <code>schema_v2_migration.rs</code>.

**Primary analogs:** <code>crates/flow-core/src/model/mod.rs</code> and <code>crates/flow-core/src/schema/mod.rs</code>.

**Imports and closed-schema pattern** — <code>model/mod.rs:7-13</code>, <code>model/mod.rs:60-73</code>:

~~~rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::anchor::Utf16Offset;
use crate::schema::{SchemaError, validate_document};

pub const SCHEMA_VERSION: u32 = 1;

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
~~~

Copy these conventions exactly:

- durable structs use <code>camelCase</code> plus <code>deny_unknown_fields</code>;
- enums use closed tagged/renamed variants, never arbitrary property bags;
- stable IDs remain validated UUID wrappers from <code>model/mod.rs:15-58</code>;
- the schema version is a canonical Rust constant, not a TypeScript switch.

**Legacy preservation seam** — <code>model/mod.rs:115-175</code> contains the current flat nodes, asset alt string, field descriptor, and closed field-kind vocabulary. Freeze equivalent private v1 structs before changing the public v2 model. Do not deserialize v1 bytes into the new v2 node enum.

**Limit and stable-code pattern** — <code>schema/mod.rs:16-50</code>, <code>schema/mod.rs:52-122</code>:

~~~rust
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
    pub const fn code(self) -> &'static str {
        match self {
            Self::CanonicalBytes => "FLOW_LIMIT_CANONICAL_BYTES",
            Self::TreeDepth => "FLOW_LIMIT_TREE_DEPTH",
            // exhaustive stable mapping
        }
    }
}

pub fn check(self, kind: LimitKind, actual: usize) -> Result<(), SchemaError> {
    let maximum = /* closed match over kind */;
    if actual > maximum {
        return Err(SchemaError::limit(kind, actual, maximum));
    }
    Ok(())
}
~~~

New editor/image/table/run/command limits belong in named Rust enums/constants and must use the same exhaustive stable-code mapping. Validate counts and checked sums before cloning, decoding, or mutation, as current document validation does at <code>schema/mod.rs:275-385</code>.

**Sequential migration pattern** — <code>schema/mod.rs:597-710</code>:

~~~rust
pub struct MigrationStep {
    pub from_version: u32,
    pub to_version: u32,
    migrate: MigrationFunction,
    validate_output: MigrationValidator,
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
    // Find exactly version -> version + 1, validate candidate, then publish it.
}
~~~

The v1→v2 hop must be added beside the v0→v1 hop at <code>schema/mod.rs:630-638</code>. Keep explicit private legacy structs like <code>LegacyDocumentV0</code> at <code>schema/mod.rs:747-797</code>. The one-way checkpoint is not complete until v0→v1→v2, v1→v2, and v2 no-op goldens all pass, including <code>MissingLegacy</code> image accessibility and exact <code>LegacyInvalid</code> field-anchor preservation.

**Test pattern** — <code>crates/flow-core/tests/migration_golden.rs:16-60</code>:

~~~rust
let first = registry.migrate(payload(OLDER_FILE)).expect("migration");
let second = registry.migrate(payload(OLDER_FILE)).expect("repeat migration");

assert_eq!(first.canonical_bytes, payload(MIGRATED_FILE));
assert_eq!(first.canonical_hash, MIGRATED_HASH.trim());
assert_eq!(first.canonical_bytes, second.canonical_bytes);
assert_eq!(first.canonical_hash, second.canonical_hash);
assert!(first.report.requires_new_snapshot);
assert!(first.report.preserve_source_records);
~~~

Use checked-in bytes and hashes, repeat the migration, and assert the exact report/hops. Keep failure tests that prove input bytes are unchanged, following <code>migration_golden.rs:93-124</code>.

---

### Grapheme positions, directional selections, and anchor algebra

**Applies to:** <code>anchor/mod.rs</code>, <code>editor_view/mod.rs</code>, transaction selection DTOs, <code>grapheme_conformance.rs</code>, and rich-text property tests.

**Analog:** <code>crates/flow-core/src/anchor/mod.rs</code>.

**Typed offset imports and errors** — <code>anchor/mod.rs:8-75</code>:

~~~rust
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{Affinity, LogicalPosition, NodeId};

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(transparent)]
pub struct Utf16Offset(u32);

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum AnchorError {
    #[error("The logical position is outside the target text")]
    OutOfRange,
    #[error("The UTF-16 position splits a surrogate pair")]
    InvalidUtf16Boundary,
}
~~~

Add a distinct grapheme-boundary error; do not repurpose scalar-boundary or range failures. Preserve public <code>LogicalPosition { node_id, utf16_offset, affinity }</code> from <code>model/mod.rs:227-240</code>.

**Conversion boundary** — <code>anchor/mod.rs:77-125</code>:

~~~rust
pub fn resolve_utf16_offset(
    value: &str,
    offset: Utf16Offset,
) -> Result<NativeByteOffset, AnchorError> {
    // checked UTF-16 -> UTF-8 scalar conversion
}

pub fn byte_to_utf16_offset(
    value: &str,
    byte_offset: NativeByteOffset,
) -> Result<Utf16Offset, AnchorError> {
    if byte_offset.get() > value.len() || !value.is_char_boundary(byte_offset.get()) {
        return Err(AnchorError::OutOfRange);
    }
    // checked UTF-8 -> UTF-16 conversion
}
~~~

The ICU4X boundary vector must sit above this conversion. ICU byte boundaries are translated once per touched immutable block/revision, then membership is checked by binary search. Never normalize text and never snap malformed public positions.

**Composable mapping pattern** — <code>anchor/mod.rs:127-218</code>:

~~~rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnchorMapping {
    pub transformations: Vec<AnchorTransformation>,
}

pub fn map(&self, position: &LogicalPosition) -> AnchorMapResult {
    let mut result = AnchorMapResult::Mapped(position.clone());
    for transformation in &self.transformations {
        let AnchorMapResult::Mapped(position) = result else { break };
        result = transformation.map(&position);
    }
    result
}
~~~

Extend the transformation enum for split, merge, compatible cross-block replace, subtree insertion/removal, and atomic-node 0/1 edge sentinels. Preserve affinity explicitly. The current detailed endpoint behavior in <code>anchor/mod.rs:220-284</code> is the style to follow: checked arithmetic, exhaustive affinity branches, and explicit invalidation.

**Existing test marks the exact Phase 2 delta** — <code>crates/flow-core/tests/preconditions.rs:210-232</code>:

~~~rust
let combining_mark_byte = text.find('\u{0306}').expect("combining mark");
let between_base_and_mark =
    byte_to_utf16_offset(text, NativeByteOffset::new(combining_mark_byte)).expect("boundary");
assert!(
    resolve_utf16_offset(text, between_base_and_mark).is_ok(),
    "Phase 1 validates scalar/UTF-16 boundaries but intentionally does not claim grapheme safety"
);
~~~

Phase 2 replaces this permitted scalar-interior result only at the editor/grapheme validation layer; the low-level scalar converter remains useful and truthful.

---

### Semantic command compilation, exact inverses, and session/document separation

**Applies to:** <code>transaction/mod.rs</code>, <code>editor_view/mod.rs</code>, <code>rich_text_transactions.rs</code>, and <code>rich_text_properties.rs</code>.

**Analog:** <code>crates/flow-core/src/transaction/mod.rs</code>.

**Imports and command envelope** — <code>transaction/mod.rs:3-40</code>:

~~~rust
use std::collections::BTreeSet;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Command {
    pub command_id: CommandId,
    pub base_revision: u32,
    pub modality: SourceModality,
    pub issued_at: String,
    pub kind: CommandKind,
}
~~~

Keep <code>SourceModality::{Ui, Keyboard, Voice, Api, System}</code> at <code>transaction/mod.rs:22-30</code>; visible, keyboard, and future voice paths must differ only by modality and physical translation, not semantic authority.

**Closed command → exact operation pattern** — <code>transaction/mod.rs:42-124</code>, <code>transaction/mod.rs:136-190</code>:

~~~rust
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CommandKind {
    InsertText { target: LogicalPosition, text: String },
    ReplaceText { range: TextRange, text: String },
    DeleteText { range: TextRange },
    // closed semantic intents
}

pub struct Transaction {
    pub forward_operations: Vec<Operation>,
    pub inverse_operations: Vec<Operation>,
    pub anchor_mapping: AnchorMapping,
    pub history_effect: HistoryEffect,
    // identity, revisions, modality, hashes
}
~~~

Phase 2 UI must not retain the generic raw <code>Batch</code> as an editor escape hatch. Add closed rich-text commands and compile them internally to operations carrying complete removed runs/subtrees/attributes.

**Apply/publish invariant** — <code>transaction/mod.rs:328-395</code>, <code>transaction/mod.rs:506-563</code>:

~~~rust
impl TransactionService {
    pub fn apply(state: &EditorState, command: Command) -> Result<AppliedCommand, CommandError> {
        validate_command_envelope(state, &command)?;
        match &command.kind {
            CommandKind::Undo => apply_undo(state, command),
            CommandKind::Redo => apply_redo(state, command),
            _ => apply_mutation(state, command),
        }
    }
}

let mut candidate = state.document.clone();
for mutation in &mutations {
    let (operation, inverse_operation) = derive_operation(&candidate, mutation)?;
    let operation_mapping = apply_operation(&mut candidate, &operation)?;
    mapping.extend(operation_mapping);
    forward.push(operation);
    inverse.insert(0, inverse_operation);
}
let (candidate, before_hash, after_hash) = finalize_document(state, candidate)?;
~~~

Copy the candidate-clone pattern: validate and mutate an unpublished candidate, prepend inverses, validate canonical output, then return a new state and transaction. Stable errors remain exhaustive via <code>CommandError::code</code> at <code>transaction/mod.rs:341-395</code>.

**Replay is validation, not best effort** — <code>transaction/mod.rs:856-929</code> verifies record format, IDs, schema, base/new revision, hashes, operation-produced anchor mapping, and the final canonical hash. Rich-text replay must add no recovery-only shortcuts.

**Field anchors participate in every mapping** — <code>transaction/mod.rs:1026-1039</code>:

~~~rust
for field in &mut document.fields {
    field.anchor = match mapping.map(&field.anchor) {
        AnchorMapResult::Mapped(position) => position,
        AnchorMapResult::Invalid(_) => return Err(CommandError::AnchorInvalidated),
    };
}
~~~

For v2, adapt this to the typed <code>GraphemeSafe</code>/<code>LegacyInvalid</code> state. Preserve an invalid legacy original position and reason; never promote or snap it during ordinary editing.

**Reversible deletion has no exact Phase 1 analog.** The current `NodeInvalidated` result is a useful fail-closed postimage, but subtree bytes alone cannot restore which selection/session/field owner held each deleted endpoint. Phase 2 must add the finalized research contract:

- public mapping returns a deterministic `Live`, `Deleted(tombstone)`, or `Invalidated(reason)` result and never exposes a replacement target;
- the transaction record privately stores one bounded, owner-keyed `AnchorPreimage` for every affected selection endpoint and persistent field, including its exact pre-command `node_id + utf16_offset + affinity` and prior field-anchor state;
- persistent-field deletion rejects before mutation unless every affected field can become reversible `TargetDeleted { original, tombstone }` state;
- undo restores and validates the original subtree IDs/order first, then restores exact endpoints from the verified preimage; redo reapplies the same deterministic tombstone slots;
- preimage owner count and encoded bytes are charged to existing field/transaction limits, and recovery rejects missing, duplicate-owner, token/root-mismatched, or client-supplied preimages.

Property and recovery tests must cover delete→undo→redo→undo for canonical bytes, IDs, directional selection/session endpoints, every field-anchor state, tombstones, and history. Keep preimages out of public editor DTOs and ordinary nonpersisted session-only updates.

**Property-test setup** — <code>crates/flow-core/tests/transaction_properties.rs:1-41</code>:

~~~rust
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

fn apply(
    state: &EditorState,
    serial: u32,
    kind: CommandKind,
) -> Result<(EditorState, Transaction), CommandError> {
    TransactionService::apply(state, command(state, serial, kind))
        .map(|applied| (applied.state, applied.transaction))
}
~~~

Copy the helper shape and assert canonical bytes, stable IDs, directional selection, field-anchor states, history cursor, undo-all, and redo-all—not only visible text. The exact field-anchor inverse style already appears at <code>transaction_properties.rs:112-139</code>.

**Session state rule:** use the DTO style below, but do not put selection/pending marks into <code>FlowDocument</code>, transaction history, audit, or IndexedDB. Rust-owned session-only actions may advance a <code>session_generation</code>, never the document revision.

---

### Public Rust result and editor-view projection

**Applies to:** <code>crates/flow-core/src/lib.rs</code>, <code>editor_view/mod.rs</code>, controller DTOs, and WASM responses.

**Analog:** <code>crates/flow-core/src/lib.rs</code>.

**Result grouping** — <code>lib.rs:165-207</code>:

~~~rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDto {
    pub canonical_json: String,
    pub canonical_hash: String,
    pub document_id: DocumentId,
    pub revision: u32,
    pub next_command_target: Option<LogicalPosition>,
    pub history: HistoryState,
}

pub struct OperationResult {
    pub session: SessionDto,
    pub view: InspectorView,
    pub commit: PersistenceCommit,
}
~~~

Extend this established grouping rather than creating parallel browser state. The Phase 2 result should carry a closed editor view/session projection plus the existing commit proof. A session-only result must be a separately discriminated response with no persistence commit.

**API response/error pattern** — <code>lib.rs:281-287</code>, <code>lib.rs:368-389</code>:

~~~rust
pub struct ApiResponse<T> {
    pub ok: bool,
    pub value: Option<T>,
    pub error: Option<ErrorDto>,
}

pub fn create_sample(request: CreateSampleRequest) -> ApiResponse<OperationResult> {
    match create_sample_inner(request) {
        Ok(result) => ApiResponse::success(result),
        Err(error) => ApiResponse::failure(error),
    }
}
~~~

Public functions stay thin: deserialize/validate in Rust, call a private pure inner function, and map to the same stable response envelope.

**Result assembly** — <code>lib.rs:1486-1527</code>:

~~~rust
let session = session_dto(&document, history, canonical_json, canonical_hash.clone())?;
let view = inspector_view(&document, canonical_hash, None, vec![audit.clone()])?;
Ok(OperationResult {
    session,
    view,
    commit: PersistenceCommit {
        replace_existing,
        snapshot,
        transaction,
        audit,
        assets,
    },
})
~~~

Build editor DTOs from the accepted Rust document/session in one place. React must not parse <code>canonical_json</code> to derive blocks, formatting, capabilities, field order, or a replacement selection.

---

### Asset validation, staging receipt, and Rust-planned persistence

**Applies to:** <code>asset/mod.rs</code>, <code>store/mod.rs</code>, <code>flow-wasm/lib.rs</code>, <code>indexeddb-store.ts</code>, <code>asset_staging.rs</code>, and IndexedDB migration fixtures/tests.

**Closest partial analogs:** <code>canonical/mod.rs</code>, <code>store/mod.rs</code>, and <code>indexeddb-store.ts</code>. There is no existing decoder or receipt table.

**Hash/length verification pattern** — <code>canonical/mod.rs:47-80</code>:

~~~rust
pub fn verify_asset_bytes(descriptor: &AssetDescriptor, bytes: &[u8]) -> Result<(), SchemaError> {
    let length_matches = usize::try_from(descriptor.byte_length)
        .is_ok_and(|expected_length| expected_length == bytes.len());
    let hash_matches = descriptor.content_hash == asset_hash(bytes);
    if !length_matches || !hash_matches {
        return Err(SchemaError::asset_hash_mismatch());
    }
    Ok(())
}

pub fn asset_hash(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}
~~~

Keep hashing, format sniffing, dimensions, pixel/allocation bounds, and receipt metadata in Rust. Browser MIME, extension, preview, and semantic command metadata are untrusted.

**Planner/port separation** — <code>store/mod.rs:124-149</code>, <code>store/mod.rs:151-240</code>, <code>store/mod.rs:531-557</code>:

~~~rust
pub struct PlannedPersistenceCommit {
    pub replace_existing: bool,
    pub snapshot: Option<SnapshotRecord>,
    pub transaction: TransactionRecord,
    pub audit: AuditRecord,
    pub assets: Vec<AssetRecord>,
}

pub trait DocumentStore {
    fn commit_planned_atomic(
        &mut self,
        commit: PlannedPersistenceCommit,
    ) -> Result<CommitDisposition, StoreError>;

    fn load_records(&self) -> Result<RecoverRequest, StoreError>;
    fn commit_migration_atomic(
        &mut self,
        commit: MigrationPersistenceCommit,
    ) -> Result<CommitDisposition, StoreError>;
}
~~~

The staged receipt is redeemed by Rust into this same logical commit. The physical adapter never decides whether bytes belong to a document or whether a receipt is valid.

**IndexedDB DTO seam requiring versioned compatibility** — <code>indexeddb-store.ts:104-169</code>:

~~~typescript
export interface PlannedPersistenceCommitDto {
  readonly replaceExisting: boolean
  readonly snapshot: SnapshotRecordDto | null
  readonly transaction: TransactionRecordDto
  readonly audit: AuditRecordDto
  readonly assets: readonly AssetRecordDto[]
}

export interface AssetRecordDto {
  readonly recordFormatVersion: number
  readonly contentHash: string
  readonly bytes: readonly number[]
}
~~~

Do not put 8 MiB image bytes into the 1 MiB semantic JSON DTO. Add a versioned physical asset envelope that accepts legacy <code>readonly number[]</code> on read and uses <code>Uint8Array</code>/<code>ArrayBuffer</code> for new records. Preserve old records and cold-open compatibility.

**One transaction for snapshot + transaction + audit + assets + head** — <code>indexeddb-store.ts:204-308</code>, <code>indexeddb-store.ts:701-785</code>:

~~~typescript
await commitGuardedRecords(
  database,
  [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS, SOURCES],
  [
    { storeName: SNAPSHOTS, records: snapshot === null ? [] : [/* guarded */] },
    { storeName: TRANSACTIONS, records: [/* guarded */] },
    { storeName: AUDITS, records: [/* guarded */] },
    { storeName: ASSETS, records: assets.map(/* guarded */) },
    { storeName: SOURCES, records: [] },
  ],
  { expected: /* base head */, resulting: /* accepted head */ },
)

const transaction = database.transaction(
  [...new Set([...storeNames, METADATA, HEADS])],
  'readwrite',
  { durability: 'strict' },
)
~~~

Write the redeemed staged asset in this transaction; publish nothing until <code>oncomplete</code>. Retain identity checks, exact-retry handling, head CAS, resource budget, and abort-on-any-error.

**Atomic failure test pattern** — <code>web/tests/indexeddb-store.test.ts:147-177</code> forces the late asset write to fail, expects <code>FLOW_STORAGE_WRITE_FAILED</code>, then proves every store still equals the baseline. Reuse that pattern for the new binary envelope and receipt redemption.

---

### Narrow WASM route pattern

**Applies to:** every new editor/session/asset export in <code>crates/flow-wasm/src/lib.rs</code>.

**Analog:** <code>crates/flow-wasm/src/lib.rs</code>.

**Imports and request-response adapter** — <code>flow-wasm/src/lib.rs:5-12</code>, <code>flow-wasm/src/lib.rs:31-46</code>:

~~~rust
use flow_core::{
    ApiResponse, ApplyCommandRequest, OperationResult,
};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn apply_command(request: JsValue) -> JsValue {
    let response = match serde_wasm_bindgen::from_value::<ApplyCommandRequest>(request) {
        Ok(request) => flow_core::apply_command(request),
        Err(_) => flow_core::decode_failure::<OperationResult>(),
    };
    serialize_response(&response)
}
~~~

Each export accepts one immutable request value and returns one serialized response. Do not export a mutable document/editor/asset-table handle. New history/session variants should validate the expected semantic command like <code>apply_history_command</code> at <code>flow-wasm/src/lib.rs:147-167</code>.

**Serialization pattern** — <code>flow-wasm/src/lib.rs:169-173</code>:

~~~rust
fn serialize_response<T: serde::Serialize>(response: &ApiResponse<T>) -> JsValue {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    serde::Serialize::serialize(response, &serializer)
        .expect("serializing a fixed response DTO cannot fail")
}
~~~

The dedicated <code>stage_asset</code> binary ingress is the one exception to JSON input: accept a bounded <code>Uint8Array</code>/byte slice plus a small closed metadata request, but still return a serialized immutable receipt response.

---

### Browser controller publication and focus/error handling

**Applies to:** <code>editor-controller.ts</code>, <code>editor-store.ts</code>, <code>editor-app.tsx</code>, and retained diagnostics.

**Analog:** <code>web/src/foundation-inspector.ts</code>.

**Imports and typed boundary** — <code>foundation-inspector.ts:1-16</code>, <code>foundation-inspector.ts:18-142</code> show type-only persistence imports and local readonly DTO interfaces. Preserve readonly DTOs, but prefer a shared generated/declared editor boundary module instead of duplicating the growing Phase 2 shape across components.

**Single in-flight action, stable error, focus restoration** — <code>foundation-inspector.ts:327-375</code>:

~~~typescript
private start(
  operation: () => Promise<void>,
  control: HTMLButtonElement,
  errorKind: ErrorPresentation = 'command',
): void {
  if (this.busy || control.disabled) return
  this.pending = operation()
    .then(() => this.restoreFocus(control))
    .catch(async (error: unknown) => {
      this.setBusy(true)
      let presentedError = error
      try {
        await this.persistErrorAudit(error, errorKind === 'recovery')
      } finally {
        this.setBusy(false)
        this.setError(errorCode(presentedError), errorKind)
        this.restoreFocus(control)
      }
    })
}
~~~

For editor commands, restore the Rust-returned logical selection to the editor rather than merely focusing the initiating button. Keep one committing state and stable visible errors.

**Durable publication boundary** — <code>foundation-inspector.ts:619-650</code>:

~~~typescript
const records = await this.store.loadRecords()
const recovered = unwrap(this.wasm.query_document(records))
if (
  recovered.session.revision !== expected.revision ||
  recovered.session.canonicalHash !== expected.canonicalHash
) {
  throw new FoundationError('FLOW_RESULT_REVISION_MISMATCH')
}
this.session = recovered.session
this.view = recovered.view
this.render()
~~~

And:

~~~typescript
const planned = unwrap(
  this.wasm.commit_record({ records, commit, reason }),
)
await this.store.commit(planned)
~~~

Keep the ordering exactly: Rust result → Rust persistence plan → physical atomic commit → Rust query/recovery verification → publish one accepted immutable editor snapshot. Never optimistically mutate React document state.

**Status/error semantics** — <code>foundation-inspector.ts:777-807</code> sets visible pending/completed text, a visible alert, and <code>aria-busy</code>. Use the UI-SPEC copy keys; do not log document/candidate/image content.

**React store delta:** no existing <code>useSyncExternalStore</code> analog exists. Implement a cached immutable snapshot identity and a subscribe function that returns cleanup, as specified by RESEARCH.md. TypeScript may own only the exact accepted DTO reference and physical composition/menu/focus state.

---

### Native input, composition, and selection bridge

**Applies to:** <code>input-adapter.ts</code>, <code>selection-bridge.ts</code>, <code>editor-input.browser.test.ts</code>, and <code>verify-ime-chromium.mjs</code>.

**Closest existing code:** inspector keyboard dispatch at <code>foundation-inspector.ts:703-719</code>:

~~~typescript
if (
  this.busy ||
  isTextEditingTarget(event.target) ||
  !(event.ctrlKey || event.metaKey) ||
  event.altKey ||
  event.key.toLowerCase() !== 'z'
) {
  return
}
event.preventDefault()
control.focus()
this.start(() => this.applyHistory(kind), control)
~~~

Reuse the explicit guard → prevent default only when appropriate → one controller action structure. Do not copy its document-wide root listener wholesale: Phase 2 native <code>beforeinput</code>, <code>input</code>, composition, paste, keydown, and selection listeners belong on/ref to the controlled input host.

There is no existing composition state machine or DOM↔logical selection implementation. Use RESEARCH.md Patterns 7–8, including:

- explicit Idle/Composing/Committing/Invalidated states;
- explicit cancel flags; empty <code>compositionend.data</code> is not cancellation;
- candidate remains noncanonical and is committed by one <code>ReplaceSelection</code>;
- changed accepted revision invalidates rather than relocates;
- DOM Range is view-local and translated only through the last accepted Rust projection;
- unexpected/noncancelable DOM mutation is discarded by restoring that accepted projection.

The CDP IME script has no repository analog and must use a deterministic standalone Playwright lifecycle with terminal exit status; do not treat synthetic events as its replacement.

---

### Semantic React components, localization, and accessibility

**Applies to:** all <code>web/src/editor/*.tsx</code>, <code>styles.css</code>, and the Ukrainian/English dictionaries.

**Closest analogs:** native DOM and localization in <code>foundation-inspector.ts</code>, <code>uk.ts</code>, <code>en.ts</code>, and <code>styles.css</code>.

**Native semantics and labelled regions** — <code>foundation-inspector.ts:1194-1203</code>, <code>foundation-inspector.ts:1293-1317</code>:

~~~typescript
const main = element('main', 'foundation-grid')
const commands = element('section', 'card command-panel')
commands.setAttribute('aria-labelledby', 'commands-heading')

const status = element('p', 'status-region')
status.setAttribute('aria-live', 'polite')
status.setAttribute('aria-atomic', 'true')

const alert = element('p', 'alert-region')
alert.setAttribute('role', 'alert')
alert.setAttribute('aria-atomic', 'true')
alert.hidden = true
~~~

React should express the same native structure declaratively: article/region, p, h1–h6, ol/ul/li, figure/img, table/thead/tbody/th/td, and focusable separator. Do not substitute generic div/role replicas or <code>dangerouslySetInnerHTML</code>.

**Key-authority pattern** — <code>web/src/i18n/uk.ts:1-112</code>, <code>web/src/i18n/en.ts:1-112</code>:

~~~typescript
export const foundationInspectorUk = {
  // authoritative complete key set
} as const

export type FoundationInspectorMessageKey = keyof typeof foundationInspectorUk

export const foundationInspectorEn = {
  // identical translated keys
} as const satisfies Record<FoundationInspectorMessageKey, string>
~~~

Use the Ukrainian editor dictionary as the authoritative key type and English as <code>satisfies Record&lt;EditorMessageKey, string&gt;</code>. Parameterize whole sentences; do not concatenate fragments.

**CSS token/focus pattern** — <code>styles.css:1-19</code>, <code>styles.css:32-61</code>:

~~~css
:root {
  --surface-app: #f8fafc;
  --surface-card: #ffffff;
  --text-primary: #334155;
  --text-secondary: #475569;
  --border-default: #cbd5e1;
  --border-control: #475569;
  --accent: #1d4ed8;
  --status-success: #166534;
  --status-error: #b91c1c;
}

button {
  min-width: 44px;
  min-height: 44px;
  border-radius: 6px;
}

button:focus-visible,
a:focus-visible {
  outline: 3px solid var(--accent);
  outline-offset: 2px;
}
~~~

Extend these project tokens rather than starting a second system. Preserve <code>[hidden] { display: none !important; }</code>, overflow wrapping at <code>styles.css:154-161</code>, mobile reflow at <code>styles.css:425-457</code>, and reduced-motion handling at <code>styles.css:459-465</code>.

---

### Vitest, browser, durability, and accessibility tests

**Applies to:** <code>vitest.config.ts</code>, all new web tests, <code>walking-skeleton.browser.test.ts</code>, and <code>indexeddb-store.test.ts</code>.

**Project layout** — <code>vitest.config.ts:4-61</code>:

~~~typescript
export default defineConfig({
  test: {
    projects: [
      {
        test: {
          name: 'unit',
          environment: 'node',
          include: ['web/**/*.test.ts'],
          exclude: ['web/**/*.browser.test.ts'],
        },
      },
      {
        test: {
          name: 'browser',
          include: ['web/**/*.browser.test.ts'],
          fileParallelism: false,
          maxWorkers: 1,
          browser: {
            enabled: true,
            headless: true,
            provider: playwright({
              launchOptions: { args: ['--single-process'] },
            }),
            instances: [{ browser: 'chromium' }],
          },
        },
      },
    ],
  },
})
~~~

Keep existing named projects; add editor tests to them or add one named editor project without removing Phase 1 coverage. Browser files remain serialized in the workspace-compatible real Chromium process.

**Responsive/localized accessibility matrix** — <code>web/tests/accessibility.browser.test.ts:19-58</code>:

~~~typescript
for (const locale of ['uk', 'en'] as const) {
  for (const width of [320, 1280] as const) {
    test(
      'localized lifecycle remains semantic and unclipped',
      async () => {
        await page.viewport(width, 900)
        // mount, assert lang/landmarks/names/status
      },
    )
  }
}
~~~

Copy the locale × viewport loop, native landmark/name assertions, one-live-region assertions, 44px target checks, focus-ring computed style, and no-horizontal-overflow helpers. Add semantic document, field projection, toolbar mixed state, dialogs, table, image, page break, and one-copy composition assertions.

**Real vertical lifecycle** — <code>web/tests/walking-skeleton.browser.test.ts:89-169</code> proves real WASM + IndexedDB create/apply/reject/undo/redo/save/reload/recover with revision/hash invariants and privacy-safe audit. Phase 2 tracer should use this exact full boundary, not a mocked controller-only route.

**Asset cold reopen** — <code>walking-skeleton.browser.test.ts:210-292</code> is the closest asset durability analog. Replace/extend its number-array-only expectation with both legacy read compatibility and the new binary envelope.

---

### Dependency, boundary, and phase-gate scripts

**Applies to:** manifests, provenance config, <code>verify-phase2-dependencies.mjs</code>, <code>verify-wasm-size.mjs</code>, <code>check-phase2.mjs</code>, and both boundary contract files.

**Fail-fast gate pattern** — <code>scripts/check-phase1.mjs:24-106</code>, <code>scripts/check-phase1.mjs:166-190</code>:

~~~javascript
export const phaseOneSteps = Object.freeze([
  step('dependency-provenance', 'Dependency provenance verifier tests', process.execPath, [
    '--test',
    'scripts/verify-dependency-provenance.mjs',
  ]),
  step('rust-clippy', 'Rust Clippy', cargoBinary, [
    'clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings',
  ], { env: rustEnvironment }),
  step('rust-tests', 'Full Rust tests', cargoBinary, [
    'test', '--locked', '--workspace', '--all-targets',
  ], { env: rustEnvironment }),
])

for (const gate of phaseOneSteps) {
  const result = runStep(gate, spawn, output)
  if (result.exitCode !== 0) return result.exitCode
}
~~~

Build <code>check-phase2.mjs</code> as a strict superset that invokes or embeds the complete Phase 1 gate, then Phase 2 focused/full lanes. Preserve pinned Rust environment, <code>shell: false</code>, no watch mode, fail-fast exit propagation, required-evidence preflight, and two deterministic replay rounds.

**Provenance verifier style** — <code>verify-dependency-provenance.mjs:8-66</code> normalizes only credential-free HTTPS GitHub identities and exact npm registry tarballs. Its request layer at <code>verify-dependency-provenance.mjs:68-149</code> uses AbortController timeouts and retries only transient HTTP statuses. Extend its exact-source/integrity/deprecation/lifecycle checks; do not create a looser “latest version” installer.

**Static boundary scanner style** — <code>tests/contracts/phase1-boundary.test.mjs:247-382</code>:

~~~javascript
function boundaryDiagnostics(snapshot) {
  const diagnostics = []
  validatePackageManifest(snapshot.packageJson, diagnostics)
  const typescriptProgram = parseTypeScriptProgram(snapshot.typescript)
  for (const [path] of snapshot.typescript) {
    validateTypeScript(
      path,
      typescriptProgram.sources.get(path),
      diagnostics,
      capabilities,
    )
  }
  validateWasmBoundary(snapshot.wasmSource, diagnostics)
  return diagnostics.sort()
}
~~~

Copy the TypeScript AST and parsed Rust-WASM export checks, not regex-only source matching. Continue rejecting semantic JSON parsing, semantic state assignment, unsafe DOM assignment, canonical hashing in TypeScript, and mutable WASM exports.

**Required Phase 1 contract adjustment:** <code>phase1-boundary.test.mjs:22-61</code> currently marks React, Vite, every <code>editor</code> path, and JSX as deferred. Phase 2 cannot both add the approved editor and run the Phase 1 regression gate unless these checks become phase-aware. Preserve the existing negative fixture tests, but scope Phase 1-only exclusions to a synthetic Phase 1 fixture/baseline and let the checked-in Phase 2 workspace use:

- exact allowlisted React/Vite packages and editor paths;
- no TypeScript canonical mutation/JSON interpretation;
- no mutable WASM handles;
- no contenteditable/innerHTML document truth;
- no canvas/layout/PDF/voice/backend scope.

Add the new allowed WASM export names explicitly, following <code>phase1-boundary.test.mjs:8-21</code>. Keep the gate-contract assertions at <code>phase1-boundary.test.mjs:921-950</code>.

## Shared Patterns

### Rust is the semantic and validation authority

**Sources:** <code>model/mod.rs:60-73</code>, <code>schema/mod.rs:275-385</code>, <code>transaction/mod.rs:328-339</code>, <code>lib.rs:440-476</code>.  
**Apply to:** every document/session/formatting/structure/asset command and every WASM export.

- Closed serde types with unknown-field rejection.
- Stable exhaustive error codes.
- Candidate mutation followed by full schema validation.
- No DOM paths, CSS strings, JSON-owned TS tree, or browser-created canonical values.

### Accepted-state publication

**Sources:** <code>foundation-inspector.ts:619-650</code>, <code>indexeddb-store.ts:701-785</code>.  
**Apply to:** editor controller, external store, undo/redo, IME commit, formatting, structures, and staged assets.

The universal sequence is:

~~~text
physical event/control
  -> closed Rust command
  -> validated operations + exact inverses + editor DTO + commit proof
  -> Rust persistence plan
  -> one guarded IndexedDB transaction
  -> Rust recovery/query verification
  -> publish one cached immutable React snapshot
~~~

On any rejection, retain the prior document, selection, formatting projection, revision, and history.

### Error handling and privacy

**Sources:** <code>transaction/mod.rs:341-395</code>, <code>indexeddb-store.ts:982-994</code>, <code>foundation-inspector.ts:790-800</code>.  
**Apply to:** core services, WASM, controller, storage, and tests.

Use typed errors carrying stable codes, translate only to localized visible consequence/next-action text at the UI edge, and never include authored text, IME candidates, alt text, raw image bytes, canonical JSON, or command payloads in errors/audit diagnostics.

### Localization parity

**Sources:** <code>uk.ts:1-112</code>, <code>en.ts:1-112</code>.  
**Apply to:** every editor control, status, error, capability reason, dialog, field review item, and shortcut description.

Ukrainian defines the key union; English must satisfy it at compile time. Browser tests render both locales.

### Atomicity and idempotence

**Sources:** <code>store/mod.rs:620-780</code>, <code>indexeddb-store.test.ts:147-190</code>, <code>indexeddb-store.test.ts:515-631</code>.  
**Apply to:** all document commits, migration commits, staged-asset redemption, exact retries, cold open, and recovery.

Test committed, exact retry, conflicting identity, partial record set, late-write abort, and interruption at each persistence stage.

### Test layering

**Sources:** <code>vitest.config.ts:4-61</code>, <code>migration_golden.rs:16-124</code>, <code>transaction_properties.rs:1-41</code>, <code>walking-skeleton.browser.test.ts:89-169</code>.  
**Apply to:** every planner task.

- Rust unit/property/golden tests prove semantics and algebra.
- WASM boundary tests prove deserialization and closed DTOs.
- Vitest unit tests prove controller/store identity and event translation.
- Real Chromium tests prove native input, semantics, focus, responsive behavior, and persistence.
- Standalone Playwright CDP proves the target-browser IME path.
- Full gate reruns Phase 1 and deterministic replay twice.

## No Exact Analog Found

These files have no close implementation analog; the planner must use the locked CONTEXT/UI-SPEC and RESEARCH patterns, while still applying shared repository conventions.

| File | Role | Data Flow | Reason / Required Source |
|---|---|---|---|
| <code>web/src/editor/input-adapter.ts</code> and <code>selection-bridge.ts</code> | utility/controller | event-driven/transform | No controlled IME or DOM↔logical selection bridge exists. Use RESEARCH Patterns 7–8 and Rust anchor validation; inspector keyboard/focus code is only a shell pattern. |
| <code>scripts/verify-ime-chromium.mjs</code> | browser verification | event-driven | No CDP IME lane exists. Use official Playwright/CDP calls from RESEARCH and the terminal/fail-fast style of existing scripts. |
| <code>fixtures/unicode/17.0.0/GraphemeBreakTest.txt</code> | conformance corpus | batch | No Unicode corpus exists. Vendor the exact official bytes and adjacent checksum/data-version metadata; do not hand-author cases as a substitute. |

Partial-only analogs that require special care:

- <code>asset/mod.rs</code>: hashing and atomic asset storage exist, but PNG/JPEG bounded decoding and one-use revision-bound receipt staging do not.
- Reversible subtree deletion: current `NodeInvalidated` and replay checks exist, but deterministic tombstones plus bounded owner-keyed private anchor preimages do not.
- React editor components and <code>useSyncExternalStore</code>: native semantic DOM/accessibility patterns exist, but the repository currently has no React implementation.
- <code>vite.config.ts</code>: current build and Vitest configs show pinned deterministic conventions, but no Vite application config exists.
- <code>fixtures/indexeddb/phase1-v3-*.json</code>: current tests build records in code; a durable checked-in cold-open fixture format must be chosen and versioned.

## Planner Warnings

1. **Do not add schema v2 and later “come back” for v1 types.** Freeze private v0/v1 decoders and goldens before the public model changes.
2. **Do not publish the React snapshot before IndexedDB completion and Rust re-query.** The existing inspector already demonstrates the correct boundary.
3. **Do not put image bytes in semantic JSON commands.** Use the dedicated bounded binary staging seam and redeem into the atomic commit.
4. **Do not store pending typing attributes or selection in FlowDocument.** They are Rust-owned, nonpersistent session state.
5. **Do not weaken Phase 1 ownership checks just to admit React/Vite.** Make the contract phase-aware and retain all semantic/unsafe-DOM/mutable-WASM prohibitions.
6. **Do not claim QUAL-04 fully closed from Chromium automation.** The Edge/Windows/screen-reader checkpoint remains explicitly outstanding; local VoiceOver is fallback evidence only.

## Metadata

**Analog families:**

1. Rust closed schema/migration/anchor/transaction/result/store.
2. Narrow immutable <code>flow-wasm</code> request-response boundary.
3. Rust-planned guarded IndexedDB atomic persistence.
4. Foundation Inspector controller/native semantics/i18n/CSS.
5. Phase 1 Rust/Vitest/Playwright/provenance/boundary/full-gate tests.

**Search scope:** <code>crates/flow-core/src</code>, <code>crates/flow-core/tests</code>, <code>crates/flow-wasm/src</code>, <code>web/src</code>, <code>web/persistence</code>, <code>web/tests</code>, <code>scripts</code>, <code>tests/contracts</code>, root manifests/config, and checked-in fixtures.  
**Pattern extraction date:** 2026-08-26  
**Source edits:** none; this map is the only file written.

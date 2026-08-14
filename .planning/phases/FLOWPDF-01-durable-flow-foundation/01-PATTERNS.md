# Phase 1: Durable Flow Foundation - Pattern Map

**Mapped:** 2026-08-14  
**Files analyzed:** 29 planned files/groups  
**Analogs found:** 0 / 29

## Scope Finding

This is a greenfield repository. The only tracked project material is `AGENTS.md` and `.planning/`; there are no Cargo manifests, Rust modules, TypeScript modules, browser assets, or tests. Therefore **no existing code analog exists** for any Phase 1 implementation file. The patterns below are the new canonical patterns prescribed by the locked decisions and research; they are not presented as pre-existing code.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| `Cargo.toml`, `rust-toolchain.toml` | config | batch | none | no existing analog |
| `crates/flow-core/Cargo.toml`, `crates/flow-core/src/lib.rs` | config / public API | request-response | none | no existing analog |
| `crates/flow-core/src/model/mod.rs` | model | transform | none | no existing analog |
| `crates/flow-core/src/schema/mod.rs` | service | transform | none | no existing analog |
| `crates/flow-core/src/canonical/mod.rs` | utility | transform | none | no existing analog |
| `crates/flow-core/src/anchor/mod.rs` | model / utility | transform | none | no existing analog |
| `crates/flow-core/src/transaction/mod.rs` | service | request-response | none | no existing analog |
| `crates/flow-core/src/store/mod.rs` | service / port | CRUD | none | no existing analog |
| `crates/flow-core/tests/{schema,migration_golden,persistence_round_trip,recovery,provenance,transaction_properties,preconditions,audit_redaction}.rs` | test | batch | none | no existing analog |
| `crates/flow-wasm/Cargo.toml`, `crates/flow-wasm/src/lib.rs` | adapter / provider | request-response | none | no existing analog |
| `package.json`, `package-lock.json`, `vitest.config.ts` | config | batch | none | no existing analog |
| `web/index.html`, `web/src/main.ts`, `web/src/foundation-inspector.ts` | component / controller | event-driven | none | no existing analog |
| `web/src/styles.css` | component styling | transform | none | no existing analog |
| `web/src/i18n/{uk,en}.ts` | utility / config | transform | none | no existing analog |
| `web/persistence/indexeddb-store.ts` | adapter / service | CRUD | none | no existing analog |
| `web/tests/{indexeddb-store,foundation-inspector}.test.ts` | test | event-driven | none | no existing analog |
| `web/tests/recovery.browser.test.ts` | test | event-driven | none | no existing analog |
| `fixtures/flowdoc/*`, `fixtures/recovery/*` | fixture | file-I/O | none | no existing analog |

## Pattern Assignments

### Workspace and manifests

**Files:** `Cargo.toml`, `rust-toolchain.toml`, `crates/flow-core/Cargo.toml`, `crates/flow-wasm/Cargo.toml`, `package.json`, `vitest.config.ts`  
**Analog:** no existing analog.

**New canonical pattern:** create a workspace-only root manifest with `flow-core` and `flow-wasm` members. Pin the exact Rust toolchain/components/WASM target in `rust-toolchain.toml`; lock Rust and npm dependencies. Keep the WASM crate as a boundary crate only. Before adding externally resolved Cargo or npm dependencies, include the required human legitimacy checkpoints from research (`01-RESEARCH.md:142`).

### Canonical FlowDocument model

**Files:** `crates/flow-core/src/model/mod.rs`, `crates/flow-core/src/canonical/mod.rs`  
**Analog:** no existing analog.

**New canonical pattern** (locked boundaries, `01-CONTEXT.md:16-22`; research `01-RESEARCH.md:211-218`):

```rust
// Boundary shape, not existing source code.
pub struct FlowDocument {
    pub schema_version: SchemaVersion,
    pub document_id: DocumentId,
    pub revision: Revision,
    // page settings, locale, styles, ordered content, assets, fields, provenance
}

pub fn canonical_json(document: &FlowDocument) -> Result<Vec<u8>, CoreError>;
pub fn canonical_hash(bytes: &[u8]) -> HashEnvelope;
```

Use typed structs in declaration order, `Vec` for ordered collections, and `BTreeMap` only where a map is unavoidable. Serialize one compact UTF-8 JSON form and hash those exact bytes with versioned BLAKE3 metadata. Never use `HashMap`, JSON floats, pretty/transport serialization, page/PDF/DOM coordinates, or embedded duplicate asset bytes.

### Schema validation and migrations

**Files:** `crates/flow-core/src/schema/mod.rs`, `fixtures/flowdoc/*`, `crates/flow-core/tests/{schema,migration_golden}.rs`  
**Analog:** no existing analog.

**New canonical pattern** (research `01-RESEARCH.md:220-224`):

```rust
pub trait Migration {
    fn from_version(&self) -> SchemaVersion;
    fn to_version(&self) -> SchemaVersion;
    fn migrate(&self, document: FlowDocument) -> Result<FlowDocument, CoreError>;
}
```

Migrations are pure, sequential one-version hops: no I/O, clock, randomness, or mutable global state. Validate after every hop; fail explicitly for future/unknown versions. Golden fixtures assert original bytes, migrated bytes, and expected hash. A successful migration creates a current-version snapshot/replay boundary while historical records stay inspectable.

### Anchors and transactional command service

**Files:** `crates/flow-core/src/anchor/mod.rs`, `crates/flow-core/src/transaction/mod.rs`, `crates/flow-core/tests/{transaction_properties,preconditions}.rs`  
**Analog:** no existing analog.

**New canonical pattern** (locked decisions D-05--D-11; research `01-RESEARCH.md:226-236`):

```rust
pub struct Anchor { pub node_id: NodeId, pub utf16_offset: Utf16Offset, pub affinity: Affinity }
pub struct Command { pub command_id: CommandId, pub base_revision: Revision, pub modality: SourceModality }
pub struct Transaction {
    pub command_id: CommandId, pub base_revision: Revision, pub new_revision: Revision,
    pub forward: Vec<Operation>, pub inverse: Vec<Operation>, pub anchor_mapping: AnchorMapping,
}

pub fn apply(document: &DocumentState, command: Command) -> Result<AppliedTransaction, CommandError>;
```

Validate every precondition against the untouched state; derive forward operations, inverse operations, and anchor mappings from that state; only then publish a new immutable revision. Structured errors for stale revision, invalid ID/range, duplicate command ID, invalid UTF-16 surrogate boundary, broken invariant, or deleted-unmapped anchor leave document bytes, revision, and history untouched. Undo/redo use the same command-to-transaction path and each receives a new revision/audit entry—never direct document mutation or command replay against a later state.

### Store port, recovery, and audit

**Files:** `crates/flow-core/src/store/mod.rs`, `crates/flow-core/tests/{persistence_round_trip,recovery,provenance,audit_redaction}.rs`, `fixtures/recovery/*`  
**Analog:** no existing analog.

**New canonical pattern** (locked D-12--D-15; research `01-RESEARCH.md:238-248`):

```rust
pub trait DocumentStore {
    fn commit(&mut self, records: CommitRecords) -> StoreResult<()>;
    fn recover(&self, document_id: DocumentId) -> StoreResult<RecoveredDocument>;
}
```

The port owns snapshot/log/audit record contracts and verified recovery; storage adapters own only physical I/O. Persist canonical snapshots plus append-only transaction records. Recovery selects the newest valid snapshot, verifies hash/revision, then replays only a contiguous schema-compatible chain. An already-applied identical record may be skipped idempotently; a gap, conflicting duplicate, or failed hash returns a structured diagnostic—never repair-by-guessing. Keep transaction records (which may require text) distinct from redacted audit records; audit DTOs allow only IDs, revision links, timestamp, category, modality, outcome, safe code, and allowlisted metadata.

### WASM boundary and IndexedDB adapter

**Files:** `crates/flow-wasm/src/lib.rs`, `web/persistence/indexeddb-store.ts`, `web/tests/indexeddb-store.test.ts`, `web/tests/recovery.browser.test.ts`  
**Analog:** no existing analog.

**New canonical pattern** (research `01-RESEARCH.md:72-80`, `238-248`): WASM exposes typed command input, serialized snapshot/transaction/audit DTOs, queries, and recovery results; it must never expose a mutable document object. TypeScript converts those DTOs to/from browser IndexedDB and does not interpret document semantics.

For each durable commit, enqueue snapshot/log/audit requests in one short read-write IndexedDB transaction with no unrelated `await` between requests; report success only after `IDBTransaction.complete`. The browser adapter translates quota/platform errors into the core's stable store error DTO. Fake-IDB tests cover adapter paths; a real Chromium/Edge test covers reload/crash windows.

### Foundation Inspector browser harness

**Files:** `web/index.html`, `web/src/main.ts`, `web/src/foundation-inspector.ts`, `web/src/styles.css`, `web/src/i18n/{uk,en}.ts`, `web/tests/foundation-inspector.test.ts`  
**Analog:** no existing analog.

**New canonical pattern** (`01-UI-SPEC.md:16-66`, `118-149`): minimal TypeScript, semantic HTML, manual CSS custom properties, and native controls—no React/Vite/component library in this phase. The controller is event-driven: every mutating action creates a fresh typed command using the displayed revision and awaits the core/store DTO; it never updates revision, hash, history, audit, or document summary optimistically.

Use `header`, `main`, labelled command `section`, labelled `aside`, native `button`, `aria-live="polite"` for progress/success, and `role="alert"` for conflict/failure. Ukrainian copy lives behind English-ready keys and `lang="uk"`; audit rendering accepts only redacted DTO fields. Preserve failure atomicity in the view; use the exact responsive, focus, color, and 44px target contracts in `01-UI-SPEC.md`.

## Shared Patterns

### Boundary ownership

**Source:** `01-CONTEXT.md:25-41`, `01-RESEARCH.md:72-80`  
**Apply to:** every Rust, WASM, and web file.

Rust owns semantic model, validation, transactions, migrations, recovery, and redaction. WASM is a typed boundary. TypeScript owns browser I/O and inspector presentation only. No adapter directly mutates `FlowDocument`.

### Determinism and atomic failure

**Source:** `01-CONTEXT.md:16-34`, `01-RESEARCH.md:211-236`  
**Apply to:** model, schema, canonical, anchors, transactions, fixtures, and tests.

Use stable IDs, ordered structures, canonical bytes, explicit revision preconditions, and structured error codes. On every invalid input, retain the pre-command state exactly; do not silently rebase, normalize, or retarget anchors.

### Privacy-minimized observability

**Source:** `01-CONTEXT.md:38-41`, `01-UI-SPEC.md:58-66`  
**Apply to:** store, WASM DTOs, IndexedDB, inspector, and tests.

Audit is an allowlisted, redacted projection—not a transaction-log mirror. Do not serialize or expose text, command arguments, transcripts, raw audio, storage payloads, or debug dumps in audit/UI output.

### Test-first contract coverage

**Source:** `01-RESEARCH.md:326-350`  
**Apply to:** every new behavior.

Use Rust golden/property/integration tests for model contracts and Vitest fake-IDB plus browser recovery tests for adapter/UI contracts. Fixtures include Ukrainian/English text, combining marks, emoji, non-BMP text, old schemas, canonical hashes, corruption, gaps, and crash windows.

## No Analog Found

All Phase 1 files have no close codebase analog because the application does not exist yet. The planner should adopt the new canonical patterns above and treat the cited phase documents as the authoritative contracts, not as code to emulate.

## Metadata

**Analog search scope:** full repository excluding `.git`; planning files and project instructions  
**Application source files scanned:** 0  
**Pattern extraction date:** 2026-08-14

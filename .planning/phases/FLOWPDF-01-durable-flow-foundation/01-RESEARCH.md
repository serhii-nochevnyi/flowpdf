# Phase 1: Durable Flow Foundation - Research

**Researched:** 2026-08-14  
**Domain:** Rust-owned canonical document model, transactions, migrations, and IndexedDB durability  
**Confidence:** MEDIUM — the architecture and library APIs were documented, but this environment cannot currently resolve package registries or run the missing Rust/browser toolchains.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

### Canonical Schema and Assets

- **D-01:** Canonical v1 representation is human-inspectable JSON with deterministic serialization, an explicit `schemaVersion`, stable UUID-compatible node/asset/field IDs, and no page/PDF/DOM coordinates. — **Reversibility:** costly — changing a published canonical representation requires migrations for stored documents and owned-PDF source payloads.
- **D-02:** Assets are referenced by content hash and metadata rather than embedded repeatedly in semantic nodes; physical package or storage adapters resolve bytes.
- **D-03:** `.flowdoc` packaging and compact binary representations are derived encodings. The in-memory and conformance truth remains the canonical semantic model.
- **D-04:** Schema evolution uses a sequential registry of pure migrations. Unknown future versions fail explicitly; migrations are covered by golden fixtures. — **Reversibility:** one-way — shipped migration chains become a persistent document compatibility contract.

### Transactions and Revisions

- **D-05:** Every mutation enters through a typed command and produces one atomic immutable transaction with `commandId`, `baseRevision`, ordered forward operations, ordered inverse operations, source modality, and a new revision.
- **D-06:** UI, keyboard, future voice, and API inputs share the same command/transaction boundary. No adapter mutates FlowDocument directly. — **Reversibility:** costly — later phases and public bindings will compile against this contract.
- **D-07:** A stale `baseRevision`, invalid target ID, invalid range, duplicate command ID, or violated invariant returns a structured error and leaves the document unchanged. Phase 1 does not silently rebase or guess targets.
- **D-08:** Undo and redo are transactions themselves, maintain deterministic revision history, and must restore canonical semantic hashes for reversible command sequences.

### Logical Anchors

- **D-09:** Persistent positions use `nodeId + UTF-16 offset + affinity` at the public/browser boundary. Index types remain explicit internally; later layout phases enforce grapheme-safe editing. — **Reversibility:** costly — comments, fields, selections, voice targets, source maps, and WASM bindings will depend on this position contract.
- **D-10:** Transactions emit explicit anchor mappings for surviving positions. If a target is deleted without a defined mapping, the anchor becomes invalid rather than moving to guessed nearby content.
- **D-11:** DOM paths, absolute document character indices, page rectangles, and PDF coordinates are prohibited as canonical anchors.

### Durability and Audit

- **D-12:** `DocumentStore` is a core port. It persists canonical snapshots and an append-only transaction log; storage implementations are adapters.
- **D-13:** The first browser adapter uses IndexedDB. Recovery loads the newest valid snapshot, verifies hashes and revisions, and replays subsequent valid transactions idempotently.
- **D-14:** Snapshot cadence is policy-controlled and benchmarked; correctness cannot depend on a specific interval.
- **D-15:** Audit records include IDs, revision links, timestamp, command type, source modality, outcome, and explicitly redacted metadata. Raw microphone audio and document text are not retained in default audit/analytics records.

### the agent's Discretion

- Exact Rust module boundaries inside the Phase 1 workspace, provided public contracts preserve the model/transaction/store separation.
- Concrete canonical JSON key ordering implementation and hash algorithm, provided they are documented, deterministic, tested, and not cryptographic-authentication claims.
- Snapshot compaction cadence and IndexedDB object-store names.
- Error enum names and diagnostic wording, provided machine-readable codes and atomic failure behavior remain stable.

### Deferred Ideas (OUT OF SCOPE)

- Rich-text input, IME and accessibility UI — Phase 2.
- Grapheme segmentation, shaping, line breaking and pagination — Phase 3.
- Compact binary FlowDocument encoding and ZIP package optimization — evidence-driven later work.
- Remote storage, authentication and multi-device synchronization — future service milestone.
- CRDT real-time collaboration — v2 collaboration work.
</user_constraints>

## Project Constraints (from AGENTS.md)

- Follow the GSD workflow for repository changes; this research artifact is the requested planning-gate output. [VERIFIED: AGENTS.md:128-140]
- Rust owns canonical model, transactions, layout, PDF syntax, reading, and writing; TypeScript/React are browser-shell and adapter layers. [VERIFIED: AGENTS.md:15-17]
- Keep the core native-and-WASM capable; reuse vetted low-level primitives and do not add a commercial PDF SDK runtime dependency. [VERIFIED: AGENTS.md:16-19]
- Preserve Ukrainian and English as first-class text, deterministic pinned inputs, hostile-input security boundaries, and explicit preservation of unsupported structure. [VERIFIED: AGENTS.md:20-26]

## Summary

Phase 1 should be a small, Rust-first workspace that establishes one canonical FlowDocument representation, a command-to-immutable-transaction application service, and an adapter-neutral durability protocol. The public/WASM boundary must expose only typed commands, serialized snapshots, transaction/audit records, and query results; it must never expose a mutable document object. This directly preserves the locked command boundary and makes later UI, keyboard, voice, API, layout, and PDF phases consumers rather than alternate mutation paths. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:25-28]

Use typed Rust structs, ordered collections, and `serde_json` default sorted maps to generate canonical UTF-8 JSON; hash exactly those canonical bytes with BLAKE3. Do not normalize user text, use `HashMap`, accept JSON float values, or hash pretty/transport-specific encodings. The hash is an equality/integrity check for stored records, not authentication or tamper protection. Serde supports typed serialization/custom serializers, while `serde_json::Value` uses sorted `BTreeMap` object keys by default. [CITED: https://serde.rs/] [CITED: https://docs.rs/serde_json/latest/serde_json/enum.Value.html]

The browser store should be a thin IndexedDB adapter around a core `DocumentStore` record protocol. Each durable command writes its append-only transaction record and success audit record in one short read-write IndexedDB transaction, then the adapter reports success only after transaction completion. Recovery must select the highest *valid* snapshot by revision, validate its canonical hash, then replay only contiguous, hash-linked records; it must stop with a structured recovery error at the first invalid gap or conflict. IndexedDB commits only on transaction completion and transactions can become inactive after an unrelated event-loop await. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/IDBTransaction/complete_event] [CITED: https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API/Using_IndexedDB]

**Primary recommendation:** Plan the work in four sequential slices: workspace/toolchain and pure model; canonical schema/migrations; command/transaction/anchor/history; then the IndexedDB durability adapter plus recovery/audit browser tests. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:77-96]

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|---|---|---|---|
| FlowDocument schema, validation, migration, canonical bytes | Rust core | WASM boundary | Canonical rules must be native/browser-identical and cannot depend on DOM state. [VERIFIED: AGENTS.md:16-17] |
| Command preconditions, operations, inverse operations, undo/redo, anchor mapping | Rust application core | WASM boundary | All future input modalities must share this mutation boundary. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:25-34] |
| Snapshot/log/audit record contract and recovery algorithm | Rust application core | Browser storage adapter | The port and validation stay in core; only physical I/O is adapter-specific. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:38-41] |
| IndexedDB records, quota/error translation, worker-refresh simulation | Browser / client | Rust WASM bridge | IndexedDB is a browser platform API; it must not define document semantics. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:38-40] |
| Audit inspection DTO | Rust application core | Future UI | Redaction occurs before the UI sees audit metadata; no raw text/audio becomes a presentation concern. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:41-41] |

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|---|---|---|
| FLOW-01 | User can create a new versioned FlowDocument with page settings, locale, styles, content, assets, and fields. [VERIFIED: .planning/REQUIREMENTS.md:18-18] | Typed root schema, opaque UUID-compatible IDs, asset descriptors plus blob resolution, and a minimal typed field envelope. |
| FLOW-02 | User can save a FlowDocument and reopen it without semantic, style, field, or asset loss. [VERIFIED: .planning/REQUIREMENTS.md:19-19] | Canonical-byte fixture/hash tests plus separate content-addressed asset-blob persistence. |
| FLOW-03 | User can open a document created by an older supported schema version through deterministic migrations. [VERIFIED: .planning/REQUIREMENTS.md:20-20] | Sequential pure migration registry, one golden fixture per supported version, and a post-migration fresh snapshot. |
| FLOW-04 | User can recover the last durable document revision after an editor refresh or worker failure. [VERIFIED: .planning/REQUIREMENTS.md:21-21] | IndexedDB snapshot/log consistency protocol, contiguous replay, corruption/gap diagnostics, and crash-window tests. |
| FLOW-05 | User can inspect the document revision and conversion provenance used for a preview or export. [VERIFIED: .planning/REQUIREMENTS.md:22-22] | Immutable revision/provenance DTO; Phase 1 records created/migrated lineage and reserves exporter provenance without fabricating an export. |
| EDIT-06 | User can undo and redo every committed content, style, structure, and field operation as an atomic transaction. [VERIFIED: .planning/REQUIREMENTS.md:31-31] | Ordered forward/inverse operations, history cursor, and canonical-hash round-trip properties. |
| EDIT-07 | User receives a deterministic conflict instead of a misplaced edit when a command targets a stale revision or invalid anchor. [VERIFIED: .planning/REQUIREMENTS.md:32-32] | Revision preconditions, UTF-16-to-internal-boundary validation, explicit invalid anchors, and no-rebase rule. |
| QUAL-08 | User can inspect a concise audit history of document mutations without raw microphone audio or sensitive transcript analytics being retained by default. [VERIFIED: .planning/REQUIREMENTS.md:103-103] | A separate redacted audit DTO/record and tests proving text/audio keys cannot be serialized into audit output. |
</phase_requirements>

## Standard Stack

### Core

| Library / tool | Version policy | Purpose | Why standard |
|---|---|---|---|
| Rust stable + Cargo | Install current stable, immediately record the exact version in `rust-toolchain.toml`; lock dependencies in `Cargo.lock`. [CITED: https://www.rust-lang.org/tools/install/] | Native core, tests, and WASM compilation. | Project policy requires a pinned Rust core targeting native and WASM. [VERIFIED: AGENTS.md:16-16] |
| `serde` + `serde_json` [WARNING: flagged as suspicious — verify before using.] | Lockfile-pinned; select the registry release during Wave 0 after the required human legitimacy check. [CITED: https://serde.rs/] | Typed JSON schema, migrations, canonical serialization. | Derive/custom serialization support; default JSON objects are sorted when represented as `Value`. [CITED: https://docs.rs/serde_json/latest/serde_json/enum.Value.html] |
| `uuid` with `v4` feature [WARNING: flagged as suspicious — verify before using.] | Lockfile-pinned; Context7 documents `uuid` 1.24 feature examples. [CITED: https://github.com/uuid-rs/uuid] | Opaque stable IDs for document/node/asset/field/command identities. | Version-4 IDs are random via the OS RNG and do not embed a timestamp. [CITED: https://github.com/uuid-rs/uuid] |
| `blake3` [WARNING: flagged as suspicious — verify before using.] | Lockfile-pinned. [CITED: https://docs.rs/blake3/latest/blake3/] | Hash canonical snapshot bytes, transaction bytes, and asset bytes. | Official Rust API hashes byte slices and renders stable hex output. [CITED: https://docs.rs/blake3/latest/blake3/fn.hash.html] |
| `thiserror` [WARNING: flagged as suspicious — verify before using.] | Lockfile-pinned; Context7 documents `thiserror` 2.0.18. [CITED: https://github.com/dtolnay/thiserror] | Ergonomic Rust error display/source composition. | Keep the project-owned machine-readable error-code enum separate from diagnostic text. [CITED: https://github.com/dtolnay/thiserror] |
| `proptest` (dev-dependency) [WARNING: flagged as suspicious — verify before using.] | Lockfile-pinned. [CITED: https://github.com/proptest-rs/proptest] | Stateful/property test command sequences and regression shrinking. | It shrinks failures and persists reproducing cases for source control. [CITED: https://github.com/proptest-rs/proptest] |
| `wasm-bindgen` + `serde-wasm-bindgen` [WARNING: flagged as suspicious — verify before using.] | Lockfile-pinned, boundary crate only. [CITED: https://rustwasm.github.io/docs/wasm-bindgen/] | Future JavaScript ABI and structured Rust/JS values. | wasm-bindgen supports exported Rust functions/structs; serde-wasm-bindgen converts Serde values to `JsValue`. [CITED: https://rustwasm.github.io/docs/wasm-bindgen/reference/arbitrary-data-with-serde.html] |

### Supporting

| Library / tool | Version policy | Purpose | When to use |
|---|---|---|---|
| `vitest` [WARNING: flagged as suspicious — verify before using.] | Pin 4.1.6 from the current official Context7 result, then lock it in `package-lock.json`. [CITED: https://github.com/vitest-dev/vitest/blob/v4.1.6/docs/guide/learn/writing-tests.md] | TypeScript adapter and browser-facing contract tests. | Use for IndexedDB adapter unit tests and a separate browser project. |
| `fake-indexeddb` (dev-dependency) [WARNING: flagged as suspicious — verify before using.] | Lockfile-pinned. [CITED: https://github.com/dumbmatter/fakeindexeddb] | Deterministic Node test double for IndexedDB recovery/error paths. | Use only in unit tests; run one real-browser IndexedDB recovery suite too. |
| `@vitest/browser-playwright` (dev-dependency) [WARNING: flagged as suspicious — verify before using.] | Lockfile-pinned, with the compatible Vitest 4.1.6 release. [CITED: https://vitest.dev/guide/browser/] | Chromium browser test provider. | Use when the Phase 1 browser fixture proves the actual IndexedDB transaction/reload path. |
| `rustfmt` and Clippy rustup components | Same pinned Rust toolchain. [CITED: https://doc.rust-lang.org/stable/clippy/installation.html] | Formatting and lint gate. | Run on every Rust change. |

### Do not add in Phase 1

- Do not add React, Vite, layout/shaping, PDF, form-rendering, CRDT, remote-storage, or font packages. They are outside the defined phase boundary. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:9-9]
- Do not add an IndexedDB wrapper at runtime. The adapter needs a narrow, explicit native IndexedDB transaction implementation; an extra wrapper would not remove the transaction-lifetime correctness obligation. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API/Using_IndexedDB]

**Installation / bootstrap (first plan only):**

```bash
# Verify the installer origin interactively, install the current stable toolchain,
# then write that exact rustc version into rust-toolchain.toml.
rustup toolchain install stable
rustup component add rustfmt clippy
rustup target add wasm32-unknown-unknown

# After human legitimacy verification, install only the browser-test development dependencies.
npm install --save-dev vitest@4.1.6 fake-indexeddb @vitest/browser-playwright
```

The official wasm-bindgen guide documents adding the `wasm32-unknown-unknown` target. [CITED: https://rustwasm.github.io/docs/wasm-bindgen/contributing/index.html]

## Package Legitimacy Audit

The mandatory legitimacy seam returned `SUS` for every queried package because it could not obtain registry age, download, or repository signals in this sandbox; direct npm registry queries also returned no data and Cargo is not installed. This is an environment limitation, not an approval. The planner must include one `checkpoint:human-verify` before the grouped Cargo dependency add and one before the grouped npm development-dependency add. [VERIFIED: local `package-legitimacy` queries on 2026-08-14]

| Package | Registry | Verdict | Disposition |
|---|---|---|---|
| `serde`, `serde_json`, `uuid`, `blake3`, `thiserror`, `wasm-bindgen`, `serde-wasm-bindgen`, `proptest` | crates.io | SUS — registry signals unavailable | Flagged; verify crate owner, release, checksum, and no unexpected features before the first `cargo fetch`. |
| `vitest`, `fake-indexeddb`, `@vitest/browser-playwright` | npm | SUS — registry signals unavailable | Flagged; verify official repository, release/tag, package-lock integrity, and install scripts before installation. |

**Packages removed due to SLOP verdict:** none.  
**Packages flagged as suspicious:** all packages above, solely because the legitimacy service lacks crates/npm registry data in this environment. [VERIFIED: local `package-legitimacy` queries on 2026-08-14]

## Architecture Patterns

### System Architecture Diagram

```text
Future input adapter / test fixture
          |
          v
Typed Command { command ID, base revision, target anchors, arguments, modality }
          |
          v
Rust application service
  validate schema + revision + anchors + invariants
          | success                         | failure
          v                                 v
Immutable Transaction               structured error + redacted failure audit
  forward ops + inverse ops
  anchor mapping + new revision
          |
          v
DocumentStore port ------------------------------+
  canonical snapshot / append-only log / audit   |
          | native test adapter                   | browser adapter
          v                                       v
in-memory fixture                         short IndexedDB transaction
                                                  |
                                                  v
                                             completion -> durable acknowledgement

Recovery: newest valid snapshot -> verify canonical hash/revision -> replay contiguous records -> recovered immutable revision
```

The diagram follows the locked command, transaction, and storage boundaries; UI/voice/PDF/layout implementation remains deferred. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:9-9]

### Recommended Project Structure

```text
Cargo.toml                         # workspace only
rust-toolchain.toml                # exact stable compiler/components/wasm target
crates/
  flow-core/
    src/model/                     # FlowDocument, IDs, styles/content/assets/fields
    src/schema/                    # version registry, pure migrations, validation
    src/canonical/                 # canonical JSON bytes and hash envelope
    src/anchor/                    # public UTF-16 anchors and transformation maps
    src/transaction/               # commands, operations, apply, inverse, history
    src/store/                     # DocumentStore port, recovery protocol, audit DTO
    tests/                         # golden and property tests
  flow-wasm/
    src/lib.rs                     # narrow command/snapshot/recovery binding only
web/
  persistence/                     # native IndexedDB adapter around generated DTOs
  tests/                           # fake-IDB and Chromium recovery contract tests
fixtures/
  flowdoc/                         # current/old-schema JSON and expected canonical hashes
  recovery/                        # snapshot/log corruption and crash-window cases
```

This is a recommended structure, not an existing tree. [ASSUMED]

### Canonical document contract

1. Make `FlowDocument` the only durable semantic root. It contains an explicit schema version, document identity/revision, page settings, locale, styles, ordered content tree, ordered asset descriptors, ordered field definitions, and provenance metadata. It contains no page, DOM, or PDF coordinates. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:18-21]
2. Store asset bytes separately by their content hash. The canonical document stores only the hash and descriptive metadata; a resolver/asset store returns bytes. Verify both descriptor round-trip and byte/hash lookup in FLOW-02 tests. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:19-20]
3. Use structs (declaration-order fields), ordered `Vec` values, and `BTreeMap` whenever a map is unavoidable. Serialize compact UTF-8 canonical JSON; do not serialize through `HashMap`, `serde_json` with `preserve_order`, or a pretty-printing transport mode. `serde_json` documents sorted `BTreeMap` behavior by default and insertion-order behavior only with `preserve_order`. [CITED: https://docs.rs/serde_json/latest/serde_json/enum.Value.html]
4. Restrict numeric fields to explicitly typed finite integer/fixed-point domain types; reject duplicate IDs, duplicate map keys, invalid parent/child placement, dangling asset/field/style references, unsupported schema versions, and public UTF-16 positions inside a surrogate pair. This last boundary check is compatible with the locked later grapheme-safety work. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:32-34] [ASSUMED]
5. Hash exact canonical bytes using BLAKE3; retain algorithm/version identifiers in the persisted hash envelope. Treat a match as detection of accidental corruption or incorrect replay, not as authorization against a storage attacker. BLAKE3 exposes direct byte hashing and hex output. [CITED: https://docs.rs/blake3/latest/blake3/fn.hash.html]

### Migration contract

Use `Migration { from_version, to_version, migrate(document) }` as a pure sequential registry, with no I/O, clock, random IDs, or mutable globals. Migrate one version at a time, validate after every hop, and fail before mutation for a future/unknown version. Golden fixtures must assert original canonical bytes, migrated canonical bytes, and expected hash for every supported path. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:21-21]

After a successful migration, write a new latest-version snapshot and append a migration provenance/audit record. Keep older append-only records for inspection, but replay only records whose schema and base revision are compatible with the selected snapshot; this prevents a new runtime from applying old-schema operations with changed semantics. [ASSUMED]

### Command, transaction, and history contract

1. A command is an immutable typed request. It carries a command ID, expected base revision, source modality, and operation-specific arguments; all target positions are public logical anchors. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:25-27]
2. Validate all preconditions against an unchanged document, then calculate ordered forward operations, inverse operations from pre-mutation state, and explicit anchor mappings. Only then construct the new immutable revision. Do not implement undo by replaying a command against a later document. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:25-28]
3. Treat a duplicate command ID, stale revision, invalid target/range, broken invariant, and deleted-unmapped anchor as structured failures with no document mutation. A surviving anchor is transformed only through its emitted mapping; an unmapped deleted target is invalid, never snapped to adjacent text. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:27-34]
4. Model undo and redo as ordinary transactions that refer to the history entry they reverse/reapply and produce their own revision/audit records. Property tests must prove a reversible sequence restores the original canonical hash. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:28-28]

### Persistence and recovery contract

- Snapshot record: document ID, schema version, revision, canonical bytes/hash, provenance, creation time, and store-record format version. [ASSUMED]
- Transaction record: document ID, command ID, base/new revisions, transaction canonical bytes/hash, forward/inverse operations, anchor mapping, and schema compatibility information. It may contain document text because recovery/undo require it; it is not an audit event. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:25-25] [ASSUMED]
- Audit record: command/transaction IDs, revision link, timestamp, command category, modality, outcome, safe error code, and explicitly redacted metadata only. It must never serialize document text, command arguments, raw microphone audio, or raw voice transcript. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:41-41]
- A browser write must keep all IndexedDB requests synchronous with respect to the transaction: enqueue snapshot/log/audit operations first and wait for completion afterward. Do not await hashing, migrations, network, or UI work in the middle of an IDB transaction. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API/Using_IndexedDB]
- Recovery accepts only a contiguous revision chain from a verified snapshot. A repeated record identical to the already-applied revision/hash is idempotently skipped; a duplicate identity with different bytes, a gap, or a failed hash is a durable diagnostic and no guessed repair. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:39-40] [ASSUMED]
- Snapshot cadence is a policy object with benchmarked candidates; tests must exercise at least no-intermediate-snapshot, snapshot-before-log, and snapshot-after-log crash windows so correctness does not depend on the chosen cadence. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:40-40]

### Code example: canonical and recovery boundaries

```rust
// Pseudocode contract only; exact public Rust names remain planner discretion.
let bytes = canonical_json(&document)?;
let digest = blake3::hash(&bytes).to_hex().to_string();
store.commit(snapshot, transaction, audit).await?;
let recovered = store.recover(document_id).await?;
assert_eq!(canonical_json(&recovered.document)?, bytes);
```

The BLAKE3 call/hex conversion is documented; the surrounding application API is intentionally illustrative and must be finalized with the model/store separation. [CITED: https://docs.rs/blake3/latest/blake3/] [ASSUMED]

## Don't Hand-Roll

| Problem | Do not build | Use instead | Why |
|---|---|---|---|
| Rust data-format framework | Bespoke derive/reflection JSON framework | `serde` + `serde_json` [WARNING: flagged as suspicious — verify before using.] | Typed serialization, custom hooks, and safe deserialization are maintained primitives. [CITED: https://serde.rs/] |
| UUID syntax/random generation | Ad hoc random identifier formatter | `uuid` v4 feature [WARNING: flagged as suspicious — verify before using.] | Official crate supports UUID parsing/generation and uses OS randomness for v4. [CITED: https://github.com/uuid-rs/uuid] |
| Byte digest | Custom hash/checksum | `blake3` [WARNING: flagged as suspicious — verify before using.] | Official API covers direct/incremental hashing and stable hex representation. [CITED: https://docs.rs/blake3/latest/blake3/] |
| Property-test shrinking/replay | Home-grown random command loop | `proptest` [WARNING: flagged as suspicious — verify before using.] | It shrinks failures and writes regression cases that can be checked in. [CITED: https://github.com/proptest-rs/proptest] |
| Browser persistence | `localStorage` or an in-memory map masquerading as recovery | Native IndexedDB transaction API | IndexedDB has transactional completion/rollback semantics required for snapshot+log durability. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API/Using_IndexedDB] |

**Key insight:** FlowPDF must own its semantic rules, transaction algebra, migrations, and recovery policy; it should reuse commodity serialization, identity, hashing, test-generation, and browser-storage primitives. [VERIFIED: AGENTS.md:18-19]

## Common Pitfalls

### Canonical JSON that is merely repeatable on one machine

**What goes wrong:** `HashMap` iteration, pretty/compact choices, or generic JSON values change byte output despite equal document meaning.  
**Avoid:** typed structs, ordered vectors/maps, one compact serializer, canonical-byte golden fixtures, and hashes over those bytes only. `serde_json` makes insertion order observable when `preserve_order` is enabled, so do not enable it in the canonical path. [CITED: https://docs.rs/serde_json/latest/serde_json/enum.Value.html]

### Migration leaves transaction records semantically orphaned

**What goes wrong:** a migrated snapshot is followed by old-schema operations interpreted under a new schema.  
**Avoid:** snapshot immediately after migration, preserve prior log records as history, and replay only records compatible with the selected snapshot/schema. [ASSUMED]

### Undo creates a new edit instead of a true inverse

**What goes wrong:** inverse operations are recomputed after later edits or an adapter directly mutates the document.  
**Avoid:** derive inverse operations from the original pre-mutation state and make undo/redo normal transactions with their own revisions. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:25-28]

### UTF-16 anchor is treated as a Rust byte index

**What goes wrong:** browser boundary positions split a surrogate pair or point into an invalid UTF-8 boundary.  
**Avoid:** explicit index newtypes/conversion helpers, reject surrogate-half positions now, and leave grapheme segmentation to Phase 2/3 as locked. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:32-34] [ASSUMED]

### IndexedDB write reported before it is durable

**What goes wrong:** code resolves after `put()` requests rather than the containing transaction completion; a refresh exposes a partial snapshot/log state.  
**Avoid:** use one short read-write transaction, enqueue all requests without unrelated awaits, and await its completion. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/IDBTransaction/complete_event]

### Audit metadata becomes a shadow document store

**What goes wrong:** command arguments, text ranges, transcripts, or raw audio are copied into convenient diagnostic JSON.  
**Avoid:** separate transaction records from audit DTOs, use allowlisted metadata fields, and test serialized audit output for forbidden-content absence. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:41-41]

## Environment Availability

| Dependency | Required by | Available | Version | Fallback |
|---|---|---:|---|---|
| Node.js | browser adapter/test tooling | Yes | 24.10.0 | — |
| npm | browser test dependencies | Yes | 11.9.0 | — |
| Rust (`rustup`, `rustc`, `cargo`) | all core implementation/tests | No | — | Install via rustup in Wave 0. [CITED: https://www.rust-lang.org/tools/install/] |
| `wasm32-unknown-unknown` target | WASM boundary compile check | No (Rust absent) | — | Add with rustup after toolchain install. [CITED: https://rustwasm.github.io/docs/wasm-bindgen/contributing/index.html] |
| wasm-bindgen/wasm-pack CLI | generated web package smoke test | No | — | Defer generated-package smoke test until the crate exports a real browser API; use `cargo check --target` now. [ASSUMED] |
| Chromium/Edge/Playwright browser binary | real IndexedDB recovery test | No | — | Install compatible browser provider in Wave 0; fake-IDB unit tests do not replace final browser verification. [CITED: https://vitest.dev/guide/browser/] |

**Missing dependencies with no fallback:** Rust toolchain and a real Chromium-compatible test browser must be installed before implementation can meet the phase validation gate. [VERIFIED: local environment probe on 2026-08-14]

**Missing dependencies with fallback:** wasm-pack is not required for Phase 1 if the wasm crate is compile-checked with Cargo only. [ASSUMED]

## Validation Architecture

### Test Framework

| Property | Value |
|---|---|
| Rust framework | Built-in `cargo test` plus `proptest` for generated stateful sequences. [CITED: https://github.com/proptest-rs/proptest] |
| Browser/adapter framework | Vitest 4.1.6 plus `fake-indexeddb`; final browser project uses the Playwright provider. [CITED: https://github.com/vitest-dev/vitest/blob/v4.1.6/docs/guide/browser/index.md] |
| Config files | None exist — create Cargo workspace manifests, `vitest.config.ts`, and package manifest in Wave 0. [VERIFIED: workspace file scan on 2026-08-14] |
| Quick run | `cargo test --workspace` and `npm test -- --project unit` |
| Full suite | `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo check -p flow-wasm --target wasm32-unknown-unknown && npm test && npm test -- --project browser` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test type | Automated command | File exists? |
|---|---|---|---|---|
| FLOW-01 | Construct/validate current schema with Ukrainian text, styles, assets, and fields. | Rust unit + golden | `cargo test -p flow-core schema` | No — Wave 0 |
| FLOW-02 | Canonical save/reopen preserves semantic/style/field/asset descriptor and resolves blob hash. | Rust integration | `cargo test -p flow-core persistence_round_trip` | No — Wave 0 |
| FLOW-03 | Every old fixture migrates through sequential pure steps; future version fails. | Rust golden | `cargo test -p flow-core migration_golden` | No — Wave 0 |
| FLOW-04 | Snapshot/log crash windows recover last committed revision; corrupt/gap record fails deterministically. | Rust integration + fake-IDB + browser | `cargo test -p flow-core recovery && npm test -- --project unit && npm test -- --project browser` | No — Wave 0 |
| FLOW-05 | Inspect revision/provenance DTO after create, command, and migration. | Rust unit | `cargo test -p flow-core provenance` | No — Wave 0 |
| EDIT-06 | Generated reversible command sequences undo/redo to original canonical hash. | Rust property | `cargo test -p flow-core transaction_properties` | No — Wave 0 |
| EDIT-07 | Stale/invalid/duplicate command and invalid anchor leave canonical bytes/revision unchanged. | Rust property + unit | `cargo test -p flow-core preconditions` | No — Wave 0 |
| QUAL-08 | Audit serializer excludes document text, raw transcript/audio, and operation arguments. | Rust unit + browser store integration | `cargo test -p flow-core audit_redaction && npm test -- --project unit` | No — Wave 0 |

### Required fixtures and properties

- Create a Ukrainian/English semantic fixture containing combining marks, apostrophes, emoji, and a non-BMP character. Assert byte preservation; it is not a Phase 1 claim of grapheme-safe editing. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:95-96]
- Create one golden JSON fixture per supported schema version and expected canonical hash after migration. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:21-21]
- Generate valid command sequences and inject one stale revision, invalid target, invalid UTF-16 boundary, duplicate command ID, or invariant violation at a time; each failure must preserve the old canonical bytes, revision, and history cursor. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:27-28] [ASSUMED]
- Test crash points before transaction completion, after log append, after snapshot write, and after audit write; recover only the last transaction whose IndexedDB transaction completed. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/IDBTransaction/complete_event] [ASSUMED]

### Sampling Rate

- **Per task commit:** formatter, Clippy, focused Rust tests, and the relevant Vitest project. [CITED: https://doc.rust-lang.org/stable/clippy/installation.html]
- **Per wave merge:** full suite above plus the WASM target compile check. [CITED: https://rustwasm.github.io/docs/wasm-bindgen/contributing/index.html]
- **Phase gate:** full suite green and a manual Chrome/Edge reload/worker-failure recovery demonstration before verification. Phase configuration enables Nyquist validation. [VERIFIED: .planning/config.json:20-24]

### Wave 0 Gaps

- [ ] Install and pin Rust stable, `rustfmt`, Clippy, and the WASM target.
- [ ] Create Cargo workspace, lockfile, test/fixture directories, and a no-floating-version policy check.
- [ ] Create `package.json`, lockfile, `vitest.config.ts`, fake-IDB setup, and browser test provider configuration.
- [ ] Install a Chromium-compatible browser test runtime and record its version.
- [ ] Add the package-legitimacy human checkpoints before installing any external crate/npm package.

## Security Domain

Security enforcement is enabled at ASVS level 1. [VERIFIED: .planning/config.json:47-49]

### Applicable ASVS Categories

| ASVS category | Applies | Phase 1 control |
|---|---|---|
| V1 Architecture, Design and Threat Modeling | Yes | Keep canonical core, command service, and storage adapter separate; document the trusted/untrusted persisted-record boundary. [VERIFIED: AGENTS.md:16-19] |
| V2 Authentication | No | No account, remote backend, or identity service is in phase scope. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:9-9] |
| V3 Session Management | No | No session exists in phase scope. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:9-9] |
| V4 Access Control | No | Revision preconditions are correctness controls, not user authorization. [ASSUMED] |
| V5 Validation, Sanitization and Encoding | Yes | Byte-size gate before JSON parsing; typed schema/invariant checks; canonical serializer; reject unsupported versions/anchors. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:21-34] [ASSUMED] |
| V6 Cryptography | Limited | Use BLAKE3 only for deterministic content identity/integrity diagnostics; make no authentication, signature, encryption, or anti-tamper claim. [CITED: https://docs.rs/blake3/latest/blake3/] |
| V8 Data Protection | Yes | Redacted audit records exclude raw microphone audio and document text by default. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:41-41] |

### Security limits and threat patterns

| Pattern | STRIDE | Standard mitigation |
|---|---|---|
| Corrupt/tampered IndexedDB snapshot or log | Tampering | Hash canonical snapshot/record bytes, validate schema/revision chain, fail recovery visibly at the first bad record. Hash alone is not authorization. [CITED: https://docs.rs/blake3/latest/blake3/] [ASSUMED] |
| Oversized/cyclic/invalid persisted JSON | Denial of service / tampering | Define a checked-in `DocumentLimits` profile before decode: maximum record bytes, nesting, nodes, text length, assets, fields, log scan, and recovery work. Select numerical limits from the Phase 1 benchmark; do not invent them in this plan. [ASSUMED] |
| Anchor retargeting to unintended text | Tampering | Base-revision precondition, strict target/range validation, explicit anchor mappings, and invalid rather than guessed deleted anchors. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:27-34] |
| Privacy leak through audit | Information disclosure | Separate transaction log from redacted audit DTO; allowlist metadata and add forbidden-content serialization tests. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:41-41] |
| Partial browser write after refresh/worker failure | Denial of service / tampering | One short IndexedDB transaction for logically atomic records; acknowledge only after completion and replay a verified durable prefix. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/IDBTransaction/complete_event] [ASSUMED] |

PDF/font/image/audio hostile-input limits remain Phase 7+ work and must not be claimed complete by this phase. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:9-9]

## State of the Art

| Old approach | Current phase approach | Impact |
|---|---|---|
| DOM paths, page rectangles, or absolute indices as saved positions | Stable node ID + UTF-16 offset + affinity with explicit mappings | Enables a browser-compatible durable position independent of layout; grapheme enforcement follows later. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:32-34] |
| UI-specific mutation paths | One typed command/transaction boundary | Future keyboard/UI/voice/API behavior shares validation, undo, and audit semantics. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:25-28] |
| Whole-document local-storage save | IndexedDB snapshots plus append-only transactions with verified recovery | Supports bounded durable recovery and auditability rather than best-effort overwrite. [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:38-40] |

## Assumptions Log

| # | Claim | Section | Risk if wrong |
|---|---|---|---|
| A1 | A migrated snapshot should create a new replay boundary while old log records remain inspectable but are not applied under the new schema. | Migration contract | Recovery/history behavior could require a different migration-of-operations design. |
| A2 | Phase 1 should reject UTF-16 positions that fall inside a surrogate pair, while grapheme boundary enforcement remains later work. | Canonical/transaction contract | Browser API compatibility or later editing semantics may need an adjusted conversion policy. |
| A3 | `DocumentLimits::V1` uses the fixed hard caps recorded in Resolved Planning Decision R-02; benchmarks may tighten snapshot frequency but may not silently raise those caps. | Security limits | A later schema version and migration are required to expand a cap. |
| A4 | A direct native IndexedDB adapter plus fake-IDB/browser tests is preferable to a runtime IndexedDB wrapper in this phase. | Standard stack | Implementation ergonomics may justify a wrapper after a separate, security-reviewed decision. |
| A5 | Current Rust/crates/npm registry releases cannot be captured until Wave 0 because Rust/Cargo are absent and direct registry resolution is unavailable in this sandbox. | Standard stack / environment | Dependency manifests need a human checkpoint and lockfile review. |

## Resolved Planning Decisions

### R-01 — Exact schema v1 field vocabulary

Schema v1 stores semantic field descriptors now and defers widget rendering/authoring behavior to Phase 5. `FieldDescriptor` contains exactly: stable `id`, unique non-empty `name`, optional `label`, logical `anchor`, typed `kind`, `required`, `read_only`, typed `default_value`, and ordered `options`. It has no arbitrary property bag.

`FieldKind` is the closed v1 enum:

- `Text { multiline, input_hint }`, where `input_hint` is `Plain | Date | Number | Email`;
- `Checkbox`;
- `RadioGroup`;
- `Select { multiple }`;
- `Signature`;
- `Button`.

`FieldOption` contains stable `id`, `label`, and unique `export_value`. `FieldValue` is `Empty | Text(String) | Checked(bool) | Selected(Vec<FieldOptionId>)`. Validation enforces kind/value compatibility, valid and unique option references, at most one radio selection, and no options on text/checkbox/signature/button fields. The schema deliberately excludes PDF/widget rectangles, appearance streams, JavaScript actions, tab coordinates, signature bytes, and arbitrary validation expressions. Adding those later requires an explicit schema migration. This resolves FLOW-01 without implementing Phase 5 UI or PDF behavior.

### R-02 — Versioned resource and recovery budgets

`DocumentLimits::V1` is a checked-in, versioned profile with hard ceilings applied before semantic publication:

- canonical document bytes: 64 MiB;
- tree depth: 128;
- semantic nodes: 200,000;
- total UTF-8 text bytes: 32 MiB;
- one text node: 4 MiB;
- styles: 4,096;
- asset descriptors: 10,000;
- field descriptors: 10,000;
- operations in one transaction: 10,000;
- one transaction record: 8 MiB;
- recovery replay records: 10,000;
- cumulative replay bytes after a snapshot: 64 MiB.

Every boundary has N and N+1 tests; an over-limit decode/command/recovery returns a stable structured error and publishes no partial state. Deployments may lower limits, but raising a v1 cap requires an explicit reviewed policy/schema change and new fixtures.

The default snapshot policy writes a snapshot on explicit local save, after every successful migration, or after 100 committed transactions / 4 MiB of uncheckpointed transaction bytes, whichever occurs first. Correctness tests run no-intermediate, every-transaction, and periodic policies. A checked-in 200-page-equivalent semantic fixture and 1,000-transaction log establish recovery measurements. The execution target is p95 recovery below 2 seconds in the pinned local Chromium/reference environment. If it misses, the executor may only lower the periodic thresholds, down to every 10 transactions / 1 MiB. If the target still fails, execution records a blocker artifact and the phase does not seal; it must not raise work limits, skip validation, or publish a guessed partial document.

The benchmark contract is executable: `fixtures/recovery/benchmark-200-page.recipe.json` is the deterministic fixture recipe, `crates/flow-core/examples/recovery_benchmark.rs` performs warm-up plus 20 measured recoveries, and `scripts/verify-recovery-benchmark.mjs` runs and validates the gate. Exactly one terminal artifact is allowed: passing metrics at `artifacts/benchmarks/phase1-recovery.json`, or a failing `artifacts/benchmarks/phase1-recovery-blocker.json` containing every attempted cadence and p50/p95. The validator rejects both-present, both-absent, malformed, or threshold-inconsistent states.

### R-03 — IndexedDB call-site ownership

Use a generated/typed DTO protocol defined by Rust core/WASM and a thin TypeScript adapter for physical IndexedDB calls. Rust owns snapshot/log/audit record schemas, recovery selection/replay, validation, redaction, and all mutation semantics. TypeScript only performs bounded native IndexedDB reads/writes and translates platform errors. This avoids pulling `web-sys` storage lifecycle into the semantic core while preserving native/WASM parity.

All three formerly open planning questions are resolved as of 2026-08-14. Later evidence may trigger an explicit migration or policy revision, never an implementation-time silent choice.

## Sources

### Primary

- [Serde](https://serde.rs/) — typed derive and custom serialization. [CITED: https://serde.rs/]
- [serde_json Value](https://docs.rs/serde_json/latest/serde_json/enum.Value.html) — default sorted map vs `preserve_order`. [CITED: https://docs.rs/serde_json/latest/serde_json/enum.Value.html]
- [wasm-bindgen guide](https://rustwasm.github.io/docs/wasm-bindgen/) — WASM export boundary and target setup. [CITED: https://rustwasm.github.io/docs/wasm-bindgen/]
- [BLAKE3 Rust API](https://docs.rs/blake3/latest/blake3/) — byte hashing and hexadecimal output. [CITED: https://docs.rs/blake3/latest/blake3/]
- [Proptest](https://github.com/proptest-rs/proptest) — shrinking and persisted regression cases. [CITED: https://github.com/proptest-rs/proptest]
- [MDN IndexedDB transaction guidance](https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API/Using_IndexedDB) — transaction lifetime/rollback. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API/Using_IndexedDB]
- [MDN IDBTransaction completion](https://developer.mozilla.org/en-US/docs/Web/API/IDBTransaction/complete_event) — durable completion event. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/IDBTransaction/complete_event]
- [Vitest Browser Mode](https://vitest.dev/guide/browser/) — browser provider requirements. [CITED: https://vitest.dev/guide/browser/]

### Project sources

- Phase decisions and boundary: [VERIFIED: .planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md:9-107]
- Requirements and phase traceability: [VERIFIED: .planning/REQUIREMENTS.md:16-117]
- Phase success criteria: [VERIFIED: .planning/ROADMAP.md:25-36]
- Stack and project constraints: [VERIFIED: AGENTS.md:13-26]

## Metadata

**Confidence breakdown:**

- Standard stack: MEDIUM — official docs confirm APIs and known tool versions for UUID/Vitest/thiserror; local crate/npm registry resolution and Rust installation are unavailable.
- Architecture: HIGH — locked phase decisions explicitly define model/transaction/anchor/store boundaries.
- Pitfalls: HIGH — canonical ordering, async IndexedDB lifecycle, stale commands, anchor invalidation, and audit minimization follow direct project decisions and official browser docs.

**Research date:** 2026-08-14  
**Valid until:** 2026-08-21 for tool/package versions; architecture remains valid until a locked Phase 1 decision changes.

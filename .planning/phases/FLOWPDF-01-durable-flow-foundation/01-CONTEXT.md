# Phase 1: Durable Flow Foundation - Context

**Gathered:** 2026-08-14
**Status:** Ready for planning

<domain>
## Phase Boundary

This phase delivers the durable semantic foundation only: a versioned FlowDocument schema, canonical serialization, explicit migrations, stable logical anchors, typed atomic transactions with undo/redo and revision preconditions, a persistence abstraction with browser recovery, and privacy-minimized audit metadata. It does not deliver rich-text interaction, typography, layout, PDF generation, forms, voice capture, collaboration, or a remote backend.

</domain>

<decisions>
## Implementation Decisions

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

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Product and Scope

- `.planning/PROJECT.md` — Core value, fixed architectural decisions, constraints, and explicit exclusions.
- `.planning/REQUIREMENTS.md` — Phase 1 requirements FLOW-01..05, EDIT-06..07, and QUAL-08 plus global Definition of Done.
- `.planning/ROADMAP.md` — Phase 1 boundary, dependencies, user-observable success criteria, and downstream phase separation.

### Architecture and Risk

- `.planning/research/ARCHITECTURE.md` — Canonical-model boundaries, command flow, provenance, and writer-first build order.
- `.planning/research/STACK.md` — Rust/WASM stack policy, version pinning, and adapter choices.
- `.planning/research/PITFALLS.md` — Anchor, schema, determinism, parser-safety, and phase-specific failure modes.
- `.planning/research/SUMMARY.md` — Research confidence, roadmap implications, and open benchmark questions.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets

- No application code exists yet. Planning artifacts and generated `AGENTS.md` are the only project assets.

### Established Patterns

- Planning artifacts are written in English; user communication remains Ukrainian.
- Work is sequential, uses specific-file staging, atomic commits, research/verification gates, and no commercial PDF SDK.
- Rust owns canonical business rules; TypeScript remains an adapter/UI language.

### Integration Points

- Phase 1 must establish a Cargo workspace and a future `wasm-bindgen` boundary without implementing the Phase 2 editor UI.
- Serialization fixtures created here become inputs for later browser, PDF associated-source, migration, and compatibility tests.

</code_context>

<specifics>
## Specific Ideas

- The initial vertical proof should be executable without a backend: create a document, apply transactions, serialize, reload, undo, simulate a crash between snapshot and later log entries, and recover the same canonical hash.
- Public examples and fixtures should include Ukrainian text even though advanced shaping is Phase 3; Phase 1 must preserve its Unicode bytes exactly.

</specifics>

<deferred>
## Deferred Ideas

- Rich-text input, IME and accessibility UI — Phase 2.
- Grapheme segmentation, shaping, line breaking and pagination — Phase 3.
- Compact binary FlowDocument encoding and ZIP package optimization — evidence-driven later work.
- Remote storage, authentication and multi-device synchronization — future service milestone.
- CRDT real-time collaboration — v2 collaboration work.

</deferred>

---

*Phase: 1-Durable Flow Foundation*  
*Context gathered: 2026-08-14*


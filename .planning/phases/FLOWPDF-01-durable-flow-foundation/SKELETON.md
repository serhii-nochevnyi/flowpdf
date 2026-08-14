# Walking Skeleton — FlowPDF

**Phase:** 1 — Durable Flow Foundation  
**Generated:** 2026-08-14  
**Execution:** local-only, fully autonomous

## Capability Proven End-to-End

> A user can create the deterministic Ukrainian/English sample FlowDocument, send one typed mutation through the Rust command boundary, durably commit canonical bytes and an immutable transaction through a narrow WASM DTO into IndexedDB, reload/recover the exact hash, and inspect its revision, provenance, and redacted audit metadata in the Foundation Inspector.

## Phase Goal

**As a** FlowPDF document author, **I want to** create, save, recover, inspect, undo, and safely target a versioned FlowDocument, **so that** every later input and renderer can depend on a durable transactional foundation.

## End-to-End Trace

```text
Foundation Inspector create/action button
  -> typed Command DTO { commandId, baseRevision, modality, target, arguments }
  -> narrow wasm-bindgen boundary
  -> Rust validation + immutable transaction + inverse + anchor mapping
  -> canonical JSON bytes + versioned BLAKE3 hash
  -> snapshot / append-only transaction / redacted audit DTOs
  -> one completing IndexedDB read-write transaction
  -> browser reload or recovery request
  -> Rust verification and idempotent contiguous replay
  -> Foundation Inspector revision/hash/provenance/audit display
```

The TypeScript side never mutates semantic document state, computes canonical bytes, interprets recovery semantics, or performs redaction. It transports DTOs, owns physical IndexedDB operations, and renders the read-only inspector.

## Architectural Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Canonical core | Rust workspace targeting native and `wasm32-unknown-unknown` | One semantic and transactional authority runs identically in tests and the browser. |
| Canonical representation | Deterministic compact UTF-8 JSON with explicit `schemaVersion`, ordered structures, stable UUID-compatible IDs, and versioned BLAKE3 hash metadata | Human-inspectable conformance truth and durable migration boundary; the hash is an equality/integrity diagnostic, not authentication. |
| Assets | Content-hash descriptor in FlowDocument plus separately resolved bytes | Prevents repeated embedded blobs while preserving exact asset identity. |
| Commands and history | Typed commands become atomic immutable transactions with stored forward/inverse operations, revision preconditions, and explicit anchor mappings | Every later modality consumes the same mutation boundary and undo/redo remains deterministic. |
| Browser position DTO | `nodeId + UTF-16 offset + affinity` | Browser-compatible public position without leaking DOM, page, layout, or PDF coordinates. |
| WASM boundary | Serialized typed request/result and persistence-record DTOs only | Prevents JavaScript from owning or directly mutating semantic state. |
| Persistence | Native IndexedDB adapter; one short transaction for each logical durable commit; acknowledgement after `complete` | Provides atomic local durability while Rust owns recovery validation and replay policy. |
| UI | Minimal semantic HTML, TypeScript, localized strings, and manual CSS for the Foundation Inspector | Proves the Phase 1 user path without introducing the later editor shell or an editable surface. |
| Authentication/backend | None | Phase 1 is local-only and has no account, service, sync, or authorization surface. |
| Deployment | Documented local build/run command plus real Chromium browser tests | Exercises the complete stack without adding remote infrastructure. |
| Dependency trust | Exact versions and lockfiles only after official registry/repository/checksum verification | Autonomous installation fails closed and records a blocker when provenance cannot be proven. |

## Stack Touched in Phase 1

- [ ] Official-registry dependency provenance and pinned Rust/Node/browser tooling
- [ ] Cargo workspace with native `flow-core` and narrow `flow-wasm` boundary
- [ ] Canonical FlowDocument creation, serialization, hash, validation, and migration
- [ ] Typed command, immutable transaction, anchor mapping, conflict, undo, and redo
- [ ] Snapshot/log/audit record port and Rust-owned recovery/redaction
- [ ] Real IndexedDB atomic read/write and reload/recovery
- [ ] Semantic Foundation Inspector interaction and accessible revision/audit presentation
- [ ] Local full-stack run command and deterministic native/WASM/Node/Chromium suite

There is no ORM, server database, schema push, network API, account, or deployment service in this skeleton.

## Walking-Skeleton Proof

The first green browser slice is Plan `01-02`, immediately after Wave 0. It must pass:

```bash
npm run test:browser -- walking-skeleton
```

The final phase proof extends that same slice rather than replacing it:

```bash
npm run check
```

`npm run check` must run formatting, Clippy, all Rust tests, the WASM target/build check, TypeScript checks, Vitest unit tests, and real Chromium recovery/accessibility tests without watch mode.

## Trust Boundaries

| Boundary | Contract |
|---|---|
| Official registries/repos → local toolchain and lockfiles | Allowlisted origins, repository identity, stable non-yanked/non-deprecated releases, registry integrity/checksum metadata, lockfile-only installs, and recorded fail-closed blocker. |
| Browser DTO → Rust/WASM | Bounded typed decode; stable error codes; no mutable document handle crosses the boundary. |
| IndexedDB bytes → Rust recovery | Treat every record as untrusted; verify size, schema, hash, identity, revision continuity, and idempotency before publication. |
| Rust audit DTO → DOM/accessibility tree | Allowlist-only redacted fields; render with safe text APIs; never transport document text or command arguments as audit metadata. |

## Out of Scope

- Rich-text editing, caret/selection, IME, grapheme-safe editing, or editable document DOM
- Typography, shaping, line breaking, layout, pagination, canvas, or display-list rendering
- PDF preview, export, import, reconstruction, native editing, or PDF coordinates
- Field authoring, field validation, or form-widget behavior
- Voice capture, speech models, dictation, or voice controls
- Remote backend, authentication, authorization, sync, collaboration, or CRDT behavior
- Compact binary or `.flowdoc` package as canonical truth

## Subsequent Slice Plan

- Phase 2 adds accessible rich-text input through the established command/anchor boundary.
- Phase 3 adds deterministic text shaping, layout, and reflow without changing canonical ownership.
- Phase 4 adds display-list preview and owned PDF export from immutable revisions.
- Phase 5 adds semantic field authoring and PDF widget resolution.
- Phase 6 adds explicit voice dictation/commands through the same transaction boundary.
- Phases 7–9 add bounded PDF reading, reconstruction/OCR, and controlled native PDF editing.


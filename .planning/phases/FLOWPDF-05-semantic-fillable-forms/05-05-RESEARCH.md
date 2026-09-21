# Phase 5 Plan 05-05 Research: Form Session WASM and Durable Browser Boundary

**Researched:** 2026-09-22

## Sources inspected

- `crates/flow-core/src/forms/mod.rs` — immutable, revision/hash-bound
  `FormSessionState`, validation, set/clear operations, and authored/effective
  value separation.
- `crates/flow-core/src/lib.rs` and `crates/flow-wasm/src/lib.rs` — existing
  `ApiResponse`/error-code boundary and the JS/WASM adapters for editor
  sessions, persistence planning, layout, and PDF export.
- `web/persistence/indexeddb-store.ts` — strict IndexedDB versioning, guarded
  physical envelopes, deterministic JSON, and fake-indexeddb test patterns.
- `web/src/editor/editor-controller.ts` and `web/src/editor/editor-store.ts` —
  current Rust-owned editor state and DTO conventions; no form current-value
  transport exists yet.

## Findings

1. Form values must not be added to `FieldDescriptor::default_value` or to the
   canonical transaction stream. The existing `FormSessionState` is already
   the correct noncanonical authority and rejects stale document identity,
   read-only fields, invalid values, and forged override maps.
2. The existing `ApiResponse<T>` envelope gives the browser a stable `ok`,
   typed value, and privacy-safe error code. A small form-session action
   protocol can reuse it without exposing a mutable Rust handle or page/PDF
   state.
3. IndexedDB currently opens one versioned database and stores only canonical
   recovery records. A separate versioned `form-sessions-v1` object store must
   be created in the same upgrade, otherwise a second adapter would race or
   open an incompatible database version.
4. Browser storage can provide identity and generation conflict protection but
   cannot reproduce Rust field validation. The browser adapter must persist only
   a Rust-accepted session response and use source document identity/revision/
   hash plus generation for optimistic retries.

## Scope boundary

This plan adds the transport and durable record boundary for noncanonical form
values. It does not add field authoring UI, semantic DOM inputs, AcroForm
appearance streams, PDF form import, viewer compatibility, flattening, or
automatic migration of a session across a changed canonical document.

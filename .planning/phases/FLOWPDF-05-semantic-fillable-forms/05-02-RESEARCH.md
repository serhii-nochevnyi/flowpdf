# Phase 5 Plan 05-02 Research: Rust Form Session Values

**Researched:** 2026-09-21

## Sources inspected

- `.planning/REQUIREMENTS.md` — FORM-05 requires fill, clear, and validation
  independently of PDF appearance state.
- `crates/flow-core/src/model/mod.rs` and `schema/mod.rs` — field defaults
  are canonical descriptor data, while a required field may legitimately start
  with an empty default.
- `crates/flow-core/src/forms/mod.rs` — closed value validation and the
  revision/hash-bound widget projection are now available for reuse.
- `crates/flow-core/src/editor_view/mod.rs` — existing editor sessions are
  immutable, revision-bound projections with generation counters and typed
  fail-closed errors.
- `crates/flow-core/src/transaction/mod.rs` — `SetField` mutates canonical
  descriptor metadata and is not a durable user-fill value record.

## Findings

1. `FieldDescriptor.default_value` must remain authored template state. Treating
   it as the user's current value would make a fill action alter canonical
   defaults and would make a reset operation ambiguous.
2. A noncanonical form session can carry only explicit overrides keyed by
   `FieldId`; an absent override resolves to the validated authored default.
   This permits an empty current value to override a nonempty default.
3. The session must bind to document ID, revision, and canonical source hash.
   A deserialized or stale session must be rejected before a value is read or
   changed; validation must reuse the forms module rather than browser rules.
4. Read-only fields must reject session mutation while remaining readable from
   their authored default. No PDF object, page coordinate, appearance, or DOM
   state belongs in this session.

## Scope boundary

This plan adds only the Rust core session contract and tests. WASM/editor UI
transport, current-value persistence, AcroForm dictionaries/appearances,
flattening, and target-viewer evidence remain later slices.

# Phase 5 Plan 05-03 Research: Session-Aware Widget Projection

**Researched:** 2026-09-21

## Sources inspected

- `crates/flow-core/src/forms/mod.rs` — current widget projection and the
  noncanonical `FormSessionState` override contract.
- `crates/flow-core/src/pdf/display_list.rs` — accepted page/source identity
  and fixed-point line/glyph provenance used for placement.
- `.planning/REQUIREMENTS.md` — FORM-04/FORM-05 require reflow-safe placement
  and fill state independent of PDF appearance state.
- Phase 5 plans 05-01 and 05-02 — established the schema/version and explicit
  default/current ownership boundaries.

## Findings

1. The current `FormWidget` exposes only authored `default_value`, so a later
   writer cannot distinguish a template default from a filled value without
   consulting an unrelated session map.
2. The projection already carries a versioned serialized DTO and result hash;
   adding an explicit effective `value` requires a projection schema bump and
   deterministic hash coverage.
3. Session-aware resolution must validate the session before page placement,
   then use only its explicit override for `value`; absent overrides remain the
   authored default, including an empty required default.
4. `FormProjectionError` should preserve the session's stable error taxonomy
   without copying authored value text or page/PDF implementation details.

## Scope boundary

This plan changes only the Rust derived projection and tests. It does not emit
AcroForm dictionaries/appearances, persist session values, expose browser/WASM
transport, or claim target-viewer behavior.

# Phase 5 Research: Semantic Fillable Forms

**Researched:** 2026-09-21

## Sources inspected

- `.planning/PROJECT.md` — active form, AcroForm, exact-owned-round-trip, and
  Rust/WASM ownership requirements.
- `.planning/research/FEATURES.md` — writer-first MVP ordering and semantic
  fields mapped to basic AcroForm widgets.
- `.planning/research/ARCHITECTURE.md` — separation of FlowDocument,
  fragment/display-list, PDF COS, and `forms` responsibilities.
- `crates/flow-core/src/model/mod.rs` and `schema/mod.rs` — current closed
  field vocabulary, options, default values, anchor state, and validation.
- `crates/flow-core/src/transaction/mod.rs` — existing `SetField` command,
  exact inverse operations, anchor mapping, and deletion preimages.
- `crates/flow-core/src/editor_view/mod.rs` — ordered field projection and
  explicit review statuses for invalid/deleted anchors.
- `crates/flow-core/src/layout/mod.rs` and `pdf/display_list.rs` — accepted
  fixed-point line geometry, UTF-16 source ranges, glyph clusters, and stable
  revision/hash identities.
- Phase 4 summaries and validation manifest — current PDF writer scope
  explicitly reports fields unsupported and keeps external reference evidence
  separate.

## Findings

1. The semantic field model exists, but `default_value` is currently the only
   field value carried by the canonical descriptor. Treating it as a filled
   value in a new PDF adapter would conflate authoring defaults and user data.
   A later plan needs a deliberate value-state decision and migration/record
   contract; the first slice must not invent it.
2. `SetField` already gives field descriptor authoring an atomic Rust command
   and exact inverse, but it is not yet exposed as a typed editor capability or
   accessible browser control. This is a later vertical slice.
3. `EditorFieldViewDto` orders valid fields by semantic node path and keeps
   invalid/deleted anchors in a review region. The forms projection should use
   the same explicit-review principle instead of silently skipping fields.
4. Phase 4 display-list lines already carry enough data for a bounded first
   anchor-to-rectangle projection: page index, source node ID, UTF-16 range,
   line rectangle, and glyph cluster/x-advance data. Placement can therefore be
   deterministic without measuring CSS text or persisting page coordinates.
5. The current PDF writer intentionally has no AcroForm dictionaries or widget
   appearance output. Adding a derived forms contract before COS emission
   keeps the next implementation reversible and lets resource/geometry tests
   run without pretending external PDF compatibility exists.

## Design implications

- Add a small `forms` core module that consumes a validated `FlowDocument` and
  an accepted `PdfDisplayList`, emits `FormWidgetProjection`, and reports
  bounded review entries/errors.
- Reuse `FieldKind`, `FieldValue`, `FieldDescriptor`, `LayoutRect`, and typed
  IDs; do not duplicate a second semantic field schema in TypeScript.
- Bind the result to document revision/hash and display-list hash. A stale
  document or display list must fail before any widget can be published.
- Keep validation errors code-only and field-ID based; authored values belong
  in the explicit form projection or future transaction result, never in
  diagnostics.

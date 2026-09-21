# Phase 5 Context: Semantic Fillable Forms

## Goal

Users will be able to author and fill bounded semantic fields in Flow mode.
Field meaning remains in `FlowDocument`; pagination resolves each valid logical
anchor to a derived page/widget rectangle, and the owned PDF writer later emits
an interoperable AcroForm representation from that derived result.

## Locked decisions

- Rust remains the sole owner of field validation, option membership, required
  state, anchor validity, widget identity, and page placement.
- `FieldDescriptor` and `FieldAnchorState` remain canonical semantic data.
  Page coordinates, widget rectangles, tab order, appearance state, and PDF
  object references remain derived and are never persisted in the document.
- A grapheme-safe semantic anchor is resolved only against an accepted,
  revision/hash-bound PDF display list. Missing, deleted, legacy-invalid, or
  unmapped anchors are returned in an explicit review list; they are never
  guessed or silently dropped.
- The first slice validates the existing closed field vocabulary and derives
  deterministic widget placements. It does not claim AcroForm serialization,
  flattening, signatures, external-PDF form import, or a target-viewer matrix.
- Browser code may render and route physical focus, but it must consume the
  Rust projection and use the existing transactional command bus for mutation.

## Current foundation

- The v3 canonical model already contains text, checkbox, radio-group, select,
  signature, and button field kinds, required/read-only flags, options, default
  values, and logical anchors.
- Schema validation already closes field IDs, option IDs/export values, default
  value shape, and radio/select cardinality.
- Structural editing already carries exact field anchor mappings and retains
  invalid/deleted anchors for review.
- Phase 3 pagination and Phase 4 display-list output expose fixed-point line
  rectangles, glyph cluster positions, source node IDs, source UTF-16 ranges,
  source revision/hash, and a deterministic display-list hash.

## Deferred work

Later plans must add the field authoring/fill command surface, explicit tab
ordering and validation rules, durable current values distinct from defaults,
AcroForm dictionaries/widgets and appearances, reflow/browser integration,
viewer/reference evidence, and explicit flattening. Each remains separately
verified and must preserve the source/display/widget ownership boundary.

phase: FLOWPDF-05-semantic-fillable-forms
plan: "15"

# Research: Rust/WASM form projection boundary

## Current boundary

The existing `layout_document` contract accepts canonical JSON, explicit font
bytes, and optional Ukrainian hyphenation data, then returns only a validated
`PaginationResult`. Form placement is deliberately one level later: the Rust
`PdfDisplayList` adds shaped glyph placements and source ranges, and
`resolve_form_widgets[_with_session]` resolves semantic anchors against those
source-backed lines. The browser therefore cannot safely derive widget
rectangles from the layout DTO or DOM geometry.

The existing PDF export boundary already accepts an optional source-bound
`PdfFormPlan`, but its caller still has to obtain that plan. Rebuilding it in
TypeScript would duplicate source/hash, session, anchor, glyph, and fixed-point
validation rules and would violate the Rust-owned geometry boundary.

## Chosen design

Add a separate, string-only Rust/WASM form-projection protocol. Its request
contains a bounded outer request id, the existing serialized layout request as
an opaque string, and an optional validated `FormSessionState`. Rust parses and
validates the nested layout request through a shared layout execution helper,
builds the display list, resolves default or session-effective widget values,
and derives a `PdfFormPlan` when the projection has no review entries. A
projection with legacy/deleted/unmapped fields remains publishable for the
browser review surface, but carries no export plan; other core failures are
fail-closed.

The result is revision/hash-bound and includes both the form projection and
optional plan. A Rust-owned result hash covers the complete result with its
hash field cleared. A companion verifier checks the envelope, source/layout
identities, nested projection/plan hashes, and result hash before a browser
worker may publish it.

## Rejected alternatives

- Deriving rectangles from `LayoutResultDto` or browser DOM boxes: loses shaped
  glyph/source mapping and can diverge after reflow, bidi, or grapheme-safe
  anchors.
- Reusing `layout_document` by parsing only its response: the response does
  not contain the admitted font catalog or display-list glyph placements.
- Recreating the form plan in TypeScript: duplicates Rust validation and would
  make the PDF export plan a browser-authored semantic object.
- Making the form endpoint mutate canonical/session state: projection is
  derived data; value mutations remain in the existing form-session protocol.

## Scope fence

This slice establishes the Rust/WASM projection transport and deterministic
test evidence. It does not claim browser rendering/selection controls,
target-viewer compatibility, external PDF form import, or general flattening.

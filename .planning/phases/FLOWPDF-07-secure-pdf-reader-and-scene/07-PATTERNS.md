# Phase 7 Patterns

## Rust reader boundary

- Keep parser internals private. Re-export only closed scene, report, limits,
  request, and error DTOs from `flow_core::pdf`.
- Use checked slices and integer arithmetic for every offset/length. Never use
  `unwrap` on PDF input paths; `expect` is reserved for impossible internal
  invariants after validation.
- Return stable `FLOW_PDF_READER_*` codes. Diagnostics identify a feature,
  page, object, or operator but never echo PDF-authored text or raw bytes.
- Keep `PdfReader` non-serializable and request-local. Scene results are derived
  values with a source hash and reader schema version.

## Scene and provenance

- Every visible item carries page index, source object number when known, and
  content-stream offset/operator when known.
- Text uses fixed-point glyph rectangles/advances and a nullable Unicode
  mapping; missing mappings are represented as report entries, not guessed.
- Paths and clips carry transformed fixed-point coordinates. Images carry only
  validated dimensions, media type/filter identity, and bounded display data.
- Forms and annotations are inspection records. No field value or action is
  allowed to become an editor transaction in this phase.

## Worker/browser projection

- Reuse the existing PDF worker cancellation and stale-request patterns, but
  keep reader scheduler state separate from export scheduler state.
- The imported scene is `aria-hidden` visual data plus a sibling report/status
  surface. File input labels, progress, errors, and unsupported-content lists
  are semantic DOM.
- Report “blocked” and “unsupported” states before rendering partial content;
  partial scenes remain visibly marked as partial.

## Verification

- Add Rust tests for valid fixtures, xref laziness, text mapping/glyph geometry,
  paths/transforms/clips, JPEG/images, forms/links/annotations, active-content
  rejection, and every budget lane.
- Add WASM boundary tests for hex/request/result limits and no-action behavior.
- Add unit/browser tests for cancellation, stale results, report visibility,
  file-input/read-only rendering, and no semantic editor mutation.
- Run the existing Phase 1–6 gates as regression checks and preserve external
  PDF/reference tools as unavailable when absent.


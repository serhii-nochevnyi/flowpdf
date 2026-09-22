# Phase 8: External Reconstruction and OCR — Context

**Gathered:** 2026-09-22
**Status:** Ready for execution planning

## Phase Boundary

This phase turns the bounded Phase 7 PDF scene into a reviewable, best-effort
FlowDocument candidate for supported single-column pages. It keeps the source
PDF in an immutable browser-owned source record, carries source mappings and
per-node confidence, preserves unsupported scene elements as source-bound
opaque islands, and gives the user an explicit side-by-side review before
acceptance. Scanned pages may supply bounded OCR candidates through a caller
owned adapter; FlowPDF does not claim to ship an OCR engine in this phase.

The phase does not promise arbitrary PDF reading order, table/layout recovery,
lossless external conversion, native PDF mutation, signatures, encryption,
remote OCR uploads, or automatic acceptance of uncertain text.

## Locked Decisions

- **D-08-01:** The original PDF is an immutable source record keyed by its
  content hash. A reconstructed FlowDocument and review decisions are derived
  records; no conversion path mutates or replaces the source bytes.
- **D-08-02:** Reconstruction consumes a verified `PdfScene`, not browser DOM
  geometry. Rust owns single-column ordering, block grouping, source mappings,
  confidence, and the candidate FlowDocument.
- **D-08-03:** Only a bounded single-column text projection is auto-grouped.
  Ambiguous ordering, missing mappings, unsupported visual elements, and OCR
  uncertainty remain explicit report/review state rather than guessed semantics.
- **D-08-04:** Unsupported paths, images, forms, links, annotations, and other
  visual elements remain source-bound `PdfOpaqueIsland` sidecar entries. They
  are rendered in the comparison surface and are not silently converted into
  editable text.
- **D-08-05:** OCR is an adapter contract. It receives bounded image/page
  inputs and returns bounded text candidates with confidence and rectangles;
  the browser never treats an unavailable provider as a successful OCR run.
- **D-08-06:** Low-confidence or missing-Unicode candidates require an explicit
  review decision before acceptance. The UI must not label external
  reconstruction exact.
- **D-08-07:** The reconstructed candidate uses a distinct external-source
  provenance lineage and source hash. It remains a valid canonical
  FlowDocument, but its provenance distinguishes it from owned round-trip and
  local sample documents.
- **D-08-08:** The reconstruction worker is separate from the reader worker;
  stale source/scene/OCR identities cannot publish a candidate for another
  PDF.

## Agent Discretion

- Exact fixed-point line/block thresholds, provided they are deterministic,
  bounded, and covered by fixtures for Ukrainian/English text.
- Candidate ID derivation, provided IDs are valid, stable for the same source
  and block order, and do not expose raw PDF text in diagnostics.
- The local source store may use IndexedDB and clone bytes on read/write; it
  must reject conflicting writes for an existing source hash.

## Deferred Ideas (OUT OF SCOPE)

- General multi-column/table/footnote/header-footer semantic inference.
- Bundling Tesseract, cloud OCR, remote upload, or a native OCR service.
- Automatic edits to the accepted FlowDocument without review decisions.
- Opaque-island semantic editing or native PDF object mutation.
- Exact visual or semantic parity claims for third-party PDFs.

# Phase 7: Secure PDF Reader and Scene — Research

**Researched:** 2026-09-22
**Domain:** bounded PDF syntax reading, scene projection, text extraction, and hostile-input handling
**Confidence:** MEDIUM

## Existing project evidence

| Source | Finding | Planning consequence |
|---|---|---|
| `crates/flow-core/src/pdf/mod.rs` | Phase 4 owns a bounded COS writer, fixed-point geometry, source manifests, forms, links, and deterministic support reports. | The reader must consume the same fixed-point geometry and expose a separate derived scene; it must not mutate export or canonical state. |
| `crates/flow-core/src/pdf/display_list.rs` and `font.rs` | Export display-list items already preserve page rectangles, source identities, glyph placements, and Unicode mapping where the FlowDocument is authoritative. | Imported PDF text needs an analogous provenance record with PDF object/operator identity, but it must never be mistaken for canonical editor text. |
| `crates/flow-wasm/src/lib.rs` | Browser-facing Rust contracts are string-only JSON and validate request/result identity before publication. | PDF reading gets a bounded hex ingress and JSON scene result; bytes remain owned by the request-local Rust reader. |
| `web/src/pdf/pdf-worker.ts` | The PDF worker already has single-flight, cancellation, stale-result, and diagnostic patterns. | Reader requests should reuse the same worker safety model, with a separate reader scheduler and no export-state coupling. |
| `.planning/phases/FLOWPDF-04-owned-pdf-preview-and-export/04-RESEARCH.md` | Arbitrary PDF import, repair, OCR, and external reconstruction were intentionally deferred to this phase or later. | Phase 7 admits a declared subset and reports everything else; Phase 8 owns reconstruction/OCR and Phase 9 owns editing. |

## Chosen supported subset

The first reader accepts unencrypted PDF 1.4–1.7 files with a classic xref
table and lazily indexed indirect objects. It supports direct/indirect COS
values, page trees, uncompressed and Flate streams, text operators with
simple-font or `ToUnicode` mappings, paths, transforms, clipping, local form
XObjects, JPEG image XObjects, internal destinations, and AcroForm/widget
inspection. It does not execute actions, resolve external resources, or
silently repair malformed data.

Unsupported xref streams/object streams, filters, CFF/unknown fonts, unmapped
text, unknown content operators, malformed annotations, encryption, and active
actions become bounded report entries. A page can still be returned when a
non-fatal unsupported item is isolated; structural or budget failures stop the
request with a stable error code.

## Security and budget decisions

- Index only xref offsets at open time; parse the catalog/page tree and page
  content on demand.
- Copy the caller's bytes into an owned request-local buffer and never expose a
  mutable PDF handle to JavaScript.
- Bound input bytes, object count, nesting, page count, token count, decoded
  stream bytes, image pixels, content recursion, and serialized scene bytes.
- Treat `/OpenAction`, `/AA`, `/JavaScript`, `/Launch`, `/GoToR`, `/URI`, file
  specifications, embedded files, and executable-looking action dictionaries as
  data to report, never as instructions.
- Keep authored PDF strings out of diagnostic messages; operator/object codes
  are sufficient for support reporting.

## Reference boundary

The phase uses the PDF Association/ISO 32000-2 specification hub and the
existing Phase 4 qpdf design research as syntax references. qpdf, Poppler,
MuPDF, and target-viewer checks remain optional external evidence. Their absence
does not turn the local bounded-parser and browser-worker checks into a full
compatibility claim.

## Recommended order

1. Add the lazy xref/object reader and stable budget diagnostics.
2. Add the page/content interpreter and provenance-rich scene types.
3. Expose a bounded WASM/worker reader request and an accessible visual reader
   panel with report-first rendering.
4. Run local gates twice, update traceability, and record external evidence as
   unavailable unless actually exercised.


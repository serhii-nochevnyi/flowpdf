# Phase 4: Owned PDF Preview and Export - Research

**Researched:** 2026-09-21
**Domain:** Deterministic PDF COS writing, selectable text, owned source recovery, and browser preview
**Confidence:** MEDIUM

## Existing project evidence

| Source | Finding | Planning consequence |
|---|---|---|
| `crates/flow-core/src/layout/mod.rs` | Phase 3 already emits fixed-point page bounds, content rectangles, source ranges, repeated header/footer fragments, stable page/fragment IDs, and result hashes. | PDF export must consume `PaginationResult` and never recompute page geometry in TypeScript or with browser text measurement. |
| `crates/flow-core/src/model/mod.rs` | Canonical schema v3 has page/section settings, semantic text runs, bounded images/assets, and provenance; paths, links, and outlines are not yet first-class semantic nodes. | Export a declared supported subset and emit explicit unsupported diagnostics for features not represented by the source model. |
| `crates/flow-core/src/layout/mod.rs` and `data/NotoSans-Regular.ttf` | Font admission is already byte-backed, hashed, bounded, and provenance-gated for layout. | Reuse the same catalog identity and fixture, then add PDF resource/subset validation without host-font discovery. |
| `crates/flow-core/src/canonical/mod.rs` | Canonical JSON and semantic hashes are deterministic and already used as revision/source gates. | Reproducibility and recovery records should bind source revision, canonical hash, layout result hash, font identities, hyphenation identity, engine version, and options. |
| `crates/flow-wasm/src/lib.rs` | The WASM boundary is string-only for layout and exposes immutable Rust results. | Add one bounded string-only export/recovery boundary; raw PDF bytes may be returned only as an explicit binary output adapter, never as mutable core state. |
| `web/src/layout/page-viewport.tsx` | The browser already renders accepted fixed-point pages while retaining semantic DOM. | Extend the existing viewport with zoom/search/selection projections instead of introducing a second editor surface. |
| local tool probe | qpdf, MuPDF, Poppler `pdftoppm`, `pdfinfo`, and `pdftotext` are not on the current PATH. | The gate must include tool-backed rows that report unavailable with safe diagnostics; CI/reference environments can promote them to pass. |

## Architectural findings

### 1. Start with a minimal owned COS writer, not an import parser

The project needs a deterministic output contract before it needs arbitrary PDF
compatibility. A small typed COS model can enforce object/reference ownership,
bounded strings/streams, canonical escaping, deterministic numbering, and a
valid xref/trailer. A reader/importer would multiply hostile-input concerns and
belongs to Phase 7.

### 2. Display-list extraction must preserve source mapping

Layout fragments currently carry source ranges but not a PDF-specific display
list. The adapter should resolve each supported fragment against canonical runs
and shaped glyph data, preserving the source node/range, glyph IDs, font face,
direction, and fixed-point placement. This is the seam for both PDF text
operators and browser search/selection.

### 3. Selectable Unicode text needs explicit font resources

The PDF text layer needs a deterministic mapping from emitted glyph IDs to
Unicode source clusters and a font resource whose widths match the same shaped
advances. The writer must reject a glyph run it cannot map rather than emitting
visually plausible but unsearchable text.

### 4. Source recovery is a provenance feature, not a generic PDF promise

Owned FlowPDF exports can carry a bounded, versioned source payload and hashes.
An external PDF without that payload cannot be called an exact FlowDocument
round trip. The recovery path therefore validates the embedded payload,
canonical hash, document ID/revision, and export manifest before returning an
owned source.

### 5. Validation must distinguish local, reference, and external evidence

Rust can prove structural invariants and byte determinism locally. Text
extraction and rendered visual comparison require reference tools or viewers;
the current machine lacks those tools. The Phase 4 gate should preserve this
distinction and never turn a missing executable into a green result.

## Recommended plan order

1. Build bounded COS values, deterministic object/xref writing, and a synthetic
   one-page export fixture.
2. Add a Rust display-list adapter and selectable Unicode text/font resources
   bound to Phase 3 glyph/source ranges.
3. Add supported asset/image streams plus explicit metadata, outline, link, and
   unsupported-feature diagnostics.
4. Add reproducibility manifest, private source payload, and exact recovery.
5. Expose export/recovery through WASM and add virtualized browser preview with
   zoom, search, and source selection.
6. Assemble structural, extraction, visual, determinism, and accessibility
   validation gates with honest unavailable-tool reporting.

## Sources

- ISO 32000-2 specification hub: https://pdfa.org/resource/iso-32000-2/
- qpdf design and object model reference: https://qpdf.readthedocs.io/en/stable/design.html
- PDF Association PDF specifications and conformance resources: https://pdfa.org/resource/
- Existing project sources listed in the evidence table above.

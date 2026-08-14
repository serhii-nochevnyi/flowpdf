# Domain Pitfalls

**Domain:** Owned flow editor and PDF engine  
**Researched:** 2026-08-14

## Critical Pitfalls

### Treating PDF Text as a Document Tree

**What goes wrong:** Positioned glyphs are mistaken for paragraphs, columns or table cells.  
**Consequences:** Corrupted reading order, false merges, unusable reflow.  
**Prevention:** Separate page scene from semantic inference; attach provenance and confidence.  
**Detection:** Visual rendering is correct while copy order or reconstructed structure is wrong.

### Typography Diverges Between Preview and PDF

**What goes wrong:** Browser-native measurement and export shaping choose different fonts, glyphs or advances.  
**Consequences:** Page breaks move during export and fields drift.  
**Prevention:** One pinned shaping/layout core, fixed-point geometry, font hashes, renderer-neutral display list.  
**Detection:** Page-count or line-break differences for the same FlowDocument revision.

### Incremental Layout Is Not Equivalent to Full Layout

**What goes wrong:** Cached pages ignore counters, headers, footnotes, floats, font state or break-token changes.  
**Consequences:** Stale or nondeterministic pages appear after editing.  
**Prevention:** Carry-state signatures and property-based tests comparing incremental against full layout.  
**Detection:** Save/reopen or full export changes the visible document.

### PDF Parser Resource Exhaustion

**What goes wrong:** Malformed recursion, object cycles, decompression bombs, extreme images or fonts exhaust memory/CPU.  
**Consequences:** Browser/service denial of service or exploitable parser behavior.  
**Prevention:** Rust core, lazy loading, hard budgets, sandboxed codecs and continuous fuzzing.  
**Detection:** Unbounded allocations, recursive stack growth, worker termination or timeouts.

### Editing Subset Fonts as if They Were Complete

**What goes wrong:** A new character is absent from an embedded subset or lacks a valid Unicode mapping.  
**Consequences:** Missing glyphs, incorrect extraction, layout changes.  
**Prevention:** Treat old font resources as immutable and embed a new licensed subset for edited content.  
**Detection:** The replacement looks correct in one viewer but copies as garbage or fails elsewhere.

## Moderate Pitfalls

### Field Coordinates Drift After Reflow

Store semantic anchors in FlowDocument and resolve page/widget rectangles only after final pagination. Never persist exported page coordinates as the Flow anchor.

### Canvas-Only Editor Breaks IME and Accessibility

Use a hidden input/editing host, logical selection model and synchronized semantic DOM. Canvas is a renderer, not the accessibility or editing model.

### Redaction Leaves Historical Data

Incremental save retains prior revisions. Secure redaction requires a full rewrite, removal of alternate representations, and extraction/search verification.

### Signed PDFs Are Edited In Place

Any meaningful content change may invalidate certification or signature permissions. Keep the signed original immutable and create a derived revision with a clear warning.

### Voice Command and Dictation Ambiguity

Use explicit modes and push-to-talk. Interim transcript never changes the document; high-risk destructive commands require a preview/confirmation policy.

### Silent Recovery of Malformed PDF

Strict and repair parsing must produce different, visible outcomes. Repair operations must be included in the conversion report.

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|----------------|------------|
| Flow schema | Page coordinates leak into canonical nodes | Schema review and serialization invariants |
| Transactions | DOM positions become persistent anchors | Stable node IDs, affinity and grapheme-boundary validation |
| Text layout | UTF-16/code point/grapheme confusion | Official Unicode test data and explicit index types |
| Pagination | Non-terminating keep/widow/orphan rules | Ordered constraint relaxation plus emergency progress rule |
| PDF writer | Visually valid but semantically broken text | Embedded fonts, ToUnicode and extraction tests |
| Forms | Viewer-dependent appearance | Generate explicit appearance streams and cross-viewer tests |
| PDF reader | Eager object graph and decompression | Lazy resolver with budgets |
| Import | Overconfident semantic classification | Per-property confidence and review UI |
| Native editing | Overlay hides but does not remove data | Normalized content rewrite and post-save validation |
| Voice | Stale selection receives late transcript | Revision and selection bookmark preconditions |

## Sources

- [qpdf design](https://qpdf.readthedocs.io/en/stable/design.html)
- [Unicode security considerations](https://www.unicode.org/reports/tr36/)
- [HarfBuzz non-responsibilities](https://harfbuzz.github.io/what-harfbuzz-doesnt-do.html)
- [PDFium fuzzers](https://pdfium.googlesource.com/pdfium/+/main/testing/fuzzers/)


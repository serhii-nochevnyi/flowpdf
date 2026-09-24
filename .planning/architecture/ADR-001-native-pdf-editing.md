# ADR-001: Controlled Native PDF Editing

**Status:** Accepted
**Date:** 2026-09-24
**Investigation:** `.planning/investigations/INV-001-native-pdf-editing/`
**Requirements:** NPDF-01, NPDF-02, NPDF-03, NPDF-04, NPDF-05

## Context

FlowPDF currently imports a bounded unencrypted classic-xref PDF subset into a
read-only scene. The reader retains source bytes and an xref index, parsing
objects lazily; it does not own a complete mutable object graph. The owned PDF
writer accepts FlowPDF-authored COS graphs and serializes a new file, but it
cannot rewrite a graph from an imported PDF. The scene preserves object,
operator, and stream-offset provenance, but it is not a byte-safe editing
model. Native editing must not convert pages to FlowDocument or silently lose
unsupported content. The original import remains immutable.

PDF redaction is a separate, destructive operation: a redaction mark only
identifies content. Applying it must actually remove content and the mark,
including raster data rather than merely masking it. The Phase 7 reader already
excludes encryption, signatures, xref/object streams, active actions, and
general repair. The local environment has no qpdf/Poppler tools, so external
structural, extraction, and raster evidence cannot be claimed locally.

## Decisions

### D1 — Use a complete bounded graph and a fresh whole-document writer

- Decision: Create a Rust-owned editable graph from all reachable objects in
  the currently admitted reader subset, validate it before mutation, and emit
  each accepted edit as a newly serialized complete PDF. Preserve parsed
  generic COS values and untouched raw encoded streams. Do not carry forward
  unreachable source objects.
- Consequence: Object numbering, xref, trailer, references, page tree, resource
  links, and output identity are validated as one transaction. The immutable
  original remains separately available.
- Scope: No incremental update, in-place source mutation, arbitrary malformed
  PDF repair, xref/object streams, encryption, or signatures.

### D2 — Fail closed when any reachable structure cannot be represented

- Decision: Refuse the entire edit before returning output if any reachable
  object, stream encoding, reference, page dependency, or trailer value cannot
  be safely parsed, bounded, retained, and written. Never reconstruct an
  edited page from the partial scene as a fallback.
- Consequence: Supported generic unknown dictionary entries may survive
  ordinary edits without semantic interpretation, while unsupported syntax is
  surfaced as a stable actionable diagnostic.
- Scope: No promise to preserve malformed/opaque byte sequences whose object
  boundaries or references cannot be validated. Redaction refuses a narrower
  unsupported-content subset even when an ordinary edit could preserve it.

### D3 — Bind Rust-owned edit sessions and commands to source and revision

- Decision: Keep the canonical edit session and all native PDF mutations in
  Rust. Every session and command binds the source hash, session revision, and
  request identity; browser/WASM responses are string-only, verified, and
  immutable at the boundary.
- Consequence: A stale command cannot edit a different imported PDF, and the
  browser only presents accepted Rust results. The source PDF remains an
  immutable origin record; edited output is a derived document.
- Scope: This session does not alter the canonical FlowDocument model or its
  existing transaction/recovery algebra.

### D4 — Start with plain note annotations and a vertical Rust/WASM/browser tracer

- Decision: The first native-edit tracer adds, reads back, moves, edits, and
  removes a plain `/Text` note annotation, then performs a full rewrite and
  verifies source immutability. It traverses Rust tests, the closed WASM
  contract, worker scheduling, and the browser native-PDF surface.
- Consequence: The smallest user-visible path proves the imported graph,
  mutation, serialization, and stale-result boundaries before the wider edit
  families are layered on.
- Scope: It does not claim arbitrary annotation appearances or interactive
  annotation actions.

### D5 — Admit only strictly recognized text islands and page objects

- Decision: Text replacement requires a reversible source character mapping,
  a supported embedded font, admitted single-line text-show operations,
  supported geometry/resources, no unsupported clipping or rendering mode,
  and a replacement that fits the original fixed-page geometry. Image/path
  edits are limited to recognized scene boundaries and supported transforms.
- Consequence: Unknown encoding, ambiguous provenance, unsupported layout, or
  overflow produces a typed refusal and actionable warning instead of
  reflowing, shrinking, or covering neighbors.
- Scope: No general font substitution, arbitrary shaping/reflow, or blind
  text/object rewriting.

### D6 — Restrict page insertion and supported interactive content

- Decision: Support blank-page insertion, reorder, rotation, same-document
  duplication, and deletion with transactional reference verification. Admit
  plain Text notes and bounded non-signature text/checkbox AcroForm fields.
- Consequence: Page-tree, widget, annotation, resource, parent, and destination
  references are checked before output. Deleting the final page is refused.
- Scope: Cross-document page import, signature widgets/documents, XFA,
  JavaScript, launch/external actions, embedded files, and arbitrary annotation
  appearance streams are not editable in this phase; unsupported cases remain
  visible and read-only.

### D7 — Make redaction fail closed and require independent post-save evidence

- Decision: Separate marking from applying. Applying redaction removes the
  recognized intersecting text, simple vector objects, raster pixels, and
  supported annotations from a complete fresh rewrite; strip document metadata.
  Publish no redacted output unless internal post-save text/reachability checks
  and the required independent structural, extraction, and raster validators
  all pass. Missing or inconclusive validators mean no result.
- Consequence: Redaction is available only for fully inspected supported
  documents. Unknown content, unsafe transforms/clips/masks, encryption,
  signatures, unsupported form/attachment structures, or hidden text stores
  outside the declared scrubber cause refusal. The original source remains
  unchanged.
- Scope: No visual overlay or incremental update is accepted as secure
  redaction. qpdf alone is not a redaction proof. This high-risk ticket requires
  a human checkpoint before implementation.

### D8 — Keep external compatibility and reference tools honest

- Decision: Local tests prove the bounded owned implementation only. The
  release gate separately runs independent qpdf structure, Poppler text, and
  Poppler raster comparisons against checked-in fixtures and reports missing
  tools/fixtures as unavailable.
- Consequence: A passing local parser/writer test is not described as broad
  compatibility. Reference tools remain development/release dependencies, not
  runtime PDF engines.
- Scope: No commercial SDK or third-party PDF engine is required at runtime;
  no tool is silently installed on the developer host.

## Consequences

The architecture begins with a separate import-to-editable-graph adapter,
session transactions, and a full writer/re-open verifier. Phase plans should
be vertical and small; text, forms/annotations, page objects, page operations,
and redaction each get explicit bounds and tests. The browser retains the
reader/reconstruction lanes as separate surfaces and does not use FlowDocument
reconstruction to mutate native PDFs. External target-viewer and reference-tool
evidence remains separate from local test status.

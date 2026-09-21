# Phase 4: Owned PDF Preview and Export - Context

**Gathered:** 2026-09-21
**Status:** Ready for execution planning

<domain>
## Phase Boundary

This phase turns one accepted, immutable FlowDocument revision and its Rust
layout result into a deterministic, selectable PDF and a synchronized browser
preview. It owns the narrow PDF COS syntax needed by FlowPDF, deterministic
resource/object ordering, text extraction mappings, licensed font admission,
supported image emission, export provenance, and exact owned-source recovery
when the export contains the source payload.

It does not deliver arbitrary external-PDF import, encryption/signatures,
AcroForm authoring, OCR, PDF/UA or PDF/A certification, hostile-PDF repair,
complex graphics, or commercial PDF runtime dependencies. Those remain later
phases or explicit deferred scope.

</domain>

<decisions>
## Locked Decisions

- **D-04-01:** The FlowDocument revision and Rust PaginationResult are the only
  semantic and geometry authorities. PDF bytes and preview display lists are
  derived artifacts and never mutate canonical document state.
- **D-04-02:** The runtime PDF layer is an owned Rust COS model and writer. A
  reference tool such as qpdf, Poppler, or a target viewer may validate output
  in development, but no commercial or external PDF SDK is a runtime
  dependency.
- **D-04-03:** PDF object numbering, resource names, dictionary key order where
  observable, stream formatting, metadata serialization, and xref/trailer
  construction are deterministic for the same source revision, catalog, and
  export options.
- **D-04-04:** Fixed-point `LayoutUnit` values cross into PDF coordinates through
  one exact rational formatter. The formatter emits canonical decimal tokens
  from integer raw units and never lets browser or host floating-point layout
  decide geometry.
- **D-04-05:** Text display-list entries retain source UTF-8/UTF-16 ranges,
  glyph IDs, advances, direction, and font identity. PDF text operators and
  Unicode mappings are generated from that Rust-owned display list rather than
  reparsing rendered browser text.
- **D-04-06:** Font bytes enter export through an explicit bounded catalog with
  provenance and license records. The writer embeds only admitted bytes and
  rejects missing glyph mappings, duplicate identities, unsupported formats,
  and unverified font provenance.
- **D-04-07:** An owned export may carry a bounded canonical source payload in a
  private, namespaced metadata stream. Recovery is exact only after the payload
  and semantic hash validate; absence or corruption produces an explicit
  unavailable/recovery diagnostic, never a best-effort exact claim.
- **D-04-08:** Owned output never contains JavaScript, launch actions, external
  resource actions, encryption, or signing claims. Unsupported source features
  are reported with stable diagnostics and are not silently discarded.
- **D-04-09:** Preview pages are a virtualized visual projection over the same
  accepted display-list geometry used by export. Semantic DOM, input, selection,
  and accessibility remain the authoring surface.
- **D-04-10:** Export validation is layered: Rust structural invariants first,
  deterministic byte/hash checks second, text extraction and visual/reference
  checks when their tools are available, and honest unavailable diagnostics
  otherwise. Missing external tools cannot be recorded as a pass.

## Agent Discretion

- The exact internal COS enum and writer buffer shape, provided names, numbers,
  streams, references, xref, and trailer are bounded and deterministic.
- The first supported PDF profile, provided it is unencrypted PDF 1.7 syntax
  compatible with the declared subset and does not claim PDF/A or PDF/UA.
- The private source-payload container, provided it is namespaced, bounded,
  versioned, hash-checked, and ignored by ordinary PDF viewers.
- The browser preview virtualization window and search index shape, provided
  selection/search resolve back to semantic source ranges and do not become a
  second document model.

## Deferred Ideas (OUT OF SCOPE)

- General PDF parser/import, external reconstruction, OCR, or arbitrary malformed
  PDF repair.
- AcroForm widgets, flattening, signatures, encryption, incremental revisions,
  tagged PDF, PDF/UA, PDF/A, annotations, JavaScript, and launch actions.
- Full font subsetting for every OpenType variation/color-font feature; the
  admitted Phase 4 font matrix remains explicit and fail-closed.
- Complex paths, transparency groups, clipping islands, floats, arbitrary
  positioned objects, and complex table graphics.

## Required Evidence Posture

Phase 4 is not complete because a file opens in one viewer. Completion requires
Rust tests for COS serialization, object/reference integrity, coordinate
rounding, glyph-to-Unicode mapping, font/resource determinism, source-payload
recovery, and bounded failures. Browser evidence must prove preview/export
revision parity, search/selection behavior, virtualization, and accessible
status. Structural, extraction, and visual reference tools are separate gates;
unavailable tools remain visible as unavailable rather than being substituted by
local Chromium evidence.

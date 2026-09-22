# Phase 7: Secure PDF Reader and Scene — Context

**Gathered:** 2026-09-22
**Status:** Ready for execution planning

<domain>
## Phase Boundary

This phase adds a bounded, read-only PDF import path for a declared unencrypted
PDF subset. It indexes and parses PDF syntax lazily, projects supported page
content into a provenance-rich internal scene, exposes mapped text geometry and
support warnings, and renders that scene through a worker-backed browser panel.

It does not reconstruct imported pages into FlowDocument blocks, perform OCR,
edit native PDF objects, redact content, repair arbitrary malformed files,
decrypt/sign/encrypt files, or execute PDF actions. Those remain Phase 8,
Phase 9, or explicit non-goals.

</domain>

<decisions>
## Locked Decisions

- **D-07-01:** The imported PDF byte buffer is immutable request-local input;
  `flow-core` owns parsing and the browser receives only bounded DTOs.
- **D-07-02:** Opening a PDF indexes classic xref offsets only. Object values,
  page dictionaries, resources, and streams are parsed on demand.
- **D-07-03:** The reader accepts a declared unencrypted subset and fails closed
  on structural corruption or budget exhaustion. Unsupported content is visible
  in a report and is not silently dropped.
- **D-07-04:** PDF coordinates cross into the scene through `LayoutUnit` fixed
  point conversion. No browser text measurement or floating-point pagination
  decides scene geometry.
- **D-07-05:** Text items retain glyph/code geometry, decoded Unicode when
  mapped, source object numbers, content-stream offsets, and operator names
  where available. Imported text is a visual/read-only projection, not editor
  canonical text.
- **D-07-06:** Actions, JavaScript, launches, external destinations, embedded
  files, and external resources are never followed or executed. Their presence
  is a report diagnostic.
- **D-07-07:** The browser reader is a separate worker-backed surface. It may
  display scene data and warnings but cannot mutate the semantic editor or
  publish imported bytes into editor state.
- **D-07-08:** Browser/target-viewer compatibility and real assistive-technology
  observations remain separate evidence rows; local Chromium tests do not
  masquerade as those observations.

</decisions>

<agent_discretion>
## Agent Discretion

- Exact classic-xref scanner and private COS representation, provided all
  offsets, lengths, nesting, and token counts are checked before allocation.
- The first font mapping matrix, provided simple ASCII/WinAnsi-like mappings
  and bounded `ToUnicode` BFChar/BFRange mappings are explicit.
- Scene rendering may use SVG/CSS primitives for paths and text and data URLs
  for admitted JPEG images, provided it remains read-only and report-first.

</agent_discretion>

<deferred>
## Deferred Ideas (OUT OF SCOPE)

- PDF xref/object streams, encryption, signatures, incremental updates, OCR,
  reading-order reconstruction, arbitrary font shaping, transparency groups,
  soft masks, 3D/embedded media, external URLs, and native PDF editing.

</deferred>


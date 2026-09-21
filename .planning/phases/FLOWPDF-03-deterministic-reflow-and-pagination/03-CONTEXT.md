# Phase 3: Deterministic Reflow and Pagination - Context

**Gathered:** 2026-09-21
**Status:** Ready for execution planning

<domain>
## Phase Boundary

This phase turns the semantic FlowDocument into a deterministic, derived layout
and pagination result. It covers text shaping, font fallback, direction,
language-aware line breaking, Ukrainian hyphenation, block fragmentation,
page/section geometry, headers and footers, keep constraints, simple-table row
pagination, incremental invalidation, and the Rust/WASM-to-worker viewport
contract.

It does not deliver PDF syntax, PDF import, form-widget authoring, OCR, voice
capture, arbitrary CSS/HTML layout, multi-column editorial layout, footnotes,
complex table spans, floating/wrapped images, or universal font/PDF
compatibility. Those remain later phases or explicit deferred scope.

</domain>

<decisions>
## Locked Decisions

- **D-03-01:** FlowDocument remains the canonical semantic source. Layout,
  fragments, glyph runs, break decisions, and display-list coordinates are
  derived revision-bound values and are never persisted as document truth.
- **D-03-02:** All layout geometry uses a project-owned signed fixed-point
  `LayoutUnit` with one exact conversion policy. Floating-point values may be
  used only inside an isolated third-party adapter and must be rounded through
  checked integer arithmetic before entering the engine or a serialized DTO.
- **D-03-03:** Rust owns the layout request, font/locale/data identities,
  shaping, segmentation, fragmentation, invalidation, and equivalence result.
  TypeScript owns worker lifecycle, physical viewport scheduling, and rendering
  of an accepted immutable result; it does not calculate line breaks or page
  coordinates.
- **D-03-04:** ICU4X supplies grapheme and line-break boundaries, RustyBuzz
  (HarfBuzz-compatible) supplies glyph mapping/positioning, `unicode-bidi`
  supplies UAX #9 ordering, and a versioned Ukrainian hyphenation data adapter
  supplies discretionary break points. None of these libraries owns document
  pagination or fallback policy.
- **D-03-05:** Font fallback is an explicit, bounded ordered catalog. A font
  face is admitted only with stable bytes, face index, family/style metadata,
  and a content identity. Missing glyphs produce a deterministic fallback or a
  visible unsupported-glyph diagnostic; the engine never silently consults
  host-system fonts.
- **D-03-06:** Section geometry, static header/footer content, and the identity
  of the section boundary are durable document settings. Adding them requires
  a sequential schema-v2-to-v3 migration with v2 no-op and legacy-route
  coverage; an ephemeral layout options object cannot satisfy LAYO-04.
- **D-03-07:** Every accepted layout result carries the source revision,
  layout-settings fingerprint, engine/data/font identities, and a deterministic
  result hash. A worker result is publishable only when those values still
  match the active request; stale results are discarded without mutating the
  editor.
- **D-03-08:** Incremental reflow may reuse unaffected fragment prefixes or
  suffixes only when a checked carry signature proves identical inputs at the
  reuse boundary. A full reflow remains the reference implementation and the
  equivalence test oracle.
- **D-03-09:** Keep-with-next, keep-together, widow/orphan, and table-row
  constraints are preferences with deterministic bounded fallback. If a
  constraint cannot fit, the engine emits a stable break reason and makes the
  smallest legal split rather than looping or dropping content.
- **D-03-10:** The semantic DOM remains the accessibility and input surface.
  The page viewport/display-list is a synchronized visual projection and never
  replaces the accessible document copy or Rust-owned selection anchors.

## Agent Discretion

- The exact `LayoutUnit` scale, provided its range, overflow rules, rational
  conversions, serialization, and golden values are documented and tested.
- The concrete RustyBuzz/ttf-parser wrapper shape and font fixture, provided
  production code receives bytes through an explicit provider and tests do not
  depend on a developer machine's installed fonts.
- The bounded Ukrainian pattern-data packaging strategy, provided its Unicode
  version, source/provenance, license, hash, and WASM-size impact are recorded.
- The number of fragment node kinds and whether display-list construction is
  introduced in this phase or left as a thin adapter over layout fragments.
- Worker protocol names and React component boundaries, provided stale-result
  rejection and semantic-DOM synchronization are tested at the revision level.

## Deferred Ideas (OUT OF SCOPE)

- PDF page streams, embedded/subset fonts, PDF text extraction, annotations,
  AcroForm widgets, or PDF round-trip claims (Phase 4+).
- Arbitrary CSS, floats, columns, footnotes/endnotes, nested tables, merged
  cells, row/column spans, chart layout, and complex positioned objects.
- Host-system font discovery as a source of deterministic production output.
- Text justification heuristics that are not represented in the fixed-point
  break/fragment contract, and browser-native layout as an oracle.
- Exact universal support for every script/font before a checked-in font and
  data catalog exists; unsupported coverage must remain explicit.

## Required Evidence Posture

Phase 3 is not complete merely because a page-looking UI renders. Completion
requires exact Rust tests for shaping/cluster/fallback/direction/break data,
pagination/reflow fixtures, incremental/full equivalence properties, and the
worker stale-result contract. Browser evidence demonstrates interaction and
semantic synchronization; it cannot promote a layout implementation that lacks
the Rust determinism and provenance checks.

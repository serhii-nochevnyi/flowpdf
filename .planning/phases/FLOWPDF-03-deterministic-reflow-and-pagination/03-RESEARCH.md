# Phase 3: Deterministic Reflow and Pagination - Research

**Researched:** 2026-09-21
**Domain:** Rust text shaping, fixed-point flow layout, pagination, and browser worker projection
**Confidence:** MEDIUM

## Research Sources

The current dependency and API boundary was checked against the project source,
the pinned Rust toolchain, local crate sources, and Context7 documentation
lookups. Exact dependency versions remain subject to the same provenance and
lockfile gates used in Phase 2.

| Source | Finding | Planning consequence |
|---|---|---|
| ICU4X `icu_segmenter` 2.3.0 docs and source | `GraphemeClusterSegmenter::segment_str` and `LineSegmenter::segment_str` expose Unicode boundary opportunities over UTF-8 text; compiled data is a separately versioned input. | Keep ICU segmentation behind a Rust adapter and record the data identity in the layout fingerprint. Do not reimplement UAX boundary logic. |
| ICU4X Context7 lookup: `/unicode-org/icu4x` | The `DataProvider` pattern allows the engine to bind a concrete provider rather than ambient host data. | Production requests must choose a pinned provider/data mode; tests use the same explicit provider path. |
| HarfBuzz “What is HarfBuzz?” and “What HarfBuzz doesn't do” | Shaping maps characters to glyphs and positions; HarfBuzz does not perform paragraph line breaking, fallback policy, or complete document layout. | RustyBuzz is a shaping adapter only. Bidi, font fallback, line breaking, hyphenation, and pagination remain owned surrounding layers. |
| HarfBuzz cluster-level documentation | Cluster metadata and unsafe-to-break information matter when deciding whether a line may break between shaped glyphs. | The layout tracer must retain source UTF-8/UTF-16 cluster mapping and reject breaks that split an unsafe cluster. |
| Local `rustybuzz` 0.20.1 source | The crate exposes `Face`, `UnicodeBuffer`, `Direction`, script/language setters, cluster level/flags, `GlyphInfo.cluster`, and `GlyphPosition` advances/offsets. | Pin the adapter against these public types, add a small wrapper, and keep third-party units out of public FlowPDF DTOs. |
| Local `ttf-parser` 0.25.1 source | Safe face parsing and glyph/metric access are available from bytes; no host font installation is required. | Admit fonts through a bounded byte provider with face index and identity; validate before shaping. |
| Local `unicode-bidi` 0.3.18 metadata/source | UAX #9 ordering is available as a separate algorithm from shaping. | Build paragraph directional runs before shaping; do not infer direction from glyph order. |
| Local `hyphenation` 0.8.4 source/build metadata | Knuth-Liang pattern support includes Ukrainian data in the crate source, but embedding all languages would unnecessarily expand the WASM payload. | Choose and document a Ukrainian-only/versioned data admission path, with a size/provenance gate; never enable an unbounded all-language feature accidentally. |
| Project model and schema | Current schema v2 has page size/orientation/margins but no durable section/header/footer model and no layout module. | Plan a focused v3 migration before page/section behavior; keep the first tracer page-neutral enough to remain independently testable. |
| Project web shell | React currently projects a semantic document and has no layout worker or canvas/page viewport. | Add a revision-aware worker protocol and page projection after the Rust fragment contract is stable. |

## Architectural Findings

### 1. The first production slice must be a text-layout tracer

The safest vertical slice is a Rust request that receives a validated
FlowDocument plus an explicit font catalog and layout settings, shapes a single
paragraph, finds legal line-break opportunities, and returns fixed-point line
fragments with source cluster ranges. The same request should already expose a
deterministic fingerprint and rejection diagnostics. This proves the most
failure-prone seam before pagination and UI work build on it.

### 2. Fixed-point geometry needs a single conversion boundary

Existing semantic values are integer millipoints/millimetres. Font metrics and
shaper positions use font units or HarfBuzz-compatible integer units. The
engine should convert all inputs with checked rational arithmetic into one
signed `LayoutUnit` scale, perform line-width accumulation and page arithmetic
in that type, and serialize integers only. The exact scale is a plan decision,
not a reason to leak floats through the worker contract.

### 3. Shaping is not line breaking

Shaping must be performed per resolved directional/font/script run. The line
breaker then works over source cluster boundaries and candidate hyphen points,
using shaped advances. Fallback can split runs but must retain a stable source
cluster map. A line break is legal only at an ICU/hyphenation opportunity that
does not split an unsafe cluster; stored text is never modified to insert a
hyphen.

### 4. Layout settings are durable, fragments are not

Page dimensions, margins, sections, and static header/footer intent affect
canonical layout requests and must survive reopen/migrate. Glyph positions,
line decisions, page breaks, and display data are revision-bound caches. A
schema migration is therefore required for new durable settings, and migration
tests must prove v2 bytes remain readable and canonical content is unchanged.

### 5. Pagination should use explicit break reasons and bounded fallback

The fragment tree needs stable node IDs, source ranges, geometry, and break
reasons. Keep constraints are evaluated before the normal break, while
widow/orphan/table rules may request a bounded rollback. When impossible, a
stable fallback reason is emitted. This makes later visual diagnosis possible
without silently changing semantic content.

### 6. Incremental equivalence is an invariant, not an optimization claim

The full reflow output is the oracle. Incremental reflow may reuse a prefix or
tail only if the carry signature includes all state crossing the boundary:
section/page geometry, active keep state, list/table context, font/data
identities, and the incoming vertical position. Property-based edits should
compare normalized fragment trees and result hashes, not only page counts.

### 7. Worker scheduling is a revision protocol

The browser worker may receive multiple requests while the active page remains
interactive. Each result must be checked against the current document revision,
request ID, and layout fingerprint. A stale response is observable as a
discarded diagnostic, never as a partial React or canvas mutation.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Host font or ICU data drift | Explicit bytes/data providers, content identities, pinned versions, and a fail-closed catalog. |
| Shaper units or overflow are misconverted | Checked rational conversion tests, boundary values, and no floating-point public fields. |
| Ukrainian hyphenation silently changes | Versioned data artifact, fixture hashes, no mutation of stored text, and explicit discretionary-break metadata. |
| Bidi/fallback order is accidentally delegated to HarfBuzz | Separate direction/fallback stages and tests with mixed Ukrainian/English/RTL control samples. |
| Keep rules cause loops or content loss | Bounded rollback budget, explicit break reasons, and an impossible-constraint fallback fixture. |
| Incremental cache reuses stale state | Carry signature plus full/incremental property tests and revision-bound invalidation. |
| Worker result updates the wrong revision | Request/revision/fingerprint checks before store publication and browser stale-result tests. |
| Font fixtures have unclear licensing | Admit only checked-in fixtures with provenance/license metadata; block production catalog claims otherwise. |

## Recommended Plan Order

1. Build the fixed-point text-layout tracer and dependency/data seams.
2. Add schema-v3 layout settings for sections and static headers/footers.
3. Build the fragment tree and deterministic pagination rules.
4. Add incremental invalidation and full/incremental equivalence properties.
5. Expose the Rust/WASM worker protocol with stale-result rejection.
6. Add the page viewport and semantic/visual synchronization gate.

## Sources

- https://docs.rs/icu_segmenter/2.3.0/icu_segmenter/struct.GraphemeClusterSegmenter.html
- https://docs.rs/icu_segmenter/2.3.0/icu_segmenter/struct.LineSegmenter.html
- https://harfbuzz.github.io/what-is-harfbuzz.html
- https://harfbuzz.github.io/what-harfbuzz-doesnt-do.html
- https://harfbuzz.github.io/harfbuzz-shaper.html
- https://github.com/unicode-org/icu4x/blob/main/components/segmenter/README.md
- https://docs.rs/rustybuzz/0.20.1/rustybuzz/
- https://docs.rs/ttf-parser/0.25.1/ttf_parser/
- https://docs.rs/unicode-bidi/0.3.18/unicode_bidi/
- https://docs.rs/hyphenation/0.8.4/hyphenation/

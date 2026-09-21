# Phase 3: Deterministic Reflow and Pagination - Patterns

## Responsibility Map

| Capability | Rust core | WASM/worker | React/browser |
|---|---|---|---|
| Font/data identity and validation | Owns | Serializes immutable request | Supplies bytes through approved adapter only |
| BiDi, fallback, shaping, segmentation | Owns | Runs in worker-safe core | No browser layout oracle |
| Fixed-point geometry and breaks | Owns | Serializes DTO/hash | Paints accepted result |
| Section/page/header/footer settings | Canonical model + migration | Reads validated request | Controls dispatch only |
| Incremental invalidation | Owns dependency/carry graph | Carries request IDs | Schedules/cancels requests |
| Accessibility/input | Existing editor view/session | Revision bridge | Semantic DOM remains truth |

## Canonical and Derived Data Boundary

Durable `FlowDocument` data contains semantic blocks, inline runs, styles,
assets, fields, and the versioned page/section settings admitted by the schema.
It does not contain glyph IDs, shaped advances, line boxes, page coordinates,
fragment caches, worker request IDs, or display-list bytes.

Derived layout types should use closed Rust structs with explicit fields:

- `LayoutRequest`: source revision/hash, layout settings identity, viewport
  hint, font catalog identity, Unicode/hyphenation data identity, and bounded
  options.
- `LayoutResult`: accepted request identity, pages, fragment tree, diagnostics,
  engine/data identities, and result hash.
- `Fragment`: stable source node ID, source UTF-16 range, fixed-point rectangle,
  line/break metadata, and child fragments.
- `LayoutDiagnostic`: stable code, node/section identity where safe, and
  bounded numeric context; never authored text or font bytes.

## Stable Break and Carry Pattern

Every page/line decision has a closed break reason such as `natural`,
`explicitPageBreak`, `keepWithNext`, `widowOrphanFallback`, `tableRow`, or
`overflowFallback`. Incremental reuse is keyed by a carry signature that
includes the prior fragment boundary, incoming vertical position, section
settings, active keep/table state, and all font/data identities. If any input
differs, the engine recomputes from the earliest invalid boundary.

## Testing Pattern

- Golden text fixtures assert source cluster ranges, glyph run order, line
  widths, discretionary hyphen metadata, and fixed-point values.
- Pagination fixtures assert page/fragment tree shape and break reasons, not
  only rendered screenshots.
- Property tests generate bounded semantic documents and edits, then compare
  normalized full and incremental results byte-for-byte.
- Worker tests send revision N, mutate to N+1, deliver N out of order, and
  prove the stale result cannot update the accepted store.
- Browser tests assert the semantic DOM remains one accessible document copy
  and that the visual page surface reports the same accepted revision.

## Error/Unsupported Pattern

Unsupported fonts, scripts, data versions, or layout constructs are returned as
typed diagnostics and retained semantic content. The engine never falls back to
browser CSS, silently drops an unsupported block, or inserts stored hyphen
characters. A bounded fallback fragment may show an unsupported-glyph marker,
but the source node and text range remain inspectable.

## Existing Project Analogues

- Use `FlowDocument`/`schema` validation and sequential migration patterns from
  `crates/flow-core/src/model/mod.rs` and `crates/flow-core/src/schema/mod.rs`.
- Use Rust-owned immutable DTOs and capability projections from
  `crates/flow-core/src/editor_view/mod.rs` and `crates/flow-wasm/src/lib.rs`.
- Use the Phase 2 semantic document, external store, and browser test patterns
  in `web/src/editor/semantic-document.tsx`, `web/src/editor/editor-store.ts`,
  and `web/tests/*.browser.test.ts`.
- Use pinned toolchain commands and privacy-safe diagnostic conventions from
  `scripts/check-phase2.mjs` and the Phase 2 validation artifacts.

# Phase 4 Plan 04-05 Summary

**Status:** Complete
**Completed:** 2026-09-21

## Delivered

- Added a closed, bounded JSON-only PDF export/recovery boundary to the
  Rust/WASM crate. PDF bytes and private source streams cross into the browser
  only as bounded hex strings; Rust remains authoritative for construction,
  verification, manifest identity, and exact recovery classification.
- Added direct WASM boundary tests for deterministic export responses,
  response verification, exact source recovery, unknown-field rejection, and
  missing-payload diagnostics. Raw canonical source text is not included in
  worker diagnostics or debug projections.
- Added a single-flight TypeScript PDF worker scheduler with abort/supersede,
  request/source/layout/manifest guards, byte-hash verification, immutable
  accepted results, bounded response validation, and explicit privacy-safe
  diagnostics.
- Added the editor PDF snapshot/controller bridge. A text revision invalidates
  prior export state before a new result can render; selection-only session
  updates preserve a result whose source revision/hash and layout identity are
  unchanged. Export bytes remain derived download state and never enter the
  canonical editor store.
- Added a visual-only PDF preview adapter with bounded zoom, page navigation,
  page-window virtualization, source-backed search, selection highlighting,
  visible status/error announcements, and caller-owned PDF download URLs. Its
  page tree is `aria-hidden`; the semantic editor and input host remain the
  sole authored/accessibility surface.
- Added unit and browser fixtures covering stale results, identity mismatches,
  abort/failure, malformed and oversized requests, exact recovery failures,
  semantic DOM uniqueness, focus continuity, search-to-selection projection,
  virtualization, zoom/navigation, and stale export invalidation.

## Verification

- `cargo fmt --all` — passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed.
- `cargo test --locked -p flow-wasm` — 6 tests passed, including the new
  export/recovery boundary suite.
- `cargo test --locked -p flow-core` — passed, including the preserved
  `grapheme_conformance.rs` suite (9 tests).
- `npm run typecheck` — passed.
- `npm exec -- vitest run --project unit web/tests/pdf-worker.test.ts` — 5
  tests passed.
- `PLAYWRIGHT_BROWSERS_PATH=./work/playwright npm exec -- vitest run
  --project browser web/tests/pdf-preview.browser.test.ts` — passed.
- Targeted layout/accessibility browser regressions — 4 tests passed.
- `npm run build:web` — passed with the pinned local WASM/browser toolchain.

## Scope boundary

The browser now has a revision-safe export/preview adapter over the current
owned Rust PDF contracts. The PDF writer still needs end-to-end text/image
content-stream emission and external structural/extraction/visual reference
validation; default application wiring for a production font/layout catalog,
target-viewer compatibility, general PDF import, forms, and certification are
not claimed by this plan.

## Notes

- The preview never reparses browser text or measures CSS text. It paints only
  accepted Rust fixed-point rectangles and source-range provenance.
- Chromium evidence is local evidence only; it does not close the inherited
  Microsoft Edge/Windows screen-reader checkpoint from Phases 2 and 3.

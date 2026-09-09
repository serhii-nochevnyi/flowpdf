---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "03"
subsystem: canonical-schema
tags: [rust, schema, migration, rich-text, unicode]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plans 02-01 and 02-02 pinned dependencies and Unicode admission
provides:
  - Immutable private v0/v1 decoders and executed one-way compatibility gate
  - Closed schema-v2 semantic tree and typed inline formatting
  - Lossless contiguous migrations with explicit historical review states
affects: [editor-transactions, persistence, grapheme-selection, rich-text-shell]
tech-stack:
  added: []
  patterns:
    - Freeze historical wire decoders before changing public canonical types
    - Separate stored-document validation from strict new-authoring validation
    - Preserve invalid historical anchors without snapping or automatic promotion
key-files:
  created:
    - crates/flow-core/src/schema/legacy.rs
    - crates/flow-core/tests/schema_v2_migration.rs
    - fixtures/flowdoc/schema-v1-rich-text-valid.json
    - fixtures/flowdoc/schema-v1-rich-text-legacy-invalid.json
    - fixtures/flowdoc/schema-v2-rich-text.json
    - fixtures/flowdoc/schema-v2-rich-text.hash
  modified:
    - crates/flow-core/src/model/mod.rs
    - crates/flow-core/src/schema/mod.rs
    - crates/flow-core/src/transaction/mod.rs
    - artifacts/benchmarks/phase1-recovery.json
key-decisions:
  - Inline runs are the only canonical text representation; block style_id retains reusable defaults.
  - Migration-only MissingLegacy, LegacyUnknown and LegacyInvalid states remain loadable but cannot be newly authored.
  - Existing plain-text commands reject unsupported rich targets atomically until later command-algebra plans.
requirements-completed: [EDIT-01, EDIT-05, QUAL-04]
coverage:
  - id: D1
    description: Frozen legacy decoders and exact historical bytes survive the schema boundary.
    requirement: QUAL-04
    verification:
      - kind: integration
        ref: crates/flow-core/tests/schema_v2_migration.rs#legacy_freeze_gate
        status: pass
    human_judgment: false
  - id: D2
    description: Contiguous migration routes and v2 no-op preserve semantic and noncontent records.
    requirement: EDIT-05
    verification:
      - kind: integration
        ref: cargo test --locked -p flow-core --test schema_v2_migration -- migration_routes --nocapture
        status: pass
    human_judgment: false
  - id: D3
    description: Closed rich-text schema enforces nesting, authoring, and resource bounds.
    requirement: EDIT-01
    verification:
      - kind: integration
        ref: crates/flow-core/tests/schema_v2_migration.rs
        status: pass
      - kind: integration
        ref: npm run check
        status: pass
    human_judgment: false
duration: multi-session
completed: 2026-09-09
status: complete
---

# Phase 2 Plan 03: Lossless Schema-v2 Boundary Summary

FlowDocument now owns a closed semantic block tree with inline formatting, exact historical decoders, and deterministic v0 → v1 → v2 migration without invented field or image semantics.

## Performance

- Started: 2026-09-07, first RED commit at 11:32 UTC.
- Completed: 2026-09-09; interrupted multi-session execution, no reliable active-time total.
- Tasks: 2. Implementation/test/evidence files: 28.
- Native 200-page-equivalent / 1,000-transaction recovery: p50 510.316083 ms, p95 522.665458 ms, below the unchanged 2,000 ms threshold. The official recipe, workload proof and 100-transaction / 4 MiB checkpoint policy are unchanged.

## Accomplishments

- Froze self-contained private v0/v1 wire types, exact source hashes and five legacy payload hashes. The blocking gate ran green and was committed before current model/schema mutation. Original older/current/migrated fixture files remain byte-identical.
- Added paragraphs, headings, lists/items, images, tables/rows/cells with header state, and page breaks; typed bold/italic/underline/font/size/color/language marks and paragraph attributes. Inline runs are canonical, with explicit bounds and invalid nesting rejection.
- Implemented only contiguous migration hops and exact v2 no-op. Stable IDs, text bytes, styles, assets, fields and provenance remain retained. Empty historical alt is MissingLegacy, unknown fonts retain their spelling, and invalid anchors retain exact IDs/offsets with review reasons.
- Added four v2 JSON/hash golden pairs under new filenames. Loading admits explicit historical review states; new authoring rejects them. Plain-command compatibility never silently flattens rich text.

## Task Commits

1. Task 1 RED: `9ddaf43`; GREEN: `c57944f`.
2. Task 2 RED: `749f890`; GREEN: `05d26f1`.

## Verification

- Task 1 freeze gate executed before Task 2; source hash remains pinned by the test. The final full run also executed it successfully.
- Exact Task 2 route/schema/persistence command ran green twice on 2026-09-09: 4 migration tests, 9 schema tests and 7 persistence tests per run. Earlier executor runs repeated the same chain after the boundary-scan optimization.
- Full schema-v2 suite: 11/11, including malformed inputs, closed values, every block/mark type, bounds, no-loss records and atomic rejection.
- `npm run check` exited 0 on 2026-09-09 in 46.97 s: live provenance, locks, boundary contracts, formatting, all-target Clippy with warnings denied, full Rust tests, recovery evidence, WASM target/build, TypeScript, 29 unit/inspector tests, focused accessibility 4/4, Chromium 14/14 and two canonical/migration replay rounds.
- Recovery artifact was produced by the official benchmark on 2026-09-08; fresh source-bound validation passed on 2026-09-09. No threshold was relaxed.
- Main-session review inspected migration validation, closed tree/mark types, field-boundary scanning and narrow transaction/browser adapters. `git diff --check` passed; no tracked files were deleted. Original frozen decoder and historical fixtures have no diff.
- Post-wave pinned `cargo build --locked` passed and `npm test` passed 43/43 across 8 files. Schema-drift check did not block; codebase drift explicitly skipped because STRUCTURE.md is absent. Shared requirement readiness is 0/3, so no phase-level requirement was prematurely completed.

## Deviations from Plan

1. **[Rule 3 - Blocking] Adapt existing flat-model consumers.** The planned file list omitted current transaction/DTO/sample callers and Rust/browser tests that directly accessed removed flat text fields or hardcoded schema 1. Narrow adapters and current-v2 goldens restore those contracts without duplicating canonical state or rewriting old goldens. Browser migration now uses a genuine frozen v0 source instead of relabeling a current document. Included in `05d26f1`.
2. **[Rule 1 - Bug] Avoid unnecessary field-boundary scans.** Benchmark profiling exposed full trailing-text scans after all requested field positions had been reached. Validation now groups requests per node, borrows single-run text and stops at the greatest requested offset. Exact bytes/hashes remain unchanged; repeated tests and the unchanged benchmark pass. Included in `05d26f1`.
3. **[Rule 3 - Blocking] Refresh recovery evidence for the current source.** Adapted the benchmark model construction and regenerated its source-bound report through the official verifier. The semantic workload/recipe and limits are unchanged. Included in `05d26f1`.

Total deviations: three narrow compatibility/performance fixes. No PDF, layout, voice or advanced rich-command functionality was added.

## Issues Encountered

- Agent execution was interrupted by usage limits and session stops; the main session completed verification and atomic closeout from preserved edits.
- **Open tooling follow-up before the Phase 2 final gate:** `scripts/verify-wasm-size.mjs` copies current core source into both variants. With production ICU usage, its baseline now also links ICU, so a fresh feature-only delta cannot be described as Phase-1-compatible ICU admission growth. The old artifact remains unchanged historical evidence; no new current-source size pass is claimed. Resolve measurement semantics before refreshing that artifact or claiming the final size gate.

## User Setup Required

None.

## Next Phase Readiness

Ready for Plan 02-04's Rust grapheme/atomic-position validation and revision-bound editor-session authority, followed by Plan 02-05's core/WASM/persistence/React tracer. Schema foundations do not imply rich editing UI, pagination, PDF export or voice completion. Shared EDIT/QUAL requirements remain pending until every declaring plan finishes. Windows/Edge and assistive-technology UAT are not claimed.

## Self-Check: PASSED

All planned artifacts exist, RED/GREEN commits are reachable, both one-way gates executed in order, final verification is green, and unrelated files remain unstaged.

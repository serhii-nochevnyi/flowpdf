---
phase: FLOWPDF-03-deterministic-reflow-and-pagination
plan: "06"
subsystem: page-viewport-and-release-gate
tags: [viewport, accessibility, wasm, worker, pagination, release-gate]
requires:
  - phase: FLOWPDF-03-deterministic-reflow-and-pagination
    provides: Rust pagination, incremental-equivalence contract, and revision-safe WASM/worker boundary
provides:
  - Accessible page viewport projection over accepted Rust fixed-point fragments
  - Semantic/visual source-revision synchronization with stale-result protection
  - Deterministic Phase 3 validation manifest, smoke contract, and dual-run gate
  - Refreshed dependency provenance and recovery evidence for the Phase 3 workspace
affects: [phase-4-pdf-preview-and-export]
tech-stack:
  added: []
  patterns:
    - Keep the semantic DOM and input host authoritative while page visuals consume only accepted Rust geometry.
    - Publish layout only when request identity, source revision/hash, and result hash match the current editor state.
    - Treat retained phase gates and external evidence as explicit contracts; never convert unavailable AT evidence into a local pass.
key-files:
  created:
    - web/src/layout/page-viewport.tsx
    - web/src/layout/page-viewport.css
    - web/tests/layout-viewport.browser.test.ts
    - scripts/check-phase3.mjs
    - tests/contracts/phase3-gate.test.mjs
    - .planning/phases/FLOWPDF-03-deterministic-reflow-and-pagination/03-VALIDATION.md
  modified:
    - web/src/editor/editor-app.tsx
    - web/src/editor/editor-controller.ts
    - web/src/editor/editor-store.ts
    - web/src/editor/semantic-document.tsx
    - web/src/layout/layout-protocol.ts
    - web/src/layout/layout-worker.ts
    - web/tests/walking-skeleton.browser.test.ts
    - tests/contracts/phase1-boundary.test.mjs
    - tests/contracts/phase2-boundary.test.mjs
    - scripts/verify-dependency-provenance.mjs
    - config/dependency-provenance.json
    - artifacts/provenance/phase1-dependencies.json
    - artifacts/benchmarks/phase1-recovery.json
    - package.json
requirements-completed: [LAYO-01, LAYO-02, LAYO-03, LAYO-04, LAYO-05, LAYO-06, LAYO-07, LAYO-08]
coverage:
  - id: T1
    description: Accepted Rust page and fragment geometry is visible beside one synchronized semantic document surface.
    requirement: LAYO-03, LAYO-05, LAYO-06
    verification:
      - kind: browser
        ref: web/tests/layout-viewport.browser.test.ts
        status: pass
        note: Multi-page output, pending background layout, source revision/result hash parity, focus, and stale-result rejection are covered.
  - id: T2
    description: Page visuals cannot replace or become the only accessible representation of document content.
    requirement: LAYO-08
    verification:
      - kind: browser
        ref: web/tests/editor-accessibility.browser.test.ts
        status: pass
        note: Existing semantic DOM, input host, polite status, and keyboard/IME accessibility checks remain green with the viewport mounted.
  - id: T3
    description: The Phase 3 release gate covers every local layout requirement and retained regression lane with safe diagnostics.
    requirement: LAYO-01..LAYO-08
    verification:
      - kind: gate
        ref: scripts/check-phase3.mjs
        status: pass
        note: The exact manifest has 16 executable rows plus one terminal orchestration row; smoke and two consecutive full runs passed.
verification:
  - command: npm run build:web
    result: pass
    note: Pinned WASM build, generated web output, and web TypeScript compilation completed.
  - command: PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/layout-viewport.browser.test.ts web/tests/editor-accessibility.browser.test.ts
    result: pass
    note: Focused viewport/accessibility Chromium coverage passed.
  - command: npm run check
    result: pass
    note: Retained provenance, locks, Rust, WASM, TypeScript, unit, accessibility, browser, recovery, and replay lanes passed after refreshing current-schema evidence.
  - command: node scripts/verify-phase2-scale.mjs --smoke
    result: pass
    note: Phase 2 retained scale smoke passed without replacing the Phase 2 external AT checkpoint.
  - command: npm run check:phase3:smoke && npm run check:phase3 && npm run check:phase3
    result: pass
    note: Contract smoke passed 5/5; both full runs reported localTasks=16 and requirements=8/8.
  - command: git diff --check
    result: pass
    note: No whitespace errors.
external:
  phase2EdgeWindowsScreenReader:
    status: unavailable
    closure: outstanding
    note: Local Chromium/macOS evidence remains a non-substitute for the explicitly required Microsoft Edge on Windows plus Windows screen-reader run.
deviations:
  - The Phase 3 boundary policy admits only layout/editor paths and the two typed layout WASM exports while Phase 1/2 fixtures continue to reject deferred layout surfaces.
  - Exact dependency provenance now covers the four Phase 3 layout crates. rustybuzz 0.20.1 has an explicit, documented archived-upstream exception; checksum, exact source, and fail-closed provenance checks remain enforced.
  - The retained walking-skeleton migration assertion now targets the current v3 schema and verifies the complete 0-to-3 lineage.
duration: resumed multi-session
completed: 2026-09-21
status: complete
---

# Phase 3 Plan 06: Page Viewport and Gate Summary

Plan 03-06 is complete. FlowPDF now presents accepted Rust-derived page and
fragment geometry through a bounded page viewport while keeping the semantic
document and input host as the authoring/accessibility surface. The viewport
does not measure text or perform browser line breaking; it consumes fixed-point
coordinates and source ranges from the revision-safe layout scheduler.

## Accomplishments

- Added the page viewport and CSS projection with page navigation, pending/error
  status, source-revision/result-hash checks, and `aria-hidden` visual surfaces
  that cannot replace semantic content.
- Connected the viewport to the editor store/controller without blocking
  semantic commands, session publication, keyboard input, or IME behavior.
- Added real Chromium coverage for multi-page layout, pending background work,
  stale results, focus, and semantic/visual revision parity.
- Added an exact Phase 3 validation manifest and executable gate with bounded
  diagnostics, deterministic fingerprinting, retained Phase 1/2 local lanes,
  and honest inherited external-AT reporting.
- Reconciled current Phase 3 dependency provenance and refreshed the recovery
  benchmark artifact after the schema/layout source manifest changed.

## Scope boundary

This closes the deterministic reflow/pagination phase locally. It does not
claim a complete rich-text editor, broad arbitrary-script coverage, PDF reader
or writer behavior, forms, voice, or Windows assistive-technology validation.
The next PDF phase remains unplanned, and Phase 2 closure still depends on the
explicit external Edge/Windows screen-reader checkpoint.

## Self-check: PASSED

The focused viewport/accessibility browser tests, full retained `npm run check`,
scale smoke, Rust/WASM/web regressions, boundary contracts, and the Phase 3
smoke plus two consecutive full gate runs all pass. No unavailable external
AT evidence was reclassified as a local success.

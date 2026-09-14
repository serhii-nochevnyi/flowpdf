---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "17"
subsystem: ui-coverage-at-evidence
tags: [ui-contract, responsive, localization, accessibility, voiceover, edge, screen-reader]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plan 02-16 semantic editor shell, field projection, and localized accessibility surface
provides:
  - Deterministic UI-SPEC-derived 108-pair ownership contract with zero fallback buckets
  - Chromium responsive/state evidence for both locales at 320px and 1280px
  - Honest local VoiceOver and external Edge/Windows screen-reader evidence records
affects: [final-phase-2-gate]
tech-stack:
  added: []
  patterns:
    - Derive a closed, source-digested UI pair contract from the approved UI-SPEC taxonomy; stale, duplicate, omitted, or fallback owners fail closed.
    - Keep browser fallback evidence separate from manual assistive-technology evidence and never promote unavailable platform runs.
key-files:
  created:
    - scripts/build-phase2-ui-contract.mjs
    - tests/contracts/phase2-ui-considerations.json
    - tests/contracts/phase2-ui-contract.test.mjs
    - tests/contracts/phase2-at-evidence.test.mjs
    - tests/manual/phase2-local-voiceover.md
    - tests/manual/phase2-edge-windows-screen-reader.md
    - web/tests/editor-responsive.browser.test.ts
    - web/tests/editor-ui-states.browser.test.ts
  modified:
    - web/src/editor/editor.css
    - .planning/ROADMAP.md
    - .planning/STATE.md
key-decisions:
  - The approved UI-SPEC matrix is materialized as exactly 108 explicit source-digested entries with one concrete browser assertion owner per pair; backstop, unresolved, and unclassified buckets remain empty.
  - Responsive evidence exercises the same localized vocabulary, bounded 44px targets, local table overflow, and no page-level horizontal overflow at 320px and 1280px.
  - Local VoiceOver is recorded as not-run because no interactive VoiceOver session was exercised; Edge on Windows plus a Windows screen reader remains unavailable/outstanding and cannot be closed by Chromium or macOS evidence.
requirements-completed: [EDIT-01, EDIT-02, EDIT-03, EDIT-04, EDIT-05, QUAL-03, QUAL-04]
coverage:
  - id: T1
    description: The generated UI contract has exact sourceExplicit=108, coveredExplicit=108, backstop=0, unresolved=0, and unclassified=0 counts with deterministic owners.
    requirement: all
    verification:
      - kind: contract
        ref: tests/contracts/phase2-ui-contract.test.mjs and scripts/build-phase2-ui-contract.mjs --check
        status: pass
  - id: T2
    description: Omission, duplicate, source-drift, stale-owner, and fallback-classification mutations fail the coverage contract.
    requirement: all
    verification:
      - kind: contract
        ref: tests/contracts/phase2-ui-contract.test.mjs
        status: pass
  - id: T3
    description: Empty/loading/error/populated/partial/zero-one-many/long-text states and responsive overflow behavior are exercised in Ukrainian and English at 320px and 1280px.
    requirement: QUAL-03
    verification:
      - kind: browser
        ref: web/tests/editor-ui-states.browser.test.ts and web/tests/editor-responsive.browser.test.ts
        status: pass
        note: Focused run passed 2 state tests plus 4 responsive tests; full browser suite passed 15 files/37 tests.
  - id: T4
    description: AT evidence records accept only observed/not-run/unavailable local states and preserve the required external unavailable/outstanding status.
    requirement: QUAL-04
    verification:
      - kind: contract
        ref: tests/contracts/phase2-at-evidence.test.mjs
        status: pass
        note: Five evidence-state tests passed, including fabricated-pass and local-substitution rejection.
  - id: T5
    description: Existing Phase 1 and Phase 2 implementation regressions remain green after the UI closure work.
    requirement: all
    verification:
      - kind: gate
        ref: npm run check
        status: pass
        note: Dependency, boundary, Rust, WASM, TypeScript, unit, accessibility, browser, recovery, and deterministic replay gates passed.
verification:
  - command: node --test tests/contracts/phase2-ui-contract.test.mjs
    result: pass
    note: 7 contract/adversarial tests passed.
  - command: node scripts/build-phase2-ui-contract.mjs --check
    result: pass
    note: UI-COVERAGE-108 sourceExplicit=108 coveredExplicit=108 backstop=0 unresolved=0 unclassified=0.
  - command: npx --no-install vitest run --project browser web/tests/editor-responsive.browser.test.ts web/tests/editor-ui-states.browser.test.ts
    result: pass
    note: 6 Chromium tests passed across Ukrainian/English and 320/1280 responsive fixtures.
  - command: node --test tests/contracts/phase2-at-evidence.test.mjs
    result: pass
    note: Local VoiceOver remains not-run; external Edge/Windows plus Windows screen-reader remains unavailable/outstanding.
  - command: npm run typecheck
    result: pass
  - command: npm run test:browser
    result: pass
    note: Full real-Chromium suite passed 15 files/37 tests.
  - command: npm run check
    result: pass
    note: Full Phase 1 regression gate passed in 25.91 seconds, including two deterministic replay rounds.
completed: 2026-09-15
status: complete
---

# Phase 2 Plan 17: UI Coverage and AT Evidence Summary

Plan 02-17 is complete. The approved Phase 2 UI taxonomy is now a deterministic,
source-digested contract with exactly 108 explicit owners and no fallback,
unresolved, or unclassified entries. Browser evidence covers the state taxonomy,
localization, responsive widths, target sizes, table-local overflow, and bounded
long content without claiming that Chromium is a screen reader.

The local VoiceOver record is intentionally `not-run`: no interactive VoiceOver
session was exercised in this run. Microsoft Edge on Windows plus a Windows
screen reader remains `unavailable/outstanding`; the automated evidence contract
rejects fabricated pass/completion claims and rejects substitution of Chromium or
macOS evidence for that external checkpoint. This keeps P2-03 flagged and does
not claim Phase 2 completion.

## Accomplishments

- Added a deterministic UI-SPEC parser/generator and checked-in 108-entry ownership artifact.
- Added adversarial contract tests for omission, duplication, source drift, stale owners, and fallback classification.
- Added localized state and responsive Chromium tests with long tokens, invalid dialogs, mixed ranges, 44px controls, and table-local overflow.
- Added manual evidence records and a fail-closed AT status validator.
- Added a 44px target style for the secondary Diagnostics disclosure discovered by the responsive contract.

## Verification

- UI contract: exact `108/108/0/0/0`, deterministic repeated generation, and 7 contract tests — pass.
- AT evidence contract: 5 tests, including fabricated-pass and substitution rejection — pass.
- Focused browser task: 6 Chromium tests across both locales and both target widths — pass.
- Full browser suite: 15 files / 37 tests — pass.
- `npm run check`: full retained dependency, boundary, Rust, WASM, TypeScript, unit, accessibility, browser, recovery, and deterministic replay gate — pass.

## Next Phase Readiness

Ready for Plan 02-18: bounded semantic scale recipes, fast smoke feedback, and
the deterministic dual-run Phase 2 release gate.

## Self-check: PASSED

Code and evidence commit `04e7d05` is pushed to `origin/main`. Preserved
untracked backup, quick-plan, lock, and `.DS_Store` files remain untouched.

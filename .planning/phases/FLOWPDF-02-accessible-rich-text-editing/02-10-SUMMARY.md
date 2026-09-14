---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "10"
subsystem: accessible-formatting-controls
tags: [react, typescript, rust, wasm, formatting, lists, selection, accessibility, browser]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plan 02-09 closed formatting/list commands and Rust-owned editor projections
provides:
  - Localized native toolbar and block/list controls driven by accepted Rust EditorViewDto state
  - Shared visible/keyboard formatting and list command routes with durable parity evidence
  - Rust-session selection synchronization, pending marks, directional DOM restoration, and responsive focus behavior
affects: [tables, page-breaks, images, accessibility, command-parity, phase-2-gate]
tech-stack:
  added: []
  patterns:
    - Keep formatting values, capability reasons, mixed state, and pending marks in Rust DTOs; React owns only physical disclosure/focus state.
    - Route visible and keyboard formatting/list actions through the same typed controller and durable Rust command service.
    - Restore directional browser selections with Selection.setBaseAndExtent so reverse anchor/focus order survives DOM reconciliation.
key-files:
  created:
    - web/src/editor/block-controls.tsx
    - web/src/editor/editor-messages.ts
    - web/src/editor/editor-toolbar.tsx
    - web/src/editor/editor.css
    - web/tests/formatting-toolbar.browser.test.ts
    - web/tests/rich-text-formatting.browser.test.ts
  modified:
    - artifacts/benchmarks/phase1-recovery.json
    - crates/flow-core/src/editor_view/mod.rs
    - crates/flow-core/src/lib.rs
    - crates/flow-core/tests/rich_text_formatting.rs
    - web/src/editor/editor-app.tsx
    - web/src/editor/editor-controller.ts
    - web/src/editor/editor-store.ts
    - web/src/editor/input-adapter.ts
    - web/src/editor/selection-bridge.ts
    - web/src/editor/semantic-document.tsx
    - web/tests/editor-controller.test.ts
    - web/tests/rich-text-structure.browser.test.ts
    - web/tests/walking-skeleton.browser.test.ts
key-decisions:
  - Formatting and list controls consume accepted Rust projections and emit explicit targets; no semantic formatting or list validity is maintained in React.
  - Collapsed inline mark changes use the revision-bound Rust editor session, while non-collapsed formatting and all block/list mutations use durable transactions.
  - Durable replay accepts the complete closed formatting/list command vocabulary, preventing valid 02-09/02-10 transactions from failing recovery with RecoveryGap.
  - Browser selection restoration uses Selection.setBaseAndExtent for directional parity and wraps long metadata/toolbar content at the required mobile viewport.
requirements-completed: [EDIT-03, EDIT-04, QUAL-03, QUAL-04]
coverage:
  - id: T1
    description: Native Ukrainian/English toolbar controls expose Rust active, mixed, pending, disabled, and localized reason state with complete key parity.
    requirement: QUAL-03
    verification:
      - kind: browser
        ref: web/tests/formatting-toolbar.browser.test.ts
        status: pass
    human_judgment: false
  - id: T2
    description: Visible bold formatting and keyboard Ctrl/Cmd+B produce equal canonical JSON, hash, revision, and history traces.
    requirement: EDIT-04
    verification:
      - kind: browser
        ref: visible and keyboard inline-formatting routes have equal durable traces
        status: pass
    human_judgment: false
  - id: T3
    description: Block styles, alignment/spacing, list conversion, reverse selection, pending collapsed marks, invalid color rejection, and focus restoration remain Rust-owned.
    requirement: EDIT-03
    verification:
      - kind: browser
        ref: web/tests/rich-text-formatting.browser.test.ts
        status: pass
    human_judgment: false
  - id: T4
    description: Browser selection synchronization preserves directional anchor/focus order through toolbar commands and DOM reconciliation.
    requirement: EDIT-04
    verification:
      - kind: browser
        ref: reverse selection and structural browser suites
        status: pass
    human_judgment: false
  - id: T5
    description: Formatting transactions replay through the durable store and recover with the accepted Rust history projection.
    requirement: QUAL-04
    verification:
      - kind: unit
        ref: formatting_commit_replays_through_the_durable_store
        status: pass
    human_judgment: false
  - id: T6
    description: Full repository quality, recovery, WASM, unit, accessibility, browser, and deterministic replay gates remain green.
    requirement: QUAL-03
    verification:
      - kind: contract
        ref: npm run check
        status: pass
      - kind: recovery
        ref: recovery benchmark p95 517.575708 ms under the 2000 ms target; validator rejected 8 adversarial cases
        status: pass
completed: 2026-09-14
status: complete
---

# Phase 2 Plan 10: Accessible Formatting Controls Summary

Plan 02-10 is complete. FlowPDF now exposes the closed Rust formatting and list
vocabulary through localized native controls and shared visible/keyboard command
paths. The browser adapter synchronizes selection into the revision-bound Rust
session, and directional selections survive DOM restoration without becoming
semantic React state.

## Accomplishments

- Added typed formatting projections and controller commands for inline marks,
  block style/attributes, list conversion, continuation/exit, indent, and
  outdent.
- Added Ukrainian/English key-identical messages and a responsive toolbar with
  44px native controls, mixed/pressed state, disabled reasons, focus return,
  and mobile wrapping at 320px.
- Added Rust pending-mark session actions and list-item projection targets,
  plus durable replay coverage for every formatting/list command type.
- Preserved reverse selection direction through `Selection.setBaseAndExtent`
  and updated structural/walking browser assertions for the new accepted
  selection lifecycle.
- Added a durable-store formatting replay regression test and refreshed the
  official recovery benchmark artifact after the Rust source manifest changed.

## Verification

- `cargo fmt --all` — pass.
- `cargo test --locked -p flow-core --lib --tests` — pass.
- `npm run build:wasm && npm run typecheck` — pass.
- `PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx vitest run --project browser web/tests/formatting-toolbar.browser.test.ts web/tests/rich-text-formatting.browser.test.ts` — 5 tests pass.
- `npm run test:browser` — 9 files and 24 tests pass.
- `node scripts/verify-recovery-benchmark.mjs --validate` — pass.
- `node scripts/verify-recovery-benchmark.mjs --self-test` — pass: 8 adversarial cases rejected.
- `npm run check` — pass, including dependency gates, Clippy, all Rust tests, WASM, TypeScript, 32 unit/inspector tests, 4 accessibility tests, 24 Chromium tests, and both deterministic replay rounds.

## Next Phase Readiness

Ready for Plan 02-11: complete table headers/removal and page-break semantics
in Rust.

## Self-check: PASSED

Both tasks have named browser and Rust evidence, the full repository gate is
green, and pre-existing untracked backup/artifact files remain untouched.

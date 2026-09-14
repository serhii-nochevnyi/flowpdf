---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "12"
subsystem: native-structural-controls
tags: [react, typescript, rust, wasm, tables, page-breaks, accessibility, browser]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plan 02-11 bounded Rust table/page-break semantics and shared confirmation metadata
provides:
  - Native table/thead/tbody/th scope=col/td projection from accepted Rust header state
  - Focusable labeled page-break separators with explicit removal paths
  - Localized table insertion, row/column/header, page-break, and whole-table controls
  - Native destructive confirmation with safe cancel focus, Escape cancellation, and editor focus restoration
  - Rust-projected table-cell selections and row-major previous/next focus metadata
affects: [images, command-parity, accessibility, phase-2-gate]
tech-stack:
  added: []
  patterns:
    - Keep structural placement, table limits, header state, target IDs, confirmation metadata, and cell focus order in Rust DTOs.
    - Route visible structural actions, keyboard cell navigation, and destructive confirmation through the shared controller.
    - Render authored content as native semantic elements and text; do not use JSX form surfaces or infer semantic coordinates from DOM order.
key-files:
  created:
    - web/src/editor/structural-controls.tsx
    - web/tests/structural-blocks.browser.test.ts
    - web/tests/table-accessibility.browser.test.ts
  modified:
    - artifacts/benchmarks/phase1-recovery.json
    - crates/flow-core/src/editor_view/mod.rs
    - crates/flow-core/src/lib.rs
    - crates/flow-core/tests/structural_blocks.rs
    - web/src/editor/editor-controller.ts
    - web/src/editor/editor-messages.ts
    - web/src/editor/editor-store.ts
    - web/src/editor/editor.css
    - web/src/editor/semantic-document.tsx
key-decisions:
  - Native table header projection uses the accepted Rust table_header_rows value; header-enabled tables expose one thead row with th scope=col and the remaining rows in tbody.
  - Rust publishes each table cell's start selection and row-major previous/next cell IDs; React performs lookup and focus only.
  - Structural insertion consumes Rust parent/index placement and Rust-advertised dimension limits; browser validation only provides localized feedback before dispatch.
  - Whole-table removal and last-row/column escalation share Rust confirmation metadata, safe-cancel initial focus, Escape cancellation, and editor focus restoration.
requirements-completed: [EDIT-05, QUAL-03, QUAL-04]
coverage:
  - id: T1
    description: Ukrainian and English browser projections expose native header-aware tables, row-major cell navigation, and labeled focusable page-break separators.
    requirement: EDIT-05
    verification:
      - kind: browser
        ref: web/tests/table-accessibility.browser.test.ts
        status: pass
    human_judgment: false
  - id: T2
    description: Visible insertion and table actions dispatch the same typed structural controller commands used by the editor surface, while Rust supplies placement, bounds, target IDs, and focus metadata.
    requirement: QUAL-03
    verification:
      - kind: browser
        ref: web/tests/structural-blocks.browser.test.ts
        status: pass
      - kind: unit
        ref: table_projection_publishes_rust_owned_row_major_focus_order
        status: pass
    human_judgment: false
  - id: T3
    description: Whole-table removal and 1x1 last-dimension removal use one native alertdialog with cancel-first safe focus, destructive accept last, cancel/Escape preservation, and accepted undo/redo behavior.
    requirement: QUAL-04
    verification:
      - kind: browser
        ref: web/tests/structural-blocks.browser.test.ts
        status: pass
    human_judgment: false
  - id: T4
    description: Invalid table dimensions show a localized alert without publishing a revision, and page-break removal remains an explicit accepted structural command.
    requirement: QUAL-03
    verification:
      - kind: browser
        ref: web/tests/table-accessibility.browser.test.ts
        status: pass
    human_judgment: false
  - id: T5
    description: The complete repository quality, recovery, WASM, unit, accessibility, browser, and deterministic replay gates remain green after the native projection.
    requirement: QUAL-04
    verification:
      - kind: contract
        ref: npm run check
        status: pass
      - kind: recovery
        ref: recovery benchmark p95 518.653958 ms under the 2000 ms target; validator rejected 8 adversarial cases
        status: pass
verification:
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo fmt --all -- --check
    result: pass
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo clippy --locked --workspace --all-targets -- -D warnings
    result: pass
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked --workspace --all-targets
    result: pass
  - command: npm run build:wasm && npm run typecheck
    result: pass
  - command: PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/table-accessibility.browser.test.ts web/tests/structural-blocks.browser.test.ts
    result: pass
    note: 2 files and 4 tests passed in real Chromium.
  - command: npm run check
    result: pass
    note: Full gate passed with 32 unit/inspector tests, 6 focused accessibility tests, 11 browser files/28 tests, and both deterministic replay rounds.
completed: 2026-09-14
status: complete
---

# Phase 2 Plan 12: Native Structural Controls Summary

Plan 02-12 is complete. FlowPDF now exposes the accepted Rust table and
page-break semantics through native accessible browser elements and one shared
typed structural command path. The implementation keeps semantic placement,
limits, header state, destructive confirmation, and table-cell focus authority
in Rust while React owns only rendering, physical focus, and dialog state.

## Accomplishments

- Added localized native insertion controls and a table dialog with optional
  header row, Rust-advertised limits, safe cancel focus, and accessible errors.
- Projected header-enabled tables as `table`/`caption`/`thead`/`th scope=col`
  plus `tbody`/`td`; header-disabled tables remain `tbody`/`td` only.
- Added Rust-owned table-cell start selections and row-major neighbor IDs so
  Tab/Shift+Tab navigation does not infer semantic focus from the DOM.
- Added focusable labeled page-break separators and explicit remove actions,
  including keyboard Delete/Backspace handling through the controller.
- Added localized table row/column/header/remove controls and a shared native
  destructive alertdialog for whole-table and last-dimension removal.
- Added real-Chromium coverage for Ukrainian/English semantics, focus order,
  confirmation/Escape/cancel behavior, invalid bounds, page-break removal,
  undo/redo, and durable state preservation; refreshed the official recovery
  benchmark evidence after the Rust projection change.

## Verification

- `cargo fmt --all -- --check` — pass.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — pass.
- `cargo test --locked --workspace --all-targets` — pass, including 7 structural
  tests and 9 full Unicode grapheme-conformance tests.
- `npm run build:wasm && npm run typecheck` — pass.
- Focused structural Chromium command — pass: 2 files, 4 tests.
- `npm run check` — pass: dependency/boundary gates, recovery evidence,
  WASM, TypeScript, 32 unit/inspector tests, 6 focused accessibility tests,
  28 Chromium tests, and two deterministic replay rounds.

## Next Phase Readiness

Ready for Plan 02-13: complete image lifecycle, BLAKE3 identity, and shared
reachability.

## Self-check: PASSED

Both plan tasks have native semantic and browser evidence, the Rust-owned focus
boundary is explicit, the full repository gate is green, and pre-existing
untracked backup/artifact files remain untouched.

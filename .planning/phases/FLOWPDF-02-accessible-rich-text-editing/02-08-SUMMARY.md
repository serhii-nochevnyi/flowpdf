---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "08"
subsystem: structural-editor-surface
tags: [react, typescript, rust, wasm, chromium, keyboard, accessibility, durability]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plans 02-01 through 02-07, including Rust-owned editor sessions, controlled input, structural algebra, and durable recovery
provides:
  - Rust-derived structural capabilities, recursive semantic block projection, and post-command selection
  - Keyboard Enter/Backspace/Delete and visible split/merge routes through durable Rust structural commands
  - Chromium evidence for structural rejection, modality parity, undo/redo, reload, recovery, and cold remount
affects: [formatting, list-semantics, tables, fields, accessibility, recovery]
tech-stack:
  added: []
  patterns:
    - Translate browser intent into a closed Rust command and publish only the accepted durable projection
    - Derive deterministic split node IDs from the accepted document context so keyboard and visible traces converge canonically
    - Render nested list/table structure recursively from Rust DTOs; never use the DOM as semantic authority
key-files:
  created:
    - web/tests/rich-text-structure.browser.test.ts
  modified:
    - artifacts/benchmarks/phase1-recovery.json
    - crates/flow-core/src/editor_view/mod.rs
    - crates/flow-core/src/transaction/mod.rs
    - crates/flow-core/tests/editor_session.rs
    - web/src/editor/editor-app.tsx
    - web/src/editor/editor-controller.ts
    - web/src/editor/editor-store.ts
    - web/src/editor/input-adapter.ts
    - web/src/editor/selection-bridge.ts
    - web/src/editor/semantic-document.tsx
    - web/src/styles.css
key-decisions:
  - Rust capabilities and command validation remain the final authority; TypeScript only translates physical browser intent and restores the accepted result.
  - Structural mutations return a logical Rust selection and are persisted/re-queried before the semantic DOM is updated.
  - Keyboard and visible split parity uses a deterministic generated node identity; random command/audit identities remain distinct and opaque.
  - Recursive list and table projections are available now, while the existing immediate-parent list-item algebra is retained; sibling list-item semantics remain the explicit 02-09 scope.
requirements-completed: [EDIT-02, EDIT-03, QUAL-03, QUAL-04]
coverage:
  - id: T1
    description: Enter, boundary Backspace/Delete, and visible split/merge controls dispatch closed structural commands, prevent native mutation only for accepted intents, and preserve stable rejection state for incompatible structure.
    requirement: EDIT-03
    verification:
      - kind: browser
        ref: web/tests/rich-text-structure.browser.test.ts — structural keys and controls
        status: pass
    human_judgment: false
  - id: T2
    description: Accepted Rust block views recursively project paragraphs, headings, ordered/unordered lists, list items, tables, rows, and cells into semantic DOM with Rust preorder preserved.
    requirement: EDIT-02
    verification:
      - kind: browser
        ref: web/tests/rich-text-structure.browser.test.ts and full Chromium suite
        status: pass
    human_judgment: false
  - id: T3
    description: Equivalent keyboard and visible structural traces converge on canonical JSON, node IDs, revisions, and hashes, then remain exact through merge, undo/redo, reload, recovery, and cold remount.
    requirement: QUAL-04
    verification:
      - kind: browser
        ref: web/tests/rich-text-structure.browser.test.ts — durable structural parity
        status: pass
    human_judgment: false
  - id: T4
    description: The repository quality gate remains green after the browser structural surface and refreshed recovery evidence.
    requirement: QUAL-03
    verification:
      - kind: contract
        ref: npm run check
        status: pass
      - kind: test
        ref: cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace --all-targets
        status: pass
      - kind: test
        ref: npm run typecheck, unit/inspector (32/32), accessibility (4/4), and full Chromium (19/19)
        status: pass
    human_judgment: false
duration: resumed multi-session
completed: 2026-09-14
status: complete
---

# Phase 2 Plan 08: Durable Structural Editor Surface Summary

Plan 02-08 is complete. Browser structural intents now enter the same closed
Rust transaction path as other editor commands. The input adapter blocks native
mutation only when it has a valid accepted intent, waits for the durable
controller result, and restores the Rust-returned logical selection. Rejected
or stale structural requests leave the accepted document, revision, history,
and semantic projection unchanged.

## Accomplishments

- Added selection-aware Rust capability projection for split, merge, and
  subtree deletion, plus structural post-command selections.
- Routed keyboard Enter, boundary Backspace/Delete, and visible split/merge
  controls through the durable controller and WASM command boundary.
- Added recursive accepted DTO projection and semantic rendering for headings,
  ordered/unordered lists, list items, tables, rows, and cells.
- Added deterministic keyboard/visible split parity and lifecycle assertions
  covering merge, undo/redo, reload, recovery, and cold remount.
- Refreshed the recovery benchmark manifest after the Rust source expansion;
  the full repository gate validates the refreshed artifact.

## Verification

- `cargo fmt --all -- --check` — pass.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — pass.
- `cargo test --locked --workspace --all-targets` — pass.
- `npm run build:wasm` and `npm run typecheck` — pass.
- `npm run test:unit` plus inspector — 32/32 pass.
- Focused accessibility Chromium suite — 4/4 pass.
- `npm run test:browser` — 19/19 pass across 7 files.
- Structural filters — both pass; Vitest 4.1.11 uses `-t` for the plan's
  former `--grep` spelling.
- `npm run check` — pass, including dependency gates, Rust/WASM, TypeScript,
  browser accessibility, recovery self-test, and deterministic replays.

## Scope Boundary

The accepted recursive projection includes list and table structure, but the
existing Rust algebra intentionally keeps list-item split/merge in its current
immediate-parent container contract. Full sibling list-item insertion/removal
semantics are the explicit subject of Plan 02-09; this summary does not claim
those behaviors are complete.

## Next Phase Readiness

Ready for Plan 02-09: closed formatting, pending marks, and list semantics in
Rust.

## Self-check: PASSED

Both tasks have executable browser evidence and the complete repository gate
is green. Pre-existing untracked backup/artifact files remain untouched.

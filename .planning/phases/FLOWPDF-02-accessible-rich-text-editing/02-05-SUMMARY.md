---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "05"
subsystem: paragraph-tracer
tags: [rust, transaction, grapheme, wasm, indexeddb, react, accessibility]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plans 02-01 through 02-04 pinned dependencies, schema-v2 FlowDocument, grapheme positions, and Rust editor sessions
provides:
  - Rust ReplaceSelection transaction, inverse, accepted caret, and closed audit vocabulary
  - Rust-owned semantic document projection after guarded persistence and re-query
  - Durable React paragraph tracer with migration, undo/redo, reload, remount, and recovery coverage
affects: [controlled-input, ime, selection, structural-editing, formatting, accessibility]
tech-stack:
  added: []
  patterns:
    - Validate directional endpoints with the Rust grapheme map before deriving a scalar operation
    - Publish React semantic snapshots only after Rust persistence planning, IndexedDB commit, and Rust re-query
    - Keep canonical JSON opaque to React while projecting trusted text blocks and editor session DTOs
key-files:
  created:
    - crates/flow-core/tests/rich_text_transactions.rs
    - web/src/editor/editor-app.tsx
    - web/src/editor/semantic-document.tsx
    - web/src/main.tsx
  modified:
    - crates/flow-core/src/audit/mod.rs
    - crates/flow-core/src/editor_view/mod.rs
    - crates/flow-core/src/lib.rs
    - crates/flow-core/src/model/mod.rs
    - crates/flow-core/src/transaction/mod.rs
    - tsconfig.json
    - tsconfig.web.json
    - web/persistence/indexeddb-store.ts
    - web/src/i18n/en.ts
    - web/src/i18n/uk.ts
    - web/src/styles.css
    - web/tests/walking-skeleton.browser.test.ts
key-decisions:
  - ReplaceSelection accepts only same-paragraph directional grapheme endpoints and produces a scalar ReplaceText operation with an exact inverse; rich-run algebra remains a later structural plan.
  - Accepted caret/session state is retained through the durable commit and Rust query_editor_view step, while recovery without a transient session derives the default Rust session.
  - The product entry is React main.tsx; the existing main.ts remains a compatibility entry for the Phase 1 Foundation Inspector iframe contract and is excluded from the production tsc emit.
requirements-completed: [EDIT-01, EDIT-02, QUAL-04]
coverage:
  - id: T1
    description: ReplaceSelection validates stale revision, duplicate command identity, grapheme-safe directional endpoints, text limits, no-op behavior, and exact inverse/accepted caret.
    requirement: EDIT-01
    verification:
      - kind: integration
        ref: cargo test --locked -p flow-core --test rich_text_transactions -- paragraph_tracer --nocapture
        status: pass
    human_judgment: false
  - id: T2
    description: The native transaction result is planned, committed, undone/redone, and recovered through the existing Rust-owned record protocol with exact canonical/session/history checks.
    requirement: QUAL-04
    verification:
      - kind: integration
        ref: crates/flow-core/tests/rich_text_transactions.rs
        status: pass
    human_judgment: false
  - id: T3
    description: Real Chromium crosses migration, ReplaceSelection, guarded IndexedDB, Rust re-query, undo/redo, reload, cold remount, recovery, and native semantic rendering without a second canonical store.
    requirement: EDIT-02
    verification:
      - kind: browser
        ref: PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/walking-skeleton.browser.test.ts --testNamePattern "Phase 2 paragraph tracer"
        status: pass
    human_judgment: false
  - id: T4
    description: The Phase 2 boundary gate, full Rust workspace, TypeScript build, unit tests, and all browser tests remain green.
    requirement: QUAL-04
    verification:
      - kind: contract
        ref: node --test tests/contracts/phase1-boundary.test.mjs
        status: pass
      - kind: build
        ref: npm run build:web && npm run typecheck
        status: pass
      - kind: test
        ref: npm run test:unit && npm run test:browser
        status: pass
      - kind: test
        ref: cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace --all-targets
        status: pass
    human_judgment: false
duration: resumed multi-session
completed: 2026-09-14
status: complete
---

# Phase 2 Plan 05: Durable Paragraph Tracer Summary

Plan 02-05 is complete. A paragraph edit now crosses the production-shaped
boundary from a Rust-validated directional selection through immutable WASM,
Rust-planned IndexedDB persistence, Rust recovery/re-query, and a React semantic
projection.

## Accomplishments

- Added `CommandKind::ReplaceSelection` and its mutation path. Rust validates
  revision, command identity, same-node directional endpoints, scalar and
  extended-grapheme boundaries, text budgets, and no-op edits before creating a
  reversible `ReplaceText` operation. Rejected requests return no operation
  result or persistence plan.
- Added the `replaceSelection` transaction/audit vocabulary and recovery
  allowlist. Accepted operation results now include an immutable Rust-owned
  editor session plus semantic block projection; recovery derives the same
  projection from verified durable content.
- Preserved the accepted caret across the physical commit and the follow-up
  `query_editor_view` call. React never parses canonical JSON or owns a second
  semantic document store.
- Added a page-neutral React shell and native semantic paragraph projection.
  The tracer exposes migration, paragraph replacement, undo/redo, reload,
  recovery, durable status, revision/hash diagnostics, and a reachable
  Foundation Inspector disclosure. Authored block text is rendered as React
  text nodes without `innerHTML`, canvas, or `contenteditable`.
- Added a real browser test covering schema migration, Ukrainian/English text,
  durable edit, caret, undo/redo, reload, cold remount, recovery, and the
  semantic paragraph count.

## Verification

- `cargo fmt --all` — pass.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — pass.
- `cargo test --locked --workspace --all-targets` — pass.
- `cargo test --locked -p flow-core --test rich_text_transactions -- paragraph_tracer --nocapture` — pass.
- `npm run build:wasm` — pass.
- `npm run build:web` — pass.
- `npm run typecheck` — pass.
- `node --test tests/contracts/phase1-boundary.test.mjs` — 13/13 pass.
- `npm run test:unit` — 29/29 pass.
- `npm run test:browser` — 15/15 pass.
- The plan's Vitest `--grep` spelling is not supported by the pinned Vitest
  4.1.11 CLI; the equivalent `--testNamePattern` filter was used. Browser
  execution required the repository's prepared `PLAYWRIGHT_BROWSERS_PATH`.

The historical Phase 1 WASM-size evidence concern remains recorded in
[`STATE.md`](../../STATE.md); this plan does not refresh or reinterpret that
gate. The full `npm run check` wrapper was not rerun because its historical
size/provenance evidence is intentionally not regenerated here.

## Next Phase Readiness

Ready for Plan 02-06: controlled input, directional DOM selection bridging,
clipboard/paste, and real Chromium IME behavior. The current React tracer is a
durable vertical proof, not the complete editor input system or full rich-run
authoring surface.

## Self-check: PASSED

Both plan tasks have implementation artifacts and passing native, WASM, web,
contract, unit, and browser verification. Durable publication is gated by
Rust re-query, and unrelated pre-existing untracked files remain untouched.

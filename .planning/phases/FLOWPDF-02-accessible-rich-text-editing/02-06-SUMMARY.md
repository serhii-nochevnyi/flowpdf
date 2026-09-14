---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "06"
subsystem: controlled-editor-input
tags: [rust, wasm, react, selection, ime, clipboard, chromium, accessibility]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plans 02-01 through 02-05 pinned dependencies, schema-v2 FlowDocument, Rust grapheme/session authority, and the durable paragraph tracer
provides:
  - Rust-projected UTF-16 grapheme spans for the semantic editor view
  - Immutable accepted EditorStore snapshots and an EditorController transaction boundary
  - Directional DOM selection translation, controlled input, plain-text paste, and cancel-aware IME handling
  - Real Chromium CDP evidence for one composition transaction and one undo entry
affects: [structural-editing, deletion, formatting, accessibility, voice-input]
tech-stack:
  added: []
  patterns:
    - Keep React and DOM state as projections; publish only immutable accepted Rust snapshots after durable commit and re-query
    - Translate DOM ranges only through Rust-provided extended-grapheme UTF-16 spans and let Rust revalidate the logical candidate
    - Route typing, paste, and IME completion through one ReplaceSelection command with explicit cancellation, invalidation, and size limits
key-files:
  created:
    - scripts/verify-ime-chromium.mjs
    - web/src/editor/editor-controller.ts
    - web/src/editor/editor-store.ts
    - web/src/editor/input-adapter.ts
    - web/src/editor/selection-bridge.ts
    - web/tests/editor-controller.test.ts
    - web/tests/editor-input.browser.test.ts
  modified:
    - artifacts/benchmarks/phase1-recovery.json
    - crates/flow-core/src/editor_view/mod.rs
    - crates/flow-core/src/lib.rs
    - crates/flow-core/tests/rich_text_transactions.rs
    - web/src/editor/editor-app.tsx
    - web/src/editor/semantic-document.tsx
    - web/src/styles.css
key-decisions:
  - EditorStore caches the exact accepted immutable snapshot identity; pending and rejected work cannot replace the accepted semantic projection.
  - Rust owns the span map used by the DOM bridge; TypeScript does not parse canonical JSON, derive grapheme boundaries, or persist DOM coordinates.
  - The input host is a focusable offscreen textarea with only transient composition state; non-cancelled empty compositionend is a real replacement/deletion, while compositioncancel is not.
  - Local Chromium emitted the final CDP IME text as beforeinput insertText without compositionend, so the adapter treats that event as the single composition completion and prevents duplicate dispatch.
  - The full Phase 1 gate regenerated the recovery benchmark evidence because the Rust editor-view source manifest changed; the benchmark remains passing and the source manifest is current.
requirements-completed: [EDIT-01, EDIT-02, QUAL-03, QUAL-04]
coverage:
  - id: T1
    description: Stable external-store identity, directional selection serialization, and persistence rejection preserve the exact accepted projection and selection.
    requirement: EDIT-01
    verification:
      - kind: unit
        ref: npm run typecheck && npx --no-install vitest run web/tests/editor-controller.test.ts
        status: pass
    human_judgment: false
  - id: T2
    description: Keyboard, text/plain paste, empty/nonempty IME completion, cancellation, revision invalidation, malformed UTF-16, and exact byte/unit/envelope limits are exercised in the controller/browser lanes.
    requirement: EDIT-02
    verification:
      - kind: browser
        ref: npx --no-install vitest run --project browser web/tests/editor-input.browser.test.ts
        status: pass
      - kind: unit
        ref: web/tests/editor-controller.test.ts
        status: pass
    human_judgment: false
  - id: T3
    description: Real Chromium CDP IME input produces one durable revision and one undo operation, with no duplicate composition transaction.
    requirement: EDIT-02
    verification:
      - kind: browser
        ref: node scripts/verify-ime-chromium.mjs
        status: pass
    human_judgment: false
  - id: T4
    description: The complete repository quality gate remains green after the controlled-input implementation and evidence refresh.
    requirement: QUAL-03
    verification:
      - kind: contract
        ref: npm run check
        status: pass
      - kind: build
        ref: npm run build:wasm && npm run build:web && npm run typecheck
        status: pass
      - kind: test
        ref: cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace --all-targets
        status: pass
      - kind: test
        ref: npm run test:unit && npm run test:browser
        status: pass
    human_judgment: false
duration: resumed multi-session
completed: 2026-09-14
status: complete
---

# Phase 2 Plan 06: Controlled Input and Selection Bridge Summary

Plan 02-06 is complete. The paragraph tracer now has a controlled input
boundary: native DOM events are translated into Rust-owned logical selections,
and only the accepted, durably committed Rust projection is rendered back to
the semantic document.

## Accomplishments

- Extracted `EditorController` and `EditorStore` from the UI shell. The store
  retains stable immutable accepted snapshots, while the controller keeps
  command serialization, persistence, re-query, focus/selection restoration,
  and stable failure behavior in one place.
- Added Rust-projected UTF-16 ranges for every extended grapheme in paragraph
  and heading text. The DOM bridge accepts only matching span boundaries and
  preserves anchor/focus direction; it never persists DOM offsets or page
  coordinates.
- Replaced the uncontrolled text path with a focusable offscreen input host.
  Keyboard text, plain-text paste, and IME completion all use the same
  directional `ReplaceSelection` request. Composition cancellation and
  revision invalidation discard transient input and restore the accepted
  projection.
- Enforced composition UTF-8/UTF-16, paste UTF-8, and command-envelope
  budgets before dispatch, including malformed UTF-16 rejection and exact
  max/max+1 tests. Empty non-cancelled composition completion remains a real
  deletion/replacement rather than being confused with cancellation.
- Added browser/controller coverage and a standalone deterministic Playwright
  Chromium CDP verifier. The verifier proves a composition creates one
  revision and one undo entry.

## Verification

- `cargo fmt --all -- --check` — pass.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — pass.
- `cargo test --locked --workspace --all-targets` — pass.
- `npm run build:wasm` — pass.
- `npm run build:web` — pass.
- `npm run typecheck` — pass.
- `npm run test:unit` — 32/32 pass.
- `npm run test:browser` — 17/17 pass.
- `npx --no-install vitest run web/tests/editor-controller.test.ts` — 3/3 pass.
- `npx --no-install vitest run --project browser web/tests/editor-input.browser.test.ts` — 2/2 pass.
- `node scripts/verify-ime-chromium.mjs` — pass: one composition, one undo.
- `node --test tests/contracts/phase1-boundary.test.mjs` — 13/13 pass.
- `node scripts/verify-recovery-benchmark.mjs --self-test` — pass: 8 adversarial cases rejected.
- `npm run check` — pass, including dependency gates, recovery evidence,
  WASM, TypeScript, unit/browser suites, accessibility, and deterministic
  replay.

## Next Phase Readiness

Ready for Plan 02-07: structural editing, deletion/preimage behavior, and
recovery algebra. The controlled input path is intentionally still limited to
the existing paragraph `ReplaceSelection` surface; rich structural authoring
remains in the next plans.

## Self-check: PASSED

Both plan tasks have implementation artifacts and passing native, WASM, web,
contract, unit, browser, and standalone CDP verification. The only untracked
files left in the worktree are pre-existing user/backup artifacts and were not
staged.

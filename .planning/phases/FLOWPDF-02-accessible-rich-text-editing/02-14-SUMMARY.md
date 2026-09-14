---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "14"
subsystem: browser-image-lifecycle
tags: [rust, wasm, indexeddb, react, images, accessibility, recovery]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plan 02-13 bounded image staging, receipt-bound Rust lifecycle, and shared physical-byte retention
provides:
  - Versioned binary IndexedDB asset envelopes with legacy number-array reads and canonical identity checks
  - Atomic durable image commit/recovery through the existing Rust-planned IndexedDB transaction
  - Browser-safe WASM asset staging clock and receipt-only semantic image command routing
  - Accessible insert, replace, description/decorative, selected-image action, delete confirmation, undo, reload, and recovery flows
  - Object URL lifecycle management that keeps previews out of canonical/editor/audit DTOs and revokes unreachable URLs
affects: [command-parity, boundary-contracts, phase-2-gate]
tech-stack:
  added: []
  patterns:
    - Keep Uint8Array bytes at the dedicated stage_asset ingress; accepted semantic commands carry only the Rust-issued receipt, session binding, placement, node identity, and accessibility metadata.
    - Store physical asset bytes in a versioned binary envelope while normalizing legacy records through the same canonical BLAKE3 identity and length checks.
    - Retain physical asset records after replacement/removal until a future Rust-owned policy can prove safe collection across undo, recovery, and shared references.
key-files:
  created:
    - web/src/editor/embedded-blocks.tsx
    - web/tests/image-lifecycle.browser.test.ts
    - .planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-14-SUMMARY.md
  modified:
    - web/persistence/indexeddb-store.ts
    - web/tests/indexeddb-store.test.ts
    - crates/flow-core/src/asset/mod.rs
    - crates/flow-core/tests/recovery.rs
    - web/src/editor/editor-controller.ts
    - web/src/editor/editor-messages.ts
    - web/src/editor/editor-store.ts
    - web/src/editor/editor.css
    - web/src/editor/semantic-document.tsx
    - web/src/editor/structural-controls.tsx
    - Cargo.lock
    - artifacts/benchmarks/phase1-recovery.json
key-decisions:
  - The WASM-only staging clock calls the JavaScript Date API through the already-pinned wasm-bindgen dependency; native builds continue using SystemTime without adding a new crate provenance surface.
  - React owns file selection, temporary preview URLs, dialog state, and physical focus only; Rust capabilities, target IDs, placements, limits, confirmations, accessibility state, and command semantics remain authoritative.
  - MissingLegacy remains a visible review state and is never silently converted to decorative or invented alt text; new and edited images require a description or explicit decorative choice.
  - Preview failures do not reject an already durable semantic commit, while unreachable content-hash URLs are revoked after each verified Rust re-query and all URLs are revoked on controller disposal.
requirements-completed: [EDIT-05, QUAL-03, QUAL-04]
coverage:
  - id: T1
    description: New binary asset records and legacy number-array records reopen through one normalized DTO path and retain canonical content identity.
    requirement: QUAL-04
    verification:
      - kind: unit
        ref: web/tests/indexeddb-store.test.ts
        status: pass
    human_judgment: false
  - id: T2
    description: The real WASM/IndexedDB browser flow inserts, replaces, changes accessibility, cancels and confirms removal, and exercises undo, reload, and recovery without putting bytes in semantic commands.
    requirement: EDIT-05
    verification:
      - kind: browser
        ref: web/tests/image-lifecycle.browser.test.ts
        status: pass
    human_judgment: false
  - id: T3
    description: Rust-planned durable recovery retains the image asset record and restores the exact canonical JSON, hash, history, and asset count.
    requirement: QUAL-04
    verification:
      - kind: unit
        ref: crates/flow-core/tests/recovery.rs::image_asset_records_survive_durable_recovery
        status: pass
    human_judgment: false
  - id: T4
    description: Selected image controls are visible, localized, keyboard reachable, cancel-first confirmation safe, and routed through the shared typed controller command bus.
    requirement: QUAL-03
    verification:
      - kind: browser
        ref: web/tests/image-lifecycle.browser.test.ts
        status: pass
    human_judgment: false
  - id: T5
    description: Existing dependency, ownership, Rust, WASM, TypeScript, unit, accessibility, Chromium, recovery-evidence, and deterministic replay gates remain green.
    requirement: QUAL-04
    verification:
      - kind: contract
        ref: npm run check
        status: pass
      - kind: recovery
        ref: refreshed phase1-recovery.json; p95 503.461709 ms under the 2000 ms target; validator rejected 8 adversarial cases
        status: pass
    human_judgment: false
verification:
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo fmt --all -- --check
    result: pass
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test -p flow-core --test recovery -- image_asset --nocapture
    result: pass
  - command: PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/image-lifecycle.browser.test.ts
    result: pass
    note: Real Chromium lifecycle test passed with the release WASM and IndexedDB store.
  - command: npm run check
    result: pass
    note: Full gate passed with dependency and boundary contracts, clippy, all workspace Rust targets, release WASM, TypeScript, 34 unit/inspector tests, 6 focused accessibility tests, 12 Chromium files/29 tests, refreshed recovery evidence, and both deterministic replay rounds.
completed: 2026-09-15
status: complete
---

# Phase 2 Plan 14: Browser Image Lifecycle Summary

Plan 02-14 is complete. FlowPDF now carries the Rust-owned image lifecycle
through the browser durability boundary and exposes it through accessible
native controls. Physical bytes enter only through bounded `stage_asset` and
are stored as versioned binary envelopes; semantic commands remain receipt-only
and are revalidated by Rust before the browser publishes a state.

## Accomplishments

- Added binary IndexedDB asset envelopes with envelope/record versions,
  canonical content-hash and byte-length validation, atomic guarded commits,
  and read compatibility for older `number[]` records.
- Added a browser-safe WASM staging clock using the existing pinned
  `wasm-bindgen` dependency, avoiding the native-only `SystemTime` panic in
  Chromium.
- Added the controller image boundary, per-session staging identity, typed
  receipt-only insert/replace/accessibility/remove commands, verified preview
  synchronization, content-hash object URL reuse/revocation, and disposal
  cleanup.
- Added localized native image insertion/replacement and accessibility dialogs,
  visible selected-image actions, MissingLegacy review messaging, safe
  cancel-first destructive confirmation, focus restoration, and semantic
  `figure`/`img`/`figcaption` projection.
- Added real browser coverage for described images, replacement, accessibility
  changes, cancel/confirm deletion, undo, reload, recovery, and preview URLs;
  added a focused Rust recovery test for durable image records.

## Verification

- Rust formatting, clippy with warnings denied, all workspace Rust targets,
  release WASM build, and TypeScript — pass.
- Focused image Chromium lifecycle — pass.
- `npm run check` — pass: dependency/boundary contracts, 34 unit/inspector
  tests, 6 focused accessibility tests, 29 Chromium tests, refreshed recovery
  evidence, and two deterministic replay rounds.

The exhaustive mutation catalog and retained Phase 1/2 boundary audit remain
the next plan's work; this summary does not claim Phase 2 is complete.

## Next Phase Readiness

Ready for Plan 02-15: generate exhaustive command parity and retain boundary
contracts after the image lifecycle is available.

## Self-check: PASSED

Both plan tasks have browser, persistence, and recovery evidence; the full gate
is green; the code commit is `84c5eb3`; and pre-existing untracked backup and
artifact files remain untouched.

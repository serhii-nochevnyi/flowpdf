---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "13"
subsystem: image-lifecycle
tags: [rust, wasm, images, accessibility, recovery, persistence]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plan 02-12 Rust-owned structural commands, editor capabilities, and durable replay
provides:
  - Bounded PNG/JPEG staging with canonical BLAKE3 identity and one-use revision-bound receipts
  - Closed Rust InsertImage, ReplaceImage, SetImageAccessibility, and RemoveImage commands
  - Image capability, limit, accessibility, target, focus, and confirmation projections
  - Exact image operation inverses and recovery validation with shared physical-byte retention
  - Binary-only stage_asset WASM ingress and a Phase 2 boundary allowlist for its typed signature
affects: [image-persistence, accessible-image-controls, command-parity, phase-2-gate]
tech-stack:
  added: [image 0.25.10]
  patterns:
    - Validate image signatures, dimensions, pixels, allocation, and full decode in Rust before staging.
    - Keep receipts and staging digests outside canonical document, transaction, audit, and editor DTOs.
    - Retain physical asset records on replacement/removal until a future persistence policy proves safe garbage collection.
key-files:
  created:
    - crates/flow-core/src/asset/mod.rs
    - crates/flow-core/tests/asset_staging.rs
    - .planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-13-SUMMARY.md
  modified:
    - crates/flow-core/src/audit/mod.rs
    - crates/flow-core/src/editor_view/mod.rs
    - crates/flow-core/src/lib.rs
    - crates/flow-core/src/model/mod.rs
    - crates/flow-core/src/store/mod.rs
    - crates/flow-core/src/transaction/mod.rs
    - crates/flow-core/tests/editor_session.rs
    - crates/flow-wasm/src/lib.rs
    - tests/contracts/phase1-boundary.test.mjs
    - artifacts/benchmarks/phase1-recovery.json
key-decisions:
  - Canonical content_hash remains the existing lowercase blake3: asset_hash result; staging_digest is receipt-private and noncanonical.
  - A staged receipt is bound to session, document, and revision, expires after five minutes, is capped at two receipts/16 MiB per session, and can be redeemed once.
  - New image authoring accepts only Described with nonempty bounded text or explicit Decorative; MissingLegacy cannot be authored through lifecycle commands.
  - Replace preserves image node identity and placement; remove/replacement retain shared physical bytes so undo and recovery cannot observe premature deletion.
  - Existing durable asset records are merged into the Rust validation proof without copying them into every semantic response or transaction.
requirements-completed: [EDIT-05]
coverage:
  - id: T1
    description: Hostile PNG/JPEG staging enforces encoded, dimension, pixel, decoded-allocation, session, expiry, and replay bounds while returning only privacy-safe stable errors.
    requirement: EDIT-05
    verification:
      - kind: unit
        ref: crates/flow-core/tests/asset_staging.rs::staging_rejects_signature_size_and_session_overflow
        status: pass
    human_judgment: false
  - id: T2
    description: Insert and replace consume revision-bound receipts, publish canonical asset references without raw bytes/receipts, and reject MissingLegacy authoring.
    requirement: EDIT-05
    verification:
      - kind: unit
        ref: crates/flow-core/tests/asset_staging.rs::insert_image_redeems_receipt_without_leaking_it_into_semantics
        status: pass
    human_judgment: false
  - id: T3
    description: Image accessibility changes, replacement, confirmed removal, undo, redo, durable commit, and recovery preserve exact semantic state and shared physical bytes.
    requirement: EDIT-05
    verification:
      - kind: unit
        ref: crates/flow-core/tests/asset_staging.rs::image_lifecycle_is_exactly_reversible_and_recoverable
        status: pass
    human_judgment: false
  - id: T4
    description: Rust editor views expose image metadata, limits, capabilities, explicit target IDs, and destructive confirmation keys without receipts or bytes.
    requirement: EDIT-05
    verification:
      - kind: unit
        ref: crates/flow-core/tests/editor_session.rs
        status: pass
    human_judgment: false
  - id: T5
    description: Existing repository quality, recovery, WASM, TypeScript, unit, accessibility, Chromium, and deterministic replay gates remain green after the image core.
    requirement: QUAL-04
    verification:
      - kind: contract
        ref: npm run check
        status: pass
      - kind: recovery
        ref: recovery benchmark p95 514.788417 ms under the 2000 ms target; validator rejected 8 adversarial cases
        status: pass
    human_judgment: false
verification:
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo fmt --all -- --check
    result: pass
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo clippy --workspace --all-targets -- -D warnings
    result: pass
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test -p flow-core
    result: pass
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo build -p flow-wasm --target wasm32-unknown-unknown --release
    result: pass
  - command: node --test tests/contracts/phase1-boundary.test.mjs
    result: pass
  - command: npm run check
    result: pass
    note: Full gate passed with 32 unit/inspector tests, 6 focused accessibility tests, 11 Chromium files/28 tests, and both deterministic replay rounds.
completed: 2026-09-15
status: complete
---

# Phase 2 Plan 13: Image Lifecycle Summary

Plan 02-13 is complete. FlowPDF now has a Rust-owned image lifecycle contract
for bounded staging, explicit accessibility intent, reversible semantic image
commands, and durable recovery. The browser/WASM boundary accepts image bytes
only through the dedicated `stage_asset` ingress; semantic commands and public
projections contain no raw bytes, staging digest, or receipt secret.

## Accomplishments

- Added strict PNG/JPEG staging limits: 8 MiB encoded, 8,192 pixels per axis,
  24M pixels, and 96 MiB decoded allocation, with full decode verification.
- Added canonical `blake3:` identity, opaque five-minute one-use receipts,
  revision/session binding, and per-session receipt/byte budgets.
- Added closed InsertImage, ReplaceImage, SetImageAccessibility, and
  confirmation-gated RemoveImage commands with exact inverse operations,
  capabilities, image limits, focus hints, and recovery vocabulary.
- Added atomic semantic asset reference updates and a safe physical-record
  policy that retains shared/undo-relevant bytes instead of collecting them
  prematurely.
- Added core lifecycle/recovery tests and updated the static WASM boundary
  contract for the intentionally binary `stage_asset(bytes, request)` export.

## Verification

- Rust formatting, clippy with warnings denied, all `flow-core` tests, and
  release WASM build — pass.
- `npm run check` — pass: dependency/boundary gates, refreshed recovery
  evidence, WASM, TypeScript, 32 unit/inspector tests, 6 focused accessibility
  tests, 28 Chromium tests, and two deterministic replay rounds.

The browser persistence/UI lifecycle and real image controls remain the next
plan's work; this summary does not claim those browser features are complete.

## Next Phase Readiness

Ready for Plan 02-14: carry image staging through atomic IndexedDB persistence
and accessible browser controls.

## Self-check: PASSED

Both plan tasks have Rust-owned semantic, bounded-ingress, inverse, and
recovery evidence; the full repository gate is green; and pre-existing
untracked backup/artifact files remain untouched.

---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "15"
subsystem: command-parity-boundaries
tags: [rust, wasm, typescript, accessibility, command-parity, boundaries]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plan 02-14 image lifecycle, receipt-only asset routing, and complete Phase 2 mutation surface
provides:
  - Exhaustive Rust-owned capability metadata for every Phase 2 mutation family
  - Deterministic visible/keyboard/future-voice parity contract and trace coverage
  - Retained Phase 1/2 ownership, DOM, WASM, byte-separation, and deferred-scope boundary gates
affects: [field-accessibility, ui-accessibility, final-phase-2-gate]
tech-stack:
  added: []
  patterns:
    - Derive the parity catalog from the closed Rust Mutation enum and fail compilation/tests when a new mutation lacks metadata.
    - Future voice is a modality label over the same command constructor; capability, validation, confirmation, undo, DTO, hash, and revision behavior remain shared.
    - Keep physical image bytes at staging/storage boundaries and keep semantic commands receipt-only; static contracts reject byte/hash leakage and deferred subsystem authority.
key-files:
  created:
    - crates/flow-core/src/capability/mod.rs
    - crates/flow-core/tests/command_parity.rs
    - tests/contracts/phase2-boundary.test.mjs
    - tests/contracts/phase2-command-parity.json
    - .planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-15-SUMMARY.md
  modified:
    - crates/flow-core/src/editor_view/mod.rs
    - crates/flow-core/src/lib.rs
    - tests/contracts/phase1-boundary.test.mjs
    - artifacts/benchmarks/phase1-recovery.json
key-decisions:
  - The closed `Mutation` enum is the source for an exhaustive 33-entry capability catalog; `Batch`, `Undo`, and `Redo` are command operations, not mutation families.
  - Visible, keyboard, and future voice routes expose the same stable intent, risk, confirmation, and reversible undo metadata; modality may remain observable only in audit metadata.
  - Phase 1/2 boundary contracts remain explicit and adversarial: Rust owns semantics, WASM exports remain immutable, DOM stays native/safe, image bytes stay outside semantic DTOs, and layout/PDF/voice/backend/field-authoring scope remains deferred.
requirements-completed: [QUAL-03]
coverage:
  - id: T1
    description: Every current mutation family has exactly one stable Rust capability record with visible, keyboard, and future-voice bindings.
    requirement: QUAL-03
    verification:
      - kind: unit
        ref: crates/flow-core/tests/command_parity.rs
        status: pass
  - id: T2
    description: Equivalent visible, keyboard, and future-voice traces preserve semantic DTOs, hashes, revisions, and undo history while allowing modality-only audit metadata.
    requirement: QUAL-03
    verification:
      - kind: unit
        ref: crates/flow-core/tests/command_parity.rs
        status: pass
  - id: T3
    description: Checked-in parity JSON is deterministic, exhaustive, nonempty, unique, and bound to the Rust catalog.
    requirement: QUAL-03
    verification:
      - kind: contract
        ref: tests/contracts/phase2-command-parity.json and tests/contracts/phase1-boundary.test.mjs
        status: pass
  - id: T4
    description: Ownership, safe-DOM, immutable-WASM, image-byte/hash separation, and deferred-scope boundary rules retain adversarial negative fixtures.
    requirement: QUAL-03
    verification:
      - kind: contract
        ref: tests/contracts/phase2-boundary.test.mjs
        status: pass
  - id: T5
    description: Existing workspace, browser, recovery, and deterministic replay evidence remains green after the parity contract change.
    requirement: QUAL-04
    verification:
      - kind: contract
        ref: npm run check
        status: pass
      - kind: recovery
        ref: refreshed artifacts/benchmarks/phase1-recovery.json; p95 510.630417 ms under the 2000 ms target; validator rejected 8 adversarial cases
        status: pass
verification:
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo fmt --all -- --check
    result: pass
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo clippy --locked --workspace --all-targets -- -D warnings
    result: pass
  - command: RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked --workspace --all-targets
    result: pass
  - command: node --test tests/contracts/phase2-boundary.test.mjs tests/contracts/phase1-boundary.test.mjs
    result: pass
    note: 29 contract tests passed, including the retained Phase 1 gate.
  - command: npm run typecheck
    result: pass
  - command: npm run check
    result: pass
    note: Full dependency, boundary, Rust, WASM, TypeScript, unit, accessibility, Chromium, recovery-evidence, and deterministic replay gate passed.
completed: 2026-09-15
status: complete
---

# Phase 2 Plan 15: Command Parity and Boundary Summary

Plan 02-15 is complete. FlowPDF now has one Rust-owned capability contract for
the complete Phase 2 mutation surface. Visible controls, keyboard commands, and
the future voice seam resolve to the same command metadata and semantic
transaction path; modality remains an audit distinction rather than a second
authority.

## Accomplishments

- Added a closed 33-family `MutationFamily` catalog with exhaustive capability
  metadata for stable intent, risk, confirmation, undo, visible routing,
  keyboard routing, and future voice routing.
- Added deterministic parity JSON and Rust traces that compare visible,
  keyboard, and future-voice outcomes across DTOs, hashes, revisions, history,
  and modality-only audit differences.
- Added phase-aware boundary contracts covering Rust semantic ownership,
  immutable WASM, safe native DOM, deferred layout/PDF/voice/backend scope,
  field-authoring exclusion, and separation of physical image bytes/hashes from
  semantic commands.
- Refreshed recovery evidence after the Rust catalog change; the p95 remained
  below the Phase 1 recovery target.

## Verification

- Formatting, warnings-denied clippy, and all locked workspace Rust targets —
  pass.
- Phase 1/2 boundary contract — 29 tests pass.
- TypeScript, release WASM, unit/accessibility/browser suites, recovery
  validation, and deterministic replay — pass through `npm run check`.

The plan's local QUAL-03 parity predicates are covered. External
Windows/Edge/screen-reader evidence and the remaining Phase 2 accessibility
plans are still open; this summary does not claim Phase 2 complete.

## Next Phase Readiness

Ready for Plan 02-16: keep existing fields and the complete editor shell
semantically accessible without adding field authoring or filling commands.

## Self-check: PASSED

Both plan tasks have executable Rust and boundary evidence; the full gate is
green; code commit `5c1280d` is pushed; and pre-existing untracked backup and
artifact files remain untouched.

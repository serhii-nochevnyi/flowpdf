---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "11"
subsystem: structural-block-semantics
tags: [rust, transactions, tables, page-breaks, accessibility, undo, recovery]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plan 02-10 closed formatting/list controls and Rust-owned editor projection
provides:
  - Bounded page-break insertion/removal with atomic behavior, editable focus, and exact inverse replay
  - Header-aware simple tables with bounded dimensions, row/column edits, nearest-survivor focus, and exact inverse replay
  - Rust-derived structural capabilities and shared destructive confirmation metadata
  - Durable recovery evidence for structural table transactions
affects: [structural-controls, table-accessibility, images, command-parity, phase-2-gate]
tech-stack:
  added: []
  patterns:
    - Use explicit parent/index placements for structural insertion; never infer semantic placement from DOM order.
    - Generate structural IDs from the accepted command and retain exact subtree preimages for inverse/recovery.
    - Keep table bounds, header state, focus, limits, and destructive confirmation in Rust projections.
key-files:
  created:
    - crates/flow-core/tests/structural_blocks.rs
  modified:
    - crates/flow-core/src/audit/mod.rs
    - crates/flow-core/src/editor_view/mod.rs
    - crates/flow-core/src/lib.rs
    - crates/flow-core/src/model/mod.rs
    - crates/flow-core/src/schema/mod.rs
    - crates/flow-core/src/transaction/mod.rs
    - crates/flow-core/tests/editor_session.rs
key-decisions:
  - Page-break insertion creates an atomic separator followed by a deterministic empty paragraph; explicit removal deletes only the separator so the document remains editable.
  - Simple tables retain the existing bounded `header_rows` sentinel, accepting only an optional first header row while preserving the established schema shape.
  - Last-row/last-column deletion escalates to the same destructive confirmation metadata as whole-table removal; the transaction layer rejects unconfirmed removal before mutation.
  - Table row/column changes clone only bounded accepted structures, preserve unaffected IDs/order, and return Rust-selected row-major nearest-survivor focus.
requirements-completed: [EDIT-01, EDIT-03, EDIT-05, QUAL-03, QUAL-04]
coverage:
  - id: T1
    description: Page breaks insert at start, middle, and end through explicit parent/index placement, remain atomic against ordinary text deletion/merge, and remove/replay exactly.
    requirement: EDIT-05
    verification:
      - kind: unit
        ref: page_break_placement_is_atomic_and_exactly_reversible
        status: pass
    human_judgment: false
  - id: T2
    description: Table insertion accepts bounded dimensions and optional header state, while zero/overflow dimensions reject without mutation.
    requirement: EDIT-05
    verification:
      - kind: unit
        ref: table_bounds_and_last_dimension_removal_fail_closed_before_mutation
        status: pass
    human_judgment: false
  - id: T3
    description: Row/column add/remove operations preserve unaffected IDs and order, focus the new or nearest surviving cell, and undo back to the inserted table.
    requirement: EDIT-03
    verification:
      - kind: unit
        ref: table_commands_preserve_ids_focus_headers_and_exact_history
        status: pass
    human_judgment: false
  - id: T4
    description: Header toggles are reversible and idempotent; whole-table and last-dimension removal use shared destructive confirmation metadata and preserve an editable fallback paragraph.
    requirement: QUAL-03
    verification:
      - kind: unit
        ref: table_capabilities_project_shared_confirmation_metadata_and_header_state
        status: pass
      - kind: unit
        ref: table_bounds_and_last_dimension_removal_fail_closed_before_mutation
        status: pass
    human_judgment: false
  - id: T5
    description: Duplicate/stale structural delivery is rejected without a second mutation and accepted table transactions survive durable store recovery.
    requirement: QUAL-04
    verification:
      - kind: unit
        ref: structural_command_duplicates_and_stale_bases_do_not_mutate
        status: pass
      - kind: unit
        ref: structural_transactions_replay_through_the_durable_store
        status: pass
    human_judgment: false
verification:
  - command: PATH=/Users/serhii/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH cargo test --locked --ignore-rust-version -p flow-core --lib --tests --quiet
    result: pass
    note: The repository requires Rust 1.97.1; this machine has that rustc without its standard library and only a complete stable cargo toolchain, so the equivalent test gate used the available compiler with the version guard bypassed.
  - command: git diff --check
    result: pass
  - command: cargo fmt --all -- --check
    result: unavailable
    note: cargo-fmt is not installed in the available local Rust toolchains.
  - command: cargo clippy --locked --ignore-rust-version -p flow-core --all-targets -- -D warnings
    result: unavailable
    note: cargo-clippy is not installed in the available local Rust toolchains.
completed: 2026-09-14
status: complete
---

# Phase 2 Plan 11: Structural Blocks Summary

Plan 02-11 is complete. FlowPDF now owns bounded page-break and simple-table
semantics in Rust, including optional header state, row/column editing,
whole-table removal, destructive confirmation metadata, deterministic focus,
and exact inverse/recovery behavior. The browser projection remains the next
plan's responsibility.

## Accomplishments

- Added explicit page-break and table command/mutation vocabulary with audit and
  durable replay coverage.
- Added checked table row, column, and product limits before generated structure
  is allocated; schema validation now enforces the same cell budget.
- Preserved unaffected table IDs and order through row/column edits, and added
  nearest-survivor or newly-created-cell focus in the Rust editor result.
- Added idempotent header toggling, confirmation-gated last-dimension and whole-
  table removal, editable fallback paragraphs, and exact subtree preimages.
- Added six structural tests covering boundaries, atomic rejection, stale and
  duplicate delivery, capabilities, focus, undo/redo, and durable recovery.

## Verification

- Core library and integration tests — pass: 6 structural tests plus all
  existing flow-core targets passed with the available stable compiler.
- `git diff --check` — pass.
- Rust format and Clippy commands are recorded as unavailable because the
  current environment lacks `cargo-fmt` and `cargo-clippy`; this is an
  environment limitation, not a claimed quality-gate pass.

## Next Phase Readiness

Ready for Plan 02-12: expose native table/page-break semantics, controls,
keyboard parity, and destructive confirmation in the browser.

## Self-check: PASSED

The named Rust acceptance behaviors and durable replay are covered, the
implementation was committed and pushed, and pre-existing untracked backup and
artifact files remain untouched.

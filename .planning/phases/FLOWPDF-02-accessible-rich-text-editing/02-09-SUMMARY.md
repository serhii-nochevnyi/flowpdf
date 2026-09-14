---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "09"
subsystem: formatting-list-semantics
tags: [rust, transactions, editor-session, formatting, lists, graphemes, undo, recovery]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plans 02-01 through 02-08, including grapheme-safe positions, Rust-owned sessions, structural algebra, and durable semantic editor projection
provides:
  - Closed inline mark, block style/attribute, and list command vocabulary with stable validation codes
  - Exact range formatting, Rust-owned pending state, mixed block/inline projection, and reversible list transformations
  - Property/example evidence for bounds, encoding, direction, idempotency, stale delivery, inverse replay, list continuation/exit, and depth limits
affects: [formatting-controls, keyboard-parity, list-semantics, accessibility, recovery, tables]
tech-stack:
  added: []
  patterns:
    - Compile formatting and list mutations against a cloned candidate and persist one exact ReplaceChildren operation with an identity anchor map
    - Normalize only the command-local logical range; preserve directional anchor/focus endpoints in the editor result
    - Keep collapsed inline attributes in the revision-bound Rust session while range/block/list changes become durable transactions
key-files:
  created:
    - crates/flow-core/tests/rich_text_formatting.rs
  modified:
    - artifacts/benchmarks/phase1-recovery.json
    - crates/flow-core/src/audit/mod.rs
    - crates/flow-core/src/editor_view/mod.rs
    - crates/flow-core/src/lib.rs
    - crates/flow-core/src/model/mod.rs
    - crates/flow-core/src/transaction/mod.rs
    - crates/flow-core/tests/editor_session.rs
key-decisions:
  - Inline formatting accepts explicit whole-mark or single-mark Set commands; uppercase #RRGGBB input is parsed to the existing typed RGB representation, and unknown legacy fonts are rejected for authoring.
  - Block style changes use Paragraph/Heading 1–6, while alignment and spacing use exact closed bounds; equal targets return a non-mutating NoOp.
  - SetListKind converts selected compatible root blocks or a complete list, Enter creates a sibling item, empty-item Enter exits to a paragraph, and indent/outdent move items without browser-side list rules.
  - List nesting is bounded at depth eight and every accepted formatting/list mutation uses exact forward/inverse semantic children, preserving stable IDs and field anchors.
requirements-completed: [EDIT-03, EDIT-04, QUAL-03, QUAL-04]
coverage:
  - id: T1
    description: Inline formatting changes exactly the selected grapheme-safe range, merges only equal adjacent runs, preserves unselected marks, and restores exact canonical bytes through undo.
    requirement: EDIT-04
    verification:
      - kind: unit
        ref: formatting_changes_exact_grapheme_range_and_inverse_restores_runs
        status: pass
    human_judgment: false
  - id: T2
    description: Collapsed selections update Rust-owned pending marks without a document revision, while block style/spacing/alignment changes are durable and reversible.
    requirement: EDIT-04
    verification:
      - kind: unit
        ref: block_style_attributes_pending_marks_and_exact_bounds_are_rust_owned
        status: pass
    human_judgment: false
  - id: T3
    description: Forward and reverse ranges converge semantically while the public directional selection is preserved; explicit no-op, stale, and duplicate requests reject without mutation.
    requirement: EDIT-04
    verification:
      - kind: unit
        ref: reverse_selection_has_the_same_formatting_and_keeps_direction plus formatting_noop_stale_and_duplicate_delivery_are_non_mutating
        status: pass
    human_judgment: false
  - id: T4
    description: Heading, font-size, spacing, color, language, and font values obey closed authoring bounds and reject invalid encoding without clamping or substitution.
    requirement: EDIT-04
    verification:
      - kind: unit
        ref: closed_formatting_bounds_and_encoding_reject_without_clamping
        status: pass
    human_judgment: false
  - id: T5
    description: Ordered/unordered list conversion, continuation, empty-item exit, indent, outdent, exact inverse behavior, and depth-nine rejection are Rust-owned and bounded.
    requirement: EDIT-03
    verification:
      - kind: unit
        ref: list_conversion_continuation_exit_indent_and_outdent_are_reversible plus list_depth_nine_rejects_before_mutation
        status: pass
      - kind: unit
        ref: cargo test --locked -p flow-core --test rich_text_formatting --test rich_text_properties -- list --nocapture
        status: pass
    human_judgment: false
  - id: T6
    description: The complete repository quality gate remains green after the new closed command vocabulary and refreshed recovery evidence.
    requirement: QUAL-03
    verification:
      - kind: contract
        ref: npm run check
        status: pass
      - kind: test
        ref: cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace --all-targets
        status: pass
      - kind: recovery
        ref: recovery benchmark p95 512.875583 ms under the 2000 ms target; validator rejected 8 adversarial cases
        status: pass
    human_judgment: false
duration: resumed multi-session
completed: 2026-09-14
status: complete
---

# Phase 2 Plan 09: Formatting and List Semantics Summary

Plan 02-09 is complete. Rust now owns the closed formatting and list command
vocabulary, range normalization, pending typing attributes, block projections,
list structure rules, exact inverses, and stable rejection behavior. The
browser-facing formatting toolbar and list controls remain the next plan's
projection work; no formatting or list rule was added to TypeScript.

## Accomplishments

- Added explicit `SetInlineMarks`, `SetInlineMark`, `SetBlockAttributes`, and
  `SetBlockStyle` commands with grapheme-safe range handling, exact run
  canonicalization, closed font/size/color/language validation, and reversible
  block changes.
- Extended the Rust editor projection with block/list formatting state and
  revision-bound formatting/list capabilities while retaining pending marks as
  session-only state.
- Added `SetListKind`, `ContinueListItem`, `ExitListItem`, `IndentListItem`,
  and `OutdentListItem` with deterministic IDs, depth-eight enforcement, exact
  semantic inverse operations, and focus positions for newly created blocks.
- Extended privacy-safe audit command/error vocabularies and refreshed the
  recovery benchmark source manifest after the Rust command expansion.
- Added seven named formatting/list edge tests covering the locked EDIT-04
  predicates plus list conversion and nesting behavior.

## Verification

- `cargo fmt --all` — pass.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — pass.
- `cargo test --locked --workspace --all-targets` — pass.
- Exact formatting filter — 4 tests pass.
- Exact list filter — 2 new tests plus the existing list-container test pass.
- `node scripts/verify-recovery-benchmark.mjs --validate` — pass.
- `node scripts/verify-recovery-benchmark.mjs --self-test` — pass: 8
  adversarial cases rejected.
- `npm run check` — pass, including dependency gates, Rust/WASM,
  TypeScript, 32 unit/inspector tests, 4 accessibility tests, 19 Chromium
  tests, and deterministic replay rounds.

## Next Phase Readiness

Ready for Plan 02-10: accessible formatting and list controls with keyboard,
visible, and accepted-Rust projection parity.

## Self-check: PASSED

Both tasks have named executable Rust evidence, the full repository gate is
green, and pre-existing untracked backup/artifact files remain untouched.

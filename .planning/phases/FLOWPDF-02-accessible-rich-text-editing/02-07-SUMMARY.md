---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "07"
subsystem: structural-editing-algebra
tags: [rust, transactions, anchors, split, merge, deletion, tombstone, recovery, property-tests]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plans 02-01 through 02-06 pinned dependencies, schema-v2 FlowDocument, grapheme/session authority, durable input controller, and recovery protocol
provides:
  - Exact split/merge and compatible cross-block replacement operations for text containers
  - Deterministic `Deleted(tombstone)` anchor mappings and bounded owner-keyed deletion proofs
  - Field-safe delete/undo/redo/recovery with exact subtree and field restoration
  - Structural property, Unicode-rich-run, list-container, and tampered-recovery evidence
affects: [structural-keyboard, visible-structure-controls, formatting, fields, accessibility, recovery]
tech-stack:
  added: []
  patterns:
    - Compile structural edits against an unpublished candidate and replace one validated child container atomically
    - Preserve original node IDs/marks in exact inverse operations; generated split/fallback IDs are deterministic from the command ID
    - Store deletion owner proofs in executable transaction operations, validate bounds/owners/tokens before replay, and keep them out of editor/audit projections
key-files:
  created:
    - crates/flow-core/tests/rich_text_properties.rs
  modified:
    - artifacts/benchmarks/phase1-recovery.json
    - crates/flow-core/src/anchor/mod.rs
    - crates/flow-core/src/audit/mod.rs
    - crates/flow-core/src/lib.rs
    - crates/flow-core/src/model/mod.rs
    - crates/flow-core/src/store/mod.rs
    - crates/flow-core/src/transaction/mod.rs
    - crates/flow-core/tests/recovery.rs
key-decisions:
  - `SplitTextBlock`, `MergeTextBlocks`, and `DeleteSubtree` are closed Rust commands; cross-block `ReplaceSelection` is accepted only for immediate compatible siblings in one container.
  - Structural edits use a typed `ReplaceChildren` operation with exact expected/replacement children, field updates, forward/inverse mappings, and a transaction-private anchor preimage rather than reconstructing a tree from surviving IDs.
  - Paragraph/heading compatibility includes style and paragraph attributes; headings split at their end create a body paragraph, while last-root-editable deletion creates a deterministic empty paragraph.
  - A field whose deleted text belongs to a node that remains after a partial replacement fails closed; `TargetDeleted` is emitted only when the original node is absent from the candidate.
  - The command/audit vocabulary and model child-vector accessors were extended alongside the planned files because closed command serialization and nested list/table-cell mutation require them.
requirements-completed: [EDIT-01, EDIT-02, EDIT-03, QUAL-03, QUAL-04]
coverage:
  - id: T1
    description: Split maps backward/forward affinity to the retained/generated blocks, merge maps the second block into the first, and both exact inverses preserve IDs and semantic bytes.
    requirement: EDIT-03
    verification:
      - kind: unit
        ref: cargo test --locked -p flow-core --test rich_text_transactions --test rich_text_properties -- structural_algebra --nocapture
        status: pass
    human_judgment: false
  - id: T2
    description: Paragraph/heading compatibility, immediate adjacency, rich inline marks, exact Unicode, and nested list-item containers reject or apply atomically without endpoint snapping.
    requirement: EDIT-03
    verification:
      - kind: unit
        ref: crates/flow-core/tests/rich_text_properties.rs
        status: pass
    human_judgment: false
  - id: T3
    description: Compatible cross-block replacement retains the first node, preserves the selected replacement and suffix, and restores the original structure through the exact inverse.
    requirement: EDIT-03
    verification:
      - kind: unit
        ref: structural_algebra_cross_block_replace_preserves_exact_unicode_and_restores_inverse
        status: pass
    human_judgment: false
  - id: T4
    description: DeleteSubtree produces deterministic tombstones, bounded owner-keyed field proofs, and a fallback editable paragraph when the last root text block is removed.
    requirement: EDIT-03
    verification:
      - kind: unit
        ref: structural_algebra_deletion_tombstones_are_deterministic_and_private_preimage_is_bounded
        status: pass
    human_judgment: false
  - id: T5
    description: Delete→undo→redo→undo restores canonical semantic bytes, node IDs, fields, and history cursor exactly; recovery accepts the valid chain and rejects a tampered tombstone/preimage.
    requirement: QUAL-04
    verification:
      - kind: unit
        ref: cargo test --locked -p flow-core --test rich_text_properties --test recovery -- deletion_preimage --nocapture
        status: pass
      - kind: recovery
        ref: crates/flow-core/tests/recovery.rs
        status: pass
    human_judgment: false
  - id: T6
    description: The complete repository gate remains green after structural operations and refreshed recovery evidence.
    requirement: QUAL-03
    verification:
      - kind: contract
        ref: npm run check
        status: pass
      - kind: build
        ref: npm run build:wasm && npm run typecheck
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

# Phase 2 Plan 07: Structural Editing and Deletion Algebra Summary

Plan 02-07 is complete. Structural text mutations now compile in Rust against
an unpublished candidate, preserve semantic structure and IDs, and publish a
single exact transaction only after the candidate validates.

## Accomplishments

- Added closed `SplitTextBlock`, `MergeTextBlocks`, and `DeleteSubtree`
  commands. Split and merge operate in the immediate parent container and
  support paragraphs, compatible headings, and nested list-item text blocks;
  incompatible styles, attributes, atomic nodes, and non-adjacent targets
  reject atomically.
- Added `ReplaceChildren` executable operations containing exact expected and
  replacement children, typed field updates, and explicit forward/inverse
  anchor mappings. Split affinity, merge offsets, and compatible cross-block
  replacement are deterministic and grapheme-validated.
- Added rich-run replacement for same-block selections. It preserves marks
  outside the replacement, applies insertion marks deliberately, and merges
  only adjacent equal-mark runs without Unicode normalization.
- Added `NodeDeleted`, split, merge, and cross-block mappings with an opaque
  tombstone result. Deleting the last root editable text block inserts a
  deterministic empty paragraph so the document remains editable.
- Added bounded `AnchorPreimage` owner proofs, deterministic field
  `TargetDeleted` transitions, duplicate/owner/token/size validation, store
  preflight checks, and recovery replay validation. Undo restores the original
  subtree IDs and field states before the history cursor advances.
- Added the named structural property lane, nested-list coverage, exact
  deletion-cycle coverage, and tampered-recovery coverage. Refreshed the
  Phase 1 recovery benchmark manifest after the Rust source expansion.

## Verification

- `cargo fmt --all -- --check` — pass.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — pass.
- `cargo test --locked --workspace --all-targets` — pass: 107 Rust tests,
  including 8 structural/property tests and 11 recovery tests.
- `cargo test --locked -p flow-core --test rich_text_transactions --test rich_text_properties -- structural_algebra --nocapture` — pass: 7 named structural tests.
- `cargo test --locked -p flow-core --test rich_text_properties --test recovery -- deletion_preimage --nocapture` — pass.
- `npm run build:wasm` — pass.
- `npm run typecheck` — pass.
- `npm run test:unit` — 32/32 pass.
- `npm run test:browser` — 17/17 pass.
- `node --test tests/contracts/phase1-boundary.test.mjs` — 13/13 pass.
- `node scripts/verify-recovery-benchmark.mjs --self-test` — pass: 8
  adversarial cases rejected; refreshed workload p95 504 ms under the 2000 ms
  target.
- `npm run check` — pass, including dependency provenance/locks, Rust/WASM,
  TypeScript, unit/browser/accessibility suites, and two deterministic replay
  rounds.

## Next Phase Readiness

Ready for Plan 02-08: durable visible and keyboard structural product paths.
The Rust algebra is available; browser Enter/Backspace/Delete, list behavior,
and visible structural controls remain the next integration surface.

## Self-check: PASSED

Both plan tasks have executable Rust operations, exact inverse/recovery
evidence, named structural tests, and a green full repository gate. Pre-existing
untracked backup/artifact files remain untouched.

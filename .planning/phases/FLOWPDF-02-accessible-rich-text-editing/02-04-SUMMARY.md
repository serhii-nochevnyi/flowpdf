---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "04"
subsystem: grapheme-editor-session
tags: [rust, icu4x, unicode, grapheme, selection, wasm, accessibility]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plans 02-01 through 02-03 pinned dependencies, Unicode evidence, and schema-v2 FlowDocument
provides:
  - ICU4X UTF-16 grapheme and atomic-position validation
  - Revision-bound Rust editor session/view/capability DTOs
  - Immutable native and WASM editor-session projections
affects: [controlled-input, selection, formatting, rich-text-shell, persistence-boundary]
tech-stack:
  added: []
  patterns:
    - Translate ICU UTF-8 byte boundaries to UTF-16 in one bounded linear pass
    - Keep directional selection/session state noncanonical and outside document history
    - Expose immutable serialized WASM request-response functions instead of mutable handles
key-files:
  created:
    - crates/flow-core/src/editor_view/mod.rs
    - crates/flow-core/tests/editor_session.rs
    - crates/flow-core/tests/grapheme_conformance.rs
  modified:
    - crates/flow-core/src/anchor/mod.rs
    - crates/flow-core/src/lib.rs
    - crates/flow-wasm/src/lib.rs
    - tests/contracts/phase1-boundary.test.mjs
key-decisions:
  - Public text positions accept only pinned UAX #29 extended-grapheme boundaries after scalar UTF-16 validation; invalid positions are rejected without snapping or normalization.
  - Image, table, and page-break nodes expose only 0/Forward before and 1/Backward after atomic sentinels.
  - Session-only actions advance session_generation but never document revision, canonical bytes, history, audit, or persistence state.
requirements-completed: [EDIT-01, EDIT-04, QUAL-03, QUAL-04]
coverage:
  - id: G1
    description: ICU4X byte boundaries translate to exact UTF-16 boundaries for the complete pinned Unicode 17 corpus.
    requirement: EDIT-01
    verification:
      - kind: integration
        ref: cargo test --locked -p flow-core --test grapheme_conformance -- --nocapture
        status: pass
    human_judgment: false
  - id: G2
    description: Empty, surrogate, combining, ZWJ, regional-indicator, non-BMP, atomic, revision, node-identity, and resource-limit cases reject or resolve deterministically.
    requirement: QUAL-04
    verification:
      - kind: integration
        ref: crates/flow-core/tests/grapheme_conformance.rs
        status: pass
    human_judgment: false
  - id: G3
    description: Directional selection, pending marks, mixed formatting, capabilities, revision, and generation are derived and validated by Rust without canonical mutation.
    requirement: EDIT-04
    verification:
      - kind: integration
        ref: cargo test --locked -p flow-core --test editor_session -- --nocapture
        status: pass
    human_judgment: false
  - id: G4
    description: Immutable editor-session/query exports build through the pinned WASM boundary and remain inside the explicit export contract.
    requirement: QUAL-03
    verification:
      - kind: build
        ref: npm run build:wasm && npm run typecheck
        status: pass
      - kind: contract
        ref: node --test tests/contracts/phase1-boundary.test.mjs
        status: pass
    human_judgment: false
duration: resumed multi-session
completed: 2026-09-14
status: complete
---

# Phase 2 Plan 04: Grapheme and Editor-Session Authority Summary

Plan 02-04 is complete in the current working tree. Rust now owns public text
position validation and the noncanonical editor session that later keyboard,
IME, UI, and assistive-technology paths will share.

## Accomplishments

- Added an ICU4X `GraphemeBoundaryMap` using the locked `icu_segmenter = 2.3.0`
  compiled data. ICU UTF-8 byte boundaries are translated to UTF-16 offsets in
  one linear pass, preserving source text bytes and rejecting scalar or
  extended-grapheme interiors without snapping.
- Added bounded paragraph/heading maps with the existing 4 MiB text-node and
  4096-inline-run limits. Empty text accepts only offset 0; images, tables, and
  page breaks use exact before/after sentinels; containers cannot be addressed
  as text positions.
- Added `DirectionalSelection`, pending marks, mixed formatting projections,
  capabilities, revision/session generation, and closed request/response DTOs
  under `editor_view`. The projection is derived and revision-bound, but is not
  persisted and does not create a document transaction, audit entry, or history
  record.
- Added native `apply_editor_session` and `query_editor_view` entry points plus
  immutable `apply_editor_session` and `query_editor_view` WASM exports. The
  boundary allowlist was extended explicitly for these two typed exports.
- Completed the previously uncommitted grapheme draft into a full 766-case
  Unicode 17 corpus suite and added seven editor-session tests. The original
  draft was treated as incomplete until the implementation and full limit
  checks passed.

## Verification

- `cargo fmt --all -- --check` — pass.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — pass.
- `cargo test --locked -p flow-core --test grapheme_conformance -- --nocapture`
  — 9/9 pass, including all 766 checked-in Unicode 17 corpus cases and the
  4 MiB boundary-vector limit case.
- `cargo test --locked -p flow-core --test editor_session -- --nocapture` —
  7/7 pass.
- Exact Plan 02-04-02 chain — pass:
  `cargo test ... editor_session && npm run build:wasm && npm run typecheck`.
- `node --test tests/contracts/phase1-boundary.test.mjs` — 13/13 pass.
- `git diff --check && npm run build:web` — pass after the final implementation
  and README changes.
- A local README audit confirmed all documented npm script names and 19 local
  Markdown destinations resolve.

The full `npm run check` Phase 1 gate was not rerun as part of this focused
plan. Its existing provenance and WASM-size evidence concern remains recorded
in [`STATE.md`](../../STATE.md); this plan does not refresh or reinterpret that
historical gate.

## Deviations and corrections

1. The untracked draft test referenced a non-existent `FlowDocument::sample()`;
   it was completed against the existing deterministic sample and retained as
   a real conformance suite rather than being assumed complete.
2. `tests/contracts/phase1-boundary.test.mjs` was updated to admit the two new
   immutable WASM exports. This is required by the existing boundary contract
   and by the plan's explicit export-allowlist rule.
3. The first boundary-vector implementation converted every boundary by
   rescanning its text prefix. The 4 MiB limit test exposed the resulting
   O(n²) behavior. It was replaced with a single linear UTF-8/UTF-16 pass; the
   public contract and corpus results are unchanged.

No commit, push, dependency change, historical-fixture change, or mutable WASM
handle was introduced. The next executable Phase 2 plan is 02-05.

## Next Phase Readiness

Ready for Plan 02-05's core/WASM/persistence/minimal-React paragraph tracer.
Phase 2 remains in progress: this plan establishes position/session authority,
not the full rich-text editor, pagination, PDF export, or accessibility release
gate.

## Self-check: PASSED

Both plan tasks have implementation artifacts and passing focused verification;
the grapheme draft is no longer treated as an unfinished placeholder, and the
working tree retains all pre-existing unrelated untracked files.

---
phase: FLOWPDF-03-deterministic-reflow-and-pagination
plan: "03"
subsystem: deterministic-pagination
tags: [pagination, fragments, reflow, constraints, tables, fixed-point]
requires:
  - phase: FLOWPDF-03-deterministic-reflow-and-pagination
    provides: Fixed-point text tracer and schema-v3 section/page/header/footer settings
provides:
  - Rust-owned derived fragment tree and bounded page paginator
  - Stable page/fragment geometry, source ranges, section bands, break reasons, and diagnostics
  - Heading/widow-orphan fallback, image overflow handling, and simple-table row/header continuation
affects: [phase-3-equivalence, phase-3-workers, phase-3-rendering]
tech-stack:
  added: []
  patterns:
    - Bind pagination to canonical revision/hash and explicit font/hyphenation identities; never mutate FlowDocument.
    - Use fixed-point page geometry and a deterministic prefix-advance index for bounded line candidate measurement.
    - Represent every source block once per derived placement, mark repeated table headers/static bands as derived, and emit closed fallback diagnostics for impossible constraints.
key-files:
  created:
    - crates/flow-core/tests/layout_pagination.rs
    - crates/flow-core/tests/layout_constraints.rs
    - fixtures/layout/phase3-pagination-corpus.json
  modified:
    - crates/flow-core/src/layout/mod.rs
    - crates/flow-core/src/lib.rs
requirements-completed: [LAYO-03, LAYO-05, LAYO-06]
coverage:
  - id: T1
    description: Ordinary semantic blocks form deterministic page/fragment output and height edits reflow the affected suffix without changing canonical text.
    requirement: LAYO-03
    verification:
      - kind: test
        ref: crates/flow-core/tests/layout_pagination.rs
        status: pass
        note: Exact page count/fixed-point A4 geometry, repeated static bands, explicit page breaks, section geometry, stable source ranges, deterministic hashes, and suffix movement passed.
  - id: T2
    description: Keep and widow/orphan preferences use bounded fallback, and impossible blocks/rows terminate with explicit diagnostics.
    requirement: LAYO-05
    verification:
      - kind: test
        ref: crates/flow-core/tests/layout_constraints.rs
        status: pass
        note: Heading keep-with-next, widow/orphan fallback, oversized-row overflow, and max-page termination passed without looping or loss of source identity.
  - id: T3
    description: Simple tables split only between rows and repeat configured header rows as derived fragments.
    requirement: LAYO-06
    verification:
      - kind: test
        ref: crates/flow-core/tests/layout_constraints.rs
        status: pass
        note: Continuation pages contain derived header-row fragments and row fragments retain stable source IDs and nonzero fixed-point geometry.
  - id: T4
    description: The derived paginator does not regress existing Rust, WASM, or web-shell contracts.
    requirement: all
    verification:
      - kind: regression
        ref: cargo test --locked -p flow-core and cargo test --locked -p flow-wasm
        status: pass
        note: Full flow-core and flow-wasm suites passed.
verification:
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test layout_pagination --test layout_constraints -- --nocapture
    result: pass
    note: Six focused pagination and constraint tests passed.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet
    result: pass
    note: Complete flow-core suite passed, including all prior schema, editor, grapheme, and text-layout tests.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo clippy --locked -p flow-core --all-targets -- -D warnings
    result: pass
    note: No warnings in the paginator or existing flow-core targets.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet
    result: pass
    note: WASM-facing crate remains green.
  - command: npm run build:wasm
    result: pass
    note: Release WASM build and wasm-bindgen generation completed.
  - command: npm run typecheck
    result: pass
    note: TypeScript shell remains type-safe after the new public layout exports.
  - command: git diff --check
    result: pass
    note: No whitespace errors.
duration: same session
completed: 2026-09-21
status: complete
---

# Phase 3 Plan 03: Fragment Tree and Bounded Pagination Summary

Plan 03-03 is complete. FlowPDF now turns validated semantic blocks into a
Rust-owned derived fragment tree and fixed-point pages. The paginator handles
ordinary flow, sections, repeated static bands, explicit page breaks, lists,
bounded image placeholders, heading keep behavior, widow/orphan fallback, and
simple tables that continue only between rows.

## Accomplishments

- Added revision/hash-bound `PaginationRequest` and `PaginationResult` with
  deterministic settings/font/data identities and result hashing.
- Added typed page, fragment, rectangle, break-reason, and diagnostic DTOs;
  canonical `FlowDocument` remains borrowed and untouched.
- Added exact millimetre-to-fixed-point page geometry, content bands, static
  header/footer repetition, stable semantic source ranges, and explicit page
  limit handling.
- Added depth-first paragraph/heading/list/image traversal, explicit page-break
  boundaries, bounded overflow fallback, and a prefix advance index that keeps
  line candidate measurement deterministic without quadratic glyph scans.
- Added simple-table row layout, derived continuation header rows, stable
  source-linked cell fragments, and explicit overflow diagnostics for rows that
  cannot fit a page.
- Added corpus and integration tests covering exact geometry/page count,
  suffix reflow, section/header/footer behavior, keep/widow-orphan fallback,
  table continuation, and bounded failure.

## Scope boundary

This plan delivers the full-reflow fragment/pagination slice. Incremental versus
full equivalence, worker scheduling/stale-result rejection, and the accessible
page viewport remain later Phase 3 plans; no PDF output or browser layout
oracle is introduced here.

## Next Phase 3 Step

Plan 03-04 will make incremental reflow reuse explicit and prove byte-identical
fragment output against full reflow for the same revision.

## Self-check: PASSED

The focused pagination suite, full Rust/WASM tests, Clippy, WASM build,
TypeScript typecheck, fixed-point goldens, and formatting checks pass. Fallbacks
are bounded and source-linked; no semantic text or canonical document state is
mutated by pagination.

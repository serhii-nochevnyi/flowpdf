---
phase: FLOWPDF-03-deterministic-reflow-and-pagination
plan: "04"
subsystem: incremental-layout-equivalence
tags: [invalidation, carry-signature, equivalence, proptest, revision-safety]
requires:
  - phase: FLOWPDF-03-deterministic-reflow-and-pagination
    provides: Full deterministic paginator, fixed-point fragments, and revision/hash-bound pagination result
provides:
  - Conservative dependency-aware invalidation plans and carry signatures
  - Revision-safe incremental pagination entry point with exact no-op cache reuse
  - Full-vs-incremental structural/hash/diagnostic equivalence properties
affects: [phase-3-workers, phase-3-rendering, phase-3-viewport]
tech-stack:
  added: []
  patterns:
    - Treat full pagination as the executable oracle; changed revisions fall back to it until verified prefix/suffix splicing is introduced.
    - Carry signatures bind source revision/hash, settings, font/data identities, boundary, and accepted page prefix; tampering disables reuse.
    - Unknown, global, stale, or unresolvable inputs fail closed to boundary zero or a typed request error.
key-files:
  created:
    - crates/flow-core/src/invalidation/mod.rs
    - crates/flow-core/tests/layout_equivalence.rs
    - crates/flow-core/tests/layout_properties.rs
  modified:
    - crates/flow-core/src/layout/mod.rs
    - crates/flow-core/src/lib.rs
requirements-completed: [LAYO-07]
coverage:
  - id: T1
    description: Text/structure/style/asset changes resolve to the earliest safe top-level owner boundary, while section/geometry/font/data/constraint changes disable prefix reuse from zero.
    requirement: LAYO-07
    verification:
      - kind: test
        ref: crates/flow-core/tests/layout_equivalence.rs
        status: pass
        note: Node boundaries, global inputs, ambiguity, stale requests, cross-font identities, and forged carry signatures are covered.
  - id: T2
    description: Incremental pagination is structurally and hash-identical to the full paginator for valid changed documents.
    requirement: LAYO-07
    verification:
      - kind: property
        ref: crates/flow-core/tests/layout_properties.rs
        status: pass
        note: Sixteen deterministic proptest cases cover generated paragraphs, headings, lists, page breaks, simple tables, images/assets, sections, and accepted text edits.
  - id: T3
    description: The new invalidation/public layout API does not regress prior Rust/WASM/web contracts.
    requirement: all
    verification:
      - kind: regression
        ref: cargo test --locked -p flow-core and cargo test --locked -p flow-wasm
        status: pass
        note: Complete flow-core and flow-wasm suites passed.
verification:
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test layout_equivalence --test layout_properties -- --nocapture
    result: pass
    note: Four equivalence tests and one sixteen-case property test passed.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet
    result: pass
    note: Full flow-core suite passed, including pagination, constraints, migration, editor, grapheme, and text-layout coverage.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo clippy --locked -p flow-core --all-targets -- -D warnings
    result: pass
    note: No warnings in the invalidation or layout API.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet
    result: pass
    note: WASM-facing crate remains green.
  - command: npm run build:wasm
    result: pass
    note: Release WASM build and wasm-bindgen generation completed.
  - command: npm run typecheck
    result: pass
    note: TypeScript shell remains type-safe after the public invalidation/layout exports.
  - command: git diff --check
    result: pass
    note: No whitespace errors.
duration: same session
completed: 2026-09-21
status: complete
---

# Phase 3 Plan 04: Incremental/Full Reflow Equivalence Summary

Plan 03-04 is complete. FlowPDF now has an explicit derived invalidation and
carry-signature contract. The incremental entry point reuses only an exact,
verified no-op result; every changed revision is recomputed through the full
deterministic paginator and exposes the earliest safe boundary that a future
splicing optimization must honor.

## Accomplishments

- Added `ChangeSet`, conservative document diffing, earliest top-level owner
  boundaries, global layout-input invalidation, and ambiguity fail-closed rules.
- Added immutable carry signatures bound to revision/hash, section/settings
  fingerprint, font catalog, hyphenation data, boundary, and accepted page
  prefix; forged signatures cannot be published as reused results.
- Added `paginate_incremental_document` and public invalidation/layout exports.
- Added deterministic equivalence tests and bounded proptest generation over
  paragraphs, headings, lists, page breaks, simple tables, images/assets,
  sections, and text edits.
- Optimized text candidate width measurement with a sorted fixed-point advance
  prefix, preserving exact output while avoiding repeated glyph scans.

## Scope boundary

This plan proves the contract and keeps the full paginator as the reference
implementation. It does not yet splice unaffected fragment prefixes/suffixes
or schedule layout in a worker; those optimizations and lifecycle boundaries
remain subject to later evidence.

## Next Phase 3 Step

Plan 03-05 will expose the revision-safe layout request/result protocol to WASM
and a worker, including stale-result rejection and immutable accepted results.

## Self-check: PASSED

Focused equivalence/property tests, the full Rust/WASM suites, Clippy, release
WASM generation, TypeScript typecheck, and formatting checks all pass. No
incremental result can bypass the full oracle on a changed or ambiguous input.

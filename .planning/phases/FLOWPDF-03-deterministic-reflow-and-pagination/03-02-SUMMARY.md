---
phase: FLOWPDF-03-deterministic-reflow-and-pagination
plan: "02"
subsystem: canonical-layout-settings
tags: [schema, migration, sections, headers, footers, geometry]
requires:
  - phase: FLOWPDF-03-deterministic-reflow-and-pagination
    provides: Deterministic fixed-point text-layout request/result boundary
provides:
  - Durable schema-v3 page, section, and static header/footer settings
  - Private canonical v2 decoder and contiguous v2-to-v3 migration
  - Bounded geometry validation, migration goldens, and stable failure codes
affects: [phase-3-pagination, phase-3-workers, phase-3-rendering]
tech-stack:
  added: []
  patterns:
    - Keep the frozen v0/v1 decoder unchanged and use a private v2 wire envelope for the new migration hop.
    - Store section intent, semantic anchors, and typed static runs only; never persist fragments, glyphs, display-list bytes, or coordinates.
    - Validate geometry, section boundaries, ordering, and header/footer limits before canonical publication.
key-files:
  created:
    - crates/flow-core/src/schema/legacy_v2.rs
    - crates/flow-core/tests/schema_v3_layout.rs
    - fixtures/flowdoc/schema-v2-layout-default.json
    - fixtures/flowdoc/schema-v3-sections.json
    - fixtures/flowdoc/schema-v3-migrated.json
    - fixtures/flowdoc/schema-v3-migrated.hash
  modified:
    - crates/flow-core/src/model/mod.rs
    - crates/flow-core/src/schema/mod.rs
    - crates/flow-core/src/lib.rs
    - crates/flow-core/tests/migration_golden.rs
    - crates/flow-core/tests/persistence_round_trip.rs
    - crates/flow-core/tests/schema_v2_migration.rs
    - crates/flow-core/tests/provenance.rs
requirements-completed: [LAYO-04]
coverage:
  - id: T1
    description: Schema-v3 page geometry, section boundaries, and static header/footer intent are durable and derived-free.
    requirement: LAYO-04
    verification:
      - kind: test
        ref: crates/flow-core/tests/schema_v3_layout.rs
        status: pass
        note: Explicit sections, stable IDs, static typed runs, canonical bytes, duplicate/boundary/geometry/size rejection, and default-section behavior passed.
  - id: T2
    description: v2 input takes one lossless v2-to-v3 hop while v3 input is an exact no-op.
    requirement: LAYO-04
    verification:
      - kind: test
        ref: crates/flow-core/tests/migration_golden.rs, crates/flow-core/tests/persistence_round_trip.rs, crates/flow-core/tests/schema_v2_migration.rs
        status: pass
        note: Legacy v0/v1 routes, frozen v2 bytes, v2 default layout, the checked-in v3 migration golden/hash, semantic preservation, and repeated no-op migration passed.
  - id: T3
    description: The schema-v3 boundary does not regress the Rust, WASM, or web shell contracts.
    requirement: all
    verification:
      - kind: regression
        ref: cargo test --locked -p flow-core and cargo test --locked -p flow-wasm
        status: pass
        note: Full flow-core and flow-wasm suites passed; the existing frozen legacy source/hash gate remained green.
verification:
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo fmt --all -- --check
    result: pass
    note: Workspace formatting is clean.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test schema_v3_layout --test schema_v2_migration --test migration_golden --test persistence_round_trip --test provenance -- --nocapture
    result: pass
    note: Focused schema, migration, persistence, and provenance suites passed.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet
    result: pass
    note: Complete flow-core unit and integration suite passed.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo clippy --locked -p flow-core --all-targets -- -D warnings
    result: pass
    note: No warnings in the schema-v3 implementation or existing flow-core targets.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet
    result: pass
    note: WASM-facing crate remains green after the schema version change.
  - command: npm run build:wasm
    result: pass
    note: Rust release build and wasm-bindgen generation completed.
  - command: npm run typecheck
    result: pass
    note: TypeScript shell remains type-safe.
  - command: git diff --check
    result: pass
    note: No whitespace errors in the tracked implementation and planning changes.
duration: same session
completed: 2026-09-21
status: complete
---

# Phase 3 Plan 02: Durable Layout Settings and v2-to-v3 Migration Summary

Plan 03-02 is complete. FlowPDF now admits schema-v3 page and section intent
as canonical typed data: page geometry, ordered semantic boundaries, and
bounded static header/footer runs are validated before publication and contain
no derived layout output.

## Accomplishments

- Added schema-v3 `SectionSettings`, typed static header/footer runs, stable
  section identity, and a deterministic default first section.
- Added checked page geometry, margin, boundary, ordering, locale, style, run,
  and header/footer limits with stable validation codes.
- Added a private canonical v2 decoder so the frozen legacy v0/v1 decoder and
  its source/hash compatibility gate remain unchanged.
- Registered a contiguous v2-to-v3 migration that preserves semantic body,
  field, asset, and provenance data, emits deterministic default layout for
  older documents, and validates every candidate before publication.
- Added explicit v3, migrated-v3, and v2-default fixtures plus exact canonical
  hashes and no-op/repeat migration coverage.

## Scope boundary

This plan makes the layout inputs durable and migration-safe. It does not claim
fragment-tree pagination, viewport-first page realization, worker scheduling,
or PDF rendering; those remain later Phase 3 plans.

## Next Phase 3 Step

Plan 03-03 will build the deterministic fragment tree and bounded pagination
algorithm on top of these schema-v3 settings and the fixed-point text-layout
tracer from 03-01.

## Self-check: PASSED

The v2 freeze, canonical v3 goldens, full Rust tests, Clippy, WASM tests, web
build/typecheck, and formatting checks all pass. Phase 2 remains open only for
the previously documented external Windows/Edge screen-reader evidence; this
plan does not alter that status.

---
phase: FLOWPDF-01-durable-flow-foundation
plan: "03"
subsystem: core
tags: [rust, wasm, canonical-json, blake3, migrations, resource-limits, unicode, forms]
requires:
  - phase: FLOWPDF-01-durable-flow-foundation
    provides: "DTO-only Rust/WASM/IndexedDB walking skeleton from Plan 01-02"
provides:
  - "Complete typed v1 FlowDocument schema with stable identities and closed semantic field vocabulary"
  - "Single bounded canonical UTF-8 serialization, versioned BLAKE3 identity, and separate asset verification"
  - "Pure sequential v0-to-v1 migration registry with golden replay-boundary evidence"
  - "Native/WASM migration DTO parity with explicit provenance and snapshot intent"
affects: [transaction-engine, durability, rich-text, pagination, pdf-export, semantic-forms]
actuals:
  tokens: 22000
  tasks: 3
  commits: 6
tech-stack:
  added: []
  patterns: [opaque stable ids, typed closed schema, bounded decode, golden canonical bytes, pure sequential migrations]
key-files:
  created: [crates/flow-core/src/model/mod.rs, crates/flow-core/src/schema/mod.rs, crates/flow-core/src/canonical/mod.rs, crates/flow-core/tests/schema.rs, crates/flow-core/tests/persistence_round_trip.rs, crates/flow-core/tests/migration_golden.rs, fixtures/flowdoc/current.json, fixtures/flowdoc/older.json, fixtures/flowdoc/migrated.json]
  modified: [crates/flow-core/src/lib.rs, crates/flow-wasm/src/lib.rs]
key-decisions:
  - "Canonical v1 remains compact declaration-ordered JSON; semantic order lives in vectors and no page, DOM, or PDF coordinates enter stored content."
  - "Every public text position is node ID plus UTF-16 offset and affinity; canonical text remains exact UTF-8 without normalization."
  - "A successful migration publishes only a fully validated current-schema snapshot boundary and preserves the source creation timestamp and old records for inspection."
patterns-established:
  - "Validation pattern: preflight byte/depth budgets, typed decode, cross-reference and field constraints, then exact canonical re-encoding before publication."
  - "Migration pattern: contiguous pure version hops, validation at the declared boundary, stable error codes, and no partial output."
requirements-completed: [FLOW-01, FLOW-02, FLOW-03]
coverage:
  - id: D1
    description: "The representative Ukrainian/English FlowDocument preserves every semantic, style, asset, and field descriptor through exact canonical save/reopen."
    requirement: FLOW-01
    verification:
      - kind: integration
        ref: "crates/flow-core/tests/schema.rs"
        status: pass
      - kind: integration
        ref: "fixtures/flowdoc/current.json"
        status: pass
    human_judgment: false
  - id: D2
    description: "Canonical identity, ordering, separate asset integrity, concurrency reconciliation, and all locked resource ceilings are deterministic and bounded."
    requirement: FLOW-02
    verification:
      - kind: integration
        ref: "crates/flow-core/tests/persistence_round_trip.rs"
        status: pass
    human_judgment: false
  - id: D3
    description: "The supported old document migrates deterministically to exact current bytes/hash; current input is a no-op and invalid paths publish nothing."
    requirement: FLOW-03
    verification:
      - kind: integration
        ref: "crates/flow-core/tests/migration_golden.rs"
        status: pass
      - kind: integration
        ref: "crates/flow-wasm/src/lib.rs"
        status: pass
    human_judgment: false
duration: 35min
completed: 2026-08-14
status: complete
---

# Phase FLOWPDF-01 Plan 03: Canonical Schema and Migration Summary

**FlowPDF now has a complete, bounded semantic source format whose canonical bytes, asset identities, Unicode positions, form descriptors, and schema migrations are deterministic in native Rust and the browser WASM boundary.**

## Performance

- **Duration:** 35 min
- **Started:** 2026-08-14T19:25:54Z
- **Completed:** 2026-08-14T20:00:30Z
- **Tasks completed:** 3 of 3
- **Files modified:** 13

## Accomplishments

- Replaced the tracer model with an explicit v1 schema covering page settings, locale, styles, ordered semantic content, separately stored asset descriptors, six closed field kinds, stable UUID-compatible identities, revision, and provenance.
- Added one canonical serialization/hash path with exact Unicode preservation, UTF-16 logical-position validation, duplicate/dangling-reference rejection, asset-content verification, and idempotent/conflicting identity reconciliation.
- Locked and tested the reviewed limits at both N and N+1: canonical bytes, nesting, nodes, text, styles, assets, fields, transaction operations/bytes, and recovery records/bytes.
- Added current, older, and migrated golden documents with expected versioned hashes, plus a pure sequential migration registry that rejects future versions, missing hops, malformed intermediate output, and interruption without partial publication.
- Exposed migration through the same serialized Rust/WASM DTO boundary, returning exact canonical JSON/hash and explicit migration provenance/replay-boundary intent.

## Task Commits

1. **Schema contract RED** — `dd6f942`
2. **Complete canonical schema GREEN** — `e1ac22c`
3. **Persistence and limit contract RED** — `19b11e6`
4. **Round-trip, assets, and resource limits GREEN** — `69cb21b`
5. **Sequential migration golden RED** — `c336bf2`
6. **Pure migration registry and WASM parity GREEN** — `bf9329b`

## Automated Evidence

- Rust formatting and workspace Clippy with warnings denied passed.
- All workspace tests passed: 4 core unit, 4 migration golden, 7 persistence/limit, and 6 schema tests.
- Native and `wasm32-unknown-unknown` compilation passed; release WASM bindings regenerated successfully.
- Strict TypeScript, unit tests, and both real-Chromium browser tests passed.
- Dependency-lock verification passed for 67 Cargo and 82 npm packages, including adversarial drift cases.
- All five checked-in canonical/golden files remained byte-clean after verification.
- Schema-drift, codebase-drift, and UI-safety post-wave gates reported no blocker.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Native/WASM parity] Added an explicit migration DTO/export**
- The plan required native and WASM callers to receive identical migrated bytes, hash, and provenance, while its initial file list named only the core registry.
- Added the narrow serialized request/result boundary and a native contract test; no mutable document object or alternate mutation path was exposed.

**2. [Rule 2 - Provenance preservation] Retained the v0 creation timestamp**
- Migration provenance originally had only version/hop metadata, which would discard useful source lineage.
- The v1 migrated variant now records the source timestamp deterministically alongside source/current versions and hop sequence.

**Total deviations:** 2 auto-fixed missing contract details. No editor, layout, PDF, voice, or backend scope was added.

## Issues Encountered

- The delegated executor produced only the initial migration test and stopped progressing. The preserved RED contract was retained, execution was safely taken over in the shared workspace, and all three tasks were completed and verified without conflicting edits.

## User Setup Required

None. The existing workspace-local Rust, wasm-bindgen, and Chromium toolchains remain sufficient.

## Next Phase Readiness

Ready for Plan `01-04`: build the typed command algebra, stable anchor mapping, atomic inverse operations, deterministic stale-target conflicts, and bounded undo/redo on top of the now-fixed canonical schema.

## Self-Check: PASSED

- All plan tasks have RED/GREEN commits and pass from the committed tree.
- Current and migrated bytes/hashes match checked-in goldens exactly.
- Canonical input rejects unknown fields, invalid identities/references, incompatible field forms, invalid UTF-16 boundaries, and all N+1 resource cases before publication.
- Migration is pure, contiguous, deterministic, exact-no-op on current input, and returns no partial document for rejected input.

---

*Phase: FLOWPDF-01-durable-flow-foundation*
*Completed: 2026-08-14*

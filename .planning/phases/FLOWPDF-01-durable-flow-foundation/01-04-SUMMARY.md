---
phase: FLOWPDF-01-durable-flow-foundation
plan: "04"
subsystem: core
tags: [rust, wasm, transactions, undo-redo, utf16, anchors, recovery, property-testing]
requires:
  - phase: FLOWPDF-01-durable-flow-foundation
    provides: "Canonical v1 FlowDocument schema, limits, hashing, and migrations from Plan 01-03"
provides:
  - "One typed command boundary for UI, keyboard, voice, API, native Rust, and browser WASM callers"
  - "Immutable revision-linked transactions with executable forward/inverse operations and explicit anchor mappings"
  - "Deterministic UTF-16 logical positions, stale-target conflicts, bounded history, and ordinary undo/redo transactions"
  - "Semantic recovery replay that binds persisted operations and history to the exact snapshot"
affects: [durability, rich-text, voice, forms, pagination, audit]
actuals:
  tokens: 26000
  tasks: 3
  commits: 10
tech-stack:
  added: []
  patterns: [validate-then-construct, stored executable inverse, semantic history hash, explicit anchor invalidation, native-wasm parity]
key-files:
  created: [crates/flow-core/src/anchor/mod.rs, crates/flow-core/src/transaction/mod.rs, crates/flow-core/tests/transaction_tracer.rs, crates/flow-core/tests/preconditions.rs, crates/flow-core/tests/transaction_properties.rs, web/tests/transaction-boundary.browser.test.ts]
  modified: [crates/flow-core/src/lib.rs, crates/flow-core/src/model/mod.rs, crates/flow-core/src/schema/mod.rs, web/persistence/indexeddb-store.ts, web/src/foundation-inspector.ts]
key-decisions:
  - "Every mutation modality compiles to the same typed Command and only TransactionService may publish a new FlowDocument revision."
  - "Public positions are node ID plus UTF-16 offset and affinity; deleted or ambiguous positions are invalidated explicitly and never guessed."
  - "Undo and redo apply the exact stored inverse/forward operation sets as new revision-linked transactions with fresh command IDs."
  - "Recovery proves internal consistency by replaying the journal backward and forward and rebuilding history; unkeyed hashes are integrity checks, not external authenticity."
patterns-established:
  - "Atomic mutation pattern: validate envelope and targets against immutable state, derive inverse from pre-state, mutate a candidate, validate/hash, then publish."
  - "History pattern: semantic hashes ignore revision while durable transaction hashes bind every concrete revision."
  - "Boundary parity pattern: browser WASM exercises the same serialized DTOs and stable failure codes as native tests."
requirements-completed: [EDIT-06, EDIT-07]
coverage:
  - id: D1
    description: "Typed content, style, structure, field, batch, undo, and redo commands produce deterministic immutable transactions with executable rollback order."
    requirement: EDIT-06
    verification:
      - kind: integration
        ref: "crates/flow-core/tests/transaction_tracer.rs"
        status: pass
      - kind: integration
        ref: "crates/flow-core/tests/transaction_properties.rs"
        status: pass
    human_judgment: false
  - id: D2
    description: "Stale, duplicate, missing, invalid UTF-16, invalidated-anchor, and over-budget commands return stable errors without publishing state."
    requirement: EDIT-07
    verification:
      - kind: integration
        ref: "crates/flow-core/tests/preconditions.rs"
        status: pass
      - kind: e2e
        ref: "web/tests/transaction-boundary.browser.test.ts"
        status: pass
    human_judgment: false
  - id: D3
    description: "Persisted transaction meaning, anchor mappings, history effects, and the final snapshot are bound by bidirectional semantic replay."
    requirement: EDIT-06
    verification:
      - kind: integration
        ref: "crates/flow-core/tests/preconditions.rs"
        status: pass
    human_judgment: false
duration: 30min
completed: 2026-08-14
status: complete
---

# Phase FLOWPDF-01 Plan 04: Transaction, Anchor, and History Summary

**FlowPDF now has one bounded, atomic command engine whose native and WASM callers produce deterministic transactions, explicit UTF-16 anchor mappings, and exactly reversible content, style, structure, and field histories.**

## Performance

- **Duration:** 30 min
- **Started:** 2026-08-14T20:05:47Z
- **Completed:** 2026-08-14T20:35:05Z
- **Tasks completed:** 3 of 3
- **Files modified:** 13

## Accomplishments

- Added a closed command/mutation/operation algebra with command identity, base revision, modality, ordered forward operations, inverse operations derived from untouched pre-state, explicit mappings, pre/post hashes, and stable errors.
- Enforced public UTF-16 offsets with distinct native-byte offsets, scalar-boundary round trips, surrogate-half rejection, deterministic affinity behavior, composed mappings, and explicit text/node invalidation.
- Made undo and redo ordinary immutable transactions with fresh IDs and revisions; bounded history size/work and proved mixed content/style/structure/field histories through 64 generated state-machine cases plus fixed regressions.
- Bound supplied history to the current semantic document, including cursor position, adjacent hash chain, replayability, inverse exactness, and branch truncation without command-ID reuse.
- Hardened recovery to reverse and replay every persisted operation, verify anchor mappings and revision hashes, rebuild every history effect, and reject semantic tampering without returning partial output.
- Added a real-Chromium WASM contract test for successful voice-modality insertion and native-equivalent stale, duplicate, and invalid UTF-16 failures.

## Task Commits

1. **Transaction/precondition contract RED** — `ac21410`
2. **Atomic typed command service GREEN** — `9c3f4ee`
3. **Anchor mapping matrix RED** — `b4c7ff7`
4. **Explicit UTF-16 anchor mappings GREEN** — `72daa27`
5. **Mixed reversible-history RED** — `3a41ba4`
6. **Deterministic undo/redo GREEN** — `5cd09af`
7. **History-binding and command-budget RED** — `1b8d656`
8. **Semantic recovery replay RED** — `21ac234`
9. **Bounded history and bidirectional recovery GREEN** — `357a3a7`
10. **Real-browser WASM parity** — `f962eee`

## Automated Evidence

- Rust formatting and workspace Clippy with warnings denied passed.
- All workspace tests passed: 5 core unit, 4 migration, 7 persistence/limits, 9 precondition/anchor, 6 schema, 6 transaction-property, and 2 transaction-tracer tests.
- The 64-case stateful property suite proves full undo/redo restoration and deterministic replay across mixed mutation categories.
- Native and release `wasm32-unknown-unknown` builds passed; browser bindings regenerated successfully.
- Strict TypeScript, unit tests, and all three real-Chromium browser tests passed.
- Dependency-lock adversarial verification passed.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Durable undo state] Carried bounded history through the canonical session/snapshot DTO**
- Stateless WASM calls need the exact validated history cursor and stored inverses to execute undo/redo through the shared Rust boundary.
- Added history to the existing session/snapshot persistence DTO rather than introducing a browser-side mutation path.

**2. [Rule 1 - Stored-history trust boundary] Added semantic history and recovery verification**
- Review found that shape-valid stored inverse operations could otherwise be substituted while retaining plausible link hashes.
- Recovery now exercises operation meaning in both directions, verifies mappings, and reconstructs history from transaction effects before publishing state.

**3. [Rule 2 - Boundary evidence] Added browser-level WASM behavior tests**
- Compile parity alone could miss serde/tag/newtype drift.
- Real Chromium now exercises the serialized success and stable-error contracts directly.

**Total deviations:** 3 auto-fixed correctness/evidence gaps. No editor UI, layout, PDF, form-authoring, or voice-capture scope was added.

## Issues Encountered

- The system shell did not expose Rust globally. All checks used the pinned workspace-local Rust/Cargo/WASM toolchain from Plan 01-01, preserving reproducibility.

## User Setup Required

None.

## Next Phase Readiness

Ready for Plan `01-05`: persist the now-fixed transaction/snapshot/audit contracts atomically, prove crash-window recovery and bounded replay, select a measured snapshot cadence, and enforce audit redaction/provenance.

## Self-Check: PASSED

- Every named mutation category has tested inverse and redo behavior.
- Stale/invalid/duplicate/over-budget commands publish no value or partial state and keep canonical state unchanged.
- UTF-16/native conversions cover BMP, combining marks, emoji, and non-BMP boundaries without making a grapheme-safety claim.
- Stored transaction meaning and history are replay-bound to the exact final snapshot.
- Native and browser WASM callers use the same Rust command service and stable error vocabulary.

---

*Phase: FLOWPDF-01-durable-flow-foundation*
*Completed: 2026-08-14*

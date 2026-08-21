---
phase: FLOWPDF-01-durable-flow-foundation
plan: "05"
subsystem: durability
tags: [rust, indexeddb, recovery, snapshots, migrations, provenance, audit, privacy, benchmark]
requires:
  - phase: FLOWPDF-01-durable-flow-foundation
    provides: "Canonical schema, migrations, immutable transactions, anchors, and reversible history from Plans 01-03/04"
provides:
  - "Rust-owned versioned snapshot, transaction, audit, asset, and migration-source records with atomic physical-store commits"
  - "Bounded bidirectional recovery with sparse checkpoints, exact history reconstruction, corruption diagnostics, and no partial publication"
  - "Closed privacy-minimized audit events plus exact creation/migration/revision provenance"
  - "Atomic browser identity CAS for concurrent saves and immutable post-migration checkpoint lineage"
  - "Source-bound 200-page-equivalent recovery benchmark with independently validated raw timing evidence"
affects: [browser-lifecycle, local-save, migrations, inspector, voice-audit, future-pdf-assets]
actuals:
  tasks: 3
  commits: 11
tech-stack:
  added: []
  patterns: [rust-owned-commit-planning, sparse-checkpoint-replay, immutable-migration-boundary, indexeddb-logical-cas, closed-audit-allowlist, source-bound-benchmark]
key-files:
  created: [crates/flow-core/src/store/mod.rs, crates/flow-core/src/audit/mod.rs, crates/flow-core/src/provenance/mod.rs, crates/flow-core/tests/recovery_tracer.rs, crates/flow-core/tests/recovery.rs, crates/flow-core/tests/audit_redaction.rs, crates/flow-core/tests/provenance.rs, crates/flow-core/examples/recovery_benchmark.rs, fixtures/recovery/benchmark-200-page.recipe.json, scripts/verify-recovery-benchmark.mjs, artifacts/benchmarks/phase1-recovery.json]
  modified: [crates/flow-core/src/lib.rs, crates/flow-core/src/schema/mod.rs, crates/flow-core/tests/persistence_round_trip.rs, crates/flow-wasm/src/lib.rs, web/persistence/indexeddb-store.ts, web/src/foundation-inspector.ts, web/tests/indexeddb-store.test.ts]
key-decisions:
  - "JavaScript stores opaque physical records, while Rust alone selects checkpoints, validates identities/hashes/schema, replays operations, rebuilds history, and projects audit/provenance."
  - "Snapshot cadence changes recovery work only: creation/migration/save are forced boundaries and normal work snapshots at 100 transactions or 4 MiB by default."
  - "A migration boundary permanently names the exact source and original migration output; later checkpoints carry that immutable boundary and prove their ancestry by replay."
  - "Normal IndexedDB commits use logical-identity CAS in one strict transaction; deliberately corrupt physical records remain readable so Rust can fail closed."
  - "Unkeyed BLAKE3 hashes demonstrate internal consistency, not authenticity against an attacker capable of rewriting the entire store."
patterns-established:
  - "Recovery pattern: preflight budgets, normalize/dedupe, choose newest valid checkpoint, reverse/forward replay, rebuild history/audit, then publish once."
  - "Durability pattern: Rust plans the optional checkpoint and adapter writes transaction/audit/assets atomically without interpreting document semantics."
  - "Evidence pattern: benchmark artifacts bind raw samples to the full recursively discovered core source graph, Cargo/toolchain inputs, fixture, runner, and exact percentile recomputation."
requirements-completed: [FLOW-02, FLOW-03, FLOW-04, FLOW-05, QUAL-08]
coverage:
  - id: D1
    description: "Exact canonical JSON/hash, ordered descriptors, Unicode text, separately hashed assets, empty/singleton collections, and immutable history survive save/reopen."
    requirement: FLOW-02
    verification:
      - kind: integration
        ref: "crates/flow-core/tests/persistence_round_trip.rs"
        status: pass
      - kind: integration
        ref: "crates/flow-core/tests/recovery_tracer.rs"
        status: pass
    human_judgment: false
  - id: D2
    description: "Strict IndexedDB transactions and logical-identity CAS prevent partial or doubly acknowledged divergent concurrent saves while exact retries converge."
    requirement: FLOW-03
    verification:
      - kind: unit
        ref: "web/tests/indexeddb-store.test.ts"
        status: pass
    human_judgment: false
  - id: D3
    description: "Recovery rejects gaps, conflicts, wrong identity/schema, hash/operation/history/audit tampering, corrupt snapshots, and hard-budget overflow without partial state."
    requirement: FLOW-04
    verification:
      - kind: integration
        ref: "crates/flow-core/tests/recovery.rs"
        status: pass
      - kind: integration
        ref: "crates/flow-core/tests/recovery_tracer.rs"
        status: pass
    human_judgment: false
  - id: D4
    description: "Creation, migration, current revision, source hashes, schema/engine identity, and unavailable preview/export provenance are represented exactly without fabrication."
    requirement: FLOW-05
    verification:
      - kind: integration
        ref: "crates/flow-core/tests/provenance.rs"
        status: pass
    human_judgment: false
  - id: D5
    description: "Typed audit serialization, debug/error/WASM surfaces exclude semantic text, command arguments, transcripts, audio, and arbitrary metadata while preserving stable IDs/codes/order."
    requirement: QUAL-08
    verification:
      - kind: integration
        ref: "crates/flow-core/tests/audit_redaction.rs"
        status: pass
      - kind: e2e
        ref: "web/tests/walking-skeleton.browser.test.ts"
        status: pass
    human_judgment: false
  - id: D6
    description: "A source-bound native recovery benchmark proves 200 page equivalents, 1,000 semantic nodes, 80,000 words, 1,000 planner transactions, and p95 below two seconds."
    requirement: FLOW-04
    verification:
      - kind: other
        ref: "artifacts/benchmarks/phase1-recovery.json"
        status: pass
    human_judgment: false
duration: multi-session
completed: 2026-08-21
status: complete
---

# Phase FLOWPDF-01 Plan 05: Durable Recovery, Provenance, and Audit Summary

**FlowPDF now persists Rust-planned immutable records atomically, recovers only a bounded fully verified revision chain, preserves exact migration lineage, and exposes privacy-minimized audit/provenance without making IndexedDB a semantic authority.**

## Performance

- **Started:** 2026-08-14T20:40:00Z
- **Completed:** 2026-08-21T08:32:47Z
- **Tasks completed:** 3 of 3
- **Task/fix commits:** 11
- **Recovery benchmark:** p50 `460.510833 ms`, p95 `483.398292 ms` against a `< 2000 ms` target

## Accomplishments

- Added versioned store records and a Rust `CommitPlanner` for optional sparse checkpoints, with creation, migration, and explicit save boundaries plus the default 100-transaction/4-MiB cadence.
- Made recovery preflight-bounded at 10,000 records/64 MiB, idempotent for exact duplicates, conflict-aware for divergent identities, and semantically bound through reverse/forward operation replay and exact history reconstruction.
- Preserved asset bytes and legacy migration source bytes separately, validated their content hashes, and kept incompatible historical records inspectable without replaying them as current-schema operations.
- Implemented immutable migration boundaries that survive later checkpoints and bind source, original migrated output, hop sequence, timestamp, system audit, and every descendant transaction.
- Replaced arbitrary audit metadata with a closed typed allowlist and added exact revision provenance with explicit unavailable preview/export state.
- Hardened the browser adapter with one-transaction logical CAS across snapshots, transactions, audits, assets, and migration sources; concurrent divergent saves now have exactly one winner.
- Checked in a real 200-page-equivalent/80,000-word/1,000-transaction benchmark. Its terminal artifact carries 20 raw samples, recomputed percentiles, an exact toolchain stamp, and a 16-file recursively complete source/dependency manifest.

## Task Commits

1. **Atomic store recovery tracer RED** — `21ea84e`
2. **Separate durable asset records RED** — `33dcdcb`
3. **Typed audit and revision provenance** — `313af6f`
4. **Bounded recovery, replay, audit, and lineage core** — `9077da2`
5. **Rust-planned atomic browser persistence** — `6a91669`
6. **Initial recovery cadence evidence** — `7377b01`
7. **Atomic divergent-save CAS** — `bb3363f`
8. **Post-migration checkpoint lineage** — `664fed3`
9. **Real 200-page benchmark workload** — `61b1436`
10. **Source-bound timing and percentile validation** — `99f3bdb`
11. **Complete recursive recovery-source manifest** — `36aef7c`

## Automated Evidence

- `cargo fmt --all -- --check`, workspace Clippy with `-D warnings`, all native workspace tests, and the `wasm32-unknown-unknown` check passed on the pinned local toolchain.
- Core evidence includes 5 unit, 4 audit, 4 migration, 7 persistence, 9 precondition, 6 provenance, 9 recovery, 8 recovery-tracer, 6 schema, 6 transaction-property, and 2 transaction-tracer tests.
- TypeScript strict checking, 13 unit tests, regenerated release WASM, all 3 real-Chromium browser tests, and dependency-lock adversarial verification passed.
- The benchmark performed 3 warm-ups and 20 measured recoveries over 1,000 planner transactions and a final 797,152-byte generated semantic payload; the first approved 100/4-MiB policy passed at p95 `483.398292 ms`.
- Benchmark validation rejects stale core/Cargo/toolchain/fixture/runner inputs and eight adversarial artifact mutations, including forged metrics and missing/unexpected source entries.
- A final independent blocker-only audit returned `NO BLOCKERS`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Rust semantic ownership] Moved checkpoint choice into a Rust commit planner**
- The physical browser adapter initially received a mandatory snapshot and could not express sparse policies safely.
- Rust now returns a planned commit with an optional checkpoint; TypeScript only performs physical atomic I/O.

**2. [Rule 1 - Concurrent save integrity] Added logical-identity CAS inside IndexedDB**
- Hash-keyed opaque records allowed two divergent same-identity saves to be acknowledged before recovery rejected the combined store.
- The normal write path now validates logical identity within the same strict read/write transaction; hostile duplicates remain testable only through deliberate raw injection.

**3. [Rule 1 - Migration ancestry] Made the original migration boundary immutable across later checkpoints**
- Operation snapshots did not carry lineage, so a cadence checkpoint after migration was invalid and recovery fell back indefinitely.
- The planner copies a verified boundary, proves the candidate against the recovered durable head, and recovery replays descendant history back to the exact original migrated output.

**4. [Rule 1 - Durable evidence] Replaced metadata-only benchmark scale with semantic workload proof**
- The first artifact labelled a small document as 200 pages.
- The final recipe constructs and verifies 1,000 paragraphs/80,000 words with a per-page byte floor and 1,000 real planner transactions.

**5. [Rule 1 - Evidence freshness] Bound raw timings to all transitive core inputs**
- Fixture/toolchain matching alone could validate stale numbers after recovery code changed, and claimed percentile fields were initially trusted.
- The validator recursively hashes every core Rust module plus Cargo/toolchain/fixture/runner inputs and recomputes nearest-rank p50/p95 from exactly 20 finite nonnegative durations.

**Total deviations:** 5 auto-fixed correctness/evidence gaps. No layout, PDF syntax, form-authoring UI, or voice-capture scope was added.

## Issues Encountered

- Rust/Cargo are intentionally workspace-local rather than globally installed; every command used the pinned Plan 01-01 toolchain.
- The realistic benchmark made workload construction materially slower, but measured recovery remained below the locked target without reducing document size or weakening the threshold.

## User Setup Required

None.

## Next Phase Readiness

Ready for Plan `01-06`: complete the localized responsive Foundation Inspector, exercise the full create/mutate/stale/undo/redo/save/reload/migrate lifecycle in real Chromium, and seal Phase 1 with one deterministic gate.

## Self-Check: PASSED

- Every Plan 05 truth/prohibition has executable native, browser, or benchmark evidence.
- Divergent identities, partial writes, gaps, corrupted source/boundary/audit records, and over-budget recovery return no partial session.
- Later migrated checkpoints recover from the newest valid boundary with only the bounded tail.
- Audit/inspector/accessibility defaults retain no semantic document or voice payload.
- The passing benchmark artifact is current for the complete discovered recovery source graph and recomputes its own percentile claims.

---

*Phase: FLOWPDF-01-durable-flow-foundation*
*Completed: 2026-08-21*

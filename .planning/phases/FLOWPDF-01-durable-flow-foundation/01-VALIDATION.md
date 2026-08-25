---
phase: 1
slug: durable-flow-foundation
status: validated
nyquist_compliant: true
wave_0_complete: true
created: 2026-08-14
---

# Phase 1 — Validation Strategy

> Per-phase validation contract synchronized to plans `01-01` through `01-06` and audited against the completed implementation on 2026-08-25.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test harness + `proptest`; Vitest unit/browser projects for the TypeScript adapter and Foundation Inspector |
| **Config file** | `Cargo.toml` and `vitest.config.ts` |
| **Quick run command** | `cargo test -p flow-core --lib` |
| **Full suite command** | `npm run check` (orchestrates format, Clippy, Rust tests, WASM target check, Vitest unit, and Chromium browser tests) |
| **Observed runtime** | Focused Chromium accessibility: 1.39 seconds; full Phase 1 gate: 20.46 seconds on 2026-08-25 |

---

## Sampling Rate

- **After every task commit:** Run the narrowest focused Rust/Vitest test plus `cargo test -p flow-core --lib` when the core changed.
- **After every plan wave:** Run `npm run check`.
- **Before `$gsd-verify-work`:** Full suite must be green in a real Chromium-compatible browser, not only `fake-indexeddb`.
- **Max feedback latency:** 30 seconds for a focused task check; 120 seconds for the full phase suite.

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirements | Threat Ref | Test Type | Automated Command | Status |
|---------|------|------|--------------|------------|-----------|-------------------|--------|
| 01-01-01 | 01-01 | 0 | all (tooling gate) | T-01-SC | Node unit + live official provenance | `node --test scripts/verify-dependency-provenance.mjs && node scripts/verify-dependency-provenance.mjs --config config/dependency-provenance.json --report artifacts/provenance/phase1-dependencies.json --blocker artifacts/provenance/phase1-blocker.json` | ✅ green |
| 01-01-02 | 01-01 | 0 | all (Rust/WASM gate) | T-01-02 | Toolchain/metadata | `npm run check` | ✅ green |
| 01-01-03 | 01-01 | 0 | all (Node/browser gate) | T-01-SC | Lock unit + installed tools | `npm run check` | ✅ green |
| 01-02-01 | 01-02 | 1 | FLOW-01, FLOW-02, FLOW-04, FLOW-05, QUAL-08 | T-02-01..03 | Real Chromium tracer | `npm run test:browser -- walking-skeleton` | ✅ green |
| 01-02-02 | 01-02 | 1 | FLOW-05, QUAL-08 | T-02-03 | TypeScript/build + browser | `npm run typecheck && npm run build:web && npm run test:browser -- walking-skeleton` | ✅ green |
| 01-03-01 | 01-03 | 2 | FLOW-01 | T-03-01, T-03-02 | Rust schema integration | `cargo test -p flow-core --test schema --locked` | ✅ green |
| 01-03-02 | 01-03 | 2 | FLOW-01, FLOW-02 | T-03-01, T-03-02 | Rust golden/limits integration | `cargo test -p flow-core --test persistence_round_trip --locked` | ✅ green |
| 01-03-03 | 01-03 | 2 | FLOW-03 | T-03-03 | Rust migration golden + WASM check | `cargo test -p flow-core --test migration_golden --locked && cargo check -p flow-wasm --target wasm32-unknown-unknown --locked` | ✅ green |
| 01-04-01 | 01-04 | 3 | EDIT-06, EDIT-07 | T-04-01 | Rust transaction/precondition tracer | `cargo test -p flow-core --test transaction_tracer --test preconditions --locked` | ✅ green |
| 01-04-02 | 01-04 | 3 | EDIT-07 | T-04-01 | Rust anchor boundary integration | `cargo test -p flow-core --test preconditions --locked` | ✅ green |
| 01-04-03 | 01-04 | 3 | EDIT-06 | T-04-02 | Rust stateful property | `cargo test -p flow-core --test transaction_properties --locked` | ✅ green |
| 01-05-01 | 01-05 | 4 | FLOW-02, FLOW-04, FLOW-05, QUAL-08 | T-05-01, T-05-03 | Rust recovery tracer + WASM check | `cargo test -p flow-core --test recovery_tracer --locked && cargo check -p flow-wasm --target wasm32-unknown-unknown --locked` | ✅ green |
| 01-05-02 | 01-05 | 4 | FLOW-02, FLOW-03, FLOW-04 | T-05-01, T-05-02 | Persistence/recovery + validated benchmark | `cargo test -p flow-core --test persistence_round_trip --test recovery --locked && node scripts/verify-recovery-benchmark.mjs --self-test` | ✅ green |
| 01-05-03 | 01-05 | 4 | FLOW-05, QUAL-08 | T-05-03, T-05-04 | Rust negative privacy + provenance | `cargo test -p flow-core --test audit_redaction --test provenance --locked` | ✅ green |
| 01-06-01 | 01-06 | 5 | FLOW-01..05, EDIT-06, EDIT-07, QUAL-08 | T-06-01, T-06-02 | Real Chromium lifecycle/recovery | `npm run test:browser` | ✅ green |
| 01-06-02 | 01-06 | 5 | FLOW-05, QUAL-08 | T-06-03, T-06-04 | Vitest DOM + TypeScript | `npm run test:unit && npm run typecheck` | ✅ green |
| 01-06-03 | 01-06 | 5 | all | T-06-01..05 | Chromium accessibility + boundary + full deterministic gate | `npm run check` | ✅ green |

### Threat Reference Index

| Ref | Threat | Required evidence |
|-----|--------|-------------------|
| T-01 | Corrupt or conflicting persisted snapshot/log | Hash/revision/schema validation and deterministic recovery failure fixtures |
| T-02 | Oversized, deeply nested, or structurally invalid canonical input | Checked `DocumentLimits`, bounded decoding/validation, and boundary fixtures |
| T-03 | Anchor or stale-command retargeting | Revision precondition, UTF-16 boundary checks, explicit mapping, and unchanged-state assertions |
| T-04 | Audit becomes a shadow content/transcript store | Allowlist-only audit DTO plus forbidden-value negative tests |
| T-05 | Browser acknowledges a partial IndexedDB write | One short transaction, acknowledgement after completion, and real-browser interruption/reload tests |

---

## Wave 0 Requirements — Plan 01-01

- [x] Task 01-01-01 verifies every direct dependency through official registries/repos and emits success report or fail-closed blocker.
- [x] Task 01-01-02 installs/checksums and pins Rust, rustfmt, Clippy, WASM target, bindgen CLI, workspace manifests, and `Cargo.lock`.
- [x] Task 01-01-03 pins npm dependencies, `package-lock.json`, TypeScript, named Vitest projects, and real Chromium.
- [x] Plan 01-02 established the real-browser walking-skeleton test before the production expansion.
- [x] Plans 01-03 through 01-06 provide named automated tests for each behavior expansion.
- [x] Task 01-06-03 reports observed focused/full-suite runtimes; no fabricated pass/test counts are gated.
- [x] Task 01-05-02 produces exactly one validated terminal recovery benchmark artifact; `npm run check` rejects a blocker or ambiguous/missing artifact.

---

## End-of-Phase Observational Review (non-blocking)

The Codex executor may visually inspect current Chrome/Edge at 320px and 1280px after Task 01-06-03, but no plan pauses for human approval. Recovery, focus/status semantics, localization, full-hash access, overflow, and audit redaction are all required automated browser assertions; observation is supplemental only.

---

## Validation Sign-Off

- [x] All final tasks have synchronized `<automated>` verification commands.
- [x] Sampling continuity: no three consecutive tasks lack an automated check.
- [x] Wave 0 covers every test/config/toolchain reference used by the phase gate.
- [x] No watch-mode flags appear in plan verification commands.
- [x] Focused feedback latency is below 30 seconds and the full suite is below 120 seconds.
- [x] `nyquist_compliant: true` is set only after every mapped row is wired and green.

**Approval:** validated — all eight Phase 1 requirements have green automated evidence and no manual-only Nyquist gaps.

## Validation Audit 2026-08-25

| Metric | Count |
|--------|-------|
| Requirements audited | 8 |
| Task rows audited | 17 |
| Gaps found | 1 |
| Resolved | 1 |
| Escalated | 0 |

The audit found that the split asset tests did not prove a non-empty asset through the complete browser boundary. Commit `91bb923` closes that gap with a real Chromium tracer that migrates a legacy document containing exact non-empty bytes and a verified BLAKE3 digest, commits it atomically to IndexedDB, cold-remounts the application, and queries the reopened document through the real Rust/WASM boundary.

The terminal `npm run check` run passed after the gap closure and UI remediation, covering dependency provenance and locks, structural boundaries, Rust formatting and warnings-as-errors, 78 Rust tests, WASM, TypeScript, 29 unit/inspector tests, 14 Chromium tests, benchmark validation, and two deterministic replay rounds. The focused accessibility pass completed in 1.39 seconds and the full gate in 20.46 seconds.

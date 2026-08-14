---
phase: 1
slug: durable-flow-foundation
status: ready
nyquist_compliant: false
wave_0_complete: false
created: 2026-08-14
---

# Phase 1 — Validation Strategy

> Per-phase validation contract synchronized to plans `01-01` through `01-06`. `nyquist_compliant` becomes true only after every mapped command is implemented and green.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test harness + `proptest`; Vitest unit/browser projects for the TypeScript adapter and Foundation Inspector |
| **Config file** | `Cargo.toml` and `vitest.config.ts` — Wave 0 creates both |
| **Quick run command** | `cargo test -p flow-core --lib` |
| **Full suite command** | `npm run check` (orchestrates format, Clippy, Rust tests, WASM target check, Vitest unit, and Chromium browser tests) |
| **Estimated runtime** | Target <30 seconds quick and <120 seconds full; Wave 0 records actual p50 on the local environment |

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
| 01-01-01 | 01-01 | 0 | all (tooling gate) | T-01-SC | Node unit + live official provenance | `node --test scripts/verify-dependency-provenance.mjs && node scripts/verify-dependency-provenance.mjs --config config/dependency-provenance.json --report artifacts/provenance/phase1-dependencies.json --blocker artifacts/provenance/phase1-blocker.json` | ⬜ pending |
| 01-01-02 | 01-01 | 0 | all (Rust/WASM gate) | T-01-02 | Toolchain/metadata | `rustc --version --verbose && cargo --version && rustup component list --installed && rustup target list --installed && wasm-bindgen --version && cargo metadata --locked --format-version 1 >/dev/null` | ⬜ pending |
| 01-01-03 | 01-01 | 0 | all (Node/browser gate) | T-01-SC | Lock unit + installed tools | `node --test scripts/verify-dependency-locks.mjs && node scripts/verify-dependency-locks.mjs && npm ci --ignore-scripts && npm exec -- playwright --version && npm exec -- vitest --version && npm exec -- tsc --version` | ⬜ pending |
| 01-02-01 | 01-02 | 1 | FLOW-01, FLOW-02, FLOW-04, FLOW-05, QUAL-08 | T-02-01..03 | Real Chromium tracer | `npm run build:wasm && npm run test:browser -- walking-skeleton` | ⬜ pending |
| 01-02-02 | 01-02 | 1 | FLOW-05, QUAL-08 | T-02-03 | TypeScript/build + browser | `npm run typecheck && npm run build:web && npm run test:browser -- walking-skeleton` | ⬜ pending |
| 01-03-01 | 01-03 | 2 | FLOW-01 | T-03-01, T-03-02 | Rust unit + deterministic round-trip | `cargo test -p flow-core schema -- --nocapture` | ⬜ pending |
| 01-03-02 | 01-03 | 2 | FLOW-01, FLOW-02 | T-03-01, T-03-02 | Rust integration + golden/limits | `cargo test -p flow-core persistence_round_trip -- --nocapture` | ⬜ pending |
| 01-03-03 | 01-03 | 2 | FLOW-03 | T-03-03 | Rust migration golden + WASM check | `cargo test -p flow-core migration_golden -- --nocapture && cargo check -p flow-wasm --target wasm32-unknown-unknown` | ⬜ pending |
| 01-04-01 | 01-04 | 3 | EDIT-06, EDIT-07 | T-04-01 | Rust transaction/precondition tracer | `cargo test -p flow-core transaction_tracer preconditions -- --nocapture && cargo check -p flow-wasm --target wasm32-unknown-unknown` | ⬜ pending |
| 01-04-02 | 01-04 | 3 | EDIT-07 | T-04-01 | Rust anchor boundary unit/property | `cargo test -p flow-core preconditions -- --nocapture` | ⬜ pending |
| 01-04-03 | 01-04 | 3 | EDIT-06 | T-04-02 | Rust stateful property | `cargo test -p flow-core transaction_properties -- --nocapture` | ⬜ pending |
| 01-05-01 | 01-05 | 4 | FLOW-02, FLOW-04, FLOW-05, QUAL-08 | T-05-01, T-05-03 | Rust recovery tracer + WASM check | `cargo test -p flow-core recovery_tracer -- --nocapture && cargo check -p flow-wasm --target wasm32-unknown-unknown` | ⬜ pending |
| 01-05-02 | 01-05 | 4 | FLOW-02, FLOW-03, FLOW-04 | T-05-01, T-05-02 | Rust persistence/recovery integration + executable p50/p95-or-blocker gate | `cargo test -p flow-core persistence_round_trip recovery -- --nocapture && node scripts/verify-recovery-benchmark.mjs` | ⬜ pending |
| 01-05-03 | 01-05 | 4 | FLOW-05, QUAL-08 | T-05-03, T-05-04 | Rust negative privacy + provenance | `cargo test -p flow-core audit_redaction provenance -- --nocapture` | ⬜ pending |
| 01-06-01 | 01-06 | 5 | FLOW-01..05, EDIT-06, EDIT-07, QUAL-08 | T-06-01, T-06-02 | Real Chromium lifecycle/recovery | `npm run build:wasm && npm run test:browser -- walking-skeleton recovery` | ⬜ pending |
| 01-06-02 | 01-06 | 5 | FLOW-05, QUAL-08 | T-06-03, T-06-04 | Vitest DOM + TypeScript | `npm run test:unit -- foundation-inspector && npm run typecheck` | ⬜ pending |
| 01-06-03 | 01-06 | 5 | all | T-06-01..05 | Chromium a11y + boundary + full deterministic gate | `npm run test:browser -- accessibility && node --test tests/contracts/phase1-boundary.test.mjs && npm run check` | ⬜ pending |

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

- [ ] Task 01-01-01 verifies every direct dependency through official registries/repos and emits success report or fail-closed blocker.
- [ ] Task 01-01-02 installs/checksums and pins Rust, rustfmt, Clippy, WASM target, bindgen CLI, workspace manifests, and `Cargo.lock`.
- [ ] Task 01-01-03 pins npm dependencies, `package-lock.json`, TypeScript, named Vitest projects, and real Chromium.
- [ ] Plan 01-02 starts the red real-browser walking-skeleton test before production implementation.
- [ ] Each behavior expansion in Plans 01-03 through 01-06 writes its named RED test before implementation.
- [ ] Task 01-06-03 records measured focused/full-suite runtimes; no fabricated pass/test counts are gated.
- [ ] Task 01-05-02 produces exactly one validated terminal recovery benchmark artifact; `npm run check` rejects a blocker or ambiguous/missing evidence.

---

## End-of-Phase Observational Review (non-blocking)

The Codex executor may visually inspect current Chrome/Edge at 320px and 1280px after Task 01-06-03, but no plan pauses for human approval. Recovery, focus/status semantics, localization, full-hash access, overflow, and audit redaction are all required automated browser assertions; observation is supplemental only.

---

## Validation Sign-Off

- [x] All final tasks have synchronized `<automated>` verification commands.
- [ ] Sampling continuity: no three consecutive tasks lack an automated check.
- [ ] Wave 0 covers all currently missing test/config references.
- [ ] No watch-mode flags appear in plan verification commands.
- [ ] Measured focused feedback latency is below 30 seconds and the full suite below 120 seconds, or an evidence-backed exception is recorded.
- [ ] `nyquist_compliant: true` is set only after every mapped row is wired and green.

**Approval:** planning synchronized — execution/verifier evidence remains pending.

---
phase: 1
slug: durable-flow-foundation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-08-14
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution. The planner must replace the provisional task references below with final task IDs and waves without dropping any requirement row.

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

Task IDs and waves are provisional until PLAN.md files exist. Every final task must reference one or more rows and include its automated command.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| planner assigns | TBD | TBD | FLOW-01 | T-02 | Reject invalid/oversized schema input before publication; preserve exact Ukrainian/English bytes | unit + golden | `cargo test -p flow-core schema` | ❌ W0 | ⬜ pending |
| planner assigns | TBD | TBD | FLOW-02 | T-01 | Canonical round-trip and asset lookup detect corruption and lose no semantic fields | integration + golden | `cargo test -p flow-core persistence_round_trip` | ❌ W0 | ⬜ pending |
| planner assigns | TBD | TBD | FLOW-03 | T-01, T-02 | Pure sequential migration rejects unknown future versions and validates every hop | golden | `cargo test -p flow-core migration_golden` | ❌ W0 | ⬜ pending |
| planner assigns | TBD | TBD | FLOW-04 | T-01, T-05 | Recovery accepts only a verified contiguous durable prefix and never guesses across gaps/conflicts | integration + browser | `cargo test -p flow-core recovery && npm run test:unit -- indexeddb-store && npm run test:browser -- recovery` | ❌ W0 | ⬜ pending |
| planner assigns | TBD | TBD | FLOW-05 | T-01 | Revision and provenance DTOs identify create/migration lineage without fabricating export provenance | unit + UI contract | `cargo test -p flow-core provenance && npm run test:unit -- foundation-inspector` | ❌ W0 | ⬜ pending |
| planner assigns | TBD | TBD | EDIT-06 | T-03 | Every reversible command sequence returns to the original canonical hash through transactional undo/redo | property | `cargo test -p flow-core transaction_properties` | ❌ W0 | ⬜ pending |
| planner assigns | TBD | TBD | EDIT-07 | T-03 | Stale, duplicate, invalid-target, and invalid UTF-16 boundary commands leave bytes/revision/history unchanged | property + unit | `cargo test -p flow-core preconditions` | ❌ W0 | ⬜ pending |
| planner assigns | TBD | TBD | QUAL-08 | T-04 | Audit serialization is allowlisted and contains no document text, command arguments, raw audio, or transcript | negative unit + browser | `cargo test -p flow-core audit_redaction && npm run test:unit -- indexeddb-store foundation-inspector` | ❌ W0 | ⬜ pending |

### Threat Reference Index

| Ref | Threat | Required evidence |
|-----|--------|-------------------|
| T-01 | Corrupt or conflicting persisted snapshot/log | Hash/revision/schema validation and deterministic recovery failure fixtures |
| T-02 | Oversized, deeply nested, or structurally invalid canonical input | Checked `DocumentLimits`, bounded decoding/validation, and boundary fixtures |
| T-03 | Anchor or stale-command retargeting | Revision precondition, UTF-16 boundary checks, explicit mapping, and unchanged-state assertions |
| T-04 | Audit becomes a shadow content/transcript store | Allowlist-only audit DTO plus forbidden-value negative tests |
| T-05 | Browser acknowledges a partial IndexedDB write | One short transaction, acknowledgement after completion, and real-browser interruption/reload tests |

---

## Wave 0 Requirements

- [ ] Install and pin Rust stable, `rustfmt`, Clippy, and `wasm32-unknown-unknown`; record exact versions.
- [ ] Independently verify package provenance from official crates.io/npm repositories, then generate reviewed `Cargo.lock` and `package-lock.json` without floating ranges.
- [ ] Create Cargo workspace manifests, `flow-core`/`flow-wasm` test targets, and fixture directories.
- [ ] Create `package.json`, `vitest.config.ts`, unit/browser projects, `fake-indexeddb` setup, and Chromium-compatible browser runtime.
- [ ] Add empty red tests or tracer tests for all eight requirement rows before their implementation tasks begin.
- [ ] Measure quick/full suite runtime and replace the target values above with recorded numbers.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Foundation Inspector visual hierarchy, focus order, live-region behavior, and 320px/1280px overflow | FLOW-05, QUAL-08 | Visual and assistive-technology quality needs human observation even when DOM assertions pass | Run the local inspector in current Chrome/Edge; complete create → mutate → stale conflict → undo/redo → save/reload/recover; inspect focus, announcements, full hash access, long Ukrainian/English copy, and audit redaction. |

The durable recovery result itself is automated; the manual row supplements rather than replaces browser tests.

---

## Validation Sign-Off

- [ ] All final tasks have `<automated>` verification or an explicit Wave 0 dependency.
- [ ] Sampling continuity: no three consecutive tasks lack an automated check.
- [ ] Wave 0 covers all currently missing test/config references.
- [ ] No watch-mode flags appear in plan verification commands.
- [ ] Measured focused feedback latency is below 30 seconds and the full suite below 120 seconds, or an evidence-backed exception is recorded.
- [ ] `nyquist_compliant: true` is set only after every mapped row is wired and green.

**Approval:** pending — planner must synchronize final task IDs/waves; verifier approves after execution evidence exists.

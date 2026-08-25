---
phase: FLOWPDF-01-durable-flow-foundation
fixed_at: 2026-08-21T11:07:32Z
review_path: /Users/serhii/Documents/Codex/2026-08-13/new-chat/.planning/phases/FLOWPDF-01-durable-flow-foundation/01-REVIEW.md
iteration: 1
findings_in_scope: 28
fixed: 24
skipped: 4
status: partial
---

# Phase 01: Code Review Fix Report

**Fixed at:** 2026-08-21T11:07:32Z
**Source review:** `/Users/serhii/Documents/Codex/2026-08-13/new-chat/.planning/phases/FLOWPDF-01-durable-flow-foundation/01-REVIEW.md`
**Iteration:** 1

**Summary:**

- Findings in scope: 28
- Fixed: 24
- Skipped: 4

## Fixed Issues

### CR-01: Concurrent IndexedDB commits can fork one durable revision

**Status:** fixed: requires human verification
**Files modified:** `web/persistence/indexeddb-store.ts`, `web/src/foundation-inspector.ts`, `web/tests/indexeddb-store.test.ts`
**Commit:** `4d03716`
**Applied fix:** Added an atomic per-document head compare-and-swap protocol, exact-retry handling, core-validated head bootstrap, and a two-writer race regression.

### CR-02: Stable IDs can be invalid or have multiple logical spellings

**Status:** fixed: requires human verification
**Files modified:** `crates/flow-core/src/canonical/mod.rs`, `crates/flow-core/src/model/mod.rs`, `crates/flow-core/tests/audit_redaction.rs`, `crates/flow-core/tests/schema.rs`
**Commit:** `17e338c`
**Applied fix:** Routed deserialization through validating constructors and canonicalized UUID identities so equivalent spellings cannot create distinct durable identities.

### CR-03: Partial stores are treated as empty and repaired by guesswork

**Status:** fixed: requires human verification
**Files modified:** `crates/flow-core/src/store/mod.rs`, `crates/flow-core/tests/recovery.rs`, `web/persistence/indexeddb-store.ts`, `web/src/foundation-inspector.ts`, `web/tests/indexeddb-store.test.ts`
**Commit:** `ca292d4`
**Applied fix:** Defined emptiness across every physical record store and made partial creation/migration sets fail closed, including every missing-member permutation.

### CR-04: Schema-valid empty and image-only documents cannot recover

**Status:** fixed: requires human verification
**Files modified:** `crates/flow-core/examples/recovery_benchmark.rs`, `crates/flow-core/src/lib.rs`, `crates/flow-core/tests/preconditions.rs`, `crates/flow-core/tests/recovery.rs`, `crates/flow-core/tests/recovery_tracer.rs`, `web/src/foundation-inspector.ts`, `web/tests/transaction-boundary.browser.test.ts`
**Commit:** `4c8b6d9`
**Applied fix:** Made the next command target optional and disabled text commands when no paragraph target exists while keeping empty/image-only documents recoverable.

### CR-05: Canonical validation accepts impossible provenance timestamps

**Status:** fixed: requires human verification
**Files modified:** `crates/flow-core/src/audit/mod.rs`, `crates/flow-core/src/provenance/mod.rs`, `crates/flow-core/src/schema/mod.rs`, `crates/flow-core/tests/migration_golden.rs`, `crates/flow-core/tests/schema.rs`
**Commit:** `823add0`
**Applied fix:** Centralized compact UTC parsing with full calendar validation and reused it for schema, audit, provenance, and migration boundaries.

### CR-06: Asset and revision-hash validation admits unrecoverable records

**Status:** fixed: requires human verification
**Files modified:** `crates/flow-core/src/provenance/mod.rs`, `crates/flow-core/src/schema/mod.rs`, `crates/flow-core/tests/provenance.rs`, `crates/flow-core/tests/schema.rs`
**Commit:** `7068234`
**Applied fix:** Required canonical lowercase hashes, rejected conflicting descriptor metadata, and enforced checked per-asset and encoded aggregate recovery budgets.

### CR-07: Browser failures and recovery omit the durable audit records produced by the core

**Status:** fixed: requires human verification
**Files modified:** `artifacts/benchmarks/phase1-recovery.json`, `crates/flow-core/src/lib.rs`, `crates/flow-core/src/store/mod.rs`, `crates/flow-core/tests/recovery_tracer.rs`, `crates/flow-wasm/src/lib.rs`, `tests/contracts/phase1-boundary.test.mjs`, `web/persistence/indexeddb-store.ts`, `web/src/foundation-inspector.ts`, `web/tests/accessibility.browser.test.ts`, `web/tests/foundation-inspector.test.ts`, `web/tests/indexeddb-store.test.ts`, `web/tests/recovery.browser.test.ts`, `web/tests/walking-skeleton.browser.test.ts`
**Commit:** `80850c4`
**Applied fix:** Preserved typed error audits, added Rust/WASM authorization plus idempotent audit-only IndexedDB commits, switched recovery to the audited boundary, refreshed verified views, retained prior views on corrupt recovery, covered action/outcome/revision/privacy contracts, and regenerated source-bound recovery evidence.

### CR-08: Every browser audit event is stamped with a fabricated date

**Status:** fixed: requires human verification
**Files modified:** `web/src/foundation-inspector.ts`, `web/tests/foundation-inspector.test.ts`
**Commit:** `8bdf191`
**Applied fix:** Injected a production clock, generated compact UTC timestamps per operation, and added deterministic browser timestamp coverage.

### CR-09: The version-3 IndexedDB upgrade makes version-2 documents inaccessible

**Status:** fixed: requires human verification
**Files modified:** `web/persistence/indexeddb-store.ts`, `web/tests/indexeddb-store.test.ts`
**Commit:** `e743c5c`
**Applied fix:** Separated database schema and record-format versions, preserved legacy stores, surfaced an explicit migration-required state, blocked incompatible writes, and required core-validated head bootstrap for existing v3 data.

### CR-10: Migrated documents cannot use reload/recover, and the browser test passes on a no-op

**Status:** fixed: requires human verification
**Files modified:** `web/src/foundation-inspector.ts`, `web/tests/walking-skeleton.browser.test.ts`
**Commit:** `8224952`
**Applied fix:** Marked migration records durable immediately and strengthened the remount/reload test so disabled-control no-ops cannot pass.

### CR-11: Pinned Vitest Browser Mode versions have critical file-access/RCE advisories

**Status:** fixed
**Files modified:** `artifacts/provenance/phase1-dependencies.json`, `config/dependency-provenance.json`, `package-lock.json`, `package.json`, `vitest.config.ts`
**Commit:** `0ae2236`
**Applied fix:** Upgraded the Vitest browser stack to 4.1.11 and restricted the browser API to loopback with file writes and command execution disabled.

### CR-13: The build executes an unverified wasm-bindgen binary

**Status:** fixed
**Files modified:** `config/dependency-provenance.json`, `package.json`, `scripts/verify-wasm-bindgen-tool.mjs`, `scripts/verify-wasm-bindgen-tool.test.mjs`
**Commit:** `3e07451`
**Applied fix:** Added a fail-closed version/receipt/SHA-256 check before the build invokes the pinned wasm-bindgen executable.

### CR-14: Lock verification silently skips its real check on encoded/cross-platform paths

**Status:** fixed: requires human verification
**Files modified:** `scripts/verify-dependency-locks.mjs`
**Commit:** `53626b4`
**Applied fix:** Normalized the entry-module path via URL/path APIs and added encoded-path coverage so direct execution cannot silently skip lock verification.

### WR-01: RevisionProvenance deserialization bypasses source-hash invariants

**Status:** fixed: requires human verification
**Files modified:** `crates/flow-core/src/provenance/mod.rs`, `crates/flow-core/tests/provenance.rs`
**Commit:** `bf338c0`
**Applied fix:** Replaced derived deserialization with checked construction that enforces canonical, unique, bounded source hashes.

### WR-02: Audit transaction-type helper constructs actions the validator rejects

**Status:** fixed: requires human verification
**Files modified:** `crates/flow-core/src/audit/mod.rs`, `crates/flow-core/tests/audit_redaction.rs`
**Commit:** `c2e4079`
**Applied fix:** Restricted command-kind conversion to actual command actions and kept create/recovery in their separate action variants.

### WR-04: Failure-atomicity tests compare against an after-the-fact clone

**Status:** fixed
**Files modified:** `crates/flow-core/tests/preconditions.rs`
**Commit:** `25add8b`
**Applied fix:** Captured document/history baselines before each rejected operation so the tests prove non-mutation rather than cloning the result.

### WR-05: Rejected initialization promises permanently poison the page and database lifecycle

**Status:** fixed: requires human verification
**Files modified:** `web/persistence/indexeddb-store.ts`, `web/src/foundation-inspector.ts`, `web/tests/indexeddb-store.test.ts`
**Commit:** `2f5c095`
**Applied fix:** Cleared rejected cached promises, handled version-change closure, and allowed blocked IndexedDB upgrades to resume when blockers close.

### WR-06: Web builds retain deleted TypeScript outputs

**Status:** fixed
**Files modified:** `package.json`, `scripts/build-web.mjs`, `scripts/build-web.test.mjs`
**Commit:** `10c4094`
**Applied fix:** Added a safe clean phase before TypeScript emit and regression coverage for stale output removal.

### WR-07: English UI still creates a Ukrainian-locale document

**Status:** fixed: requires human verification
**Files modified:** `web/src/foundation-inspector.ts`, `web/tests/foundation-inspector.test.ts`
**Commit:** `df59a8d`
**Applied fix:** Mapped UI locale `uk` to `uk-UA` and `en` to `en-US`, with snapshot assertions for both.

### WR-08: Static-file read errors can terminate the inspector server

**Status:** fixed: requires human verification
**Files modified:** `scripts/serve-inspector.mjs`, `scripts/serve-inspector.test.mjs`
**Commit:** `61a6212`
**Applied fix:** Awaited the file pipeline and handled post-header stream failures without an unhandled rejection or process termination.

### WR-09: Configured provenance retries skip transient HTTP responses

**Status:** fixed: requires human verification
**Files modified:** `scripts/verify-dependency-provenance.mjs`
**Commit:** `e2fc001`
**Applied fix:** Implemented bounded retry for configured transient statuses while preserving fail-closed behavior for permanent provenance failures.

### WR-11: Phase-boundary enforcement is bypassable through trivial aliases

**Status:** fixed: requires human verification
**Files modified:** `tests/contracts/phase1-boundary.test.mjs`
**Commit:** `36333ab`
**Applied fix:** Added symbol-aware TypeScript capability/alias analysis and lexical typed Rust/WASM export validation with adversarial fixtures.

### WR-13: Recovery helper treats unexpected transaction aborts as success

**Status:** fixed: requires human verification
**Files modified:** `web/tests/recovery.browser.test.ts`
**Commit:** `5869972`
**Applied fix:** Split normal transaction completion from the deliberate-abort helper so unexpected abort/error events reject tests.

### WR-14: Required Node runtime features are not pinned or enforced

**Status:** fixed
**Files modified:** `.node-version`, `package-lock.json`, `package.json`, `scripts/build-web.mjs`, `scripts/check-phase1.mjs`, `scripts/node-version.mjs`, `scripts/node-version.test.mjs`, `scripts/serve-inspector.mjs`, `scripts/verify-recovery-benchmark.mjs`
**Commit:** `dc8dbef`
**Applied fix:** Pinned Node 24.10.0/npm 11.9.0 and enforced the runtime before every script that depends on its APIs.

## Skipped Issues

### CR-12: The dependency gate can omit newly added direct dependencies

**File:** `scripts/verify-dependency-provenance.mjs:284`
**Reason:** An attempted manifest/report completeness fix failed the focused provenance fixture with `report identities missing ecosystem`; it was rolled back rather than weakening the fail-closed provenance contract. This needs a coordinated report-schema and fixture migration.
**Original issue:** Verification iterated configured identities rather than proving every direct manifest dependency is represented in config and the signed report.

### WR-03: Future migration hops validate only a version number

**File:** `crates/flow-core/src/schema/mod.rs:237`
**Reason:** No multi-hop schema or version-specific decoder exists in the current codebase, so inventing a future hop contract would guess unsupported semantics. A concrete future schema/migration specification is required first.
**Original issue:** A future additional hop could pass version continuity without validating the intermediate representation.

### WR-10: npm provenance accepts insecure registry tarball URLs

**File:** `scripts/verify-dependency-provenance.mjs:350`
**Reason:** The strict tarball-origin candidate broke existing provenance fixtures; the change was rolled back to avoid silently changing the accepted evidence contract. The registry-origin policy and fixtures must be migrated together.
**Original issue:** npm tarball URLs were checked for exact string equality but not an HTTPS npm-registry origin policy.

### WR-12: Node toolchain test is tautological

**File:** `scripts/node-version.test.mjs:6`
**Reason:** An explicit Vitest Node-environment marker caused the project typecheck to omit required Node ambient types; the attempt was rolled back. The test-runner environment contract needs a compatible typed configuration change.
**Original issue:** The test verifies the same process that loaded it and does not prove Vitest selected the intended Node environment.

## Verification

Targeted verification first ran in the isolated review-fix worktree. After transactional fast-forward and worktree cleanup, the reproducible aggregate gate ran in the main checkout on commit `80850c4` with Node 24.10.0, npm 11.9.0, and Vitest 4.1.11.

- `npm run check` — passed in the main checkout; full Phase 1 runtime 45.79s.
- Dependency provenance, 67 Cargo locks, 83 npm locks, and the Phase 1 boundary contract — passed.
- Rust format and workspace Clippy with warnings denied — passed.
- Full Rust workspace suite — passed, 74 tests.
- Recovery benchmark evidence was regenerated against the fixed source manifest; p95 487.241458ms under the 2000ms target, and 8 adversarial validator cases were rejected.
- WASM target check, verified wasm-bindgen release build, and TypeScript typecheck — passed.
- Node plus Inspector unit suites — passed, 25 tests.
- Focused accessibility suite — passed, 4 Chromium tests.
- Full real-Chromium suite — passed, 12 tests.
- Canonical and migration deterministic replays — passed twice each.

---

_Fixed: 2026-08-21T11:07:32Z_
_Fixer: the agent (gsd-code-fixer)_
_Iteration: 1_

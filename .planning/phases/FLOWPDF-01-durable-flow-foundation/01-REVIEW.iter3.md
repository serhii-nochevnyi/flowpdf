---
phase: FLOWPDF-01-durable-flow-foundation
snapshot_iteration: 3
reviewed: 2026-08-21T11:27:25Z
depth: standard
files_reviewed: 54
files_reviewed_list:
  - .gitignore
  - Cargo.toml
  - config/dependency-provenance.json
  - crates/flow-core/Cargo.toml
  - crates/flow-core/examples/recovery_benchmark.rs
  - crates/flow-core/proptest-regressions/transaction_properties.txt
  - crates/flow-core/src/anchor/mod.rs
  - crates/flow-core/src/audit/mod.rs
  - crates/flow-core/src/canonical/mod.rs
  - crates/flow-core/src/lib.rs
  - crates/flow-core/src/model/mod.rs
  - crates/flow-core/src/provenance/mod.rs
  - crates/flow-core/src/schema/mod.rs
  - crates/flow-core/src/store/mod.rs
  - crates/flow-core/src/transaction/mod.rs
  - crates/flow-core/tests/audit_redaction.rs
  - crates/flow-core/tests/migration_golden.rs
  - crates/flow-core/tests/persistence_round_trip.rs
  - crates/flow-core/tests/preconditions.rs
  - crates/flow-core/tests/provenance.rs
  - crates/flow-core/tests/recovery.rs
  - crates/flow-core/tests/recovery_tracer.rs
  - crates/flow-core/tests/schema.rs
  - crates/flow-core/tests/transaction_properties.rs
  - crates/flow-core/tests/transaction_tracer.rs
  - crates/flow-wasm/Cargo.toml
  - crates/flow-wasm/src/lib.rs
  - package.json
  - rust-toolchain.toml
  - scripts/build-web.mjs
  - scripts/check-phase1.mjs
  - scripts/serve-inspector.mjs
  - scripts/verify-dependency-locks.mjs
  - scripts/verify-dependency-provenance.mjs
  - scripts/verify-recovery-benchmark.mjs
  - tests/contracts/phase1-boundary.test.mjs
  - tsconfig.json
  - tsconfig.web.json
  - vitest.config.ts
  - web/index.html
  - web/persistence/indexeddb-store.ts
  - web/src/foundation-inspector.ts
  - web/src/i18n/en.ts
  - web/src/i18n/uk.ts
  - web/src/main.ts
  - web/src/styles.css
  - web/tests/accessibility.browser.test.ts
  - web/tests/foundation-inspector.test.ts
  - web/tests/indexeddb-store.test.ts
  - web/tests/recovery.browser.test.ts
  - web/tests/toolchain.browser.test.ts
  - web/tests/toolchain.test.ts
  - web/tests/transaction-boundary.browser.test.ts
  - web/tests/walking-skeleton.browser.test.ts
findings:
  critical: 8
  warning: 7
  info: 0
  total: 15
status: issues_found
---

# Phase FLOWPDF-01: Code Review Report

**Reviewed:** 2026-08-21T11:27:25Z
**Depth:** standard
**Files Reviewed:** 54
**Status:** issues_found
**Reviewed Commit:** 80850c4

## Narrative Findings (AI reviewer)

## Summary

Phase 1 remains unsafe to ship. The re-review verified 20 of the 24 claimed fixes, found four claimed fixes incomplete, and confirmed that all four skipped findings still apply. Independent probes also found new data-integrity failures: authorized writes can cross the recovery envelope and brick the next reopen, recovery silently ignores unsupported-schema transactions below a checkpoint, and the new standalone-audit boundary accepts facts that were never derived by a core operation.

The normal suites are green, but they do not exercise these seams. The review passed 74 Rust tests, workspace Clippy with warnings denied, 25 unit/inspector tests, 12 real-Chromium tests, TypeScript checking, dependency lock checks, and npm audit with zero reported vulnerabilities. Targeted adversarial probes reproduced the record-limit, unsupported-transaction, omitted-provenance, unsafe-registry-URL, and audit-identity failures described below.

## Fix Verification

- Verified fixed (20): original CR-01 through CR-06, CR-09 through CR-11, CR-14, WR-01, WR-02, WR-04 through WR-09, WR-13, and WR-14.
- Claimed fixed but incomplete (4): original CR-07, CR-08, CR-13, and WR-11.
- Skipped and still applicable (4): original CR-12, WR-03, WR-10, and WR-12.
- Original WR-05's rejected-open and version-change cases are fixed; WR-06 below is a separate abnormal-close lifecycle gap.

## Critical Issues

### CR-01 [BLOCKER]: Authorized writes can exceed the hard recovery envelope and brick the next reopen

**Files:**

- /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/schema/mod.rs:68-82
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/lib.rs:789-837
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/store/mod.rs:169-226,513-548,826-840
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/web/persistence/indexeddb-store.ts:202-371,688-729

**Issue:** The 10,000-record and 64 MiB limits are checked only against an existing recovery request. Normal commit planning, migration creation, and standalone-audit authorization never project and preflight the complete post-write record image. The browser then persists the returned records without an equivalent count/byte bound. A probe starting from a valid image at exactly 10,000 records showed that plan_standalone_audit accepted one more audit and the in-memory store returned Committed; the immediately following recover failed with FLOW_LIMIT_RECOVERY_RECORDS. A normal browser transaction can similarly take a 9,999-record image to 10,001 after the transaction and audit are already durable. A 10,000-asset document also leaves no room for its mandatory snapshot/audit/source records.

**Fix:** Build the exact candidate RecoverRequest for normal, migration, and audit-only writes, apply the same exact serialized-byte and record-count preflight before returning authorization, and repeat a core-issued post-image bound inside the atomic adapter transaction. Reserve mandatory record overhead when validating assets. Add N/N+1 write-then-reopen tests for all three write paths.

### CR-02 [BLOCKER]: Recovery silently discards unsupported-schema transactions below a checkpoint

**File:** /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/lib.rs:999-1030,1115-1124,1261-1273

**Issue:** Transaction normalization checks document and identity conflicts, but duplicate-revision, checkpoint-prefix, and audit-completeness checks filter to the current schema. Tail replay considers only records newer than the selected snapshot. A probe appended a fresh-ID schema_version 99 transaction at revision 1, without an audit, to an otherwise valid revision-1 image; recover still succeeded. Unsupported durable content is therefore ignored rather than rejected or preserved explicitly, violating the fail-closed recovery contract.

**Fix:** Account for every normalized transaction before snapshot selection. Reject unsupported schemas and record formats unless a transaction belongs to an explicit, validated legacy-preservation boundary; then prove every record is in the verified prefix or replay tail. Add pre-checkpoint future-schema, old-schema, and unsupported-format fixtures.

### CR-03 [BLOCKER]: Standalone audit authorization accepts fabricated durable facts

**Files:**

- /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/lib.rs:226-231,457-466,1189-1237
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/store/mod.rs:513-548
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-wasm/src/lib.rs:111-119

**Issue:** The exported WASM authorization endpoint accepts a caller-supplied AuditRecord, performs shape/identity/revision-anchor checks, and returns that same record. It does not prove that apply_command or recover_audited produced the event, nor bind the failure code, schema metadata, modality, durable sequence, or timestamp to a source operation. A fresh-ID command-failure audit with arbitrary allowed facts at revision 1 is authorized. Recovery is weaker still: its command-failure and recovery branches perform no chain binding, so a valid-shaped injected failure at unanchored revision 0 is exposed in the inspector even though the write-time planner would reject it. The audit log can therefore contain invented evidence.

**Fix:** Do not authorize serialized audit facts directly. Accept the source operation/recovery context and rerun the Rust-owned derivation, or issue a core-owned opaque authorization bound to the exact audit bytes and record head. During recovery, validate every standalone event against an existing revision anchor and the same derived schema/modality/sequence semantics. Add forged-fact and nonexistent-revision tests through native recovery and WASM.

### CR-04 [BLOCKER]: Duplicate-command failure audits are unpersistable by construction

**Files:**

- /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/lib.rs:579-600,1483-1514
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/store/mod.rs:513-548
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/web/persistence/indexeddb-store.ts:370,783-795
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/web/src/foundation-inspector.ts:301-310

**Issue:** A successful command uses its command ID as audit, transaction, and command identity. Retrying the same command returns FLOW_DUPLICATE_COMMAND, but rejected_command_audit reuses that same identity for a different failure record. Standalone authorization and IndexedDB correctly reject the divergent bytes as FLOW_STORE_IDENTITY_CONFLICT. Consequently, the new CR-07 persistence flow can never retain this legitimate failure and the UI replaces the useful duplicate-command error with the storage conflict. Existing tests use a rejected command whose ID was never durable, so they miss this sequence.

**Fix:** Give each invocation a distinct attempt/audit identity while retaining the logical command ID as linkage. Update standalone identity rules so an exact retry is idempotent but a failure after a prior success is a distinct event. Add an end-to-end success → same-command duplicate → authorize → persist → recover test.

### CR-05 [BLOCKER]: Fresh-page corrupt recovery fails before entering the audited boundary

**Files:**

- /Users/serhii/Documents/Codex/2026-08-13/new-chat/web/src/foundation-inspector.ts:502-518,587-598
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/web/tests/recovery.browser.test.ts:100-129

**Issue:** When no live session exists, restoreFromStorage first calls unaudited query_document to discover the expected document and revision. Corrupt records fail at that call, before recover_document_audited runs. The resulting error has no audit, so persistErrorAudit returns immediately. Existing corruption tests retain the already-verified live session and therefore exercise only the path that skips this pre-query. A cold start or remount loses exactly the recovery-failure evidence CR-07 claimed to make durable.

**Fix:** Persist or derive a minimal trusted recovery context from the atomic document-head record and call recover_document_audited first. The core should bind/validate that context while deriving the audit. Add create → corrupt → remount → open/recover tests that assert the failure audit is durable without relying on a live session.

### CR-06 [BLOCKER]: Sample creation still records a fabricated timestamp

**Files:**

- /Users/serhii/Documents/Codex/2026-08-13/new-chat/web/src/foundation-inspector.ts:316-324
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/lib.rs:373-408
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/model/mod.rs:464-466
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/web/tests/foundation-inspector.test.ts:242-268

**Issue:** The clock fix covers mutation, history, migration, and recovery calls, but createSample does not pass the injected timestamp. The core still writes 2026-08-14T00:00:00Z into creation provenance, transaction, and audit records. The regression test deliberately slices away the first audit row, so it cannot detect the omission. Browser-created documents continue to make a false chronology claim.

**Fix:** Add a validated issuedAt/createdAt field to CreateSampleRequest and use it consistently for provenance, the creation transaction, and the audit. Keep deterministic fixtures deterministic by injecting a fixed clock in tests, and assert the creation row rather than slicing it away.

### CR-07 [BLOCKER]: The dependency gate still accepts omitted direct dependencies

**Files:**

- /Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/check-phase1.mjs:24-33
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/verify-dependency-locks.mjs:30-80
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/verify-dependency-provenance.mjs:240-254

**Issue:** This is skipped original CR-12. The phase gate runs provenance fixture tests rather than live manifest verification. Lock checks validate configured/report entries against locks, but never require exact equality among Cargo/npm manifests, provenance configuration, and the checked-in report. An independent fixture added an exactly pinned direct npm dependency to package.json and its root/node_modules lock entries while omitting it from provenance; verifyNpmLock accepted the fixture. Cargo has the same report-driven gap.

**Fix:** Derive the full direct-dependency set from package.json and every workspace Cargo manifest, require exact ecosystem/name/version/kind equality with configuration and report entries, and bind or regenerate the live evidence in the mandatory gate. Add omitted and extra dependency fixtures for both ecosystems.

### CR-08 [BLOCKER]: The wasm-bindgen verifier executes the binary before checking its checksum

**Files:**

- /Users/serhii/Documents/Codex/2026-08-13/new-chat/package.json:12
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/config/dependency-provenance.json:12-20
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/verify-wasm-bindgen-tool.mjs:49-61
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/verify-wasm-bindgen-tool.test.mjs:41-61

**Issue:** This is incomplete original CR-13. Function-argument evaluation invokes execFileSync(binary, --version) before readFileSync supplies the bytes whose hash is checked. A replaced executable therefore runs arbitrary code during the act of verifying it, even though the validator rejects its checksum afterward. The new unit test calls only the pure validator with pre-supplied version text and cannot observe execution ordering.

**Fix:** Read and hash the file, validate its receipt, target, and approved checksum, and only then execute a race-safe handle/copy of those exact bytes if a version call is still required. Add an integration test whose checksum-mismatched sentinel executable records invocation and assert that it is never run.

## Warnings

### WR-01 [WARNING]: Non-final migration hops validate only a schema-version number

**File:** /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/schema/mod.rs:645-677

**Issue:** This is skipped original WR-03. Each hop probes schemaVersion, but full canonical decoding occurs only when the candidate is already the current version. The current registry has one v0 → v1 hop, so the defect is dormant today; the implementation nevertheless violates the locked sequential-migration contract to validate every intermediate and will accept a malformed non-final document as soon as a second hop is registered.

**Fix:** Register a version-specific decoder/validator with every migration step and validate the complete intermediate representation before the next hop. Add a malformed v0 → v1 → v2 intermediate fixture when the next schema is introduced.

### WR-02 [WARNING]: npm provenance and lock checks accept unsafe registry URLs

**Files:**

- /Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/verify-dependency-provenance.mjs:208-220
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/verify-dependency-locks.mjs:56-62

**Issue:** This is skipped original WR-10. Provenance checks only hostname, permitting plaintext HTTP, embedded credentials, and non-default ports. Lock verification adds HTTPS but still permits credentials and non-default ports. Independent probes confirmed acceptance of credential-bearing registry URLs.

**Fix:** Validate the complete URL: exact HTTPS origin, empty username/password, empty/default port, no query or fragment, and the expected registry package/tarball pathname. Add adversarial URL fixtures to both verifiers.

### WR-03 [WARNING]: The phase-boundary scanner remains bypassable across modules and renamed WASM exports

**File:** /Users/serhii/Documents/Codex/2026-08-13/new-chat/tests/contracts/phase1-boundary.test.mjs:170-190,217-234,342-371,586-616,643-655

**Issue:** This is incomplete original WR-11. Each TypeScript file is parsed in a separate noResolve program, so an exported globalThis.fetch alias in file A and an imported call in file B both pass. The Rust scanner also records the Rust function name and ignores wasm_bindgen(js_name = ...), allowing an approved Rust name to expose a different JavaScript surface. The added same-file alias fixtures do not cover either bypass.

**Fix:** Build one module-resolving TypeScript Program for all production sources and follow aliased symbols through imports/exports. For WASM, inspect the effective generated export name or parse wasm_bindgen attributes, and add cross-file and js_name adversarial fixtures.

### WR-04 [WARNING]: The Node toolchain test remains tautological

**File:** /Users/serhii/Documents/Codex/2026-08-13/new-chat/web/tests/toolchain.test.ts:3-6

**Issue:** This is skipped original WR-12. The test asserts only that a three-item literal has length three. It passes under any Vitest project or runtime and provides no evidence that the file ran in the intended named Node project.

**Fix:** Inject an explicit project marker from the unit-project configuration and assert both it and a Node-only environment invariant, with the appropriate Node types isolated to that test configuration.

### WR-05 [WARNING]: New Node regression tests are absent from the mandatory phase gate

**Files:**

- /Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/check-phase1.mjs:24-85
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/package.json:14-19
- /Users/serhii/Documents/Codex/2026-08-13/new-chat/vitest.config.ts:4-33

**Issue:** The fixes added build-web.test.mjs, node-version.test.mjs, serve-inspector.test.mjs, and verify-wasm-bindgen-tool.test.mjs, but npm run check never invokes them. Its unit step runs only the Vitest projects. These regressions for claimed WR-06, WR-08, WR-14, and CR-13 protection can all break while the mandatory phase gate remains green.

**Fix:** Add an explicit node --test step covering the helper test files to phaseOneSteps, and include that step in the gate's own contract assertions.

### WR-06 [WARNING]: Unexpected IndexedDB closure leaves a poisoned cached connection

**File:** /Users/serhii/Documents/Codex/2026-08-13/new-chat/web/persistence/indexeddb-store.ts:563-626

**Issue:** The cache is cleared on open rejection and versionchange, but the resolved IDBDatabase has no close-event handler. If the browser abnormally closes the connection, later calls reuse a resolved promise pointing at a closed database and continue failing until the page or store instance is replaced.

**Fix:** Clear databasePromise when the database emits close, using the same identity guard as the rejection/versionchange paths. Add a forced-close regression that proves the next public operation reopens successfully.

### WR-07 [WARNING]: A zero-measurement benchmark recipe panics instead of returning a diagnostic

**File:** /Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/examples/recovery_benchmark.rs:121-131,154-172,587-589

**Issue:** validate_recipe does not reject measurements == 0. The measurement loop then leaves an empty duration vector and percentile indexes element zero, panicking. Invalid evidence input therefore crashes the runner rather than producing its structured failure.

**Fix:** Require at least one measurement during recipe validation and make percentile return Result for an empty slice. Add a zero-measurement recipe test that asserts a stable diagnostic and no panic.

---

_Reviewed: 2026-08-21T11:27:25Z_
_Reviewer: the agent (gsd-code-reviewer)_
_Depth: standard_

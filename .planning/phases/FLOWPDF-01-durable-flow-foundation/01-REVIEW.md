---
phase: FLOWPDF-01-durable-flow-foundation
reviewed: 2026-08-21T09:49:46Z
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
  critical: 14
  warning: 14
  info: 0
  total: 28
status: issues_found
---

# Phase FLOWPDF-01: Code Review Report

**Reviewed:** 2026-08-21T09:49:46Z  
**Depth:** standard  
**Files Reviewed:** 54  
**Status:** issues_found

## Narrative Findings (AI reviewer)

## Summary

The Phase 1 foundation is not safe to ship. The review reproduced a cross-tab IndexedDB race that durably forks a document, several schema/deserialization paths that admit identities and records the rest of the core cannot interpret, and upgrade/recovery paths that either hide stored data or silently repair partial state. The browser also fails to retain core-generated failure/recovery audits and records fabricated timestamps. Finally, the dependency gate can approve incomplete or unverified provenance, and the pinned browser-test stack contains critical file-access and remote-code-execution advisories.

The existing suites are green (`cargo test -p flow-core --all-targets --locked`: 66 tests; `npm run test:unit`: 18 tests; `npm run typecheck`: pass), but independent probes reproduced the failures below. `npm audit --json` reported the Vitest advisories in CR-11.

## Critical Issues

### CR-01 [BLOCKER]: Concurrent IndexedDB commits can fork one durable revision

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/src/foundation-inspector.ts:493-505`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/persistence/indexeddb-store.ts:345-450`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/lib.rs:995-1007`

**Issue:** The inspector validates a proposed commit against records loaded before the IndexedDB write transaction. The later transaction checks only record identities (`transactionId`, `auditId`, and so on), not the expected document head. Two tabs can therefore both plan different `N -> N+1` transactions, then both commit because their IDs differ. A fake-indexeddb probe with two store instances reproduced two fulfilled commits at revision 2. Recovery subsequently rejects the two records with the same `newRevision`, so an ordinary cross-tab race corrupts the recoverable chain.

**Fix:** Put a per-document head/CAS record in IndexedDB and compare the core-issued expected head (document ID, base revision, and canonical hash) inside the same read/write transaction that writes the records. Advance that head atomically; the losing writer must receive a stale/conflict result and write nothing. Add a two-store-instance test with divergent commands from the same base and assert one winner, one loser, and a recoverable store.

### CR-02 [BLOCKER]: Stable IDs can be invalid or have multiple logical spellings

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/model/mod.rs:15-27`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/schema/mod.rs:513-519`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/canonical/mod.rs:57-69`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/audit/mod.rs:288-304,475-490`

**Issue:** `stable_id!` derives transparent `Deserialize`, bypassing its validating constructor. Its constructor also parses a UUID but stores the original text. UUID accepts simple, hyphenated, braced, URN, and case variants, while duplicate and identity checks compare raw strings. Probes showed that invalid audit/transaction/command IDs deserialize, validate, and survive full recovery, and that hyphenated and simple encodings of the same UUID are treated as independent identities. Arbitrary strings supplied as migration/recovery identifiers can therefore enter the durable audit trail as well.

**Fix:** Represent IDs internally as `Uuid`, or normalize every accepted value to lowercase hyphenated form. Implement custom checked deserialization through that representation, compare parsed identity values, and revalidate all IDs in hostile-input boundaries such as `AuditEvent::validate`. Add invalid-ID and alternate-spelling tests through canonical decode, audit recovery, migration, and duplicate-command detection.

### CR-03 [BLOCKER]: Partial stores are treated as empty and repaired by guesswork

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/persistence/indexeddb-store.ts:258-301,345-430`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/store/mod.rs:349-360`

**Issue:** `loadRecords` decides that storage is empty solely from the snapshot count, even when transaction, audit, asset, or migration-source records remain. Creation commit validation then returns success immediately for `base_revision == 0` without requiring every durable store to be empty. The physical adapter accepts missing records because it rejects only conflicting identities. A probe starting with only the creation snapshot showed that `commit_record` approved and filled in the missing records. This both hides corruption from recovery diagnostics and violates the fail-closed rule against reconstructing partial durable state.

**Fix:** Define empty storage as all five stores having zero records. For a creation/retry, distinguish a truly empty store, a byte-for-byte complete idempotent commit, and any partial/divergent set; reject the last as `PartialRecordSet`. Couple that logical decision to the same IndexedDB head/CAS transaction described in CR-01 and test every missing-record permutation.

### CR-04 [BLOCKER]: Schema-valid empty and image-only documents cannot recover

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/schema/mod.rs:275-287`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/lib.rs:1400-1425`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/tests/schema.rs:52-60`

**Issue:** The schema and its explicit test permit empty content, but `session_dto` requires the first paragraph to construct `next_command_target`. A fully consistent empty creation snapshot/transaction/audit passed durable-record validation and then recovery failed with `FLOW_INVALID_TARGET`. Image-only documents fail for the same reason, so the canonical schema admits documents that the product cannot reopen.

**Fix:** Preserve the documented empty-collection contract: make `SessionDto.next_command_target` optional, return `None` when no paragraph exists, and disable paragraph-targeted commands in the UI until a valid target exists. Add recovery and WASM round-trip tests for empty and image-only documents.

### CR-05 [BLOCKER]: Canonical validation accepts impossible provenance timestamps

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/schema/mod.rs:385-442`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/provenance/mod.rs:61-90`

**Issue:** Canonical schema validation checks only timestamp punctuation and digits, whereas `ProvenanceTimestamp` applies calendar checks. `2026-99-99T99:99:99Z` consequently passes `canonical_bytes` but later fails `RevisionProvenance::from_document`. A supposedly canonical document can thus be published/persisted and rejected by another core boundary.

**Fix:** Use one shared, calendar-valid timestamp type or validator for canonical documents, audit timestamps, and revision provenance. Test invalid month/day/hour/leap-day values at canonicalization, migration, persistence, and recovery boundaries.

### CR-06 [BLOCKER]: Asset and revision-hash validation admits unrecoverable records

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/model/mod.rs:124-131`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/schema/mod.rs:68-82,308-319,523-527`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/canonical/mod.rs:43-52,72-75`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/store/mod.rs:974-999`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/lib.rs:780-806`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/provenance/mod.rs:25-38,214-244`

**Issue:** Three incompatible representations pass validation: (1) asset and revision hashes accept uppercase hex although computed hashes are lowercase and later compared as strings; (2) two descriptors may share one content hash while declaring different lengths, although the store supports one record per hash; and (3) an asset may declare a length beyond the 64 MiB recovery budget. Probes confirmed both the uppercase-hash verification failure and the duplicate-hash/different-length impossibility. Revision provenance likewise accepts lower- and uppercase forms of one digest as two source hashes. These are schema-valid values for which no successful verify/recover path exists.

**Fix:** Parse hashes into fixed digest bytes or require their exact canonical lowercase textual form at every boundary. During document validation, maintain `hash -> expected descriptor metadata` and reject conflicts (or require unique descriptor hashes). Add checked individual and aggregate asset-byte limits that account for record encoding and the recovery-record budget. Use the same canonical digest type in `RevisionHash` and source-hash deduplication.

### CR-07 [BLOCKER]: Browser failures and recovery omit the durable audit records produced by the core

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/src/foundation-inspector.ts:18-28,96-105,332-357,454-505,815-819`

**Issue:** The error DTO contains an optional audit, but `unwrap` discards it when throwing. The browser boundary also omits `recover_document_audited` and restores through unaudited `query_document`. No standalone audit commit is persisted for a stale/failed command or recovery attempt. The UI therefore demonstrates failure atomicity for document state while silently losing the privacy-minimized audit evidence the core generated.

**Fix:** Preserve `error.audit` in a typed thrown result, expose/use audited recovery, and add a core-authorized idempotent standalone-audit persistence operation. Persist the audit without mutating document state, then refresh the view. Tests should assert the added audit row, action/outcome/revision link, and absence of command text—not only unchanged document state.

### CR-08 [BLOCKER]: Every browser audit event is stamped with a fabricated date

**File:** `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/src/foundation-inspector.ts:300-317,332-350,359-375,394-409`

**Issue:** All real commands, history actions, and migrations use hard-coded August 2026 timestamps. Durable audit chronology is therefore false, repeated operations are indistinguishable by time, and persisted provenance claims an issuance time unrelated to the action.

**Fix:** Inject a clock into `FoundationInspector` and format its current instant as the required compact UTC timestamp. Production should use the current time; deterministic tests should inject a fixed clock. Validate monotonic/auditable timestamps at the core boundary.

### CR-09 [BLOCKER]: The version-3 IndexedDB upgrade makes version-2 documents inaccessible

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/persistence/indexeddb-store.ts:151-157,258-317`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/tests/indexeddb-store.test.ts:415-440`

**Issue:** On upgrade, the adapter creates new `*-v3` stores but neither migrates records from legacy stores nor reports that legacy data exists. `loadRecords` reads only v3, so an existing user is told storage is empty while their records remain stranded. The upgrade test checks only that the old raw object store still exists; it never proves the document can be loaded or recovered.

**Fix:** Migrate legacy envelopes atomically in `onupgradeneeded`, or enter an explicit migration-required state that blocks new writes and preserves access. Add a test that seeds a complete v2 document, upgrades, then loads and recovers the same revision/hash through the public adapter.

### CR-10 [BLOCKER]: Migrated documents cannot use reload/recover, and the browser test passes on a no-op

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/src/foundation-inspector.ts:394-428,587-600`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/tests/walking-skeleton.browser.test.ts:23-30,100-125`

**Issue:** `openOlderSchema` commits migration records but never sets `hasDurableRecords`, so reload and recover remain disabled. The walking-skeleton helper calls `.click()` on the disabled reload button; the DOM correctly performs no action, and comparing the unchanged in-memory snapshot makes the test pass. The claimed migrated-document reload path is therefore untested and broken.

**Fix:** Set/derive durable availability after a successful migration commit and render control state. Make the test assert that reload is enabled, perturb or remount the session, invoke reload, and verify the result came from IndexedDB (or query storage directly) before comparing provenance.

### CR-11 [BLOCKER]: Pinned Vitest Browser Mode versions have critical file-access/RCE advisories

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/package.json:17-22`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/vitest.config.ts:19-53`

**Issue:** The project pins Vitest Browser Mode packages at 4.1.6 and actively launches the affected browser API. `npm audit --json` reports GHSA-p63j-vcc4-9vmv (Browser Mode provider commands can bypass the file-access gate) and GHSA-g8mr-85jm-7xhm (an exposed Browser Mode API can proxy CDP/overwrite config, leading to code execution), with critical CVSS scores. This makes developer and CI machines unsafe when the browser server is reachable by an attacker.

**Fix:** Upgrade `vitest`, `@vitest/browser-playwright`, and the resolved `@vitest/browser` dependency in lockstep to patched 4.1.11 or newer, regenerate the lockfile, and rerun the complete unit/browser gate plus `npm audit`. Keep the Browser Mode server loopback-only and do not expose it from CI.

### CR-12 [BLOCKER]: The dependency gate can omit newly added direct dependencies

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/check-phase1.mjs:22-34,168-197`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/verify-dependency-locks.mjs:29-67`

**Issue:** The phase gate runs the provenance verifier's fixture tests, not its live verification, and accepts any existing report unless a separate blocker file exists. Lock verification walks the entries already present in the report/provenance instead of proving one-to-one coverage against the Cargo and npm manifests. A newly added, exactly pinned direct dependency can therefore be present in a valid lockfile yet omitted from both provenance configuration and the stored report while the mandatory gate remains green.

**Fix:** Derive the exact direct-dependency set from every workspace Cargo manifest and `package.json`; require set equality (ecosystem, name, version, kind) with configuration and report entries, and reject missing or extra entries. Run live verification or regenerate validated evidence in the gate, and add omitted-Cargo and omitted-npm negative fixtures.

### CR-13 [BLOCKER]: The build executes an unverified `wasm-bindgen` binary

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/config/dependency-provenance.json:5-14`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/verify-dependency-locks.mjs:29-44`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/check-phase1.mjs:168-183`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/package.json:8`

**Issue:** Provenance approves `wasm-bindgen-cli` 0.2.108, but Cargo lock verification explicitly skips tool entries and preflight checks only whether a binary exists. The build then executes whatever `work/toolchains/cargo/bin/wasm-bindgen` resolves to, including a stale or replaced binary. The generated WASM boundary is consequently outside the claimed dependency/version guarantee.

**Fix:** Compare `wasm-bindgen --version` exactly with the approved version and verify its installation receipt or recorded binary checksum/signature before build execution. Fail closed on any mismatch and cover wrong-version/replaced-binary fixtures.

### CR-14 [BLOCKER]: Lock verification silently skips its real check on encoded/cross-platform paths

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/verify-dependency-locks.mjs:121-124`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/check-phase1.mjs:28-30`

**Issue:** The script's main-module guard compares a URL-encoded `.pathname` directly with `process.argv[1]`. Paths containing spaces differ because the URL contains `%20`; Windows paths also differ in separators and leading slash. In those environments the script runs its imported fixture tests but never calls `verifyDependencyLocks()`, exits successfully, and lets the phase gate report lock verification as passed.

**Fix:** Compare `fileURLToPath(import.meta.url)` with normalized/resolved `process.argv[1]`, or move the executable entry point into an unconditional dedicated CLI module. Add real subprocess tests from a path containing spaces and representative Windows path coverage.

## Warnings

### WR-01 [WARNING]: `RevisionProvenance` deserialization bypasses source-hash invariants

**File:** `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/provenance/mod.rs:180-191,232-246`

**Issue:** The type derives `Deserialize`, while the maximum of 16 and uniqueness checks exist only in `with_source_hashes`. A probe deserialized 17 identical valid source hashes successfully, so hostile persisted/bridge JSON can create a state public constructors reject.

**Fix:** Remove `Deserialize` if the type is output-only, or deserialize through a private wire type and call a single invariant-validating constructor before exposing `RevisionProvenance`.

### WR-02 [WARNING]: Audit transaction-type helper constructs actions the validator rejects

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/audit/mod.rs:121-138,524-543`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/tests/audit_redaction.rs:132-147`

**Issue:** `AuditCommandKind::from_transaction_type` returns `Create` and `Recovery`, but `validate_action_semantics` rejects those values whenever they are represented as `AuditAction::Command`. Conversely, the special `AuditAction::Create` follows the generic `base + 1` rule, and the test blesses creation at revision `4 -> 5`.

**Fix:** Map special transaction types directly to `AuditAction::Create`/`Recovery`, or remove them from `AuditCommandKind`. Require creation to be exactly `0 -> 1` and correct the test fixture.

### WR-03 [WARNING]: Future migration hops validate only a version number

**File:** `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/src/schema/mod.rs:600-617`

**Issue:** A non-final migration result is only probed for `schemaVersion`; full canonical validation is deferred until the current version. Once a multi-hop registry exists, malformed or noncanonical output from one step is passed into the next step, obscuring which migration violated its contract and allowing downstream code to consume invalid intermediate structure.

**Fix:** Associate a version-specific decoder/validator with every registered migration step and validate each step's complete output before invoking the next hop. Add a malformed-intermediate multi-hop test.

### WR-04 [WARNING]: Failure-atomicity tests compare against an after-the-fact clone

**File:** `/Users/serhii/Documents/Codex/2026-08-13/new-chat/crates/flow-core/tests/preconditions.rs:40-47,58,71,143,179`

**Issue:** The affected assertions pass `&state` and `&state.clone()` after the failed operation. Those values are necessarily equal, so the tests cannot detect a mutation made before returning the error and overstate their failure-atomicity coverage.

**Fix:** Capture `let before = state.clone();` before calling `apply`, then compare `state` with `before` after the expected failure.

### WR-05 [WARNING]: Rejected initialization promises permanently poison the page and database lifecycle

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/src/foundation-inspector.ts:133-145`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/persistence/indexeddb-store.ts:303-318`

**Issue:** Both WASM and IndexedDB initialization cache the first promise forever, including a rejected promise. `onblocked` rejects even though the open request may later succeed, potentially leaving an unreachable connection, and opened databases have no `versionchange` handler to close them. A transient import/open/upgrade failure therefore requires a page reload and can keep other tabs' upgrades blocked.

**Fix:** Clear each cache on rejection, close any connection that succeeds after a terminal rejection, and install `database.onversionchange = () => database.close()`. Prefer waiting/reporting a blocked upgrade separately from a terminal open failure and add retry/version-change tests.

### WR-06 [WARNING]: Web builds retain deleted TypeScript outputs

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/build-web.mjs:4-17`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/tsconfig.web.json:2-8`

**Issue:** TypeScript emits into `dist/web`, but the build script never removes that directory; it only replaces `dist/web/generated`. When a source module is deleted or renamed, its previous JavaScript/source-map output remains in the distributable tree and can be served or shipped as stale code.

**Fix:** Remove the complete `dist/web` directory before TypeScript emission, then recreate/copy assets, or emit to a fresh temporary directory and atomically replace the prior output. Add a build test that deletes/renames a fixture module and verifies no stale output remains.

### WR-07 [WARNING]: English UI still creates a Ukrainian-locale document

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/src/foundation-inspector.ts:279-285`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/tests/foundation-inspector.test.ts:59-77`

**Issue:** `createSample` always sends `requestedLocale: 'uk-UA'`, regardless of the inspector's selected English or Ukrainian locale. The English shell therefore creates a Ukrainian canonical document, and the English test does not assert document locale.

**Fix:** Map the UI locale to the intended document locale (or expose a separate explicit document-locale selection) and assert the canonical/view locale in both language tests.

### WR-08 [WARNING]: Static-file read errors can terminate the inspector server

**File:** `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/serve-inspector.mjs:25-52`

**Issue:** The `stat` call is inside `try/catch`, but `createReadStream(...).pipe(response)` has no error handler. A permission change, truncation, or other asynchronous read failure after `stat` emits an unhandled stream error outside the catch block and may terminate the server process.

**Fix:** Use `stream/promises.pipeline()` inside the async handler or install explicit source/response error handlers that safely terminate the response. Add a fixture that makes a file unreadable or fails mid-stream.

### WR-09 [WARNING]: Configured provenance retries skip transient HTTP responses

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/verify-dependency-provenance.mjs:49-99`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/config/dependency-provenance.json:2-4`

**Issue:** Every non-OK HTTP response becomes `ProvenanceError`, and that error breaks the retry loop immediately. The configured retry count therefore applies only to thrown network/timeout failures; a transient 429, 408, or 5xx produces a blocker report on its first response.

**Fix:** Retry only transient statuses (408, 429, and 5xx) with bounded backoff and `Retry-After` support, fail fast for other 4xx responses, and test status-based retry/exhaustion behavior.

### WR-10 [WARNING]: npm provenance accepts insecure registry tarball URLs

**File:** `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/verify-dependency-provenance.mjs:177-190`

**Issue:** Tarball validation checks only `hostname`. A URL using plaintext HTTP, embedded credentials, or a non-default port still passes as long as its hostname is `registry.npmjs.org`, weakening the provenance claim for fetched package bytes.

**Fix:** Require `https:`, no username/password, the default port, and the expected registry pathname. Apply the same complete origin policy when validating lockfile resolutions and add adversarial URL fixtures.

### WR-11 [WARNING]: Phase-boundary enforcement is bypassable through trivial aliases

**File:** `/Users/serhii/Documents/Codex/2026-08-13/new-chat/tests/contracts/phase1-boundary.test.mjs:253-324`

**Issue:** The TypeScript checks match narrow source spellings such as bare `fetch` and selected direct assignments. `globalThis.fetch`, an alias/destructure, `XMLHttpRequest`, or `Object.assign` can introduce the deferred behavior without detection. The Rust regex similarly checks export names rather than typed signatures and can miss valid annotated forms.

**Fix:** Use TypeScript symbol-aware AST resolution or a strict positive import/capability allowlist. Inspect Rust exports through `syn`, rustdoc JSON, or generated WASM declarations, and add adversarial alias/annotation fixtures.

### WR-12 [WARNING]: Node toolchain test is tautological

**File:** `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/tests/toolchain.test.ts:3-6`

**Issue:** The test asserts the length of a three-item literal, so it passes in any project or runtime and provides no evidence that Vitest routed the file through the intended named Node environment.

**Fix:** Assert a Node-only runtime marker and the intended Vitest project/environment identity, preferably supplied explicitly by project configuration.

### WR-13 [WARNING]: Recovery helper treats unexpected transaction aborts as success

**File:** `/Users/serhii/Documents/Codex/2026-08-13/new-chat/web/tests/recovery.browser.test.ts:167-181`

**Issue:** `transactionTerminal` resolves on both `complete` and `abort` and ignores `error`. Unexpected failures during corruption injection or record reads can therefore let the test proceed as if setup succeeded, producing false positives or misleading downstream assertions.

**Fix:** Make the normal helper reject on `abort`/`error` and create a separate `expectTransactionAbort` helper only for tests where abort is the asserted outcome.

### WR-14 [WARNING]: Required Node runtime features are not pinned or enforced

**Files:**

- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/package.json:1-24`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/build-web.mjs:4-5`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/check-phase1.mjs:9-13`
- `/Users/serhii/Documents/Codex/2026-08-13/new-chat/scripts/serve-inspector.mjs:8`

**Issue:** Scripts rely on modern runtime behavior such as `import.meta.dirname`, but `package.json` declares neither an `engines.node` constraint nor a project runtime pin/preflight. An older Node installation fails before the supposedly self-checking phase gate can report an actionable toolchain error.

**Fix:** Pin the supported Node release using `engines` plus the project's chosen version-manager/Volta file, and perform an explicit version check before importing scripts that require newer syntax/runtime APIs.

---

_Reviewed: 2026-08-21T09:49:46Z_  
_Reviewer: the agent (gsd-code-reviewer)_  
_Depth: standard_

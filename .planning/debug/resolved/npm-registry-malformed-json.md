---
status: resolved
trigger: "Phase 2 Plan 02-02 mandatory npm run check repeatedly deletes successful Phase 1 provenance evidence and emits a malformed JSON blocker for different official npm package endpoints."
created: 2026-08-31T06:41:20Z
updated: 2026-09-04T13:22:57Z
---

# Debug Session: npm registry malformed JSON

## Symptoms

### Expected behavior

`npm run check` should fetch bounded official npm metadata, parse valid responses, preserve fail-closed guarantees, atomically refresh `artifacts/provenance/phase1-dependencies.json`, remove any stale blocker, and allow the Phase 2 Task 2 verification chain to finish.

### Actual behavior

The Phase 1 live dependency-provenance step intermittently reports `npm publish metadata / malformed JSON response` for different packages, deletes the tracked success evidence, writes `artifacts/provenance/phase1-blocker.json`, and blocks Plan 02-02 despite the WASM size and boundary gates passing.

### Errors

- `playwright` at `2026-08-30T19:02:39.132Z`
- `vite` at `2026-08-30T19:03:22.541Z`
- `react-dom` at `2026-08-31T06:39:06.959Z`
- One standalone live verification succeeded between failures.

### Timeline

First observed during the Phase 2 Plan 02-02 exact Task 2 chain after Plan 02-01 upgraded and pinned the approved build graph. Reproduced across two dates and multiple package endpoints.

### Reproduction

From the repository root, run the exact Plan 02-02 Task 2 verification chain ending in `npm run check`. The `dependency-provenance-live` step may fail after bounded attempts and leave the success artifact deleted plus a blocker artifact.

## Protected Partial Work

Do not stage, discard, reset, or rewrite these active Plan 02-02 outputs while debugging:

- `tests/contracts/phase1-boundary.test.mjs`
- `scripts/verify-wasm-size.mjs`
- `artifacts/benchmarks/phase2-wasm-size.json`

The deleted success evidence and new blocker are verifier-generated failure state and must be reconciled by a successful, scientifically verified fix—not hidden with a manual checkout.

## Current Focus

hypothesis: Confirmed and resolved — response-body parse failures must consume the bounded retry budget, equivalent cross-report provenance overlap must collapse only after immutable binding equality, and generated recovery evidence must bind the current source manifest.
test: The complete npm run check after official provenance and recovery-evidence reconciliation.
expecting: Met — every Phase 1 gate, including official provenance, validated report overlay, recovery evidence, WASM, TypeScript, unit, accessibility, browser, and deterministic replay, exited 0.
next_action: none — session resolved
bug_class: heisenbug-mandelbug (the live trigger is non-deterministic network/response behavior; the underlying branch is replayed deterministically offline)
candidate_causes:
  - code: mergeProvenanceReports enforces global disjointness and has no validated overlay state.
  - data: the official live Phase 1 refresh legitimately contains the current full config, including Phase 2 identities.
  - config: the top-level config was expanded intentionally by Phase 2 and is the mandatory live verifier input.
and_gate: yes — the adjacent failure requires both the refreshed full Phase 1 data and the merge function's permanent-disjointness assumption.
reasoning_checkpoint:
  hypothesis: Recovery benchmark validation fails because its tracked terminal report's sourceManifest contains pre-Phase-2 hashes for Cargo.lock, Cargo.toml, and crates/flow-core/Cargo.toml, while validation recomputes the manifest from current committed files.
  confirming_evidence:
    - validateReport compares the report sourceManifest byte-for-byte with resolveSourceManifest and throws the observed generic malformed error on mismatch.
    - Exactly Cargo.lock, Cargo.toml, and crates/flow-core/Cargo.toml have recorded hashes different from current bytes.
    - Git history shows those three files changed in committed Phase 2 dependency commit 7396d8a, after the Phase 1 recovery evidence commits.
  falsification_test: The stale-evidence hypothesis is false if an official benchmark rerun writes a current sourceManifest but --self-test still rejects the terminal artifact, or if it produces a legitimate blocker instead of a passing report.
  fix_rationale: Running the owned verifier is the only source-grounded way to recompute measurements, bind current source hashes/toolchain, atomically replace terminal evidence, and preserve its fail-closed adversarial validation.
  blind_spots: Benchmark performance can vary under host load; a fresh blocker would be legitimate and must not be overridden.
  candidate_causes:
    - data (confirmed): benchmark terminal evidence carries three stale source hashes.
    - code (eliminated as defect): validator correctly binds current sources and fails closed on stale evidence.
    - environment (possible outcome only): host load could legitimately push fresh p95 above budget.
  and_gate: no — the current deterministic validation failure is fully explained by stale source-manifest data; performance environment affects only whether regeneration passes or blocks.
tdd_checkpoint:

## Evidence

- timestamp: 2026-08-31T06:45:37Z
  checked: Phase 0 semantic recall fallback
  found: MemPalace tooling is unavailable and .planning/debug/knowledge-base.md does not exist.
  implication: No known-pattern candidate is available; investigate the live verifier directly.

- timestamp: 2026-08-31T06:47:34Z
  checked: Complete fetchJson implementation in scripts/verify-dependency-provenance.mjs
  found: A response.json parse failure throws ProvenanceError('malformed JSON response'); the outer catch then breaks immediately for every ProvenanceError, without checking attempt versus retries.
  implication: Malformed successful responses consume only one attempt even when config.retries is 2; HTTP 408/429/5xx and network errors use the bounded retry path.

- timestamp: 2026-08-31T06:47:34Z
  checked: Existing verifier tests
  found: The malformed JSON negative test uses retries: 0 and only asserts eventual rejection; retry tests cover transient HTTP statuses but not response-body parse failures.
  implication: The current test suite cannot distinguish fail-closed exhaustion from premature termination before the retry budget.

- timestamp: 2026-08-31T06:47:34Z
  checked: config/dependency-provenance.json and writeVerificationOutcome
  found: Production configuration permits two retries, and every terminal verifier error deliberately removes phase1-dependencies.json and writes phase1-blocker.json.
  implication: Premature parse-error termination directly explains the success artifact deletion and blocker creation; fail-closed artifact behavior itself is intentional.

- timestamp: 2026-08-31T06:48:56Z
  checked: Unmodified verifier test suite
  found: node --test scripts/verify-dependency-provenance.mjs passed 7 of 7 tests.
  implication: The existing suite has no regression oracle for malformed-body retry behavior.

- timestamp: 2026-08-31T06:48:56Z
  checked: Phase 1.25 SBFL eligibility and failure classification
  found: The live failure is intermittent and environment-response dependent, and the suite has no failing test or per-test coverage spectrum before the new repro.
  implication: SBFL is skipped; use deterministic response-sequence record/replay and stability checks for the heisenbug-mandelbug trigger.

- timestamp: 2026-08-31T06:48:56Z
  checked: Common bug pattern map
  found: The symptom maps to Data Shape/API Contract (unexpected non-JSON response) plus Error Handling (retryable parse error classified as terminal), with an Environment trigger.
  implication: The next experiment must distinguish persistent bad data from a transient malformed response recoverable on the next bounded attempt.

- timestamp: 2026-08-31T06:50:44Z
  checked: Deterministic offline response-sequence regression on unmodified source
  found: The malformed-then-valid test rejected at fetchJson line 88 after the first response; the persistent-malformed exhaustion test observed 1 call instead of the configured 3. Seven prior tests remained green.
  implication: The generic ProvenanceError break condition, not retry configuration or later validation, deterministically prevents response-body failures from using the budget.

- timestamp: 2026-08-31T06:50:44Z
  checked: Live official-registry parity probe reported by read-only explorer
  found: One concurrent probe of all 11 configured npm package documents produced TypeError('terminated') while consuming the react response body; later probes succeeded. Packuments were large (vite about 38.9 MB, playwright about 16.7 MB, react-dom about 9.3 MB) and npm verification uses Promise.all.
  implication: A transient 200-response stream/body-read failure is directly observable under the production access pattern; wrapping every response.json failure as terminal malformed JSON hides its transient nature and prevents recovery.

- timestamp: 2026-08-31T06:51:54Z
  checked: Current verifier-generated artifacts/provenance/phase1-blocker.json
  found: The blocker records package react-dom, check npm publish metadata, reason malformed JSON response, timestamp 2026-08-31T06:39:06.959Z.
  implication: The persisted live failure enters the same npm package-document fetchJson branch exercised by the deterministic test.

- timestamp: 2026-08-31T06:54:03Z
  checked: Focused target suite after minimal fix
  found: node --test scripts/verify-dependency-provenance.mjs passed 9 of 9 tests; both new regression cases are green.
  implication: The one-line predicate change recovers when valid JSON arrives within budget and still exhausts to fail-closed artifacts for persistent malformed input.

- timestamp: 2026-08-31T06:55:07Z
  checked: Retry-boundary neighbors and mutation-tool availability
  found: Focused suite passed with exact attempts 1, 2, and 3 for retries 0, 1, and 2; no Stryker dependency or configuration exists in package metadata.
  implication: Boundary coverage is green; automated Stryker degrades to a logged skip, while a manual fix-site predicate mutant will be used for mutation and revert/reconfirm evidence.

- timestamp: 2026-08-31T06:55:47Z
  checked: Reverted fix with regression tests retained
  found: Focused suite returned to 7 pass / 2 fail; malformed-then-valid rejected at fetchJson line 88 and exhaustion again observed 1 call instead of 3.
  implication: The original bug returns when only the fix-site predicate is reverted, and the agent-authored tests kill that manual mutant.

- timestamp: 2026-08-31T06:56:36Z
  checked: Reapplied fix with regression tests retained
  found: Focused suite returned to 9 pass / 0 fail after only the one-line fetchJson predicate was restored.
  implication: Revert-and-reconfirm passes; the fix directly controls recovery and exhaustion behavior.

- timestamp: 2026-08-31T06:57:50Z
  checked: Final source/test diff and whitespace
  found: git diff --check passed; verifier diff contains one production predicate change plus agent-authored retry/fail-closed tests, with protected and unrelated worktree paths untouched.
  implication: The fix expands bounded retry behavior rather than deleting validation or weakening fail-closed provenance policy.

- timestamp: 2026-08-31T06:58:46Z
  checked: Exact live official provenance verification after fix
  found: Command exited 0 in 8.6 seconds; generated report is schemaVersion 1, status success, with 11 crates and 11 npm entries; blocker was removed by writeVerificationOutcome.
  implication: The corrected verifier reconciled failure state only through a successful official check and did not weaken fail-closed artifact policy.

- timestamp: 2026-08-31T06:59:36Z
  checked: First full npm run check after provenance fix
  found: Both dependency-provenance and dependency-provenance-live passed 9/9; dependency-locks reported all 6 tests passed but exited 1 because asynchronous post-test activity raised 'crates.io provenance reports contain duplicate identities'.
  implication: The original gate now passes; full acceptance is blocked by an adjacent test-lifecycle or generated-data issue that must be differentiated before accepting the fix.

- timestamp: 2026-08-31T07:03:04Z
  checked: Complete dependency-lock verifier and both current provenance reports
  found: The test module has no unawaited duplicate-identity assertion; phase1-dependencies.json is internally unique but now repeats every Phase 2 identity when merged with phase2-dependencies.json (2 crate identities and 6 npm identities).
  implication: Fresh official-report overlap is directly present in data and satisfies mergeProvenanceReports' synchronous duplicate failure condition; focused test-versus-CLI execution will confirm whether any lifecycle leak also exists.

- timestamp: 2026-08-31T07:03:48Z
  checked: Isolated dependency-lock test module
  found: node --test scripts/verify-dependency-locks.mjs exited 0 with 6 pass and no asynchronous-activity warning.
  implication: The test lifecycle is clean; the earlier combined output was the test half succeeding before the chained live CLI validated real artifacts.

- timestamp: 2026-08-31T07:04:38Z
  checked: Dependency-lock module invoked as live CLI against current reports
  found: node scripts/verify-dependency-locks.mjs exited 1 after all six embedded tests passed, with the top-level live verification error 'crates.io provenance reports contain duplicate identities'.
  implication: Node's embedded test harness presents the main-module rejection as asynchronous activity, but the failure is deterministic real-report validation, not an unawaited negative-test rejection.

- timestamp: 2026-08-31T07:06:40Z
  checked: Check orchestration, Phase 2 plan/deviation history, tracked baseline report, expanded config, and overlapping provenance bindings
  found: The lock merge was introduced to combine a disjoint tracked Phase 1 baseline with Phase 2 admission evidence, while npm run check obligatorily refreshes Phase 1 from the expanded current config; all 8 present overlaps agree on every immutable registry/repository binding enforced by both verifiers.
  implication: Acceptance genuinely requires a two-state merge contract: disjoint reports before live refresh and equivalent overlap after refresh, with same-report or contradictory cross-report duplicates still rejected fail closed.

- timestamp: 2026-08-31T07:08:19Z
  checked: Focused adjacent regression on unmodified merge logic
  found: The new equivalent-overlay test failed exactly at mergeProvenanceReports' blanket crates.io duplicate-identity guard; the prior 6 tests remained green.
  implication: The test reproduces the post-refresh acceptance blocker without network or artifact mutation and is ready to drive the narrow validated-overlay change.

- timestamp: 2026-08-31T07:09:10Z
  checked: Focused dependency-lock suite after validated-overlay change
  found: node --test scripts/verify-dependency-locks.mjs exited 0 with 7 pass; equivalent overlap collapses, while same-report duplicate, checksum contradiction, and integrity contradiction assertions remain fail closed.
  implication: The narrow merge change handles the required pre/post-live report states without accepting contradictory provenance.

- timestamp: 2026-08-31T07:10:05Z
  checked: Live dependency-lock CLI after validated-overlay change
  found: node scripts/verify-dependency-locks.mjs exited 0, reporting 107 Cargo packages and 96 npm packages verified; embedded tests were 7/7.
  implication: The real refreshed Phase 1 plus Phase 2 artifacts now satisfy identity coverage, lock integrity/checksums, and Phase 2 lock-intent checks.

- timestamp: 2026-08-31T07:11:23Z
  checked: Adjacent validated-overlay revert-and-reconfirm
  found: With the new regression retained, temporarily restoring the old blanket duplicate guard failed the focused test at the original crates.io duplicate error; reapplying only the validated overlay made the same focused test pass.
  implication: The adjacent source hunk directly controls the acceptance blocker, and its regression kills the old-guard mutant without touching provenance artifacts.

- timestamp: 2026-08-31T07:13:13Z
  checked: Exact Plan 02-02 Task 2 acceptance chain after both fixes
  found: WASM size passed (+15716 raw, +5483 gzip), protected boundary tests passed 11/11, live provenance passed 9/9, live dependency locks passed 7/7 with 107 Cargo and 96 npm packages, Node regressions/boundary/rustfmt/Clippy/all Rust tests passed; npm run check then stopped at recovery-benchmark self-test with 'recovery benchmark terminal artifact is malformed'.
  implication: The reported npm bug and duplicate-report acceptance blocker are fixed in the real chain; full-check completion now depends on an independent benchmark-artifact validator issue that must be sourced before any action.

- timestamp: 2026-08-31T07:14:33Z
  checked: Recovery benchmark validator, tracked artifact, current source hashes, and git history
  found: Only Cargo.lock, Cargo.toml, and crates/flow-core/Cargo.toml differ from their recorded sourceManifest hashes; all three changed in committed Phase 2 dependency commit 7396d8a after the Phase 1 benchmark evidence commits.
  implication: The validator is correctly rejecting stale generated evidence; acceptance requires regeneration through the official benchmark runner, not a source-code relaxation or manual JSON edit.

- timestamp: 2026-08-31T07:21:11Z
  checked: Official recovery benchmark regeneration
  found: node scripts/verify-recovery-benchmark.mjs exited 0, atomically refreshed current source hashes, and measured p95 471.511 ms against the locked 2000 ms target with passed=true and blocked=false.
  implication: Stale evidence was the only benchmark blocker; the owned workload remains safely within budget and is ready for adversarial terminal validation.

- timestamp: 2026-08-31T07:22:03Z
  checked: Regenerated recovery benchmark adversarial self-test
  found: node scripts/verify-recovery-benchmark.mjs --self-test exited 0 and rejected all 8 forged terminal-report cases.
  implication: Benchmark reconciliation preserved fail-closed validation; the full check can now resume without source relaxation.

- timestamp: 2026-09-04T13:22:57Z
  checked: Final exact npm run check after all source-grounded reconciliation
  found: Command exited 0 in 29.26 seconds; dependency provenance passed 9/9 both offline and live, dependency locks passed 7/7 with 107 Cargo and 96 npm packages, boundary contracts passed 11/11, and all Rust, recovery, WASM, TypeScript, unit, accessibility, browser, and two-round deterministic replay gates passed.
  implication: The reported intermittent registry failure recovers within its bounded budget, persistent malformed responses remain fail closed, and the complete mandatory acceptance chain is green.

- timestamp: 2026-09-04T13:22:57Z
  checked: Final diff and generated-artifact audit
  found: git diff --check passed; phase1-dependencies.json is a successful official report with 11 crates and 11 npm packages; phase1-recovery.json is passed=true and blocked=false; phase1-blocker.json is absent; protected Plan 02-02 and unrelated paths remain outside the debug-owned diff.
  implication: The terminal artifacts were reconciled only through their official verifiers, and the debug commit can be scoped without capturing unrelated work.

## Eliminated

- hypothesis: Production retry configuration is zero or missing.
  evidence: config/dependency-provenance.json sets retries to 2, and the deterministic test explicitly sets retries above zero yet observes one call.
  timestamp: 2026-08-31T06:51:54Z

- hypothesis: A persistently malformed package document or package-specific schema defect is the primary cause.
  evidence: Failures moved among playwright, vite, and react-dom; standalone and follow-up live runs succeeded; malformed-then-valid replay still aborts before reading the valid second response.
  timestamp: 2026-08-31T06:51:54Z

- hypothesis: Artifact deletion/blocker writing is an independent corruption bug.
  evidence: writeVerificationOutcome intentionally removes stale success evidence and writes a blocker only after verifyManifest rejects; the exhaustion regression observes this fail-closed behavior.
  timestamp: 2026-08-31T06:51:54Z

## Resolution

root_cause: A transient npm response-body termination/truncation during concurrent large packument reads was converted into a terminal ProvenanceError, so fetchJson stopped after one call instead of using retries + 1 attempts; the official refresh then exposed an adjacent blanket duplicate guard that rejected equivalent Phase 1/Phase 2 provenance overlays.
fix: Let response.json body/parse failures consume the existing bounded retry budget, add deterministic recovery and exhaustion coverage, and collapse only equivalent cross-report provenance entries while retaining fail-closed duplicate and contradiction checks.
verification: npm run check exited 0; offline and live provenance 9/9; dependency locks 7/7; boundary contracts 11/11; all remaining Rust, recovery, WASM, TypeScript, unit, accessibility, browser, and deterministic replay gates passed.
oracle_type: derived (retry budget means up to retries + 1 attempts; recovery and exhaustion must preserve that contract)
files_changed: [scripts/verify-dependency-provenance.mjs, scripts/verify-dependency-locks.mjs, artifacts/provenance/phase1-dependencies.json, artifacts/benchmarks/phase1-recovery.json]
cycles: { investigation: 3, fix: 2 }
tdd: false
specialist_review: none (no mapped JavaScript specialist skill was available)

## Blameless Postmortem

why_not_caught: The malformed-JSON negative test used retries=0, so it proved terminal rejection but not retry-budget consumption; the lock merge tests assumed permanently disjoint reports and did not cover the post-live-refresh equivalent-overlay state.
guard_artifact: Deterministic malformed-to-valid and exhaustion tests in scripts/verify-dependency-provenance.mjs, plus equivalent-overlay and contradiction tests in scripts/verify-dependency-locks.mjs, are enforced by npm run check.

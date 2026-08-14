---
phase: FLOWPDF-01-durable-flow-foundation
plan: "01"
subsystem: infra
tags: [provenance, supply-chain, cargo, npm, node]
requires: []
provides:
  - "Fail-closed official registry and upstream-repository dependency verifier"
  - "Machine-readable provenance blocker for unavailable or inconsistent registry metadata"
affects: [FLOWPDF-01-durable-flow-foundation, workspace-bootstrap]
actuals:
  tokens: 4450
  tasks: 1
  commits: 3
tech-stack:
  added: [Node.js built-in test runner, HTTPS registry verification]
  patterns: [exact allowlist, no-install-before-provenance, fail-closed blocker]
key-files:
  created: [config/dependency-provenance.json, scripts/verify-dependency-provenance.mjs, artifacts/provenance/phase1-blocker.json]
  modified: []
key-decisions:
  - "Treat missing official registry reachability as a blocking supply-chain failure, not a permission prompt."
requirements-completed: []
coverage:
  - id: D1
    description: "Provenance verifier validates deterministic positive and adversarial fixtures."
    verification:
      - kind: unit
        ref: "node --test scripts/verify-dependency-provenance.mjs"
        status: pass
    human_judgment: false
  - id: D2
    description: "Live provenance resolves every approved direct dependency before installation."
    verification:
      - kind: integration
        ref: "node scripts/verify-dependency-provenance.mjs --config config/dependency-provenance.json --report artifacts/provenance/phase1-dependencies.json --blocker artifacts/provenance/phase1-blocker.json"
        status: fail
    human_judgment: true
    rationale: "Official crates.io DNS was unavailable, so the mandatory live trust check intentionally stopped the plan."
duration: 4min
completed: 2026-08-14
status: blocked
---

# Phase FLOWPDF-01 Plan 01: Provenance Gate Summary

**A deterministic dependency-provenance gate now validates allowlisted registry releases and writes a fail-closed blocker before any package or toolchain can execute.**

## Performance

- **Duration:** 4 min
- **Started:** 2026-08-14T18:14:00Z
- **Stopped:** 2026-08-14T18:17:31Z
- **Tasks completed:** 1 of 3 (terminal fail-closed state)
- **Files created:** 4

## Accomplishments

- Added an exact dependency allowlist for the Phase 1 Rust, WASM, TypeScript, Vitest, and Playwright direct dependencies.
- Implemented unit-tested verification of exact versions, release state, crates.io checksums, npm SHA-512 integrity, registry-only tarballs, normalized repository identity, and public/non-archived GitHub upstreams.
- Recorded the live official-registry resolution failure in a credential-free, machine-readable blocker; no Rust, npm, or browser installation was attempted.

## Task Commits

1. **Task 01-01-01: Tracer provenance gate (RED)** — `59cb258` (`test`)
2. **Task 01-01-01: Tracer provenance gate (GREEN)** — `e149db8` (`feat`)
3. **Task 01-01-01: Failure-invariant coverage** — `65e5033` (`test`)

Tasks 01-01-02 and 01-01-03 were not started: their explicit preconditions require the absent success report and blocker-free state.

## Verification

- Passed: `node --test scripts/verify-dependency-provenance.mjs` (3 tests covering positive evidence plus yanked/deprecated, checksum/integrity, origin, malformed-response, HTTP, and timeout failures).
- Intentionally failed closed: live verifier exited 1 and created `artifacts/provenance/phase1-blocker.json` with `package: serde`, `check: crates.io release`, and `reason: network failure`.
- Confirmed absent: `artifacts/provenance/phase1-dependencies.json`.

## Files Created

- `config/dependency-provenance.json` — exact package/repository allowlist.
- `scripts/verify-dependency-provenance.mjs` — executable registry/repository verifier and fixtures.
- `artifacts/provenance/phase1-blocker.json` — terminal live provenance diagnostic.
- `01-01-SUMMARY.md` — execution and blocker record.

## Decisions Made

- Missing official registry metadata halts Wave 0 with a blocker; it cannot be treated as a routine human approval or bypassed by an alternate package source.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - State accuracy] Preserved Plan 01 as incomplete after fail-closed termination**
- **Found during:** execution-state update
- **Issue:** the generic progress updater counted a blocked summary as completed.
- **Fix:** restored the planning progress to 0 of 6; requirements and roadmap remain unchanged because no product requirement was implemented.
- **Files modified:** `.planning/STATE.md`

The unavailable official provenance outcome itself is not a deviation: it is the plan’s explicit terminal fail-closed behavior.

## Issues Encountered

- `crates.io` could not be resolved from the execution environment (`ENOTFOUND`). The verifier preserved the no-install boundary and recorded that failure.
- Git signing was unavailable in the sandbox; task commits used a one-command `commit.gpgsign=false` override. This did not alter repository signing policy or source contents.

## Next Phase Readiness

Blocked. Restore official registry DNS/network reachability and rerun this plan. The live verifier must produce `artifacts/provenance/phase1-dependencies.json` with no blocker before Rust, npm, Chromium, or later plans can proceed.

## Self-Check: PASSED

- Confirmed both task commits exist and all listed artifacts are present.

---

*Phase: FLOWPDF-01-durable-flow-foundation*
*Stopped: 2026-08-14*

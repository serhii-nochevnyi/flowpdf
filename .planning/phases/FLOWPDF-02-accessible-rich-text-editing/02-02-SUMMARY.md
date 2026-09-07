---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "02"
subsystem: unicode-and-build-contracts
tags: [unicode, icu4x, wasm, supply-chain, boundaries]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Plan 02-01 accepted exact dependency identities and locks
provides:
  - Exact offline Unicode 17 grapheme corpus and adversarial identity verifier
  - Deterministic isolated release-WASM size evidence with verified bindgen execution
  - Phase-aware architectural boundaries retaining Rust semantic authority
affects: [schema-v2, grapheme-segmentation, editor-shell, phase2-release-gate]
tech-stack:
  added: [Unicode 17.0.0 GraphemeBreakTest]
  patterns:
    - Validate one executable byte snapshot before executing its private temporary copy
    - Bind stable normalized provenance identities separately from live observation timestamps
key-files:
  created:
    - fixtures/unicode/17.0.0/GraphemeBreakTest.txt
    - fixtures/unicode/17.0.0/provenance.json
    - scripts/verify-unicode-corpus.mjs
    - scripts/verify-unicode-corpus.test.mjs
    - scripts/verify-wasm-size.mjs
    - artifacts/benchmarks/phase2-wasm-size.json
  modified:
    - tests/contracts/phase1-boundary.test.mjs
key-decisions:
  - Only ICU feature activation differs between isolated baseline and candidate builds.
  - Mutable live provenance reports are validated on each run; immutable normalized identities bind the size artifact.
  - The size verifier uses an approved byte snapshot for both bindgen version checking and both builds.
requirements-completed: [EDIT-01, QUAL-03, QUAL-04]
coverage:
  - id: D1
    description: Exact offline Unicode 17 corpus and provenance reject independent drift.
    requirement: EDIT-01
    verification:
      - kind: unit
        ref: node --test scripts/verify-unicode-corpus.test.mjs
        status: pass
      - kind: integration
        ref: node scripts/verify-unicode-corpus.mjs
        status: pass
    human_judgment: false
  - id: D2
    description: Fresh isolated WASM builds reproduce checked measurements within both budgets.
    requirement: QUAL-04
    verification:
      - kind: integration
        ref: node scripts/verify-wasm-size.mjs
        status: pass
    human_judgment: false
  - id: D3
    description: Phase 2 editor allowances preserve architecture and reject substituted build tools.
    requirement: QUAL-03
    verification:
      - kind: unit
        ref: node --test tests/contracts/phase1-boundary.test.mjs
        status: pass
      - kind: integration
        ref: npm run check
        status: pass
    human_judgment: false
duration: multi-session
completed: 2026-09-07
status: complete
---

# Phase 2 Plan 02: Unicode Identity and WASM Payload Summary

Unicode 17 conformance bytes are pinned offline, ICU grapheme data adds 15,716 raw bytes / 5,483 deterministic-gzip bytes, and editor dependency allowances retain architectural prohibitions.

## Performance

- Started: 2026-08-30 (first RED commit at 18:47 UTC).
- Completed: 2026-09-07; interrupted multi-session execution, no reliable active-time total.
- Tasks: 2. Plan files: 7. Separate debug repair: 4 code/evidence files plus its session record.

## Accomplishments

- Vendored the official 126,570-byte GraphemeBreakTest corpus with SHA-256 `e2d134d2c52919bace503ebb6a551c1855fe1a1faec18478c78fff254a1793ec`; seven tests reject corpus, metadata, ICU identity, and feature drift.
- Added one canonical size gate. Baseline: 1,378,296 raw / 321,227 gzip bytes; candidate: 1,394,012 raw / 326,710 gzip bytes. Deltas are below the 524,288 raw / 163,840 gzip budgets. Source hashes, locks, toolchain, flags, verified tool bytes and compression bind fresh measurements.
- Added narrow React/Vite/TSX allowances while retaining semantic TypeScript, unsafe DOM, mutable WASM, layout/PDF/voice/backend prohibitions. Thirteen tests include deferred filename tokens, forged size evidence, exceeded budgets, and same-version executable substitution.

## Task Commits

1. Task 1 RED: `c3073fd`; GREEN: `b9448c9`.
2. Task 2 RED: `eaf88c8`; GREEN: `4ad3291`.
3. Separate debug repair: `f644079`; resolved session: `087915b`.

## Verification

- Both exact plan commands passed on 2026-09-07: Unicode suite 7/7 plus exact CLI hash; size gate, boundary suite 13/13, and full `npm run check` exit 0.
- Full gate: 26.73 seconds; live provenance, dependency locks, Rust formatting/Clippy/tests, recovery evidence, WASM, TypeScript, unit/browser/accessibility suites, and two canonical/migration replay rounds passed.
- On 2026-09-04, the executor proved record → normal verification → live Phase 2 provenance refresh → normal → normal. The artifact remained byte/hash exact. Only volatile report observations were restored with apply_patch; accepted dependency identities were unchanged.
- Independent final review found no remaining findings and independently passed 13/13 boundary tests and the normal size gate after the security/determinism corrections.
- `git diff --check` passed. No tracked files were deleted by Task 2.
- Post-wave verification on 2026-09-07: pinned `cargo build` passed; `npm test` passed 43/43 tests across 8/8 files. Schema drift and UI safety gates did not block; codebase drift explicitly skipped because no STRUCTURE.md exists. Shared requirement readiness was 0/3, so phase-level requirements remain pending.

## Deviations from Plan

1. **[Rule 3 - Blocking] Live dependency verification repairs.** Task 2 exposed premature termination on transient npm response-body failures, rejection of equivalent cross-report identities, and recovery evidence bound to old Cargo sources. Bounded retry regressions, validated equivalent overlays, and official benchmark regeneration restored the full gate. Files and experiments are recorded in `.planning/debug/resolved/npm-registry-malformed-json.md`; commits `f644079` and `087915b`.
2. **[Rule 2 - Missing Critical] Verify executed bindgen bytes.** Review found that a version string alone could admit a substituted tool. The size verifier now reuses approved checksum/receipt validation, executes a private copy of the verified bytes, and tests that a malicious same-version fixture never executes. Included in `4ad3291`.
3. **[Rule 1 - Bug] Retain deferred filename boundaries.** Review found deferred names such as `editor/pdf-export.ts` escaped the original slash/dot rule. Token boundaries now include hyphen/underscore; explicit layout/PDF/voice/backend fixtures reject these paths. Included in `4ad3291`.
4. **[Rule 1 - Bug] Stable provenance binding.** Full live JSON hashes included changing observation timestamps and adoption counts. Both reports remain validated on every run; the artifact binds normalized ICU/tool identities and approved binary bytes. Repeat verification after live refresh is proven. Included in `4ad3291`.

Total deviations: four auto-fixed items (two bugs, one missing-critical safeguard, one blocking debug repair). All retain the approved dependencies and architectural scope.

## Issues Encountered

Execution was interrupted by agent usage limits. Final acceptance and atomic closeout were completed in the main session. No implementation blocker remains.

## User Setup Required

None.

## Next Phase Readiness

Ready for Plan 02-03's executed legacy freeze before schema-v2 mutation. The frontmatter lists this plan's contributed requirements; shared phase-level EDIT/QUAL requirements remain pending until all declaring plans finish. Windows/Edge and assistive-technology UAT are not claimed by these infrastructure checks.

## Self-Check: PASSED

All seven plan artifacts exist, RED/GREEN commits are reachable, exact plan verification passed, reviewed fixes are included, and no unrelated work was staged.

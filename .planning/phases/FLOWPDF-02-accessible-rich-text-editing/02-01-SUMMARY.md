---
phase: FLOWPDF-02-accessible-rich-text-editing
plan: "01"
subsystem: dependency-intake
tags: [supply-chain, react, vite, icu4x, image, lockfiles]
dependency-graph:
  requires:
    - phase: FLOWPDF-01-foundation
      provides: Pinned Rust/WASM and TypeScript foundation with provenance and lock verification
  provides:
    - Fail-closed Phase 2 dependency admission from official metadata
    - Exact React/Vite and ICU4X/image manifest and lock boundaries
    - Vite build seam preserving the Foundation Inspector entry
  affects:
    - FLOWPDF-02 editor shell and semantic editing plans
    - Phase 2 Unicode segmentation and image codec work
tech-stack:
  added:
    - react@19.2.8
    - react-dom@19.2.8
    - vite@8.1.5
    - "@vitejs/plugin-react@6.0.5"
    - "@types/react@19.2.17"
    - "@types/react-dom@19.2.3"
    - icu_segmenter@2.3.0
    - image@0.25.10
  patterns:
    - Official-metadata candidate admission before package-manager mutation
    - Exact lock intent coupled to integrity, checksum, lifecycle, legitimacy, and Cargo feature evidence
key-files:
  created:
    - scripts/verify-phase2-dependencies.mjs
    - scripts/verify-phase2-dependencies.test.mjs
    - artifacts/provenance/phase2-dependencies.json
    - vite.config.ts
  modified:
    - config/dependency-provenance.json
    - scripts/verify-dependency-locks.mjs
    - package.json
    - package-lock.json
    - Cargo.toml
    - Cargo.lock
    - crates/flow-core/Cargo.toml
decisions:
  - Candidate fallback is allowed only across predeclared exact versions; missing or contradictory official evidence remains terminal.
  - Once admitted, the exact lock intent is immutable across later policy-age changes and is revalidated rather than reselected.
  - npm legitimacy uses exact-release age, while crate legitimacy uses package inception age and still records exact-release publication evidence.
  - ICU segmentation is locked to compiled_data only and image decoding to PNG/JPEG only, with default Cargo features disabled.
metrics:
  duration: 20m 7s active execution
  completed: 2026-08-30
status: complete
actuals:
  tokens: 23110
  tasks: 2
  commits: 6
---

# Phase 2 Plan 01: Dependency and Build Graph Admission Summary

Fail-closed official-registry admission now binds exact React/Vite and ICU4X/image releases to deterministic npm/Cargo locks before editor production work begins.

## Performance

- **Duration:** 20m 7s active execution (17m 7s original execution + 3m post-wave repair)
- **Started:** 2026-08-29T10:52:19Z
- **Original completion:** 2026-08-29T11:09:26Z
- **Post-wave repair completed:** 2026-08-30
- **Tasks:** 2
- **Files changed:** 11
- **Actual diff scale:** 92,442 characters / 4 = 23,110 tokens

## Accomplishments

- Added a portable Phase 2 verifier that compares package documents and exact-version endpoints, canonical repository/tarball/integrity or checksum evidence, publication timestamps, numeric adoption snapshots, deprecation/yank state, and install lifecycle scripts.
- Added 12 adversarial tests covering deterministic fallback, repository/tarball/integrity contradiction, deprecation, lifecycle execution, floating versions, stale lock intent, non-OK legitimacy, fail-closed blocker output, and absence of shell/package-manager execution.
- Admitted six npm packages and two crates with `OK` legitimacy evidence and a stable lock-intent digest of `27c9a5f2f41d0148ee1ff8e277ee1990f48135f367f555d709daed7604666bae`.
- Materialized npm and Cargo locks without npm lifecycle execution and enforced the closed `icu_segmenter/compiled_data` and `image/png,jpeg` feature graph.
- Added a Vite configuration for the existing `web/index.html`/WASM Inspector entry while retaining all named Vitest projects and the Foundation Inspector route.
- Kept admitted dependency pins stable across policy-age rollover and made the generic full test command launch all browser projects from the pinned workspace Playwright cache.

## Task Commits

Each task was committed atomically:

1. **Task 1: Prove exact dependency candidates** — `fdf7378` (`feat`)
2. **Task 2: Lock accepted dependencies and build seam** — `7396d8a` (`chore`)
3. **Post-wave: Preserve admitted pins across policy-age rollover** — `f5d8d75` (`fix`)
4. **Post-wave: Use the local Playwright cache in full tests** — `acb04ed` (`fix`)

## Files Created/Modified

- `scripts/verify-phase2-dependencies.mjs` — Official-source candidate selection, legitimacy policy, atomic success/blocker evidence, and no-override failure path.
- `scripts/verify-phase2-dependencies.test.mjs` — Deterministic and adversarial dependency-admission tests.
- `config/dependency-provenance.json` — Ordered exact candidates, accepted lock intent, repositories, and lifecycle/adoption policy.
- `artifacts/provenance/phase2-dependencies.json` — Accepted npm/crate identities and evidence snapshots.
- `package.json` / `package-lock.json` — Exact React, type, Vite, and plugin dependencies with canonical lock integrity; the full test script uses the pinned local Playwright browser cache.
- `Cargo.toml` / `Cargo.lock` / `crates/flow-core/Cargo.toml` — Exact ICU4X and image dependencies with closed feature sets.
- `vite.config.ts` — Existing Inspector entry, React plugin, strict filesystem boundary, and deterministic output directory.
- `scripts/verify-dependency-locks.mjs` — Phase 1 + Phase 2 provenance merge and explicit Phase 2 manifest/feature intent checks.

## Decisions Made

- No package name or version was substituted after registry inspection. Only candidates listed in the committed configuration could be selected.
- Contradictory or missing official metadata is a hard failure. A fallback candidate is considered only when the preceding exact candidate has complete evidence but a non-OK policy verdict.
- A later policy-age change cannot replace the committed accepted pin; repeat verification revalidates the immutable lock intent while still checking earlier configured candidates for contradictory evidence.
- npm install hooks remain disabled during lock materialization and verification; the admission verifier has no child-process execution path.
- Existing Vitest project names and the Phase 1 Inspector source route remain unchanged; Vite adapts the legacy stylesheet reference during its own HTML transform.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Extended the Phase 1-only lock verifier to consume the Phase 2 report**

- **Found during:** Task 2
- **Issue:** The planned `node scripts/verify-dependency-locks.mjs` gate read only `phase1-dependencies.json`, so newly accepted direct dependencies would be reported as provenance drift.
- **Fix:** Merged successful Phase 1 and Phase 2 evidence with duplicate rejection and added explicit npm section/version plus Cargo default-feature/feature-set assertions.
- **Files modified:** `scripts/verify-dependency-locks.mjs`
- **Commit:** `7396d8a`

**2. [Rule 3 - Blocking] Adapted the legacy Inspector stylesheet for Vite's HTML pipeline**

- **Found during:** Task 2 build verification
- **Issue:** The existing Inspector HTML intentionally referenced `/styles.css` for the Phase 1 builder, which Vite left unresolved because the source is `web/src/styles.css`.
- **Fix:** Added a pre-order Vite HTML transform so Vite fingerprints the stylesheet without changing the Phase 1 route or source files.
- **Files modified:** `vite.config.ts`
- **Commit:** `7396d8a`

**3. [Rule 1 - Bug] Kept admitted pins stable after candidate policy-age rollover**

- **Found during:** Post-wave Plan 02-01 provenance rerun on 2026-08-30
- **Issue:** `@types/react@19.2.18`, rejected during initial admission for being under 30 days old, later crossed the age threshold and the selector attempted to replace the committed `19.2.17` lock intent.
- **Fix:** Candidate evidence is still checked in order, but repeat verification now revalidates the immutable admitted lock intent instead of selecting a newly eligible release. The deterministic test advances far enough for the earlier candidate to age into policy.
- **Files modified:** `scripts/verify-phase2-dependencies.mjs`, `scripts/verify-phase2-dependencies.test.mjs`
- **Commit:** `f5d8d75`

**4. [Rule 3 - Blocking] Pointed the generic full test command at the project-local Chromium cache**

- **Found during:** Post-wave integration verification
- **Issue:** `npm test` ran the Node projects but browser projects could not locate Chromium because only `test:unit` and `test:browser` exported `PLAYWRIGHT_BROWSERS_PATH`.
- **Fix:** Kept one-shot `vitest run` and added `PLAYWRIGHT_BROWSERS_PATH=./work/playwright` to the generic `test` script; no browser download, dependency, or lockfile change was made.
- **Files modified:** `package.json`
- **Commit:** `acb04ed`

## Authentication Gates

None.

## Verification

- `node --test scripts/verify-phase2-dependencies.test.mjs && node scripts/verify-phase2-dependencies.mjs` — passed; 12/12 tests, 6 npm packages and 2 crates accepted.
- `npm ci --ignore-scripts && node scripts/verify-dependency-locks.mjs && npm ls --all && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo tree --locked -e features` — passed; 6/6 lock tests, 96 npm entries and 107 Cargo packages verified.
- `npm run build:vite` — passed; existing Inspector entry built with hashed CSS and JavaScript assets.
- `npm test` — passed on the final code state; 8/8 test files and 43/43 tests, including browser projects using the existing local Chromium executable.
- Post-wave provenance rerun — passed after the age-rollover fix; 12/12 admission tests and the same 6 npm / 2 crate lock intent remained accepted.
- Tracer feedback gate was rerun after Task 1 commit and passed before Task 2 began.

## Threat Scan

No security-relevant surface beyond the plan's threat model was introduced. Registry/repository reads and local provenance report writes are covered by T-02-01-SC; report-to-lock integrity is covered by T-02-01-02; and closed Cargo features are covered by T-02-01-03. No new runtime endpoint, authentication path, schema boundary, or imported-file handling path was added.

## Known Stubs

None. A scan of all created and modified files found no TODO/FIXME/placeholder text or empty UI data source introduced by this plan.

## Next Phase Readiness

- The Phase 2 dependency/build graph is exact, lockfile-backed, and admitted for subsequent editor work.
- Phase-level editing and quality requirements remain pending until their implementation plans complete; this infrastructure plan does not claim them early.

## Self-Check: PASSED

All created artifacts exist, both original task commits and both post-wave fix commits are reachable, the full suite passes 43/43 tests across 8/8 files, and the committed provenance report retains successful evidence for six npm packages and two crates.

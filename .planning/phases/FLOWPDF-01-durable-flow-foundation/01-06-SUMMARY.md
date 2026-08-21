---
phase: FLOWPDF-01-durable-flow-foundation
plan: "06"
subsystem: browser-validation
tags: [rust, wasm, indexeddb, vitest, chromium, accessibility, localization, audit, recovery, deterministic-gate]
requires:
  - phase: FLOWPDF-01-durable-flow-foundation
    provides: "Canonical schema, migrations, typed transactions, Rust-owned recovery/audit/provenance, and atomic IndexedDB commits from Plans 01-01 through 01-05"
provides:
  - "Complete real-Chromium create, mutate, stale-conflict, undo, redo, save, reload, recovery, and older-schema migration lifecycle"
  - "Localized Ukrainian-default and English-key-identical Foundation Inspector with semantic responsive accessibility and privacy-safe audit presentation"
  - "Structural Rust/TypeScript ownership and deferred-scope contract with adversarial fixtures"
  - "One fail-closed Phase 1 gate spanning dependency locks, Rust/WASM, TypeScript, Chromium, benchmark evidence, and two-round deterministic replay"
affects: [phase-2-editor-input, browser-shell, local-durability, accessibility, audit-presentation, phase-gates]
actuals:
  tokens: 31751
  tasks: 3
  commits: 7
tech-stack:
  added: []
  patterns: [typed-wasm-lifecycle, localized-semantic-inspector, allowlist-audit-table, structural-phase-boundary, fail-closed-gate, deterministic-two-process-replay]
key-files:
  created: [web/src/i18n/uk.ts, web/src/i18n/en.ts, web/tests/accessibility.browser.test.ts, tests/contracts/phase1-boundary.test.mjs, scripts/check-phase1.mjs]
  modified: [crates/flow-wasm/src/lib.rs, web/persistence/indexeddb-store.ts, web/src/foundation-inspector.ts, web/src/main.ts, web/src/styles.css, web/tests/walking-skeleton.browser.test.ts, web/tests/recovery.browser.test.ts, web/tests/foundation-inspector.test.ts, package.json, vitest.config.ts]
key-decisions:
  - "Rust remains the sole semantic authority: browser actions call typed WASM create/open/apply/undo/redo/commit/query DTO functions, while TypeScript performs physical IndexedDB I/O and presentation only."
  - "Inspector strings live in key-identical Ukrainian/English resources; audit rendering is a closed DTO allowlist and never exposes document text, arguments, transcripts, audio, or storage payloads."
  - "The Phase 1 gate validates the existing single passing benchmark artifact without regenerating evidence, runs all browser suites unfiltered, and replays canonical and migration goldens in two separate rounds."
  - "Emitted browser modules use explicit .js relative specifiers so the dependency-free static inspector server exercises the same built artifact users open."
patterns-established:
  - "Browser lifecycle pattern: publish only a Rust-verified exact revision/hash recovered after the IndexedDB transaction completes; retain the prior verified view on every failure."
  - "Presentation pattern: native controls, localized parameterized copy, stable audit row identities/source order, accessible full identifiers, explicit unavailable provenance, and internal horizontal table overflow at 320px."
  - "Gate pattern: preflight immutable evidence, spawn each non-watch subprocess with shell disabled, preserve its exit code, stop at first failure, and print observed rather than fabricated runtimes."
requirements-completed: [FLOW-01, FLOW-02, FLOW-03, FLOW-04, FLOW-05, EDIT-06, EDIT-07, QUAL-08]
coverage:
  - id: D1
    description: "A real Chromium session creates, mutates, saves, reloads, and recovers the exact durable FlowDocument revision and hash through Rust/WASM and IndexedDB."
    requirement: FLOW-02
    verification:
      - kind: e2e
        ref: "web/tests/walking-skeleton.browser.test.ts#walking-skeleton lifecycle"
        status: pass
      - kind: e2e
        ref: "web/tests/recovery.browser.test.ts#last-durable recovery"
        status: pass
    human_judgment: false
  - id: D2
    description: "Opening the supported older schema migrates through Rust, commits a current replay boundary, and preserves exact source/current lineage."
    requirement: FLOW-03
    verification:
      - kind: e2e
        ref: "web/tests/walking-skeleton.browser.test.ts#older-schema migration"
        status: pass
      - kind: integration
        ref: "npm run check#two-round migration golden replay"
        status: pass
    human_judgment: false
  - id: D3
    description: "Aborted physical writes, corrupt hashes, revision gaps, and conflicting duplicates fail closed without replacing the verified browser view or durable truth."
    requirement: FLOW-04
    verification:
      - kind: e2e
        ref: "web/tests/recovery.browser.test.ts"
        status: pass
    human_judgment: false
  - id: D4
    description: "Stale commands remain atomic, and button/keyboard undo and redo share one typed command path while each successful history action creates a new revision and audit event."
    requirement: EDIT-07
    verification:
      - kind: automated_ui
        ref: "web/tests/foundation-inspector.test.ts#history and stale atomicity"
        status: pass
      - kind: automated_ui
        ref: "web/tests/accessibility.browser.test.ts#keyboard parity and focus return"
        status: pass
    human_judgment: false
  - id: D5
    description: "The Ukrainian/English inspector exposes semantic landmarks, native names/disabled states, polite status and alert semantics, full IDs/hashes, visible focus, and unclipped layouts at 320px and 1280px."
    requirement: QUAL-08
    verification:
      - kind: automated_ui
        ref: "web/tests/accessibility.browser.test.ts"
        status: pass
      - kind: other
        ref: "supplemental built-page 320px/1280px visual review"
        status: pass
    human_judgment: false
  - id: D6
    description: "Audit DOM/accessibility output contains only stable allowlisted identities, revisions, timestamps, categories, modality, outcome/code, and safe metadata in durable source order."
    requirement: FLOW-05
    verification:
      - kind: automated_ui
        ref: "web/tests/foundation-inspector.test.ts#audit allowlist and stable identity"
        status: pass
      - kind: automated_ui
        ref: "web/tests/accessibility.browser.test.ts#audit redaction"
        status: pass
    human_judgment: false
  - id: D7
    description: "A single fail-closed command verifies locks, boundary fixtures, formatting, Clippy warnings-as-errors, all Rust tests, benchmark evidence, WASM, TypeScript, unit/browser suites, and two-round deterministic replay."
    verification:
      - kind: other
        ref: "npm run check"
        status: pass
    human_judgment: false
  - id: D8
    description: "The inspector does not fabricate PDF preview/export provenance or present Phase 1 creation/migration lineage as proof of a preview or export."
    requirement: FLOW-05
    verification:
      - kind: automated_ui
        ref: "web/tests/foundation-inspector.test.ts#explicit unavailable preview/export provenance"
        status: pass
    human_judgment: true
    rationale: "The plan marks this descriptor-less prohibition as judgment: automation proves the explicit unavailable state, while a reviewer must still judge that the surrounding lineage presentation cannot be misconstrued as preview/export proof."
duration: 46min
completed: 2026-08-21
status: complete
---

# Phase FLOWPDF-01 Plan 06: Browser Lifecycle and Phase Closure Summary

**FlowPDF now proves its complete durable lifecycle in real Chromium through Rust/WASM and IndexedDB, presents a localized accessible privacy-minimized inspector, and closes Phase 1 behind one deterministic fail-closed gate.**

## Performance

- **Started:** 2026-08-21T08:38:01Z
- **Completed:** 2026-08-21T09:23:13Z
- **Duration:** 46 minutes
- **Tasks completed:** 3 of 3
- **Files modified:** 15 production/test/gate files
- **Task/fix commits:** 7
- **Final focused accessibility runtime:** 0.92 seconds
- **Final full Phase 1 gate runtime:** 12.81 seconds

## Accomplishments

- Extended the narrow WASM contract and browser inspector through create/open/migrate/apply/stale/undo/redo/commit/query operations while keeping semantic mutation, migration, recovery validation, and audit redaction in Rust.
- Proved successful reload/last-durable recovery and fail-closed aborted-write/hash/gap/conflict behavior through real IndexedDB in Chromium.
- Shipped the approved Ukrainian-default, English-key-identical inspector with responsive semantic structure, native controls, history shortcuts, focus/status/error semantics, copyable full identifiers, explicit unavailable export provenance, and privacy-safe audit rows.
- Added adversarial structural fixtures that reject deferred editor/canvas/voice/backend/React/ORM surfaces, TypeScript semantic owners, deferred Rust dependencies, and mutable/unexpected WASM exports.
- Added `npm run check` as a fail-fast, non-watch gate over verified dependencies, Rust formatting/Clippy/tests, one immutable benchmark result, WASM, TypeScript, all browser suites, and two independent canonical/migration replay rounds.
- Verified the emitted static web build visually at 320px and 1280px after fixing browser-resolvable ESM specifiers and empty lifecycle groups.

## Task Commits

1. **Task 1 RED — Browser lifecycle/recovery contract** — `fc3bf12`
2. **Task 1 GREEN — Durable browser lifecycle** — `825d84f`
3. **Task 2 RED — Inspector presentation contract** — `d4f29d4`
4. **Task 2 GREEN — Localized responsive inspector** — `4331a05`
5. **Task 3 RED — Accessibility and phase-boundary gates** — `12aef95`
6. **Task 3 GREEN — Deterministic Phase 1 orchestrator** — `c327776`
7. **Rule 1 fix — Built inspector ESM and lifecycle groups** — `720d639`

## Files Created/Modified

- `crates/flow-wasm/src/lib.rs` — typed undo/redo/open/commit/query exports backed by flow-core.
- `web/persistence/indexeddb-store.ts` — isolated database-name support for deterministic real-browser cases.
- `web/src/foundation-inspector.ts` — complete lifecycle controller and semantic localized inspector presentation.
- `web/src/i18n/{uk,en}.ts` — key-identical parameterized Ukrainian/English resources.
- `web/src/styles.css` — approved typography, colors, 44px controls, focus, responsive grid, truncation/copy, and audit overflow.
- `web/tests/{walking-skeleton,recovery,accessibility}.browser.test.ts` — successful and negative real-Chromium lifecycle, recovery, privacy, accessibility, and responsive proof.
- `web/tests/foundation-inspector.test.ts` — localized DOM states, history, pending concurrency, conflict atomicity, and stable audit identity.
- `tests/contracts/phase1-boundary.test.mjs` — AST/import/manifest/WASM export boundary enforcement with adversarial fixtures.
- `scripts/check-phase1.mjs` — deterministic fail-fast Phase 1 orchestrator.
- `package.json` and `vitest.config.ts` — named serial browser/unit projects and non-silent test scripts.

## Decisions Made

- Kept browser responsibilities narrow: typed command construction, physical IndexedDB calls, platform-error translation, and semantic presentation; all durable interpretation stays in Rust.
- Used a real table with an internal horizontal overflow region at narrow widths so source order and header semantics remain identical rather than maintaining a second rendering model.
- Made the benchmark an immutable pre-existing terminal input to the phase gate; a missing, blocked, ambiguous, stale, or forged report fails instead of being regenerated into a pass.
- Used Vitest Browser Mode's pinned `page.viewport(width, height)` API for actual iframe viewport evidence and explicit `.js` ESM specifiers for the dependency-free built server.

## Automated Evidence

- A clean `npm ci --ignore-scripts` completed from `package-lock.json`, followed by the exact Task 3 accessibility, boundary, and full-gate command chain.
- The final gate passed 66 native Rust tests across model/schema/migration/transaction/recovery/audit/provenance suites with formatting clean and Clippy `-D warnings` on all targets.
- Dependency provenance tests (5), lock/adversarial tests (3), structural boundary tests (4), unit/inspector tests (18), and all real-Chromium browser tests (12) passed.
- Accessibility ran in Ukrainian and English at both 320px and 1280px, proving landmarks, names, 44px targets, 3px focus, disabled history, keyboard parity, status/alert semantics, full identifiers, privacy, source order, and absence of viewport overflow.
- The passing recovery benchmark was the sole terminal artifact; its validator rejected eight adversarial mutations and no blocker artifact existed.
- Canonical current-fixture and older-schema migration golden tests passed in two separate deterministic replay rounds.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Browser build bug] Restored the dependency-free static inspector**
- **Found during:** End-of-phase supplemental 320px/1280px visual review.
- **Issue:** TypeScript emitted extensionless relative ESM imports; the static server returned 404 for `src/foundation-inspector`, leaving the built page blank even though Vite-backed module tests passed.
- **Fix:** Changed production relative imports to explicit `.js` specifiers that TypeScript resolves to source `.ts` and browsers resolve in emitted output. Hid command group containers when every lifecycle control in that group is unavailable.
- **Files modified:** `web/src/main.ts`, `web/src/foundation-inspector.ts`, `web/src/i18n/en.ts`.
- **Verification:** `npm run build:web`, focused unit/accessibility tests, live built-page screenshots at 320px/1280px, and the final `npm run check` all passed.
- **Committed in:** `720d639`.

**Total deviations:** 1 auto-fixed correctness issue. The fix was required for the planned local built-page outcome and added no product scope.

## Issues Encountered

- `npm ci --ignore-scripts` reported three critical advisories in the pinned Vitest Browser Mode development dependency family (`GHSA-p63j-vcc4-9vmv`, `GHSA-g8mr-85jm-7xhm`). The phase intentionally did not perform an unverified dependency upgrade because Plan 01-01 locks exact versions, integrity, repositories, and provenance. Remediation is recorded in `deferred-items.md`; browser-mode tooling remains local-only and must not be exposed to an untrusted network before the verified refresh.

## Known Stubs

None. Stub-pattern scanning found only typed null/default/error branches and empty accumulator initialization; no empty/mock value flows to the shipped inspector.

## User Setup Required

None.

## Next Phase Readiness

- Phase 1's canonical document, typed transaction/history, durability/recovery, audit/provenance, WASM/browser lifecycle, and accessibility contracts are ready for Phase 2 input/editor work.
- Phase 2 must continue to consume the Rust command/anchor boundary and must not make DOM/editor state canonical.
- Refresh the pinned Vitest Browser Mode family through the dependency-provenance workflow before exposing its development server outside the local trusted environment.

## Self-Check: PASSED

- All five key output artifacts and all seven Task 01-06 implementation/fix commits exist.
- Coverage classification parsed 8 deliverables with no schema errors: D1–D7 are deterministically auto-covered and D8 remains explicitly routed to human judgment as required by the plan.
- Stub, skipped-test, unrun-verification, and new unmodeled threat-surface scans found none.
- The final post-fix `npm run check` passed in 12.81 seconds.

---
*Phase: FLOWPDF-01-durable-flow-foundation*
*Completed: 2026-08-21*

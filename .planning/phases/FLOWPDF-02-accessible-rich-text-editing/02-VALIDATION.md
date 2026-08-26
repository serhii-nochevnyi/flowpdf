---
phase: 2
slug: accessible-rich-text-editing
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-08-26
---

# Phase 2 — Validation Strategy

> Per-phase validation contract for schema-v2 rich-text editing, browser input, semantic accessibility, and the unchanged Phase 1 durability boundary.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test harness + `proptest`; Vitest unit/browser projects with Playwright Chromium; standalone Playwright CDP lane for target-browser IME |
| **Config file** | `Cargo.toml`, `vitest.config.ts`, and Wave 0 `scripts/check-phase2.mjs` |
| **Quick run command** | `npm run check:phase2:quick` (Wave 0 adds it as a fail-fast focused gate) |
| **Full suite command** | `npm run check:phase2` (Wave 0 adds it as a strict superset of `npm run check`) |
| **Baseline runtime** | 23.75–23.91 seconds for the complete unchanged Phase 1 gate on 2026-08-25/26; Phase 2 focused target <30 seconds and full target <120 seconds |

---

## Sampling Rate

- **After every task commit:** Run the narrowest mapped Rust/Vitest/browser test; core changes also run `cargo test -p flow-core --lib --locked`, and web changes run `npm run typecheck`.
- **After every plan wave:** Run `npm run check:phase2:quick` plus any wave-specific property, migration, image, or real-browser lane.
- **Before `$gsd-verify-work`:** `npm run check:phase2` must be green, deterministic replay must pass twice, and the external AT checkpoint must be reported honestly.
- **Max feedback latency:** 30 seconds for a focused task check; 120 seconds for the full phase suite.

---

## Requirement Verification Map

Task IDs and plan/wave ownership are finalized after planning; these rows are the non-negotiable evidence contracts that every plan must consume.

| Provisional ID | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|----------------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-W0-01 | all (dependency/tooling gate) | T-02-09 | Only exact official, integrity-verified, lockfile-matched dependencies are accepted; ICU release-WASM delta must be ≤512 KiB raw and ≤160 KiB deterministic gzip | Node provenance + build measurement | `node scripts/verify-phase2-dependencies.mjs && node scripts/verify-wasm-size.mjs` | ❌ W0 | ⬜ pending |
| 02-W0-02 | EDIT-01 | T-02-03 | Rust accepts only pinned UAX #29 extended-grapheme UTF-16 boundaries, identically in native/WASM, without snapping | Unicode conformance + Rust integration/property | `cargo test --locked -p flow-core --test grapheme_conformance` | ❌ W0 | ⬜ pending |
| 02-W0-03 | EDIT-02 | T-02-02, T-02-03, T-02-04 | Keyboard, 256-KiB-bounded paste, and IME use one Rust command path; explicit cancel/invalidation produces no command and an empty composition payload is not inferred as cancel | Rust transaction + browser input + Chromium CDP IME | `cargo test --locked -p flow-core --test rich_text_transactions replace_selection && npx --no-install vitest run --project browser web/tests/editor-input.browser.test.ts && node scripts/verify-ime-chromium.mjs` | ❌ W0 | ⬜ pending |
| 02-W0-04 | EDIT-03 | T-02-03, T-02-05 | Split/merge/cross-block edits follow the compatibility matrix and exact forward/inverse anchor algebra | Rust integration + property + replay | `cargo test --locked -p flow-core --test rich_text_transactions split_merge && cargo test --locked -p flow-core --test rich_text_properties` | ❌ W0 | ⬜ pending |
| 02-W0-05 | EDIT-04 | T-02-05, T-02-08 | Closed formatting bounds reject invalid CSS-like values; collapsed inline formatting changes only Rust-owned session state until insertion | Rust bounds/session + browser formatting/parity | `cargo test --locked -p flow-core --test rich_text_transactions formatting && npx --no-install vitest run --project browser web/tests/editor-formatting.browser.test.ts` | ❌ W0 | ⬜ pending |
| 02-W0-06 | EDIT-05 | T-02-01, T-02-06, T-02-07 | Semantic structures remain valid; image bytes are decoded under limits and redeemed once from a revision-bound receipt into one atomic asset+document commit | Rust resource/asset/transaction + WASM/browser | `cargo test --locked -p flow-core --test rich_text_transactions embedded_blocks --test resource_limits --test asset_staging` | ❌ W0 | ⬜ pending |
| 02-W0-07 | QUAL-03 | T-02-08 | Every mutation family has a labeled visible route and keyboard route that produce the same semantic command as the future voice-intent seam | Generated catalog contract + browser parity | `npx --no-install vitest run --project browser web/tests/editor-parity.browser.test.ts && node --test tests/contracts/phase2-boundary.test.mjs` | ❌ W0 | ⬜ pending |
| 02-W0-08 | QUAL-04 | T-02-01, T-02-08 | Native semantic content, existing valid/legacy-invalid field descriptors, statuses, errors, and prompts are keyboard/screen-reader reachable without field authoring or a duplicate document copy | Browser accessibility/keyboard + local AT fallback | `npx --no-install vitest run --project browser web/tests/editor-accessibility.browser.test.ts` | ❌ W0 | ⬜ pending |
| 02-W0-09 | all (migration/recovery/regression) | T-02-03, T-02-07 | v0→v1→v2 and v1→v2 are lossless; `MissingLegacy` alt and invalid anchors are never invented/snapped; interrupted persistence never publishes partial truth | Goldens + recovery + full gate | `npm run check:phase2` | ❌ W0 | ⬜ pending |

### Threat Reference Index

| Ref | Threat | Required evidence |
|-----|--------|-------------------|
| T-02-01 | Text, alt text, status, or field metadata becomes executable DOM/HTML/CSS/URL content | React text/native attributes only; no `innerHTML`, HTML paste, SVG, arbitrary CSS, or dynamic code |
| T-02-02 | Browser DOM mutation becomes canonical state | Restore the accepted projection; publish only a Rust-accepted, durably committed snapshot |
| T-02-03 | Stale, malformed, or non-grapheme selection/composition retargets text | Revision preconditions, stable IDs, pinned grapheme validation, explicit invalidation, unchanged-state assertions |
| T-02-04 | Duplicate/replayed input commits twice | Command identity, one committing state, one revision/undo entry, stable duplicate rejection |
| T-02-05 | Structural or formatting operation lacks an exact inverse or changes adjacent content | Compatibility matrix, typed operations, bounded transaction-private anchor preimages/tombstones, explicit forward/inverse mapping, byte-identical undo/redo properties |
| T-02-06 | Malformed/compressed image or structural fragmentation exhausts resources | Signature/decode/allocation/pixel/run/cell/nesting/command limits with zero/min/max/max+1 and malformed fixtures |
| T-02-07 | Forged/stale asset receipt or partial IndexedDB write publishes dangling truth | Opaque one-use revision-bound receipts and one guarded snapshot+transaction+audit+asset+head commit |
| T-02-08 | Visible, keyboard, voice-intent, and accessibility capabilities diverge | Rust-generated closed command/capability catalog and browser enumeration/parity assertions |
| T-02-09 | Dependency substitution, unsafe lifecycle script, or unacceptable WASM growth | Official-source/tarball/integrity/deprecation/lifecycle evidence, lockfile equality, measured size artifact, no approval bypass |

---

## Wave 0 Requirements

- [ ] Pin and verify the exact Rust/npm dependency graph, vendor Unicode 17 `GraphemeBreakTest.txt` with checksum/provenance, and add the numerical WASM-size gate.
- [ ] Freeze private legacy v0/v1 decoders and fixtures before the one-way schema-v2 checkpoint; cover nonempty/empty image alt and valid/interior/missing-node field anchors.
- [ ] Add `grapheme_conformance.rs`, `schema_v2_migration.rs`, `rich_text_transactions.rs`, `rich_text_properties.rs`, `resource_limits.rs`, and `asset_staging.rs`; deletion properties must prove exact selection/field-anchor restoration from bounded transaction preimages through undo/redo and recovery replay.
- [ ] Add the React/Vite entry without removing the Foundation Inspector or existing Vitest projects.
- [ ] Add controller/input/formatting/parity/accessibility/responsive browser tests and the real-Chromium IME probe.
- [ ] Add `tests/contracts/phase2-boundary.test.mjs`, `scripts/check-phase2.mjs`, `check:phase2:quick`, and `check:phase2`.
- [ ] Build cold-open IndexedDB fixtures that prove v1 migration, legacy asset-envelope compatibility, interruption recovery, and atomic staged-asset redemption.
- [ ] Add 100-page and 200-page semantic fixtures that reject quadratic initial projection/interaction behavior without implementing Phase 3 pagination.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| macOS VoiceOver fallback smoke pass for semantic content, existing-field navigation, focus return, status, error, and confirmation | QUAL-04 | Accessibility-tree automation cannot prove real AT interaction quality; this is locally available fallback evidence only | Enable VoiceOver on macOS; navigate the editor and valid/legacy-invalid field projections by keyboard in Ukrainian and English; exercise success, error, and confirmation flows; record browser/OS/AT versions and observed focus/announcement behavior |
| Microsoft Edge on Windows plus a Windows screen reader | QUAL-04 | Required environment is not installed or equivalent on this macOS workspace | Run the same scripted matrix on the supported Windows/Edge target with a named screen reader; until recorded, mark the external support-matrix/QUAL-04 evidence checkpoint outstanding—never passed by inference from Chromium or VoiceOver |

---

## Validation Sign-Off

- [ ] Every final task has a synchronized automated command or explicit Wave 0 dependency.
- [ ] Sampling continuity: no three consecutive tasks lack automated verification.
- [ ] Wave 0 creates every referenced test, fixture, script, and config before dependent implementation.
- [ ] No watch-mode flags appear in verification commands.
- [ ] Focused feedback latency remains <30 seconds and the full suite remains <120 seconds, or measured regressions are investigated and documented.
- [ ] All seven requirement rows have implementation, automated tests, corpus/limit evidence, and observable verification in the same committed revision.
- [ ] The Edge/Windows/screen-reader checkpoint is either evidenced or explicitly outstanding; fallback evidence is not mislabeled.
- [ ] `nyquist_compliant: true` and `wave_0_complete: true` are set only after every mapped row is wired and green.

**Approval:** pending planning and implementation.

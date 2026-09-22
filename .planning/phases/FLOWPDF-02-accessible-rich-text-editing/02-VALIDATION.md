---
phase: 2
slug: accessible-rich-text-editing
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-08-26
revised: 2026-08-28
---

# Phase 2 — Validation Strategy

> Exact per-task evidence contract for schema-v2 rich-text editing, browser input, semantic accessibility, and the unchanged Phase 1 durability boundary.

## Test Infrastructure

| Property | Value |
|---|---|
| Framework | Rust test harness + proptest; Vitest unit/browser with Playwright Chromium; standalone Playwright CDP IME lane; Node contract scripts |
| Config | `Cargo.toml`, `vitest.config.ts`, `scripts/check-phase2.mjs` (created by 02-18-02 after every focused owner exists) |
| Quick | `npm run check:phase2:smoke` (created by 02-18-01) |
| Full | `npm run check:phase2` (created by 02-18-02; strict superset of all rows and Phase 1) |
| Runtime | focused target <30 seconds; full target <120 seconds; terminal task runs smoke before two full deterministic passes |

## Sampling Rate

- After each task: execute exactly its mapped `<automated>` command below; a task creates every new referenced test/script before running it.
- After each plan: run the two task commands in order; no row invokes a test first owned by a later wave.
- Terminal: run `npm run check:phase2:smoke && npm run check:phase2 && npm run check:phase2`.
- External Edge/Windows/screen-reader evidence remains `unavailable/outstanding`; local evidence cannot promote it to pass.

## Requirement Verification Map

Each `Automated Command` is character-for-character the decoded shell command in the owning task's `<automated>` element. `Availability` names whether the referenced executable file is pre-existing/prior-wave or created by the same task before execution.

| Final Task ID | Plan | Wave | Requirement | Threat | Secure behavior | Automated Command | Availability | Status |
|---|---:|---:|---|---|---|---|---|---|
| 02-01-01 | 02-01 | 1 | all | T-02-01-SC | Checked-in official dependency evidence fails closed before install | `node --test scripts/verify-phase2-dependencies.test.mjs && node scripts/verify-phase2-dependencies.mjs --check` | verifier/test created same task | pending |
| 02-01-02 | 02-01 | 1 | all | T-02-01-02 | Only accepted exact pins enter lifecycle-disabled locks | `npm ci --ignore-scripts && node scripts/verify-dependency-locks.mjs && npm ls --all && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo tree --locked -e features` | prior verifier; locks/config same task | pending |
| 02-02-01 | 02-02 | 2 | EDIT-01 | T-02-02-01 | Unicode corpus/provenance drift rejects offline | `node --test scripts/verify-unicode-corpus.test.mjs && node scripts/verify-unicode-corpus.mjs` | corpus/verifier/test created same task | pending |
| 02-02-02 | 02-02 | 2 | EDIT-01, QUAL-03, QUAL-04 | T-02-02-02..04 | One canonical numerical WASM gate and retained Phase 1 boundary | `node scripts/verify-wasm-size.mjs && node --test tests/contracts/phase1-boundary.test.mjs && npm run check` | size script/artifact/boundary created or revised same task | pending |
| 02-03-01 | 02-03 | 3 | EDIT-01, EDIT-05, QUAL-04 | T-02-03-01 | One-way frozen bytes/hash/private decoder test executes before schema mutation | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test schema_v2_migration -- legacy_freeze_gate --nocapture` | legacy decoder/test/fixtures created same task | pending |
| 02-03-02 | 02-03 | 3 | EDIT-01, EDIT-05, QUAL-04 | T-02-03-02..04 | All sequential/no-op routes execute before persistence/tracer | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test schema_v2_migration -- migration_routes --nocapture && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test schema --test persistence_round_trip` | migration test prior task; schema/model same task | pending |
| 02-04-01 | 02-04 | 4 | EDIT-01 | T-02-04-01 | Pinned native grapheme/atomic boundaries reject without snapping | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test grapheme_conformance -- --nocapture` | corpus prior wave; test created same task | pending |
| 02-04-02 | 02-04 | 4 | EDIT-01, EDIT-04, QUAL-03, QUAL-04 | T-02-04-02..04 | Rust session/capability truth and immutable WASM boundary | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test editor_session -- --nocapture && npm run build:wasm && npm run typecheck` | session test/module same task; toolchain prior | pending |
| 02-05-01 | 02-05 | 5 | EDIT-01, EDIT-02 | T-02-05-01..02 | Minimal paragraph command crosses real core/WASM/persistence/recovery | `cargo test -p flow-core --test rich_text_transactions -- paragraph_tracer --nocapture && npm run build:wasm` | command/test created same task | pending |
| 02-05-02 | 02-05 | 5 | EDIT-01, EDIT-02, QUAL-04 | T-02-05-02..03 | React semantic paragraph publishes only durable accepted state | `npm run build:web && npx --no-install vitest run --project browser web/tests/walking-skeleton.browser.test.ts --testNamePattern "Phase 2 paragraph tracer"` | shell/test created same task; core prior task | pending |
| 02-06-01 | 02-06 | 6 | EDIT-01, EDIT-02, QUAL-04 | T-02-06-01..02 | Accepted store/selection bridge never adopts DOM truth | `npm run typecheck && npx --no-install vitest run web/tests/editor-controller.test.ts` | modules/test created same task | pending |
| 02-06-02 | 02-06 | 6 | EDIT-02, QUAL-03, QUAL-04 | T-02-06-01..04 | Keyboard/paste/explicit-cancel IME share one bounded Rust command | `npx --no-install vitest run --project browser web/tests/editor-input.browser.test.ts && node scripts/verify-ime-chromium.mjs` | browser test/CDP script created same task | pending |
| 02-07-01 | 02-07 | 7 | EDIT-01, EDIT-03 | T-02-07-01..03 | Complete split/merge/cross-block mappings and inverses | `cargo test -p flow-core --test rich_text_transactions --test rich_text_properties -- structural_algebra --nocapture` | tests extended same task | pending |
| 02-07-02 | 02-07 | 7 | EDIT-03, QUAL-04 | T-02-07-02..04 | Bounded private deletion preimages and fail-closed recovery | `cargo test -p flow-core --test rich_text_properties --test recovery -- deletion_preimage --nocapture` | tests/store changes same task | pending |
| 02-08-01 | 02-08 | 8 | EDIT-02, EDIT-03, QUAL-03, QUAL-04 | T-02-08-01..02 | Native structural keys/controls route only through Rust | `npx --no-install vitest run --project browser web/tests/rich-text-structure.browser.test.ts --testNamePattern "structural keys and controls"` | browser test created same task | pending |
| 02-08-02 | 02-08 | 8 | EDIT-03, QUAL-03, QUAL-04 | T-02-08-01..03 | Visible/keyboard structural durability and recovery are equal | `npx --no-install vitest run --project browser web/tests/rich-text-structure.browser.test.ts --testNamePattern "durable structural parity"` | test exists from prior task and expanded same task | pending |
| 02-09-01 | 02-09 | 9 | EDIT-04 | T-02-09-01..03 | Closed exact formatting and Rust-only pending state | `cargo test -p flow-core --test rich_text_formatting --test rich_text_properties -- formatting --nocapture` | tests created same task | pending |
| 02-09-02 | 02-09 | 9 | EDIT-03, EDIT-04 | T-02-09-01..03 | List conversion/Enter/exit/nesting has exact inverse/focus | `cargo test -p flow-core --test rich_text_formatting --test rich_text_properties -- list --nocapture` | tests prior task and expanded same task | pending |
| 02-10-01 | 02-10 | 10 | EDIT-04, QUAL-03, QUAL-04 | T-02-10-01..02 | Native localized toolbar reflects only Rust state | `npx --no-install vitest run --project browser web/tests/formatting-toolbar.browser.test.ts` | components/test created same task | pending |
| 02-10-02 | 02-10 | 10 | EDIT-04, QUAL-03, QUAL-04 | T-02-10-01..03 | Formatting/list parity, focus, and responsive behavior | `npx --no-install vitest run --project browser web/tests/rich-text-formatting.browser.test.ts` | style/test created same task | pending |
| 02-11-01 | 02-11 | 11 | EDIT-05 | T-02-11-01..03 | Explicit page-break placement/removal and editable paragraph invariant | `cargo test -p flow-core --test structural_blocks --test rich_text_properties -- page_break --nocapture` | tests/model/commands same task | pending |
| 02-11-02 | 02-11 | 11 | EDIT-05, QUAL-04 | T-02-11-01..04 | Bounded table/header/row/column/remove exact algebra | `cargo test -p flow-core --test structural_blocks --test rich_text_properties -- table --nocapture` | tests prior task and expanded same task | pending |
| 02-12-01 | 02-12 | 12 | EDIT-05, QUAL-04 | T-02-12-01..03 | Native thead/th scope/table/page-break semantics/navigation | `npx --no-install vitest run --project browser web/tests/table-accessibility.browser.test.ts` | projection/controls/test same task | pending |
| 02-12-02 | 02-12 | 12 | EDIT-05, QUAL-03, QUAL-04 | T-02-12-01..04 | Structural controls/parity/confirmation/recovery | `npx --no-install vitest run --project browser web/tests/structural-blocks.browser.test.ts` | styles/test same task | pending |
| 02-13-01 | 02-13 | 13 | EDIT-05, QUAL-04 | T-02-13-01..05 | Bounded staging retains canonical blake3 identity and P2-02 | `cargo test -p flow-core --test asset_staging --test recovery -- stage_insert --nocapture` | module/tests created same task | pending |
| 02-13-02 | 02-13 | 13 | EDIT-05, QUAL-03, QUAL-04 | T-02-13-02..05 | Replace/accessibility/remove exact inverse and shared reachability | `cargo test -p flow-core --test asset_staging --test recovery -- image_lifecycle --nocapture` | lifecycle/tests expanded same task | pending |
| 02-14-01 | 02-14 | 14 | EDIT-05 | T-02-14-01..03 | Binary ingress and atomic shared-safe IndexedDB lifecycle | `npx --no-install vitest run web/tests/indexeddb-store.test.ts && cargo test -p flow-core --test recovery -- image_asset --nocapture` | WASM/store/tests same task | pending |
| 02-14-02 | 02-14 | 14 | EDIT-05, QUAL-03, QUAL-04 | T-02-14-02..05 | Complete image controls, confirmation, focus, and recovery | `npx --no-install vitest run --project browser web/tests/image-lifecycle.browser.test.ts` | UI/test created same task | pending |
| 02-15-01 | 02-15 | 15 | QUAL-03 | T-02-15-01..02 | Exhaustive nonvacuous command parity including new lifecycle families | `cargo test -p flow-core --test command_parity -- --nocapture` | catalog/test/JSON same task | pending |
| 02-15-02 | 02-15 | 15 | all | T-02-15-03 | Retained Rust/DOM/WASM/deferred-scope boundaries | `node --test tests/contracts/phase2-boundary.test.mjs tests/contracts/phase1-boundary.test.mjs` | Phase 2 contract created same task; Phase 1 prior | pending |
| 02-16-01 | 02-16 | 16 | QUAL-04 | T-02-16-01..04 | Every existing field/review state remains ordered/reachable | `cargo test -p flow-core --test field_accessibility -- --nocapture` | projection/test same task | pending |
| 02-16-02 | 02-16 | 16 | all | T-02-16-01..04 | Semantic editor/field/status/error/confirmation local accessibility | `npx --no-install vitest run --project browser web/tests/editor-accessibility.browser.test.ts` | shell/UI/test same task | pending |
| 02-17-01 | 02-17 | 17 | all | T-02-17-01 | Exact 108/108/0/0/0 UI ownership contract | `node --test tests/contracts/phase2-ui-contract.test.mjs && node scripts/build-phase2-ui-contract.mjs --check` | generator/contract/test same task | pending |
| 02-17-02 | 02-17 | 17 | all | T-02-17-02..03 | Responsive/state evidence and no fabricated external AT pass | `npx --no-install vitest run --project browser web/tests/editor-responsive.browser.test.ts web/tests/editor-ui-states.browser.test.ts && node --test tests/contracts/phase2-at-evidence.test.mjs` | browser/manual/evidence tests same task | pending |
| 02-18-01 | 02-18 | 18 | all | T-02-18-01..03 | Fast bounded semantic scale/privacy smoke | `node --test tests/contracts/phase2-scale.test.mjs && node scripts/verify-phase2-scale.mjs --smoke` | fixtures/verifier/test same task | pending |
| 02-18-02 | 02-18 | 18 | all | T-02-18-01..04 | Smoke plus complete deterministic release gate twice | `npm run check:phase2:smoke && npm run check:phase2 && npm run check:phase2` | runner/gate test/package scripts same task; all focused owners prior | pending |

## Wave Ownership

- Waves 1–4: exact dependencies, Unicode/size/boundary evidence, executable one-way legacy/migration gates, and grapheme/session authority.
- Waves 5–8: minimal production paragraph tracer, input/IME expansion, structural algebra, and structural product paths.
- Waves 9–12: formatting/list core/UI and complete page-break/table/header/remove core/UI.
- Waves 13–14: complete image lifecycle core, BLAKE3/reachability, WASM/storage/UI.
- Waves 15–17: exhaustive parity/boundaries, existing-field accessibility, exact UI 108 and honest AT evidence.
- Wave 18: focused scale smoke followed by two complete release passes.

## Manual/External Evidence Contract

| Owner | Behavior | Local status rule | External status rule |
|---|---|---|---|
| 02-17-02 | macOS VoiceOver semantic/focus/status/error/confirmation smoke | Record only an actual observed result or explicitly not-run/unavailable; never infer from Chromium | Does not close Windows/Edge evidence |
| 02-17-02 | Microsoft Edge on Windows plus Windows screen reader | Not available on this macOS workspace | Must remain exactly `unavailable/outstanding` until executed externally; the automated contract rejects a local pass claim |

## Validation Sign-Off

- [ ] 36/36 task IDs and decoded automated commands match exactly between PLAN and this map.
- [ ] Every referenced file exists from a prior wave or is created by its owning task before execution.
- [ ] The one-way legacy freeze executes before current model/schema mutation; all migration routes execute before persistence/tracer.
- [ ] No future-wave test appears in an earlier task command.
- [ ] One Unicode verifier name and one `scripts/verify-wasm-size.mjs` name are used everywhere.
- [ ] Requirements 7/7, decisions D-01..D-16, edges 33/33, UI 108/108/0/0/0, prohibitions 3/3 flagged, and all Phase 1 regressions are included.
- [ ] Terminal task starts with fast smoke and retains two full release passes.
- [ ] External Edge/Windows/screen-reader evidence is outstanding and never represented as pass.

**Approval:** pending implementation and exact evidence.

# Phase 3 Validation Manifest

This manifest is the executable contract for Phase 3 deterministic reflow and
pagination. Commands are run from the repository root with the pinned Rust
toolchain and checked-in Playwright cache. The gate does not install packages,
modify dependency locks, or claim PDF, forms, voice, reader, or external AT
scope.

## Exact validation rows

| ID | Requirement lane | Exact command |
| --- | --- | --- |
| 03-01-01 | LAYO-01 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test layout_fixed_point --test layout_text -- --nocapture` |
| 03-01-02 | LAYO-01 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test grapheme_conformance --test layout_text -- --nocapture` |
| 03-02-01 | LAYO-04 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test schema_v2_migration --test schema_v3_layout -- --nocapture` |
| 03-03-01 | LAYO-02, LAYO-03, LAYO-05, LAYO-06 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test layout_pagination --test layout_constraints -- --nocapture` |
| 03-04-01 | LAYO-07 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test layout_equivalence --test layout_properties -- --nocapture` |
| 03-05-01 | LAYO-08 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test layout_exports -- --nocapture` |
| 03-05-02 | LAYO-08 | `npm run build:wasm` |
| 03-05-03 | LAYO-08 | `npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run web/tests/layout-worker.test.ts` |
| 03-06-01 | LAYO-08, accessibility dependency | `npm run build:web && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/layout-viewport.browser.test.ts web/tests/editor-accessibility.browser.test.ts` |
| 03-06-02 | Rust/WASM regression | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet` |
| 03-06-03 | Web regression | `npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit` |
| 03-06-04 | Boundary regression | `node --test tests/contracts/phase1-boundary.test.mjs tests/contracts/phase2-boundary.test.mjs` |
| 03-06-05 | Rust quality | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo clippy --locked --workspace --all-targets -- -D warnings` |
| 03-06-06 | Phase 1 retained local gate | `npm run check` |
| 03-06-07 | Phase 2 retained scale smoke | `node scripts/verify-phase2-scale.mjs --smoke` |
| 03-06-08 | Workspace hygiene | `git diff --check` |
| 03-06-09 | Orchestration | `npm run check:phase3:smoke && npm run check:phase3 && npm run check:phase3` |

The final row is an orchestration command and is not recursively executed by
`check-phase3.mjs`. The intended verification is two consecutive full gate
runs after the contract smoke.

## Evidence boundary

The local rows cover the eight LAYO requirements with Rust, WASM, worker, and
Chromium evidence. The inherited Phase 2 Microsoft Edge on Windows plus
Windows-screen-reader checkpoint remains an external dependency recorded as
`unavailable/outstanding`; local Chromium/macOS evidence does not substitute
for it and the Phase 3 gate does not rewrite that record.

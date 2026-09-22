# Phase 5 Validation

Phase 5 closes only when every local task below passes and every Phase 5 plan
has a completed summary. External viewer and accessibility evidence remains an
explicit release input; an unavailable tool is never recorded as a pass.

| ID | Scope | Command |
| --- | --- | --- |
| 05-01-01 | Rust semantic form validation and projection | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test forms --test field_authoring --test field_accessibility -- --nocapture` |
| 05-02-01 | Rust form session and PDF form contracts | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_forms --test pdf_recovery -- --nocapture` |
| 05-03-01 | WASM form projection and session adapters | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test form_projections --test form_sessions -- --nocapture` |
| 05-04-01 | Form unit contracts | `npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/form-projection.test.ts web/tests/form-session.test.ts` |
| 05-05-01 | Form browser contracts | `PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/form-controls.browser.test.ts web/tests/form-projection.browser.test.ts` |
| 05-06-01 | PDF request and worker form integration | `PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/pdf-request.test.ts web/tests/pdf-worker.test.ts && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/pdf-request.browser.test.ts web/tests/pdf-preview.browser.test.ts` |
| 05-07-01 | Full local Rust package regression | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet` |
| 05-08-01 | Full local unit regression | `npm run test:unit` |
| 05-09-01 | Boundary contracts and source hygiene | `node --test tests/contracts/phase1-boundary.test.mjs tests/contracts/phase2-boundary.test.mjs && git diff --check` |
| 05-10-01 | Web production build | `npm run build:web` |
| 05-11-01 | Default production runtime smoke | `PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/production-runtime.browser.test.ts` |
| 05-12-01 | Phase 5 gate orchestration | `npm run check:phase5:smoke && npm run check:phase5 && npm run check:phase5` |

The last row is orchestration-only. The executable manifest is kept in
`scripts/check-phase5.mjs` and must remain byte-for-byte aligned with this
table's IDs and commands.

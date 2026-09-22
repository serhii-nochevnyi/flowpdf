# Phase 7 Validation Manifest

The phase gate separates local parser/WASM/browser evidence from optional PDF
reference-tool evidence. Commands use the repository-pinned Rust toolchain and
the checked-in Playwright cache; no packages or reference tools are installed
by the gate.

## Exact local validation rows

| ID | Requirement lane | Exact command |
| --- | --- | --- |
| 07-01-01 | PDFI-01, QUAL-05, QUAL-06 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_reader -- --nocapture` |
| 07-02-01 | PDFI-02, PDFI-03, PDFI-05 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_scene -- --nocapture` |
| 07-03-01 | PDFI-01, PDFI-05, QUAL-05, QUAL-06 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test pdf_reader -- --nocapture` |
| 07-03-02 | PDFI-01, PDFI-02, PDFI-05 | `npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/pdf-reader.test.ts` |
| 07-03-03 | PDFI-02, PDFI-05, QUAL-06 | `npm run build:web && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/pdf-reader.browser.test.ts` |
| 07-04-01 | QUAL-05, QUAL-06 | `node --test tests/contracts/phase7-gate.test.mjs && git diff --check` |
| 07-04-02 | QUAL-05, QUAL-06 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet` |
| 07-04-03 | Regression | `npm run check:phase6 && npm run check:phase5 && npm run check:phase4` |
| 07-04-04 | QUAL-05, QUAL-06 | `npm run check:phase7 && npm run check:phase7 && npm run check:planning` |
| 07-04-05 | Formatting | `git diff --check && cargo fmt --all -- --check` |

## Reference validation rows

These rows are reported as `unavailable` unless both a declared executable and
an explicitly checked-in Phase 7 fixture exist:

| ID | Tool | Evidence |
| --- | --- | --- |
| 07-R1 | `qpdf` | structural check of `artifacts/phase7/reader-fixture.pdf` |
| 07-R2 | `pdftotext` | extraction comparison for the mapped-text fixture |
| 07-R3 | `pdftoppm` | raster comparison for the scene fixture |
| 07-R4 | Chrome/Edge plus external AT | manual report/status and read-only scene observation |

Missing reference tools are not local passes. The phase can be locally closed
only with that limitation recorded in `07-CLOSURE.md`.

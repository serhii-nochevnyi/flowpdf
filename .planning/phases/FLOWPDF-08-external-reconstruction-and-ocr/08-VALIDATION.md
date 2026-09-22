# Phase 8 Validation Manifest

The phase gate proves the bounded local reconstruction/OCR-adapter contract.
It does not claim external OCR quality, arbitrary PDF compatibility, target
viewer behavior, or external assistive-technology evidence when those inputs
are unavailable.

## Exact local validation rows

| ID | Requirement lane | Exact command |
| --- | --- | --- |
| 08-01-01 | PDFI-04, PDFI-06, QUAL-01, QUAL-02 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_reconstruction -- --nocapture` |
| 08-01-02 | PDFI-04, QUAL-02 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test provenance -- --nocapture` |
| 08-02-01 | PDFI-04, PDFI-08, QUAL-02 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test pdf_reconstruction -- --nocapture` |
| 08-02-02 | PDFI-08, QUAL-01, QUAL-02 | `npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/pdf-reconstruction.test.ts` |
| 08-03-01 | PDFI-07, PDFI-08, QUAL-01, QUAL-02 | `npm run build:web && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/pdf-reconstruction.browser.test.ts` |
| 08-03-02 | QUAL-01 | `PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/pdf-source-store.test.ts` |
| 08-04-01 | QUAL-01, QUAL-02 | `node --test tests/contracts/phase8-gate.test.mjs && git diff --check` |
| 08-04-02 | Regression | `npm run check:phase7 && npm run check:phase6 && npm run check:phase5` |
| 08-04-03 | Formatting | `git diff --check && cargo fmt --all -- --check && npm run check:planning` |
| 08-04-04 | QUAL-01, QUAL-02 | `npm run check:phase8 && npm run check:phase8 && npm run check:planning` |

## External/reference rows

| ID | Evidence | Status policy |
| --- | --- | --- |
| 08-R1 | Declared OCR adapter/provider on Ukrainian/English scan corpus | `unavailable` when no provider/corpus is supplied; never a local pass |
| 08-R2 | qpdf/Poppler structural/text/raster comparison | `unavailable` when executable and fixture are absent |
| 08-R3 | Chrome/Edge target-viewer and external AT observation | `unavailable` in this local gate unless explicitly run and recorded |

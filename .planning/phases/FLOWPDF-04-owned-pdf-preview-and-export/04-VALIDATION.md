# Phase 4 Validation Manifest

This manifest is the executable contract for owned PDF preview and export.
Commands run from the repository root with the pinned Rust toolchain, local
WASM tooling, and checked-in Playwright cache. The gate does not install
packages, update dependency locks, or turn missing reference tools into a
pass. Structural Rust/WASM proof, browser projection evidence, and external
PDF/reference evidence remain separate rows.

## Exact local validation rows

| ID | Requirement lane | Exact command |
| --- | --- | --- |
| 04-01-01 | PDFX-01, PDFX-02 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_cos -- --nocapture` |
| 04-02-01 | PDFX-01, PDFX-02 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_text -- --nocapture` |
| 04-03-01 | PDFX-03, PDFX-05 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_resources -- --nocapture` |
| 04-04-01 | PDFX-04, PDFX-07 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_recovery -- --nocapture` |
| 04-05-01 | PDFX-01, PDFX-03, PDFX-06 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test pdf_exports -- --nocapture` |
| 04-05-02 | PDFX-01, PDFX-06 | `npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/pdf-worker.test.ts` |
| 04-05-03 | PDFX-01, PDFX-06, QUAL-07 | `npm run build:web && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/pdf-preview.browser.test.ts web/tests/layout-viewport.browser.test.ts web/tests/editor-accessibility.browser.test.ts` |
| 04-06-01 | QUAL-07 | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet` |
| 04-06-02 | QUAL-07 | `npm run test:unit` |
| 04-06-03 | QUAL-07, retained Phase 1/2/3 regression | `npm run check:phase2 && npm run check:phase3` |
| 04-06-04 | QUAL-07 | `git diff --check` |
| 04-06-05 | Orchestration | `npm run check:phase4:smoke && npm run check:phase4 && npm run check:phase4` |

The final row is orchestration-only and is not recursively executed by
`check-phase4.mjs`. The intended release verification is the smoke contract
followed by two consecutive full gate runs.

## Reference validation rows

Reference rows are executed only when both the named executable and the
declared fixture are present. Otherwise they emit `unavailable`; they never
emit `pass` merely because the command is known or because local Chromium
evidence exists.

| ID | Evidence lane | Tool | Fixture | Exact command |
| --- | --- | --- | --- | --- |
| 04-06-R1 | Structural PDF/reference | `qpdf` | `artifacts/phase4/flowpdf.pdf` | `qpdf --check artifacts/phase4/flowpdf.pdf` |
| 04-06-R2 | Text extraction/reference | `pdftotext` | `artifacts/phase4/flowpdf.pdf` | `pdftotext artifacts/phase4/flowpdf.pdf artifacts/phase4/flowpdf.txt` |
| 04-06-R3 | Visual raster/reference | `pdftoppm` | `artifacts/phase4/flowpdf.pdf` | `pdftoppm -png artifacts/phase4/flowpdf.pdf artifacts/phase4/render` |
| 04-06-R4 | Target viewer visual/reference | `Microsoft Edge on Windows` | `artifacts/phase4/flowpdf.pdf` | `target-viewer visual-diff evidence for artifacts/phase4/flowpdf.pdf` |

## Evidence boundary

The local rows cover the implemented owned COS/resource/provenance/WASM/
worker/preview contracts and retain the Phase 1/2/3 local regression gates.
End-to-end content-stream extraction and target-viewer visual compatibility
remain reference evidence. The current environment has no checked-in Phase 4
PDF fixture and the research probe found no qpdf, MuPDF, Poppler `pdftoppm`,
`pdfinfo`, or `pdftotext` executable on PATH; the runner must therefore retain
those rows as `unavailable` until a reference environment supplies both tool
and fixture.

The manifest does not claim AcroForm, encryption/signing, OCR, external PDF
import, voice, PDF/A, or PDF/UA scope. Unsupported features remain explicit in
the Rust support report and are not silently promoted by this gate.

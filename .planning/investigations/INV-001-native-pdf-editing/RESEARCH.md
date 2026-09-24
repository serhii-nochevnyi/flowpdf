# Research

<!-- Draft; each codebase observation names its source path. No .planning/codebase map exists, so this pass inspected the affected modules directly. -->

## Current system state

- `crates/flow-core/src/pdf/reader.rs` owns bounded, lazy PDF syntax reading and page-tree discovery. The admitted reader remains limited to its declared unencrypted syntax subset.
- `crates/flow-core/src/pdf/scene.rs` projects pages into fixed-point scene elements with source object/operator provenance. It is read-only and reports unsupported or blocked content.
- `crates/flow-core/src/pdf/mod.rs` owns a deterministic COS object graph and PDF writer used for FlowPDF-generated exports. `export_pdf` builds a new graph from page plans; it does not accept an imported reader object graph for mutation.
- `crates/flow-core/src/pdf/forms.rs` emits bounded AcroForm structures for owned exports. It does not yet provide a native imported-PDF widget-edit transaction.
- `crates/flow-wasm/src/lib.rs` exposes reader and reconstruction operations through verified string-only JSON contracts. No native PDF edit operation is present.
- `web/src/pdf/pdf-reader-worker.ts`, `web/src/pdf/pdf-reader-panel.tsx`, and `web/src/pdf/pdf-reader-panel.css` implement a separate reader worker and read-only scene display.
- `web/src/pdf/pdf-reconstruction-worker.ts` and `web/src/pdf/pdf-reconstruction-panel.tsx` handle a separate reconstruction/review lane; reconstruction does not mutate the source PDF.
- Relevant tests are `crates/flow-core/tests/pdf_reader.rs`, `crates/flow-core/tests/pdf_scene.rs`, `crates/flow-core/tests/pdf_forms.rs`, `web/tests/pdf-reader.test.ts`, and `web/tests/pdf-reader.browser.test.ts`.

## Constraints

### Technical

- The project architecture assigns canonical document semantics, PDF syntax, validation, and mutation authority to Rust; TypeScript is the physical browser-I/O and presentation layer (`.planning/PROJECT.md`, project architecture constraints).
- Imported PDFs are hostile input and parsing, decompression, fonts, images, and embedded actions require strict limits and sandboxing (`.planning/PROJECT.md`, Security constraint).
- The current reader uses explicit budgets and fixed-point geometry (`crates/flow-core/src/pdf/reader.rs`, `crates/flow-core/src/pdf/scene.rs`). Native edits must preserve bounded behavior and stable diagnostics.
- Existing signatures/encryption and arbitrary repair are outside the Phase 7 reader boundary (`.planning/phases/FLOWPDF-07-secure-pdf-reader-and-scene/07-CONTEXT.md`). Any rewrite must state how signatures and unsupported content are handled.
- A redaction annotation only marks content for a removal step. The PDF 1.7 specification describes a separate application-specific content-removal phase that removes the identified content and the redaction annotation (`https://opensource.adobe.com/dc-acrobat-sdk-docs/standards/pdfstandards/pdf/PDF32000_2008.pdf`).
- qpdf documents `--check` as a syntax/structure check with limits; a passing result does not establish full PDF conformance or visual equivalence (`https://qpdf.readthedocs.io/en/stable/cli.html`).

### Product

- Native edits must not convert a page to Flow mode; this is explicit in NPDF-01 (`.planning/REQUIREMENTS.md`).
- Unsafe font, layout, transform, clipping, or resource cases require an actionable warning (`.planning/REQUIREMENTS.md`, NPDF-02 and Phase 9 success criterion 2 in `.planning/ROADMAP.md`).
- Unsupported content must be surfaced explicitly and must not be silently discarded (`.planning/PROJECT.md`, Data integrity constraint).
- The original imported PDF is retained as an immutable source and reconstruction is best-effort (`.planning/phases/FLOWPDF-08-external-reconstruction-and-ocr/08-CONTEXT.md`).

### Delivery

- Phase 9 is currently `0/TBD` with no executable plans (`.planning/ROADMAP.md`).
- The local reader/reconstruction gates exist, but the Phase 8 closure records qpdf/Poppler, target-viewer, and external assistive-technology evidence as unavailable (`.planning/phases/FLOWPDF-08-external-reconstruction-and-ocr/08-CLOSURE.md`).
- No `.planning/codebase/` map is present; this investigation used the paths listed above directly.

## Unknowns

- The minimum supported object graph for a native edit and the refusal boundary for opaque objects are unresolved; see `OPEN-QUESTIONS.md`.
- Text-island eligibility and preservation of spacing, encodings, clipping, and appearance are unresolved; see `OPEN-QUESTIONS.md`.
- Page insertion semantics and cross-document page import are unresolved; see `OPEN-QUESTIONS.md`.
- Redaction coverage and the independent post-save validators/corpus are unresolved; see `OPEN-QUESTIONS.md`.

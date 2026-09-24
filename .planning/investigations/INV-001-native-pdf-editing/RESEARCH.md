# Research

This investigation inspected current source and phase evidence directly; no
`.planning/codebase/` map exists. Code observations were checked with focused
`rg -n`, `sed`, and `nl` reads of the named paths.

## Current system state

- `crates/flow-core/src/pdf/reader.rs` owns bounded, lazy syntax reading and
  page-tree discovery. `PdfReader` retains source bytes, source hash, the
  classic-xref index, trailer, root, and limits; its documentation explicitly
  states the complete object graph is absent. `object_with_depth` parses
  objects lazily. The admitted reader is an unencrypted, classic-xref subset.
- `crates/flow-core/src/pdf/scene.rs` projects pages into fixed-point scene
  elements with source object/operator provenance and stream offsets. It is
  read-only. Provenance is not an exact mutation span for every content
  operator and cannot itself authorize source-byte replacement.
- `crates/flow-core/src/pdf/mod.rs` owns a deterministic COS object graph and
  writer. `CosDocument::write` serializes a complete new graph and classic
  xref; `export_pdf` builds a graph from FlowPDF page plans. There is no
  imported-reader-to-writer graph adapter.
- `crates/flow-core/src/pdf/forms.rs` emits bounded AcroForm structures for
  owned exports and uses source-hash-bound form plans. It has no transaction
  for widgets imported from an external PDF.
- `crates/flow-wasm/src/lib.rs` exposes reader/reconstruction through verified
  string-only JSON contracts; it exposes no native PDF edit command.
- `web/src/pdf/pdf-reader-worker.ts` and `pdf-reader-panel.tsx` implement a
  separate read-only scene lane. Reconstruction is separate in
  `web/src/pdf/pdf-reconstruction-worker.ts` and
  `pdf-reconstruction-panel.tsx`; it does not mutate imported PDF bytes.
- Relevant existing tests include
  `crates/flow-core/tests/pdf_reader.rs`, `pdf_scene.rs`, `pdf_forms.rs`,
  `pdf_reconstruction.rs`, plus `web/tests/pdf-reader.test.ts` and
  `pdf-reader.browser.test.ts`.

## Constraints and findings

- `.planning/PROJECT.md` assigns semantic authority and mutation to Rust;
  TypeScript owns browser I/O and presentation. Imported PDFs are hostile input
  and must remain bounded.
- `.planning/phases/FLOWPDF-07-secure-pdf-reader-and-scene/07-CONTEXT.md`
  excludes xref/object streams, encryption, signatures, incremental updates,
  arbitrary repair, and action execution. Phase 8 keeps the original PDF
  immutable and places reconstruction in a separate best-effort lane.
- `.planning/REQUIREMENTS.md` defines NPDF-01 through NPDF-05 but leaves their
  supported object subsets unspecified. It requires native edits not to
  convert the document to Flow mode, explicit warnings/refusal for unsafe
  cases, and validated full-rewrite redaction.
- A redaction annotation is an identification mark, not content removal. The
  PDF 1.7 specification requires application-specific removal of identified
  content and the redact annotation; it warns that image data must be
  destroyed rather than hidden and calls out XFA/XMP as content to consider:
  [PDF 32000-1:2008](https://opensource.adobe.com/dc-acrobat-sdk-docs/standards/pdfstandards/pdf/PDF32000_2008.pdf).
- Adobe documents the same distinct mark-then-apply lifecycle and removal of
  the marks after applying redaction:
  [Working with redaction annotations](https://opensource.adobe.com/dc-acrobat-sdk-docs/library/plugin/Plugins_Annotations.html).
- qpdf's `--check` has limited syntax/structure claims; it does not establish
  full conformance, semantic preservation, secure redaction, or visual
  equivalence: [qpdf CLI documentation](https://qpdf.readthedocs.io/en/stable/cli.html).
- PDF Association publishes the ISO 32000-2 entry and specification archive:
  [ISO 32000-2](https://pdfa.org/resource/iso-32000-2/),
  [PDF specification archive](https://pdfa.org/resource/pdf-specification-archive/).
- `command -v qpdf pdftotext pdftoppm mutool` returned no paths in this
  environment. Phase 7/8 validation manifests likewise mark qpdf/Poppler rows
  unavailable when the executable and fixture are absent.

## Alternatives conclusion

Rebuilding pages from the scene is not a safe default: the scene is partial,
read-only, and cannot preserve all page semantics. Remaining read-only would
not deliver NPDF-01..05. The viable path is therefore an independent,
Rust-owned source-bound edit session over a complete bounded object graph and
a fresh whole-document rewrite. Parseable generic COS values and untouched
raw stream bytes are retained; unknown reachable syntax, unsupported stream
encodings, active/external behavior, and inputs outside the admitted source
subset fail closed before an output is published. Redaction uses a stricter
eligible subset and separate post-save validators.

## Delivery evidence

Phase 9 has no prior executable plans. The exact checked requirements are
`rg -n 'NPDF-0[1-5]' .planning/REQUIREMENTS.md`; existing reader/writer seams
were found with `rg -n 'Request-local owned PDF reader|object graph is
intentionally absent|fn object_with_depth' crates/flow-core/src/pdf/reader.rs`
and `rg -n 'pub struct CosDocument|pub fn write\\(|pub fn export_pdf'
crates/flow-core/src/pdf/mod.rs`. The current source-only/reference-tool gap
must stay explicit in Phase 9's validation manifest and release status.

The resolved product/technical boundaries are in `OPEN-QUESTIONS.md` and
`DECISIONS.md`; the accepted ADR is
`.planning/architecture/ADR-001-native-pdf-editing.md`.

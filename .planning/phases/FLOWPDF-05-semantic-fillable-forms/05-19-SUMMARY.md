---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "19"
status: complete
completed: 2026-09-22
requirements: [FORM-07, QUAL-07]
---

# Phase 5 Plan 05-19 Summary

## Delivered

- Added `createPdfExportRequest`, a reusable browser-side builder for the
  closed Rust PDF export wire payload. It copies accepted source/layout
  identities, canonical JSON, Rust-produced page bounds, and reproducibility
  identities without reading DOM text or measuring browser geometry.
- Carried a verified source-bound `PdfFormPlan` and normalized partial/all
  `flattenedFieldIds` into the opaque payload, while an ordinary request emits
  no form plan and an empty flatten list.
- Added fail-closed source, request/result, settings, font, hyphenation, page,
  request-ID, and selection identity checks before serialization.
- Replaced the PDF preview browser fixture's hand-written opaque JSON with the
  shared builder, preserving the revision-safe stale export and accessibility
  evidence.

## Verification

- `npm run typecheck` — passed.
- Focused unit tests — 3 tests passed for ordinary, partial/all, and stale
  request/selection cases.
- `npm run test:browser -- web/tests/pdf-preview.browser.test.ts` — 1 test
  passed through the shared request builder.
- `npm run check` — passed: boundary 14/14, Rust formatting/clippy and full
  Rust suite, WASM build, TypeScript, 58 unit tests, 8 focused accessibility
  tests, 57 Chromium tests, and deterministic replay 2/2.
- `npm run check:phase4` — passed: 11/11 local tasks, 4/4 external reference
  rows explicitly `unavailable`, and 9/9 mapped requirements.
- `git diff --check` — passed.

## Scope boundary and closure decision

This slice closes the browser request-construction seam for form-aware PDF
export while keeping Rust authoritative for form plans, flattening, geometry,
and PDF bytes. It does not activate the inspector's production workers or
font catalog, add target-viewer evidence, implement external form import, or
claim general PDF text/image compatibility.

## Commits

- `62f40eb` — Rust-compatible form-aware PDF request builder, unit evidence,
  and preview integration

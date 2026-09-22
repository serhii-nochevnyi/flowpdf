---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "13"
status: complete
completed: 2026-09-22
requirements: [FORM-07, QUAL-07]
---

# Phase 5 Plan 05-13 Summary

## Delivered

- Added explicit `PdfExportOptions.flattened_field_ids` selection with
  bounded duplicate-free ASCII identity validation and source-bound plan
  validation. Empty selection preserves the ordinary export identity and
  manifest shape.
- Added deterministic manifest/fingerprint binding for non-empty selections;
  caller order is normalized against the accepted canonical field order.
- Added derived page-content emission that reuses the bounded Plan 05-12
  appearance body at each accepted fixed-point field rectangle. Partial
  flattening retains unselected field dictionaries, widgets, appearances, and
  page annotations; selecting every field omits the `/AcroForm` catalog entry
  and widget annotations while retaining page resources/content.
- Kept flattening export-only: canonical `FlowDocument`, noncanonical form
  session, and accepted form plan are not mutated. The private source envelope
  still recovers the canonical source exactly, and selected text is emitted
  only through bounded PDF string data rather than operators or names.
- Kept browser/WASM flattening transport, general text/image flattening,
  external PDF form import, and target-viewer claims outside this slice.

## Verification

- `cargo fmt --all`
- `cargo test --locked -p flow-core --test pdf_forms -- --nocapture` — 6 tests
  passed.
- `cargo test --locked -p flow-core --test pdf_recovery -- --nocapture` — 3
  tests passed.
- `npm run check` — passed: Rust/Clippy, 93 Rust tests, recovery benchmark
  p95 507.16 ms against the 2,000 ms target, WASM, TypeScript, 49 unit tests,
  8 focused accessibility tests, 55 Chromium tests, and two deterministic
  replay rounds.
- `npm run check:phase4` — passed: 11/11 local tasks, 4/4 external reference
  rows explicitly `unavailable`, and 9/9 mapped requirements.
- `git diff --check`

## Scope boundary and closure decision

This slice closes the explicit derived core export path for selected semantic
fields and preserves the editable/source-bound contract. It does not close
Phase 5: browser/WASM flattening integration, target-viewer evidence, and
external form import remain open, and no general document flattening or full
Unicode/PDF compatibility is claimed.

## Commits

- `4b85043` — plan/research
- `610162e` — explicit flattening implementation, tests, and refreshed
  benchmark evidence

---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "12"
status: complete
completed: 2026-09-22
requirements: [FORM-06, QUAL-07]
---

# Phase 5 Plan 05-12 Summary

## Delivered

- Added derived, deterministic AcroForm normal appearance streams to the
  owned PDF writer without changing `FlowDocument`, form-session state, or the
  public `PdfFormPlan` schema.
- Added one bounded built-in Helvetica resource plus explicit AcroForm `/DR`,
  `/DA`, and `/NeedAppearances false` entries. Appearance streams use checked
  fixed-point widget geometry and a small deterministic border/background
  vocabulary.
- Added normal appearances for text, choice, signature, and push-button
  widgets. Checkbox and radio widgets now emit `/AP /N` state dictionaries
  containing `/Off` and only validated Rust-derived on-state names; `/AS` is
  emitted only for the accepted current value.
- Encoded appearance text as bounded uppercase hexadecimal UTF-16BE strings,
  preventing user text from becoming PDF operators or names. Added fail-closed
  validation for oversized text and forged button/choice states.
- Kept actions, JavaScript, external references, viewer-specific fallbacks,
  flattening, and external form import out of the export contract.

## Verification

- `cargo fmt --all`
- `cargo test --locked -p flow-core --test pdf_forms -- --nocapture` — 4 tests
  passed.
- `npm run check` — passed: Rust/Clippy, 93 Rust tests, recovery benchmark
  p95 508.44 ms against the 2,000 ms target, WASM, TypeScript, 49 unit tests,
  8 focused accessibility tests, 55 Chromium tests, and two deterministic
  replay rounds.
- `npm run check:phase4` — passed: 11/11 local tasks, 4/4 external reference
  rows explicitly `unavailable`, and 9/9 mapped requirements.
- `git diff --check`

## Scope boundary and closure decision

This slice proves explicit bounded appearance structure and deterministic
state coverage in owned PDF bytes. The built-in Helvetica resource and
UTF-16BE representation do not claim complete Ukrainian/arbitrary-Unicode
glyph coverage or target-viewer compatibility. Phase 5 remains open for
viewer evidence, flattening, and external form import.

## Commits

- `1e48ba9` — plan/research
- `da3d731` — appearance resources/streams, validation, tests, and refreshed
  benchmark evidence

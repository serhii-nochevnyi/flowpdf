---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "20"
status: complete
completed: 2026-09-22
requirements: [FORM-07, QUAL-07]
---

# Phase 5 Plan 05-20 Summary

## Delivered

- Added a real Chromium/WASM smoke for the shared ordinary PDF request path.
- The test obtains canonical JSON, revision, and hash from Rust `create_sample`,
  builds the request through `createPdfExportRequest`, sends only its opaque
  `serializedRequest` to Rust `export_pdf`, and verifies the complete response
  with `verify_pdf_export_response`.
- Assertions bind the returned PDF result and manifest to the Rust-produced
  source identity and the accepted layout settings/result identity.

## Verification

- `npm run typecheck` — passed.
- `npm run test:browser -- web/tests/pdf-request.browser.test.ts` — 1 test
  passed through generated Rust/WASM.
- `npm run check` — passed: boundary 14/14, Rust formatting/clippy and full
  Rust suite, WASM build, TypeScript, 58 unit tests, 8 focused accessibility
  tests, 58 Chromium tests, and deterministic replay 2/2.
- `git diff --check` — passed.

## Scope boundary and closure decision

This slice proves the ordinary browser builder against the actual generated
Rust/WASM PDF boundary. It does not mount the inactive editor entry, create
production workers, add font bytes or catalog loading, expand form-plan
transport beyond the already verified Rust/unit paths, or claim target-viewer
compatibility or external form import.

## Commits

- `9018aab` — generated-WASM browser smoke and 05-20 plan/research

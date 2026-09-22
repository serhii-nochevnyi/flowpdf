---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "24"
status: ready
---

# Phase 5 Plan 05-24 Research

## Scope

The PDF browser boundary currently exposes `createWasmPdfExportEngine` and
`RevisionAwarePdfExportScheduler` as separate seams. Callers must manually
copy the engine's `run` and `verifyResultHash` into scheduler options. The
generated-WASM smoke also calls `export_pdf` directly, so it does not exercise
the complete adapter-to-scheduler path.

## Existing seams

- `createWasmPdfExportEngine` decodes and Rust-verifies the string-only
  response, then exposes its result-hash guard.
- `RevisionAwarePdfExportScheduler` owns cancellation, source/layout/manifest
  validation, and accepted publication.
- `PdfExportSchedulerOptions` already contains only caller-owned diagnostics
  and publication callbacks beyond the adapter guard.
- `pdf-request.browser.test.ts` has a valid Rust-created source and a closed
  request builder fixture suitable for the full scheduler path.

## Chosen approach

1. Add `createWasmPdfExportScheduler(wasm, options?)` that composes the
   existing adapter into the existing revision-aware scheduler.
2. Preserve optional caller diagnostics/publication options while always using
   the adapter's Rust-backed result hash guard.
3. Add a unit test for successful composition and retain the adapter's
   malformed-response coverage.
4. Change the generated-WASM browser smoke to submit through the helper and
   assert the published accepted result rather than invoking Rust directly.

## Risks and fences

- This helper runs a caller-provided WASM boundary; it does not create a
  browser `Worker`, load font bytes/catalogs, or activate an application entry.
- Rust remains authoritative for PDF request/result validation, bytes,
  manifest identity, and source recovery.
- Existing direct engine and scheduler APIs remain available for custom
  integrations.

## Verification target

- `npm run typecheck`
- `npm run test:unit -- web/tests/pdf-worker.test.ts`
- `npm run test:browser -- web/tests/pdf-request.browser.test.ts`
- `npm run check`

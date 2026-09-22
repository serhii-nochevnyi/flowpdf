---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "22"
status: ready
---

# Phase 5 Plan 05-22 Research

## Scope

Plan 05-21 made the shared `createPdfExportRequest` builder the
`EditorController` default when a PDF scheduler is supplied without a custom
factory. The existing real-Chromium PDF preview smoke still provides an
equivalent factory only to capture the request. This slice should remove that
test-only duplication and capture the request at the scheduler engine seam.

## Existing seams

- `EditorApp` renders `PdfPreview` only when the controller reports a PDF
  scheduler and request path.
- `RevisionAwarePdfExportScheduler` receives the immutable request before
  invoking its caller-owned engine.
- `EditorController` now builds the Rust-compatible request by default,
  including the accepted layout pages and optional form selection.
- `pdf-preview.browser.test.ts` already exercises React, accepted layout,
  export scheduling, stale revision cancellation, and download suppression.

## Chosen approach

1. Remove the browser test's explicit `createPdfExportRequest` factory and
   request counter.
2. Capture the request in the fixture engine immediately before it returns a
   pending result.
3. Keep the fixture scheduler, fake result, and Rust/WASM boundary out of
   production code; existing assertions continue to prove stale export
   suppression and source-bound preview behavior.
4. Add an assertion that the captured default request is valid and carries the
   accepted page/layout identity.

## Risks and fences

- This is browser evidence for the controller seam, not production worker or
  font-catalog wiring.
- The browser fixture does not claim target-viewer compatibility or external
  form import.
- Rust remains authoritative for canonical text, geometry, form projection,
  and PDF bytes; the test only captures the opaque request at scheduling.

## Verification target

- `npm run typecheck`
- `npm run test:browser -- web/tests/pdf-preview.browser.test.ts`
- `npm run check`

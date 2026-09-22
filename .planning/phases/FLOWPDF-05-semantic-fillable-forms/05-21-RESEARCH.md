---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "21"
status: ready
---

# Phase 5 Plan 05-21 Research

## Scope

The controller currently requires callers to provide both a PDF scheduler and
an opaque request factory. Plan 05-19 introduced the shared Rust-compatible
builder, and Plan 05-20 proved it against generated WASM. The next small
integration should make that builder the controller default whenever a caller
has supplied a PDF scheduler, while preserving an explicit custom factory.

## Existing seams

- `EditorControllerDependencies` in `web/src/editor/editor-controller.ts`
  already separates the optional scheduler from the request factory.
- `EditorController.requestPdfExport` validates the accepted layout and form
  selection before invoking the factory and scheduler.
- `createPdfExportRequest` is now the tested source/layout/form-selection
  builder and returns `null` on stale or incomplete accepted projections.
- `EditorApp` checks `controller.hasPdfExport()` before rendering preview; no
  default worker, font catalog, or active `web/index.html` entry is present.

## Chosen approach

1. Import the shared builder into `EditorController`.
2. If a PDF scheduler exists and no custom factory is supplied, install a
   controller-local monotonic request-ID factory that forwards the optional
   source-bound form selection to `createPdfExportRequest`.
3. Keep an explicitly supplied factory authoritative and leave scheduler,
   worker, WASM, font, and active-entry ownership with the caller.
4. Add unit evidence that the default path carries the verified selection and
   accepted page/layout identity into the scheduler.

## Risks and fences

- This does not instantiate a scheduler or worker and does not make the
  inactive editor entry production-ready.
- Request IDs are local scheduling identities only; Rust remains authoritative
  for payload validation, form plans, flattening, and PDF bytes.
- Existing custom factories remain behaviorally unchanged.

## Verification target

- `npm run typecheck`
- `npm run test:unit -- web/tests/editor-controller.test.ts`

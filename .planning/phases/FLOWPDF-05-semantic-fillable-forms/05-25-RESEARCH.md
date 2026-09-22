---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "25"
status: ready
---

# Phase 5 Plan 05-25 Research

## Scope

`installPdfWorker` already owns the dedicated worker message loop around the
revision-aware scheduler, but the current unit suite covers the scheduler and
WASM adapter without asserting the worker-scope messages. The next bounded
slice should test accepted and cancelled outbound messages at that seam
without wiring a real worker into the active application entry.

## Existing seams

- `installPdfWorker` accepts a minimal `PdfWorkerScope`, an export engine, and
  the scheduler result-hash guard.
- The scheduler already emits accepted results through `onPublished` and
  maps discarded/cancelled/failed outcomes to worker messages.
- `pdf-worker.test.ts` already has deterministic request/result fixtures and a
  deferred helper for cooperative cancellation.

## Chosen approach

1. Add a fake scope that records outbound messages and retains the installed
   `onmessage` callback.
2. Drive one export to an accepted result and assert only the closed accepted
   message is emitted.
3. Drive one pending export through a matching cancel message and assert the
   closed cancelled message is emitted after the scheduler observes abort.
4. Keep production worker construction and active entry wiring unchanged.

## Risks and fences

- The fake scope tests protocol routing only; it does not claim browser Worker
  startup, transfer performance, or production font/catalog wiring.
- Messages contain result DTOs already guarded by the scheduler; no authored
  canonical payload is introduced by the test.
- Rust remains authoritative for PDF result identity and bytes.

## Verification target

- `npm run typecheck`
- `npm run test:unit -- web/tests/pdf-worker.test.ts`
- `npm run check`

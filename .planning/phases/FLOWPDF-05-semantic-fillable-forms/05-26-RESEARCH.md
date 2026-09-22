# Phase 5 Plan 05-26 Research: Production Worker and Font-Catalog Wiring

**Researched:** 2026-09-22

## Scope

The previous Phase 5 slices intentionally stopped at caller-owned scheduler
seams. The remaining repository-owned gap is the default runtime composition:
the browser must be able to admit the checked-in font bytes, obtain the same
Rust catalog identity used by layout, and run the existing layout/PDF message
loops in dedicated workers without moving semantic or PDF authority into
TypeScript.

## Findings

1. `RevisionAwareLayoutScheduler` and
   `RevisionAwarePdfExportScheduler` already own stale-result, cancellation,
   result-identity, and hash publication guards. New worker clients should
   use these schedulers with a message-backed engine rather than duplicate
   those guards.
2. `installLayoutWorker` and `installPdfWorker` are already complete scope
   boundary loops. Dedicated entries only need to load the generated WASM
   module, create the Rust-verifying adapter, and forward messages safely
   during asynchronous WASM initialization.
3. The Rust layout request requires explicit font bytes and a catalog identity.
   `FontCatalog::identity()` is a BLAKE3 identity over ordered face metadata
   and content hashes, so JavaScript must not reimplement or guess it. A small
   Rust/WASM identity endpoint is the narrowest safe bridge.
4. The PDF request currently carries font manifest identities but the browser
   builder emits an empty list. The runtime can pass the same verified face
   identity to the builder; physical font bytes remain in the layout request
   only and never enter the PDF manifest.
5. The checked-in `NotoSans-Regular.ttf` is the existing deterministic Phase 3
   fixture. It can be bundled as a Vite asset and loaded with a bounded fetch;
   no host-font discovery or network font service is needed.
6. The editor controller already accepts caller-owned layout/PDF dependencies,
   while `main.tsx` is the production entry. Default runtime composition can
   be added there without changing test callers that intentionally inject fake
   dependencies.

## Approach

- Add a Rust/WASM catalog-identity response with bounded JSON validation and
  tests.
- Add a browser font-catalog loader, layout request builder, worker entries,
  and message-backed schedulers that delegate to the existing revision-aware
  schedulers.
- Extend the PDF request builder with optional verified font manifest
  identities and compose the default runtime at the production entry.
- Keep all custom dependency injection paths intact and prove the new wiring
  with focused unit/build/browser checks.

## Risks and fences

- Worker startup must not lose requests sent before WASM initialization; the
  entry wrapper queues forwarding until its handler is installed.
- A catalog identity mismatch must fail in Rust before layout publication.
- A worker error or termination must reject pending requests and never publish
  an accepted result.
- This slice does not claim target-viewer compatibility or external PDF form
  import; those require separate evidence/reader work.

---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "23"
status: ready
---

# Phase 5 Plan 05-23 Research

## Scope

The editor shell can receive a prebuilt `EditorController`, but
`mountEditorApp` constructs its own controller without accepting
`EditorControllerDependencies`. A caller therefore cannot compose the existing
layout and PDF schedulers through the shell entry point without manually
constructing the controller. The next small boundary should expose that
composition seam while keeping all adapters caller-owned.

## Existing seams

- `EditorController` already accepts optional document store, WASM, layout,
  and PDF scheduler dependencies.
- `EditorApp` already accepts an optional controller and uses the same options
  to construct one when omitted.
- `mountEditorApp` returns the constructed controller and is used by the
  browser editor suites.
- The PDF preview smoke already owns fixture schedulers and now proves the
  controller default request builder.

## Chosen approach

1. Export `EditorControllerDependencies` through `editor-app.tsx`.
2. Add optional `controllerDependencies` to `EditorAppProps` for React-shell
   composition when a caller does not supply a controller.
3. Add an optional dependencies argument to `mountEditorApp` and pass it to
   `EditorController`.
4. Move the PDF preview smoke from manual controller construction to the
   `mountEditorApp` dependency seam, keeping its scheduler/layout fixtures
   local to the test.

## Risks and fences

- This exposes dependency injection only; it does not instantiate workers,
  load fonts, activate the current Foundation Inspector entry, or change PDF
  byte ownership.
- Existing callers remain source-compatible because the new arguments are
  optional and the supplied-controller path remains authoritative.
- Rust remains authoritative for canonical state, layout, form projection,
  and PDF bytes.

## Verification target

- `npm run typecheck`
- `npm run test:browser -- web/tests/pdf-preview.browser.test.ts`
- `npm run check`

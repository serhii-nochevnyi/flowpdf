# Phase 5 Plan 05-06 Research: Accessible Semantic Form Controls

**Researched:** 2026-09-22

## Sources inspected

- `web/src/forms/form-session.ts` — the versioned Rust/WASM action bridge and
  guarded persistence contract completed by Plan 05-05.
- `web/persistence/indexeddb-store.ts` — the physical browser store that owns
  only accepted noncanonical session DTOs.
- `web/src/editor/editor-app.tsx`, `semantic-document.tsx`, and
  `field-navigation.tsx` — the current accepted editor projection and the
  existing read-only field review surface.
- `web/src/editor/editor-controller.ts` — the shared cached WASM loader and
  accepted canonical snapshot identity (`canonicalJson`, document ID, revision,
  and hash).
- `web/src/editor/editor-store.ts` and `editor-messages.ts` — field descriptor,
  authored-value DTOs, locale parity, and accessible status conventions.
- `web/tests/editor-accessibility.browser.test.ts` and existing editor browser
  suites — real Chromium mounting and the current field-region assertions.

## Findings

1. The field projection is already Rust-owned, but `FieldNavigation` currently
   renders only metadata. It explicitly tells users that fields cannot change,
   so no browser control may be added without a session bridge and a separate
   status/error path.
2. Plan 05-05 provides the correct boundary: the browser can submit a typed
   `FormSessionActionDto`, while Rust validates field kind, options, required
   state, read-only state, source revision, and generation before the store is
   touched.
3. The editor's accepted snapshot already carries the canonical JSON and exact
   document identity needed to start/validate a session. A session coordinator
   can reset on source identity changes without entering the canonical command
   bus or changing editor revision/history.
4. Native semantic controls are appropriate for the current field vocabulary:
   text/date/number/email inputs, checkbox, radio group, and select. Signature
   and button fields remain explicit non-editable placeholders until their
   separate semantics are specified; review-region fields must not be
   presented as writable controls.
5. Existing browser tests mount the real app and already assert accessible
   field ordering/labels. The new controls can preserve those selectors while
   adding stable labels, `aria-describedby` error/status links, and explicit
   loading/failed session states.

## Scope boundary

This slice adds accessible fill controls for already-authored valid fields and
connects them to the Rust-owned noncanonical session. It does not add field
authoring/descriptor insertion, field-anchor authoring UI, AcroForm appearance
streams, PDF viewer behavior, flattening, external form import, or canonical
`SetField` mutations.

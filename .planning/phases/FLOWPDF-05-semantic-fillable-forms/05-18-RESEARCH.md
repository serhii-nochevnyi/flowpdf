---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "18"
status: complete
---

# Phase 5 Plan 05-18 Research

## Scope

The next vertical slice should make explicit flatten selection usable from the
browser projection surface and carry that selection to the existing PDF export
request factory. The browser must not parse or rewrite the opaque PDF request;
Rust remains authoritative for the source-bound `PdfFormPlan`, field validity,
and flattening semantics.

## Existing seams

- `FormProjectionResultDto.formPlan` is present only when Rust found no review
  entries and already carries source revision/hash, display-list identity, and
  a deterministic `resultHash`.
- `FormProjectionViewport` is currently read-only and is the correct accessible
  surface for selection controls. The page overlay must remain `aria-hidden`.
- `EditorApp` owns the projection snapshot and is the only place that can
  safely reset UI selection when the source/projection identity changes.
- `EditorController.requestPdfExport()` invokes a caller-owned
  `pdfExportRequestFactory`; adding an optional third, typed selection argument
  preserves existing factories and lets the caller encode the selection into
  the already-opaque Rust request.
- Rust export already accepts `formPlan` plus `flattenedFieldIds` and rejects
  unknown, duplicate, planless, or review-invalid selections. No Rust changes
  are needed in this slice.

## Chosen approach

1. Add a versioned `FormProjectionSelectionDto` and a fail-closed constructor
   that copies the verified Rust plan, binds every identity, and normalizes
   selected IDs in plan order.
2. Add an accessible fieldset with per-field checkboxes plus select-all/clear
   actions. Controls are enabled only for a synchronized projection with a
   non-null plan and no review entries; changing them never mutates canonical or
   form-session state.
3. Keep selection in `EditorApp` UI state, clear it whenever the accepted
   projection identity changes, and pass the derived selection to
   `requestPdfExport`.
4. Extend the export factory seam with an optional selection parameter and
   reject source-mismatched selections before scheduling. The opaque serialized
   request remains the factory's responsibility.

## Risks and fences

- A stale selection must never reach export: source revision/hash and plan
  identities are checked in the helper and controller; the selection is reset
  after projection changes.
- Review-bearing projections intentionally expose a disabled explanation
  rather than offering a selection that Rust would reject.
- An empty selection is valid and means “retain all editable widgets”; it is
  still source-bound so the factory can choose whether to include the plan.
- This does not claim target-viewer behavior, external PDF form import, or
  general text/image flattening.

## Verification targets

- Unit tests cover valid normalization, plan/review/source fences, unknown and
  duplicate IDs, and controller propagation/rejection.
- Chromium evidence covers keyboard-accessible selection, select-all/clear,
  disabled review state, source-change reset, and coexistence with the semantic
  input host.
- TypeScript, boundary, Rust/WASM, full browser, replay, and Phase 4 gates
  remain green.

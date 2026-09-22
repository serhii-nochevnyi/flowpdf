---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "19"
status: complete
---

# Phase 5 Plan 05-19 Research

## Scope

Plan 05-18 carries a validated selection to the caller-owned PDF export
factory, but the repository still has no reusable browser builder for the
closed Rust PDF request payload. The next slice should provide that builder so
an export integration can include the verified form plan and selected IDs
without parsing or mutating opaque requests in React components.

## Existing seams

- `PdfExportRequestDto.serializedRequest` is the only browser-to-Rust PDF
  payload. Rust's `PdfExportWireRequest` accepts canonical JSON, accepted page
  bounds, layout/font/hyphenation identities, optional `formPlan`, and
  `flattenedFieldIds`.
- `AcceptedEditorSnapshot` already contains the canonical source revision/hash
  and JSON; `AcceptedLayoutDto` already contains the accepted page bounds and
  result identities.
- `FormProjectionSelectionDto` contains a Rust-verified source-bound plan and
  normalized IDs. The builder can validate its source identity before copying
  it into the opaque payload.
- `EditorController` remains the revision-safe caller boundary. The helper
  should return `null` for invalid/stale inputs so existing factories can keep
  their fail-closed request-factory contract.

## Chosen approach

1. Add `createPdfExportRequest` under `web/src/pdf/` with explicit request
   options (`requestId`, optional producer, optional form selection).
2. Validate request/source/layout/selection identities before serialization;
   use accepted Rust page bounds and layout identities verbatim, with no DOM
   measurement or browser text parsing.
3. Emit the exact Rust wire field names, including `formPlan: null` and an
   empty `flattenedFieldIds` when no selection is supplied. The helper is the
   one place that knows this payload shape; callers still own scheduling.
4. Prove ordinary, partial-selection, all-selection, and stale-selection
   payloads in unit tests, then use the helper in the existing PDF preview
   fixture to demonstrate the controller callback path.

## Risks and fences

- The builder does not create a plan or geometry. It copies only accepted Rust
  layout/form data and fails closed on identity mismatch.
- `fontFaces` remains an identity-only list in this PDF request; production
  font catalog loading and target-viewer evidence stay outside the slice.
- The helper does not activate the inspector's web entry or invent default
  worker/font configuration. It is a reusable integration seam for the next
  application-wiring step.
- Empty `flattenedFieldIds` remains an ordinary editable export; all-field
  selection is represented explicitly by the full plan field-ID list.

## Verification targets

- Unit tests inspect the parsed opaque payload and run the existing PDF request
  metadata validator.
- The PDF preview Chromium fixture uses the builder and asserts its request
  remains revision/layout-bound.
- TypeScript, full repository, replay, and Phase 4 gates remain green.

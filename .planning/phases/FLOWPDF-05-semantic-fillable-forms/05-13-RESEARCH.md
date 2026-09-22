# Phase 5 Plan 05-13 Research

## Scope

The next vertical slice adds an explicit export-time operation for flattening
selected accepted form fields into the owned PDF page content. It is not a
canonical document mutation: `FlowDocument`, the noncanonical form session,
and the accepted `PdfFormPlan` remain available to the caller, while the
export request records which field IDs were selected for this derived output.

## Existing contracts

- `PdfExportOptions` already carries deterministic document/export options and
  `PdfExportRequest::fingerprint` binds them to the export identity.
- `export_pdf` allocates one bounded empty content stream per page and passes
  fixed-point page references/bounds to `forms::emit_form_objects`.
- Plan 05-12 emits reusable normal appearance commands and a shared `/Helv`
  resource. The appearance body can be translated into page coordinates with
  a checked `q ... cm ... Q` wrapper.
- The private `FlowPDF.Source` stream preserves the canonical source payload;
  exact recovery must continue to return the editable source even when the
  derived PDF output is flattened.

## Findings

1. A flatten selection belongs in export options, not in `FlowDocument`,
   `PdfFormPlan`, or form-session state. A sorted, duplicate-free list of
   selected field IDs makes the request explicit and makes equivalent
   selections byte-identical regardless of caller order.
2. The form emitter can omit selected fields from `/Fields` and page `/Annots`
   while appending their derived appearance bodies to the corresponding page
   content streams. Unselected fields retain the complete AcroForm field,
   widget, `/AP`, and `/AS` structure.
3. When every field is selected, no `/AcroForm` catalog entry is needed. When
   only some fields are selected, the catalog contains only the remaining
   editable fields. Flattened pages need a direct `/Resources /Font /Helv`
   entry for text appearance commands.
4. Empty flatten selections must preserve the existing export fingerprint and
   manifest shape. Non-empty selections must be represented in the
   reproducibility manifest and fingerprint, so changing the derived output
   cannot collide with the editable export.
5. Unknown, duplicate, oversized, or plan-less selections fail before the COS
   graph is published. No actions, JavaScript, external resources, viewer
   behavior, or general document text/image flattening are inferred.

## Design decisions

- Add `flattened_field_ids` to `PdfExportOptions` with an empty default; keep
  empty serialization/fingerprinting backward-compatible and include sorted
  IDs only when the selection is non-empty.
- Add the normalized selection to `PdfManifestOptions` with an omitted-empty
  serde representation so old empty-option manifests remain stable.
- Extend the internal form emission result with optional AcroForm reference,
  per-page flattened content, and the shared appearance font reference. The
  existing field/widget path remains unchanged when the selection is empty.
- Reuse the Plan 05-12 appearance body, validate page geometry before adding
  any selected content, and replace only the preallocated page content stream
  with a bounded COS stream.
- Treat flattening as an explicit derived export, not as destructive source
  replacement. The current scope proves owned bytes and recovery identity; it
  does not claim a browser UI, undo history mutation, or target-viewer matrix.

## Verification target

- `cargo fmt --all -- --check`
- focused `cargo test --locked -p flow-core --test pdf_forms -- --nocapture`
- focused `cargo test --locked -p flow-core --test pdf_recovery -- --nocapture`
- `npm run check`
- `npm run check:phase4`
- `git diff --check`

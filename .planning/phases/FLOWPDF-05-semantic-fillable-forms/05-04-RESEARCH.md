# Phase 5 Plan 05-04 Research: Bounded AcroForm COS Emission

**Researched:** 2026-09-21

## Sources inspected

- `crates/flow-core/src/pdf/mod.rs` — owned COS graph, page envelope,
  deterministic object allocation, export request/result, and page annotation
  hooks.
- `crates/flow-core/src/pdf/metadata.rs` — support-report vocabulary and the
  existing rejection of generic/active annotations.
- `crates/flow-core/src/forms/mod.rs` — versioned session-aware widget
  projection with authored/effective values and explicit review entries.
- `.planning/REQUIREMENTS.md` and `.planning/research/ARCHITECTURE.md` —
  FORM-06/FORM-07 boundaries and the writer/forms ownership split.

## Findings

1. The COS writer already allocates page objects and appends typed page
   annotation references, so form widgets can be emitted as ordinary bounded
   `/Widget` annotations with a parent field object and one catalog `/AcroForm`
   dictionary.
2. `PdfExportRequest` has no form input today. A builder-attached derived
   `PdfFormPlan` can bind the source revision/hash and keep field emission out
   of canonical document state.
3. The first form writer slice can emit field dictionaries, values/defaults,
   option arrays, flags, rectangles, page references, and deterministic object
   identity without inventing appearance streams. Appearance generation,
   viewer evidence, and flattening must remain explicit follow-on work.
4. Generic actions/external annotations remain unsupported. The support report
   can promote only semantic form fields when a validated plan is attached;
   generic annotation support stays excluded.

## Scope boundary

This plan adds bounded AcroForm field/widget structure to owned exports. It
does not claim explicit appearance streams, target-viewer consistency, external
PDF form import, signing, JavaScript/actions, or flattening.

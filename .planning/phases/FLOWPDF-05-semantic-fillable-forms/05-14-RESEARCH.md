# Phase 5 Plan 05-14 Research

## Scope

The next slice carries the already-implemented explicit form flattening
contract through the narrow Rust/WASM PDF export envelope. It admits an
optional source-bound `PdfFormPlan` and an explicit `flattenedFieldIds`
selection, then delegates all validation and output ownership to
`flow-core`. It does not add browser controls, derive a display list in the
browser, or claim target-viewer behavior.

## Existing contracts

- Plan 05-13 adds `PdfExportOptions::flattened_field_ids`, normalized
  fingerprint/manifest identity, retained-widget emission, and exact owned
  source recovery in `flow-core`.
- `crates/flow-wasm/src/lib.rs` currently accepts page bounds and ordinary PDF
  options but intentionally has no form plan or flatten selection. It builds a
  core request from the string-only envelope and keeps unknown JSON rejected.
- The TypeScript PDF worker already treats `serializedRequest` as an opaque
  Rust-owned JSON DTO. It can transport new admitted fields without parsing or
  rewriting semantic data; UI selection and projection construction are a
  separate boundary.

## Findings

1. The form plan is already serializable, source revision/hash bound, and
   validated by `PdfExportRequest::with_form_plan`; the WASM adapter should
   deserialize it and call that method rather than duplicate validation.
2. Flatten selection belongs in the wire request with an empty default. The
   core constructor rejects malformed selection shape, and attaching a plan
   rejects unknown field IDs, stale source identity, and invalid plan data
   before any COS result is published.
3. Existing no-form requests must remain byte-compatible. Optional serde
   fields with empty defaults preserve current callers and keep the worker's
   opaque transport model intact.
4. WASM tests can build a real source-bound plan from the checked-in Noto Sans
   fixture, pagination, display list, and form projection. That proves the
   envelope path without inventing browser-side geometry or form authority.

## Design decisions

- Add optional `formPlan` and `flattenedFieldIds` fields to
  `PdfExportWireRequest`, with `deny_unknown_fields` retained and empty/default
  behavior for existing requests.
- Construct `PdfExportOptions` with the explicit selection and attach the plan
  through `with_form_plan` before adding the canonical source payload.
- Keep the public TypeScript DTO opaque in this slice; no browser UI or client
  side reimplementation of `PdfFormPlan` is introduced. A later plan may add
  a Rust-owned projection/request factory and explicit accessible selection
  controls once the browser has a validated display-list/form-plan source.
- Preserve protocol version 1 because the new fields are optional and older
  request shape remains valid; malformed or semantically unsupported data keeps
  stable core error codes in the existing response envelope.

## Verification target

- `cargo fmt --all -- --check`
- `cargo test --locked -p flow-wasm --test pdf_exports -- --nocapture`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- `npm run check`
- `npm run check:phase4`
- `git diff --check`

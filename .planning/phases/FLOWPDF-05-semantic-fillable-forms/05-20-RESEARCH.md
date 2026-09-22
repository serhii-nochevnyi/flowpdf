---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "20"
status: ready
---

# Phase 5 Plan 05-20 Research

## Scope

Plan 05-19 proves the request builder with TypeScript payload fixtures and a
preview callback, but it does not execute the resulting opaque JSON through
the generated WASM boundary. The next low-risk slice should add one real
browser smoke that builds an ordinary request from a Rust-produced sample
session and sends it to `export_pdf`.

## Existing seams

- `web/src/pdf/pdf-request.ts` is the single browser builder for the closed
  `PdfExportWireRequest` shape.
- `web/generated/flow_wasm.js` exposes `init`, `create_sample`, `export_pdf`,
  and `verify_pdf_export_response`, and existing browser suites already use
  the generated module after `await init()`.
- `crates/flow-wasm/src/lib.rs` uses `serde` with `deny_unknown_fields` for
  the PDF request and routes accepted canonical payloads to Rust's owned
  exporter, so a successful response proves both decode and source checks.
- `EditorAcceptedSnapshot` and `AcceptedLayoutDto` are browser projections;
  the smoke can construct only their accepted identity/page-bound fixture and
  use the real Rust-produced canonical session as the source.

## Chosen approach

1. Add a browser test that initializes generated WASM and obtains canonical
   JSON, hash, and revision from `create_sample`.
2. Build an ordinary request with `createPdfExportRequest`, using one accepted
   fixed-point page bound and matching source/layout identities.
3. Call Rust `export_pdf` with `serializedRequest`, assert a successful
   response and `verify_pdf_export_response`, and check manifest source/layout
   identities. No browser text parsing, font bytes, or page measurement is
   introduced.

## Risks and fences

- This is an ordinary planless export smoke; form-plan transport remains
  covered by Rust boundary tests and TypeScript unit fixtures.
- It does not mount the inactive editor entry, create a Worker, or claim that
  the current inspector has production font-catalog wiring.
- The test uses Rust's sample document only to obtain valid canonical source;
  it does not make the browser a canonical-document authority.

## Verification target

- `npm run typecheck`
- `npm run test:browser -- web/tests/pdf-request.browser.test.ts`

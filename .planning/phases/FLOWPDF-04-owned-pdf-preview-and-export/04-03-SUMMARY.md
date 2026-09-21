# Phase 4 Plan 04-03 Summary

**Status:** Complete  
**Completed:** 2026-09-21

## Delivered

- Added a revision-owned PDF image-resource adapter for admitted PNG/JPEG
  records. It reuses the canonical asset hash, encoded/decode/dimension limits,
  exact descriptor/record checks, and stable content-identity deduplication.
- Added fixed-point `PdfImageItem` display-list entries carrying the source
  node, asset identity, derived/repeated provenance, and image rectangle. A
  missing source-to-asset mapping is a bounded privacy-safe diagnostic.
- Added deterministic typed metadata, outline, and internal-link options. The
  owned COS writer emits escaped Info/catalog values, bounded page destinations,
  outline objects, and internal link annotations without URI, launch, file,
  JavaScript, or encryption entries.
- Added an export support report that makes the supported text/image/metadata/
  outline/internal-link subset visible and reports form fields, actions,
  external resources, arbitrary annotations/paths, and encryption as explicit
  unsupported features.
- Added structural tests for valid resources, deduplication, hash/record
  failures, limit reuse, fixed-point image rectangles, metadata escaping,
  outline/link ordering, forbidden-action absence, and unsafe option rejection.

## Verification

- `cargo fmt --all` — passed.
- `cargo clippy --locked -p flow-core --all-targets -- -D warnings` — passed.
- `cargo test --locked -p flow-core --test pdf_resources --test pdf_cos` — 13
  passed.
- `cargo test --locked -p flow-core` — all unit/integration/doc tests passed,
  including the preserved `grapheme_conformance.rs` suite (9 tests).

## Scope boundary

This plan provides bounded image/structure resource inputs and emits metadata,
outline, and internal-link structure through the minimal COS envelope. It does
not yet claim end-to-end text/image content streams, revision-safe browser
export, exact source recovery, virtualized preview, or target-viewer
compatibility; those remain in Plans 04-04 through 04-06.

## Notes

- Physical asset bytes remain in derived `PdfImageResource` values and are not
  copied into semantic document DTOs or diagnostics.
- The image admission path now reads dimensions before applying decode limits,
  preserving a distinct dimension-limit diagnostic while retaining bounded
  decode allocation checks.
- The support report intentionally distinguishes the explicitly supported
  internal-link subset from arbitrary annotations and external actions.

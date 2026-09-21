# Phase 4 Plan 04-02 Summary

**Status:** Complete
**Completed:** 2026-09-21

## Delivered

- Added a revision/hash/catalog/hyphenation-bound Rust display-list projection
  for paragraph, heading, table-cell, header, and footer text. Entries retain
  source node/range, UTF-8/UTF-16 cluster positions, direction, glyph IDs,
  fixed-point placements, page order, and repeated-band provenance.
- Added privacy-safe diagnostics for unsupported fragments, missing source
  mappings, missing glyph mappings, and unsupported glyph coverage. Diagnostics
  do not carry authored text or font bytes.
- Added deterministic admitted-font resources. The supported first profile is
  a bounded TrueType subset: `.notdef` and composite dependencies are kept,
  glyph IDs and horizontal metrics are compacted deterministically, a format-12
  font cmap is generated, and PDF ToUnicode mappings preserve logical Unicode
  sequences including combining and Ukrainian text.
- Added explicit rejection for TTC/CFF/unsupported sfnt variants, malformed
  table directories, invalid locations/composites/metrics, and resource/stream
  limits. Resource identity includes the catalog/face identities, subset
  bytes, and ToUnicode bytes.
- Added the Phase 4 text corpus fixture and focused tests for page order,
  repeated header/footer provenance, stale inputs, Ukrainian/English mappings,
  combining clusters, compact-font metric preservation, deterministic resource
  naming, unsupported glyph diagnostics, and authored-text redaction.

## Verification

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --locked -p flow-core --all-targets -- -D warnings` — passed.
- `cargo test --locked -p flow-core --test pdf_text` — 7 passed.
- `cargo test --locked -p flow-core` — all unit/integration/doc tests passed,
  including the preserved `grapheme_conformance.rs` suite (9 tests).

## Scope boundary

This plan produces the derived display-list and font-resource inputs for the
owned PDF writer. It does not yet claim end-to-end selectable PDF bytes:
content streams, image resources, metadata, source recovery, browser preview,
and target-viewer extraction remain in Plans 04-03 through 04-06.

## Notes

- Host-font discovery is not used; resources are built only from the admitted
  byte-backed catalog and checked-in provenance fixture.
- The PDF resource layer is runtime-owned Rust and adds no commercial PDF SDK.

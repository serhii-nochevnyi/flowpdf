# Phase 5 Plan 05-01 Summary

**Status:** Complete (first local slice; Phase-level closure remains open)
**Completed:** 2026-09-21

## Delivered

- Added a Rust-owned semantic form module that validates the existing closed
  field vocabulary: required values, bounded text, multiline rules, date,
  number, and email hints, checkbox values, radio/select option membership and
  cardinality, and unsupported signature/button values.
- Added stable, code-only validation and projection errors. Authored field
  values are not copied into diagnostics or error strings.
- Added a revision/source-hash-bound `FormWidgetProjection` derived from an
  accepted PDF display list. Grapheme-safe logical anchors resolve through
  UTF-16 source ranges and glyph clusters into deterministic fixed-point
  rectangles, stable widget identities, and derived tab order.
- Added explicit review output for legacy-invalid, target-deleted, and missing
  display-list mappings. No page rectangle, widget state, or PDF object
  reference is written back into `FlowDocument`.
- Added focused integration coverage for every current field kind, validation
  limits, deterministic/reflowed placement, stale identities, and review
  paths.

## Verification

- `cargo fmt --all` — passed.
- `cargo test --locked -p flow-core --test forms -- --nocapture` — 5 tests
  passed, including the required-empty-authored-default regression.
- `cargo clippy --locked -p flow-core --all-targets -- -D warnings` — passed.
- `cargo test --locked -p flow-core` — passed, including the preserved
  `grapheme_conformance.rs` suite (9 tests).

## Scope boundary

This slice does not claim field authoring UI, a distinct durable current-value
record, AcroForm dictionaries or appearances, PDF flattening, external-PDF
form import, voice control, or target-viewer compatibility. Those contracts
remain follow-on Phase 5 work and must continue to consume the Rust projection
and transaction boundary.

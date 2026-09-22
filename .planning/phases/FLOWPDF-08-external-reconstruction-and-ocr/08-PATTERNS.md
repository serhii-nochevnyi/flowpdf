# Phase 8 Patterns

## Reconstruction core

- Keep reconstruction internals private and re-export only closed DTOs,
  reports, candidate FlowDocument data, and accept/review functions.
- Sort text elements by page, fixed-point top coordinate, then fixed-point x
  and source identity. Group only when vertical overlap/line gap and left
  alignment fit bounded thresholds.
- Detect more than one stable horizontal column and return an explicit
  reading-order warning; do not interleave columns by guessed text order.
- Compute confidence in basis points from mapped glyph coverage, ordering
  certainty, and OCR/provider confidence. `reviewRequired` is data, not a UI
  heuristic.

## Provenance and preservation

- Use a distinct `ExternalReconstruction` FlowDocument provenance variant with
  the normalized immutable source hash and reconstruction schema identity.
- Every candidate block maps to page/rectangle/object/operator identities. An
  opaque island maps to one or more scene element identities and remains
  read-only.
- No diagnostics echo PDF text, OCR text, image bytes, URLs, or scripts.

## OCR boundary

- OCR requests are page-scoped, bounded, source-hash-bound, and adapter-owned.
- OCR results contain text, fixed-point rectangles, confidence, and provider
  identity only. The worker validates candidate count, text size, coordinates,
  and source identity before Rust reconstruction.
- Provider absence, cancellation, stale source, malformed result, and low
  confidence are separate visible states.

## Browser/source store

- Store the original bytes under a content-hash key with put-if-absent
  semantics. Return cloned `Uint8Array` values; expose no mutation/delete
  method in the reconstruction surface.
- Keep the source store independent of the semantic document store and never
  include PDF bytes in commands, audits, or candidate FlowDocument JSON.

## Review UI

- Report and review controls are semantic DOM. The original scene remains
  read-only visual/`aria-hidden`; the candidate is a semantic preview with
  confidence/review markers.
- Acceptance is disabled while required review items lack an explicit
  decision, while an opaque source binding is missing, or while source/scene
  identities are stale.
- Never render external links as anchors or invoke PDF actions. Comparison
  status must say “best effort/reconstruction”, never “exact”.

## Verification

- Rust tests cover deterministic grouping, mapped/missing text confidence,
  single-column rejection, opaque-island projection, external provenance,
  OCR review gating, and bounded failure.
- WASM/TypeScript tests cover request/result validation, source/scene identity,
  adapter absence/cancellation, and malformed candidates.
- Chromium tests cover source immutability, side-by-side comparison, report and
  review visibility, disabled acceptance, and accepted candidate state.

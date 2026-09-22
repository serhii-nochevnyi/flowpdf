# Research: browser adapter for Rust/WASM form projection

**Researched:** 2026-09-22

## Current boundary

Plan 05-15 added a closed Rust/WASM endpoint that accepts an opaque nested
layout request and an optional source-bound `FormSessionState`. Rust executes
layout, builds the display list, resolves widget geometry and effective values,
and verifies the complete response. The browser currently has no typed
adapter for that endpoint; `loadWasm` exposes the existing layout and PDF
bridges only, and the editor has no projection snapshot to publish.

The layout scheduler already establishes the browser-side pattern required
here: validate request metadata before starting work, cancel superseded
revision-bound requests, validate every identity before publication, and keep
the accepted result immutable. The PDF worker follows the same string-only
WASM pattern and asks Rust to verify the serialized response before mapping it
to browser DTOs.

## Chosen design

Add `web/src/forms/form-projection.ts` with a versioned browser request/result
catalog, runtime guards for the derived projection envelope, a
`createWasmFormProjectionEngine` adapter, and a revision-aware single-flight
scheduler. The scheduler request carries the exact `LayoutRequestDto` used by
the accepted layout plus its expected layout result hash and an optional
`FormSessionStateDto`. The adapter serializes the layout request into the
Rust-owned `layoutRequestJson` field; it never parses canonical document text,
calculates rectangles, or constructs a PDF plan.

Publication requires all of the following to match: request id, source
revision/hash, nested layout identity, expected layout result hash, Rust
projection identity, and any returned plan identity. Rust's verifier remains
the authority for the serialized response hash and nested projection/plan
hashes. The browser scheduler only publishes the verified derived DTO and
reports bounded codes for stale, malformed, cancelled, or failed work.

## Rejected alternatives

- Deriving widgets from `LayoutResultDto` or DOM boxes: loses display-list
  source mapping and would make CSS measurement a second layout authority.
- Reconstructing `PdfFormPlan` in TypeScript: duplicates Rust validation and
  would turn a derived export plan into browser-authored semantic data.
- Mutating `FormSessionCoordinator` from the projection adapter: session
  values are an input snapshot; fill/clear mutations remain on the existing
  Rust session boundary.
- Publishing a recomputed projection without comparing the accepted layout
  result hash: a late request could show geometry from a different layout
  fingerprint even when the document revision is unchanged.

## Scope fence

This slice provides the browser transport/scheduling seam and focused unit
evidence. It does not render widget overlays, add accessible selection or
flattening controls, change canonical fields, or claim target-viewer or
external-PDF form compatibility.

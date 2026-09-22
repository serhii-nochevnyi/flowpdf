# Research: browser form projection viewport integration

**Researched:** 2026-09-22

## Current boundary

Plan 05-16 provides a typed `RevisionAwareFormProjectionScheduler`, but the
editor shell does not request or render its accepted result. `PageViewport`
already paints accepted Rust layout fragments as a visual-only surface, and
`EditorApp` already owns the accepted layout snapshot plus the source-bound
`FormSessionCoordinator` snapshot. Those are the two inputs needed to request
session-aware projection after layout publication.

The existing page viewport is explicitly `aria-hidden` and must remain a
visual projection. The semantic form controls in the document editor remain
the authoring/accessibility surface. A separate accessible summary can expose
projection readiness, widget identities/pages, and review entries without
duplicating editable controls or claiming that canvas coordinates are an
authoring model.

## Chosen design

Add an opt-in scheduler seam to `EditorApp`: when an accepted layout matches
the current editor revision/hash, the app builds a request with the exact
accepted layout result hash and the current valid form-session snapshot. The
default scheduler lazily loads the typed WASM projection exports; tests may
inject a scheduler fixture. The scheduler snapshot is subscribed through
`useSyncExternalStore` and is cancelled when its source/layout identity is no
longer current.

Extend the visual page viewport with fixed-point widget rectangles from the
verified projection. The overlay is a page-painting detail and stays
`aria-hidden`; it never measures DOM text or handles field mutation. Add an
accessible projection summary beside it that announces pending/error/ready
state, lists valid projected widget identities and pages, and retains explicit
review entries. It has no selection or flattening action yet.

## Rejected alternatives

- Rendering projection from the semantic field cards alone: field cards have
  no accepted page geometry after reflow.
- Treating overlay boxes as form inputs: the canvas is not the semantic DOM or
  IME/accessibility authority.
- Starting projection from a layout request before layout acceptance: the
  browser would publish a projection whose display-list identity was never
  accepted by the layout scheduler.
- Hiding review entries when no plan exists: invalid/deleted/unmapped fields
  must remain explicit for repair and cannot be silently omitted.

## Scope fence

This slice wires verified projection into the editor and renders visual and
read-only accessible derived views. It does not add field selection, flatten
selection persistence, PDF export option wiring, external form import, or
target-viewer evidence.

# Phase 5 Plan 05-11 Research

## Scope

The next vertical slice adds explicit tab-order authoring for valid semantic
fields. The numeric `tab_order` remains derived widget metadata; it is not
added to `FieldDescriptor` and is not persisted as page or PDF state. Rust
reorders the canonical field descriptor vector through one checked transaction,
then the editor and form projection expose the resulting order.

Review-region fields remain read-only. This slice does not add appearance
streams, flattening, external-PDF form import, or target-viewer evidence.

## Existing contracts

- `FlowDocument.fields` is an ordered canonical collection of complete field
  descriptors. `InsertField`, `SetField`, and `RemoveField` already preserve
  descriptor identity and route every change through the transaction/history
  boundary.
- `EditorDocumentViewDto` currently sorts valid cards by semantic anchor
  position, while `FormWidget.tab_order` is assigned after spatial widget
  sorting. Both views need an explicit derived order value without giving the
  browser authority over canonical fields.
- The PDF form plan currently copies widget identity and geometry but has no
  explicit tab-order member. Carrying the derived value through the plan lets
  the owned writer keep deterministic field-array order without claiming
  viewer-specific behavior.
- Form sessions are source revision/hash bound. A successful field reorder is a
  canonical revision, so the existing coordinator must rebind the noncanonical
  session rather than carry values across a new source identity.
- The command catalog, audit projection, recovery allowlist, parity JSON, and
  Phase 2 gate are closed contracts. `MoveField` must be added to all of them
  together.

## Implementation approach

1. Add `MoveField { field_id, target_index }` to the Rust command/mutation
   catalog. `target_index` is the zero-based position among currently valid
   semantic fields, not a DOM index or PDF coordinate. Rust rejects unknown,
   review-region, out-of-range, and no-op requests before publication.
2. Derive an exact `MoveField`/inverse operation from the current canonical
   vector. Replay checks the descriptor preimage at the source index, so undo,
   redo, durable recovery, stale bases, and forged indexes remain atomic.
3. Add a derived `tab_order` to valid editor field DTOs and to the PDF form
   plan. Keep editor card order/geometry deterministic, validate contiguous
   tab-order values, and emit the owned PDF field array in that derived order
   while preserving widget rectangles.
4. Add localized native “move earlier/later” controls to valid field cards.
   The browser sends only the field ID and desired valid-field index through
   `structuralCommand`; it never reorders arrays or rewrites canonical JSON.
   Add Ukrainian/English Chromium coverage for boundaries, revision/history,
   exact order, rebind behavior, and review-control absence.

## Risks and fences

- Review fields may remain in the canonical vector for preservation. The
  command must not move them or expose controls for them; valid-field indexes
  are resolved by Rust, not by browser array assumptions.
- The existing editor card list is spatially ordered for semantic navigation.
  A separate `tabOrder` value avoids changing that accessibility ordering while
  still making the authored keyboard order observable and controllable.
- A field reorder changes canonical bytes and therefore invalidates old form
  session identity. Tests must assert that prior fill overrides do not leak
  into the new source revision.
- `MoveField` is a structural authoring operation, not a PDF appearance or
  target-viewer compatibility claim.

## Verification target

- `cargo fmt --all -- --check`
- focused Rust field, forms, PDF-form, and command-parity tests
- `node --test tests/contracts/phase1-boundary.test.mjs`
- `npm run typecheck`
- focused Chromium form/accessibility suites in both locales
- `npm run check` and `npm run check:phase4`

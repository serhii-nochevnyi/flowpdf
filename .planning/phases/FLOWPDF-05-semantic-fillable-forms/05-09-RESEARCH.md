---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "05-09"
status: ready
---

# Phase 5 Plan 05-09 Research

## Scope

The next vertical slice adds insertion of one default text field at the
current accepted collapsed Rust caret. The new field is immediately visible
through the existing accessible field projection and can be configured through
the completed 05-07 descriptor editor. Field deletion, arbitrary tab ordering,
and appearance/viewer work remain separate plans.

## Existing contracts

- `FieldDescriptor` already contains the complete closed field vocabulary,
  grapheme-safe logical anchor, typed default, options, stable identity, and
  schema validation. A new field can therefore enter canonical state as one
  fully typed descriptor rather than as a browser-side partial object.
- `CommandKind::SetField` is exact and reversible for existing fields. It does
  not admit insertion, so insertion needs a distinct closed command and
  operation with an exact inverse for durable history/recovery.
- Command parity is generated from `MutationFamily::ALL`, checked against the
  checked-in Phase 2 contract, and consumed by audit and future-voice routes.
  `InsertField` must be added to that catalog deliberately; no unlisted
  transaction command is acceptable.
- Editor capabilities are Rust-derived from the accepted selection. The
  insertion affordance should be enabled only for a collapsed selection whose
  endpoints are text positions; the command still revalidates the descriptor
  and anchor before publication.
- Browser code can generate a UUID for a new descriptor and route the typed
  command, but Rust owns uniqueness, anchor validity, schema limits,
  persistence, undo/redo, and recovery.

## Implementation approach

1. Add `InsertField { field }` to the Rust command/mutation catalog and use
   exact `InsertField`/`RemoveField` transaction operations with checked vector
   indexes. Reject duplicate field identity, non-grapheme-safe anchors, and
   malformed descriptors atomically through existing final schema validation.
2. Extend audit command naming, capability metadata, the checked-in parity
   contract, and the Phase 2 validation manifest counts so the new Phase 5
   command remains visible/keyboard/future-voice complete.
3. Add a Rust-derived `insertField` editor capability and a localized
   accessible toolbar action that creates a bounded default text descriptor at
   the accepted caret. The existing descriptor disclosure remains the editing
   surface for changing kind/default/options after insertion.
4. Add Rust transaction/recovery tests and Ukrainian/English Chromium tests
   for insertion, exact anchor/identity preservation, revision/history,
   accessible projection, and rejected non-collapsed/duplicate payloads.

## Risks and fences

- The parity artifact is a closed contract; changing its count and expected
  command list must be done together with Rust catalog and phase-gate checks.
- The browser must never append to `document.fields`, synthesize canonical
  JSON, or infer field uniqueness from rendered cards. UUID generation is only
  an input identifier; Rust remains the validator.
- The first insertion action intentionally creates a text field with a safe
  generated name/label and empty default. Type-specific authoring remains
  available through `SetField`, while field deletion and tab-order semantics
  remain unimplemented.
- A non-collapsed selection or atomic node must not expose an enabled insert
  action, and forged commands must remain canonical no-ops.

## Verification target

- `cargo fmt --all -- --check`
- focused Rust field/command-parity/recovery tests
- `node --test tests/contracts/phase1-boundary.test.mjs`
- `npm run typecheck`
- focused Chromium form/accessibility tests in both locales
- `npm run check` and `npm run check:phase4`

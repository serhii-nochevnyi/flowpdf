---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "10"
status: ready
---

# Phase 5 Plan 05-10 Research

## Scope

The next vertical slice adds explicit, reversible removal of one valid
semantic field. The action is exposed from the accessible field card and
requires an explicit confirmation before the Rust transaction is published.
Fields already placed in the review region remain outside the authoring
surface until a later repair workflow exists. Tab-order authoring,
appearance streams, flattening, external form import, and viewer evidence
remain separate plans.

## Existing contracts

- `FieldDescriptor` entries are canonical vector members with stable IDs,
  validated names, typed defaults, and grapheme-safe or explicit review
  anchors. Removing one descriptor does not require a document-tree anchor
  mapping.
- `Operation::RemoveField` already exists for the inverse of `InsertField`,
  while `Operation::InsertField` currently assumes append-only insertion.
  Exact undo of a field removed from the middle therefore requires allowing a
  checked insertion index in the operation applier; forward `InsertField`
  derivation remains append-only.
- The transaction/recovery path validates the candidate document, records
  privacy-minimized audit facts, and replays an allowlisted command type.
  `RemoveField` must enter every one of those closed mappings.
- The command parity catalog is the shared Rust/WASM contract used by audit,
  boundary checks, and future voice routing. The new destructive command needs
  an explicit confirmation policy and a visible keyboard-reachable field-card
  route.
- `FieldNavigation` already receives the Rust-owned field projection and the
  typed structural command target. It can render a native button and a local
  alert dialog without reading or mutating canonical JSON in the browser.
- Form-session values are bound to the canonical source revision/hash. A
  removal therefore causes the existing coordinator to bind a new source;
  undo also creates a new revision, so it must not be treated as the original
  source identity. The safe observable contract is rebind/no-leak: the field
  returns from its canonical default and an old override is not copied into a
  different source revision.

## Implementation approach

1. Add `RemoveField { field_id, confirmed }` to the Rust command/mutation
   enums. Derive an exact `RemoveField`/`InsertField` pair for the located
   descriptor, reject missing IDs or missing confirmation atomically, and let
   the existing candidate validation and history/recovery machinery publish
   the result.
2. Generalize only the operation applier's checked `InsertField` index from
   `index == len` to `index <= len`; forward insertion still derives at the
   end, while a removal inverse can restore the original vector position.
3. Extend audit naming, capability metadata, checked-in parity JSON, boundary
   expectations, and parity tests from 34 to 35 current shared families.
4. Add localized field-card removal controls and an alert dialog. The browser
   sends only `{ type: 'removeField', fieldId, confirmed: true }` after the
   user confirms; it never filters or rewrites the field array locally. Add
   Ukrainian/English Chromium evidence for confirmation, removal, undo/redo,
   source-bound form-session behavior, and review-field protection.

## Risks and fences

- A forged or unconfirmed payload must not advance revision/history or alter
  the form session.
- The inverse must restore the exact descriptor bytes and original order,
  including when the removed field is not the last vector entry.
- Deleting a review-region field would hide unresolved source content; keep
  that region read-only until a dedicated repair contract is implemented.
- The confirmation is a safety boundary, not a browser-only convention:
  Rust rejects `confirmed: false` for every modality.

## Verification target

- `cargo fmt --all -- --check`
- focused Rust field-authoring and command-parity tests
- boundary/parity contract tests
- `npm run typecheck`
- focused Chromium form/accessibility suites in both locales
- `npm run check` and `npm run check:phase4`
- `npm run check:phase2` after refreshing its current 35-family contract

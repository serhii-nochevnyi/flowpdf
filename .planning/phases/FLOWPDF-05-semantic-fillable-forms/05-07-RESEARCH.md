---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "05-07"
status: ready
---

# Phase 5 Plan 05-07 Research

## Scope

The next small vertical slice is descriptor configuration for fields that are
already present in a valid FlowDocument. It is intentionally not field
insertion, deletion, placement, or PDF appearance work. Those operations need
separate anchor/UX contracts and remain follow-on plans.

## Existing contracts

- `FieldDescriptor` is canonical Rust model data. It owns the stable field ID,
  name, label, grapheme-safe anchor, closed field kind, required/read-only
  flags, typed default value, and bounded options.
- `CommandKind::SetField` already translates to a reversible transaction. Rust
  requires the field to exist, preserves its ID, accepts only a grapheme-safe
  anchor, validates the resulting document through the normal schema path, and
  records an inverse `Operation::SetField` for undo/recovery.
- `EditorController.structuralCommand` is the existing browser command bus. It
  persists the Rust-planned commit and publishes the accepted editor view; the
  browser must not mutate canonical JSON or editor view objects directly.
- `FieldNavigation` receives accepted Rust field projections and the
  `InputCommandTarget`. Valid cards are already separated from review cards,
  and Plan 05-06 established native value controls backed by the noncanonical
  Rust form session.
- An accepted descriptor edit changes canonical revision/hash. The existing
  form-session coordinator therefore naturally drops the prior noncanonical
  fill state and starts a source-bound session for the new accepted source.

## Implementation approach

1. Admit `setField` in the TypeScript structural command DTO without creating
   a second browser-side mutation path.
2. Add an accessible disclosure/form to each valid field card. Keep draft state
   local to the editor until submit, then construct one typed descriptor that
   preserves the Rust-provided ID and anchor and dispatch it through
   `structuralCommand`.
3. Cover all current descriptor variants: text hint/multiline settings,
   checkbox, radio group, single/multiple select, signature, and button. For
   option-bearing fields expose bounded option label/export-value editing and
   typed default selection; generate UUID option IDs only for newly added UI
   options.
4. Keep review cards without configuration controls and keep invalid drafts
   canonical-no-op by relying on Rust schema rejection. Browser evidence will
   assert accepted edits advance revision, invalid edits do not, and field
   values/session controls remain source-bound after a successful edit.

## Risks and fences

- A descriptor edit is canonical and must be persisted through the existing
  controller queue; no optimistic DOM or form-session update is allowed.
- The editor must preserve the original anchor and field ID. Changing either
  would be a different authoring operation and could orphan widget identity.
- Empty/duplicate option IDs or export values and incompatible defaults must
  fail in Rust rather than being silently repaired by the UI.
- This slice does not claim creation of a new semantic field, field deletion,
  arbitrary anchor selection, tab-order authoring, AcroForm appearances,
  viewer compatibility, flattening, or external form import.

## Verification target

- `cargo test --locked -p flow-core --test field_accessibility`
- `npm run typecheck`
- focused Chromium form/accessibility tests covering both locales
- full `npm run check` after the focused slice is green

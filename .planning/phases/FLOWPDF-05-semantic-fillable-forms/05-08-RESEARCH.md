---
phase: FLOWPDF-05-semantic-fillable-forms
plan: "05-08"
status: ready
---

# Phase 5 Plan 05-08 Research

## Scope

The next small vertical slice is accessible anchor placement for fields that
are already present and valid in a FlowDocument. It uses the existing Rust
`SetField` transaction with a new grapheme-safe anchor and intentionally does
not add field insertion, deletion, tab-order authoring, or PDF appearance work.

## Existing contracts

- `FieldDescriptor` owns the field identity, descriptor metadata, and
  `FieldAnchorState`; the browser must preserve the ID while changing only the
  accepted anchor through the existing command bus.
- `EditorSessionState.selection` is a Rust-validated, grapheme-safe logical
  selection. A collapsed accepted selection can therefore be used as an
  anchor candidate without browser-side UTF-16 boundary reconstruction.
- `CommandKind::SetField` already requires the field to exist, preserves the
  field ID, accepts only `GraphemeSafe` anchors, validates the full document,
  and records an exact inverse operation for undo/recovery.
- The source-bound form-session coordinator observes accepted editor snapshots.
  An accepted anchor move changes canonical revision/hash and therefore
  naturally rebinds the noncanonical session without copying values into the
  canonical descriptor.
- Valid field cards already receive the `InputCommandTarget`; review cards are
  deliberately rendered without authoring controls.

## Implementation approach

1. Add a localized “place at caret” action to the existing valid-field
   descriptor disclosure. Read the current accepted editor selection from the
   command target, require it to be collapsed, and construct a typed
   `GraphemeSafe` anchor from the Rust-projected logical position.
2. Preserve every descriptor property, including field ID, name, options,
   default, flags, and kind; dispatch exactly one `setField` command. Do not
   patch the field card, canonical JSON, session values, or DOM selection.
3. Add stable browser evidence in Ukrainian and English for a successful move,
   identity/descriptor preservation, revision advancement, session rebinding,
   and a non-collapsed-selection no-op. Keep review cards outside the action.

## Risks and fences

- The action must use an accepted Rust selection, never `window.getSelection()`
  or a DOM offset, because canvas/semantic DOM coordinates are not canonical.
- A non-collapsed selection must be disabled and must not issue a malformed
  command. Rust remains the final validator for forged or stale payloads.
- Moving a field is not insertion or deletion. New field lifecycle commands,
  explicit destructive confirmation, and tab-order semantics remain separate
  follow-on contracts.

## Verification target

- `npm run typecheck`
- focused Chromium form/accessibility suites in both locales
- `cargo test --locked -p flow-core --test field_accessibility`
- `npm run test:browser`
- `npm run check` after the focused slice is green

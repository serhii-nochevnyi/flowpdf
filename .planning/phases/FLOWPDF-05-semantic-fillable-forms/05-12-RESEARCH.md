# Phase 5 Plan 05-12 Research

## Scope

The next vertical slice adds deterministic derived AcroForm appearance
streams to the owned PDF writer. Appearance state is export-time PDF data: it
must consume the accepted `PdfFormPlan`, never enter `FlowDocument`, form
session persistence, or transaction history, and must remain separate from
target-viewer compatibility and flattening.

## Existing contracts

- `PdfFormPlan` already carries validated field kind, flags, default/current
  value, options, fixed-point rectangle, page, and canonical tab order.
- `emit_form_objects` already owns field dictionaries, widget annotations,
  page `/Annots`, and the catalog `/AcroForm` reference. It currently emits no
  `/AP`, `/DR`, `/DA`, or `/NeedAppearances` entries.
- The owned COS layer provides bounded dictionaries, arrays, names, strings,
  references, and streams with deterministic `BTreeMap` serialization and
  stream/output ceilings.
- Form values are already converted to bounded PDF text/name values. Checkbox,
  radio, and select option names are Rust-derived and PDF-name validated; text
  values remain bounded source strings.
- The export fingerprint already includes the complete form plan, so derived
  appearance bytes remain bound to the accepted source/session identity even
  though appearance implementation details are not canonical document state.

## Findings

1. Appearance resources can be deterministic without adding a second semantic
   field model: one built-in Type1 Helvetica resource can be referenced by the
   AcroForm default appearance and each text/choice stream. This is a bounded
   structural appearance contract, not a claim that built-in-font viewers
   render every Ukrainian or arbitrary Unicode value correctly.
2. Text-like fields need one normal stream containing a fixed border/background
   and a bounded hex-encoded PDF text operand. Checkboxes and radio groups need
   a normal appearance state dictionary containing `/Off` plus every admitted
   on-state; the widget `/AS` remains the Rust-derived selected name. Push
   buttons and signatures receive a deterministic normal stream without an
   editable value state.
3. `/NeedAppearances false` makes the writer's explicit streams authoritative
   for this bounded export. The writer must not emit JavaScript, actions, or
   viewer-specific recovery instructions.
4. Object insertion order must be fixed: shared appearance font, field/widget
   objects in canonical tab order, then the AcroForm dictionary. Stream
   content must use checked fixed-point geometry and bounded text encoding; no
   raw user value may become a PDF operator or name token.
5. Existing PDF reference/viewer rows remain unavailable. Tests can prove
   deterministic bytes, explicit AP structure, state coverage, source/session
   identity, and fail-closed limits, but must not label a target viewer as
   compatible from byte inspection alone.

## Design decisions

- Keep `PdfFormPlan` unchanged; appearance streams are derived during COS
  emission and are not serialized into the plan hash or canonical document.
- Add only internal helpers/error mapping needed to build bounded appearance
  streams. Reuse the existing `CosValue::stream`, `PdfName`, `LayoutRect`, and
  `PdfFormValue` validation paths.
- Use deterministic `/Helv` resource metadata and explicit `/DA`/`/DR` on the
  AcroForm. Keep the appearance content intentionally small: border,
  background, and value/state marks only.
- Encode text operands as bounded uppercase hexadecimal PDF strings with a
  UTF-16BE BOM. This preserves a deterministic source representation and
  prevents operator injection; it does not claim full Unicode glyph coverage
  until subset-font resources are wired into form appearances.

## Verification target

- `cargo fmt --all -- --check`
- focused `cargo test --locked -p flow-core --test pdf_forms`
- full `npm run check`
- `npm run check:phase4`
- `git diff --check`

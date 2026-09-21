# Phase 4 Patterns

## Rust boundary

- Keep canonical source in `flow-core::model`; derived export types belong in a
  dedicated `flow-core::pdf` module and are re-exported only as closed DTOs.
- Validate revision, canonical hash, layout result hash, font catalog identity,
  and export options before building any output bytes.
- Return stable error/diagnostic codes and no partial PDF bytes on failure.
- Use `serde(rename_all = "camelCase", deny_unknown_fields)` for serialized
  request/result DTOs, matching `layout` and `editor_view`.

## Deterministic serialization

- Use owned `Vec<u8>` buffers, checked length limits, deterministic object
  numbering, and canonical decimal/escape helpers.
- Never use `HashMap` iteration order in serialized output; sort resource IDs and
  preserve canonical source ordering.
- Keep PDF byte generation independent from browser APIs and host fonts.

## Browser projection

- Follow `web/src/layout/layout-protocol.ts` and `layout-worker.ts` for
  request/revision/hash guards and single-flight scheduling.
- Reuse `semantic-document.tsx` as the accessible/input truth. A PDF/page
  canvas or SVG projection must be `aria-hidden` and never duplicate editable
  content as an alternative DOM truth.
- Publish accepted export/preview state atomically through `editor-store.ts`.

## Verification

- Add focused Rust tests next to the module and browser tests under
  `web/tests/`.
- Extend the phase gate from `scripts/check-phase3.mjs` rather than silently
  weakening Phase 1/2/3 gates.
- Preserve the distinction between local structural proof and unavailable
  qpdf/Poppler/target-viewer evidence.

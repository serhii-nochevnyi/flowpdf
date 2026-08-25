# Phase 2: Accessible Rich-Text Editing - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-08-25
**Phase:** 2-Accessible Rich-Text Editing
**Areas discussed:** Input, selection, and IME; Semantic structure and transformations; Formatting controls and editor shell; Accessible navigation and embedded blocks

The user explicitly authorized autonomous use of recommended answers. Every selected option below is the recommended default chosen under that authorization.

---

## Input, Selection, and IME

| Option | Description | Selected |
|--------|-------------|----------|
| Controlled input host plus synchronized semantic DOM | Keep browser input/accessibility functional while Rust and logical positions remain authoritative. | ✓ |
| `contenteditable` DOM as truth | Persist browser DOM structure and positions. | |
| Canvas-only editing | Paint text and emulate input/accessibility without a semantic DOM. | |

**User's choice:** Controlled input host plus synchronized semantic DOM.
**Notes:** Use one directional logical selection, validate pinned Unicode grapheme boundaries in Rust, and commit only final IME composition as one transaction.

---

## Semantic Structure and Transformations

| Option | Description | Selected |
|--------|-------------|----------|
| Typed block tree plus inline runs/marks | Model each required semantic structure explicitly with stable identities and migrate the canonical schema. | ✓ |
| Flat blocks with ad hoc fields | Preserve the Phase 1 flat list and encode lists/tables/formatting indirectly. | |
| HTML-like snapshot | Persist browser-oriented markup as document structure. | |

**User's choice:** Typed block tree plus inline runs/marks.
**Notes:** Paragraph, heading, list, image, table, and page-break behavior receives explicit split/merge rules; atomic blocks never merge into text implicitly.

---

## Formatting Controls and Editor Shell

| Option | Description | Selected |
|--------|-------------|----------|
| Page-neutral word-processor surface | Use a centered continuous editor, persistent app bar, formatting toolbar, insert menu, and visible statuses until real layout arrives. | ✓ |
| Simulated pages | Draw page-looking boxes before deterministic pagination exists. | |
| Developer inspector | Keep the Phase 1 diagnostic command surface as the primary UI. | |

**User's choice:** Page-neutral word-processor surface.
**Notes:** Formatting is selection-aware, mixed values are indeterminate, every mutation has keyboard and visible-control parity, and toolbar actions preserve the logical selection.

---

## Accessible Navigation and Embedded Blocks

| Option | Description | Selected |
|--------|-------------|----------|
| Native synchronized semantic tree | Expose headings, lists, tables, images, page breaks, statuses, and errors through native semantics. | ✓ |
| Canvas with ARIA overlay | Approximate document semantics over a visual-only editor. | |
| Plain-text mirror | Flatten structures into a separate screen-reader representation. | |

**User's choice:** Native synchronized semantic tree.
**Notes:** Images require alt text or an explicit decorative choice; simple tables get semantic cells and bounded controls; page breaks are labeled focusable atomic separators.

## the agent's Discretion

- React component boundaries and styling details inside the later UI-SPEC.
- Vetted Unicode grapheme library choice after compatibility, WASM-size, and provenance checks.
- Exact bounded limits for assets, tables, marks, nesting, composition, and batch commands.
- Diagnostics route versus collapsible diagnostics panel.

## Deferred Ideas

- Typography, BiDi, line breaking, pagination, page canvas, and reflow.
- Advanced image/table tooling and rich HTML clipboard import.
- Forms, PDF import/export, voice capture, and collaboration.

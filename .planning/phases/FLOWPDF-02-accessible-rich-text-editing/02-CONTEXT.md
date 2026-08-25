# Phase 2: Accessible Rich-Text Editing - Context

**Gathered:** 2026-08-25
**Status:** Ready for planning

<domain>
## Phase Boundary

This phase delivers an accessible browser rich-text editor over the Rust-owned FlowDocument: one logical caret/selection model, Ukrainian and English keyboard/IME editing, deterministic paragraph split/merge behavior, semantic block creation, inline and block formatting, keyboard/visible-control parity, and a synchronized semantic accessibility tree. It does not deliver shaping, BiDi layout, line breaking, pagination, page-canvas rendering, PDF generation/import, fillable-field authoring, or voice capture; those remain downstream phases.

</domain>

<decisions>
## Implementation Decisions

### Input, Selection, and IME

- **D-01:** The editor exposes one contiguous directional selection with `anchor` and `focus` endpoints expressed only as stable node ID, UTF-16 offset, and affinity. DOM paths, DOM ranges, absolute character indices, and pixel coordinates remain view-local and are never persisted. — **Reversibility:** costly — keyboard, UI, future voice, fields, source maps, and WASM DTOs depend on this position contract.
- **D-02:** A pinned Unicode grapheme segmenter in Rust is the authority for valid caret and edit boundaries. Normal keyboard/pointer navigation lands only on valid grapheme boundaries; malformed API/WASM targets return a stable error instead of silently snapping or splitting a cluster.
- **D-03:** Browser input uses a controlled input host together with a synchronized, visible semantic DOM. The DOM is projected from the last accepted Rust revision; `contenteditable`/DOM mutations are not document truth, and every committed change passes through the typed command service.
- **D-04:** IME composition is transient, visibly previewed, and noncanonical. `compositionend` produces one atomic transaction, cancellation produces none, and a changed base revision invalidates the composition bookmark rather than relocating text. Plain-text paste uses the same input path; arbitrary clipboard HTML is not imported as canonical structure in this phase.

### Semantic Structure and Transformations

- **D-05:** Evolve the canonical schema through an explicit migration to a typed semantic block tree with stable IDs plus inline text leaves/runs and marks. HTML or browser DOM snapshots are prohibited as canonical content. — **Reversibility:** one-way — once schema v2 documents are persisted, the sequential migration and canonical byte contract must remain supported.
- **D-06:** First-class block kinds are paragraph, heading, ordered list, unordered list, list item, image, simple table, table row, table cell, and explicit page break. Lists and tables use semantic nesting; they are not encoded as styled text or opaque payloads.
- **D-07:** Bold, italic, underline, font family, font size, color, and language are inline range marks/attributes. Alignment, paragraph spacing, heading level, and list semantics are block attributes. A collapsed selection carries pending typing attributes; a mixed selection exposes an indeterminate formatting state.
- **D-08:** Split and merge rules are explicit and undoable: paragraphs preserve compatible style; a mid-heading split preserves heading semantics while Enter at the end creates a body paragraph; Enter in a list creates a sibling item and an empty item exits the list; adjacent compatible text blocks merge while preserving inline marks. Images, tables, and page breaks never merge implicitly.
- **D-09:** The document always retains an editable paragraph when its last editable block would otherwise be removed. Atomic blocks are selected or removed through explicit commands and visible controls, never by corrupting neighboring text ranges.

### Formatting Controls and Editor Shell

- **D-10:** The Phase 2 UI is a page-neutral, centered word-processor surface. It has a persistent application bar for document identity/durability and undo/redo, a selection-aware formatting toolbar, a labeled insert menu, the document surface, and visible status/error regions. Simulated pages wait for the real Phase 3 layout engine.
- **D-11:** Every Phase 2 mutation has both a visible labeled control and a keyboard path. Standard undo/redo and bold/italic/underline shortcuts, text input, structural Enter/Backspace behavior, toolbar actions, and insert actions all compile to the same typed command bus. Toolbar use preserves the logical selection and returns focus to the editor.
- **D-12:** The React/TypeScript shell owns event translation, focus, physical browser I/O, localization, and rendering only. Rust owns selection validation, semantic mutations, migrations, transaction construction, undo/redo, and canonical serialization. The Phase 1 Foundation Inspector remains available as secondary diagnostics/test support rather than the primary product surface.

### Accessible Navigation and Embedded Blocks

- **D-13:** The synchronized document tree uses native semantic elements for headings, paragraphs, ordered/unordered lists, tables, images, and page-break separators inside one labeled editor region. It is keyboard navigable and screen-reader readable without canvas or an ARIA-only replica; statuses use a polite live region and failures use atomic alerts with visible text.
- **D-14:** Image insertion accepts bounded PNG/JPEG assets through a visible file control, requires useful alt text or an explicit decorative choice, renders an inline preview, and exposes visible replace/remove actions. Cropping, freeform resizing, floating, and text wrap are deferred.
- **D-15:** Simple tables are bounded semantic tables with editable cells, an optional header row, keyboard cell navigation, and visible row/column add/remove controls. A page break is a labeled, focusable atomic separator; it carries semantic intent now and receives physical pagination behavior only in Phase 3.
- **D-16:** Ukrainian remains the default UI locale with key-identical English resources. Accessibility names, formatting state, live statuses, validation errors, and shortcuts are localized; document language metadata is distinct from interface locale.

### the agent's Discretion

- Exact React component boundaries, CSS class names, and token values, provided the UI-SPEC and Phase 1 accessibility patterns are preserved.
- The vetted Rust Unicode grapheme library selected after official-data compatibility, WASM size, and dependency-provenance checks; Unicode algorithms are reused, not reimplemented.
- Concrete bounded limits for image bytes, table dimensions, nesting, mark count, composition text, and command batch size, provided research documents the rationale and tests all failure codes.
- Whether the diagnostic inspector is exposed as a development-only route or a collapsible diagnostics panel, provided Phase 1 recovery/audit behavior remains testable.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Product and Phase Scope

- `.planning/PROJECT.md` — Product boundary, Rust/WASM and React constraints, accessibility contract, and explicit exclusions.
- `.planning/REQUIREMENTS.md` — EDIT-01..05 and QUAL-03..04 plus milestone-wide acceptance criteria.
- `.planning/ROADMAP.md` — Phase 2 goal, dependencies, success criteria, and the Phase 3 layout boundary.

### Architecture and Risk

- `.planning/research/ARCHITECTURE.md` — Shared command boundary, canonical-model separation, web-editor responsibilities, and downstream layout flow.
- `.planning/research/STACK.md` — Pinned Rust/WASM/React stack, Unicode adapter policy, hidden input host, semantic DOM, and test stack.
- `.planning/research/PITFALLS.md` — Grapheme/index hazards, canvas/IME accessibility failure mode, and stable-anchor constraints.
- `.planning/research/SUMMARY.md` — Writer-first sequence and editable-flow vertical-slice intent.

### Prior Locked Decisions

- `.planning/phases/FLOWPDF-01-durable-flow-foundation/01-CONTEXT.md` — Canonical JSON, typed command bus, UTF-16 logical positions, atomic transactions, and Rust/TypeScript ownership split.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets

- `crates/flow-core/src/anchor/mod.rs`: explicit UTF-16/native offset types and deterministic anchor mappings provide the base for a grapheme-safe selection layer.
- `crates/flow-core/src/transaction/mod.rs`: typed commands, immutable operations, batches, stale-revision checks, undo/redo, and forward/inverse replay are the mandatory mutation path.
- `crates/flow-wasm/src/lib.rs`: narrow serialized DTO functions keep Rust authoritative without exposing mutable core handles to JavaScript.
- `web/src/foundation-inspector.ts`: localized native controls, keyboard undo/redo, live status/alert regions, focus management, durable reload, and privacy-safe diagnostics are reusable browser patterns.
- `web/src/i18n/uk.ts` and `web/src/i18n/en.ts`: key-identical localization pattern already validated in Chromium.

### Established Patterns

- `ContentNode` currently supports only flat paragraph/image nodes and whole-node `style_id`; Phase 2 requires a schema migration rather than ad hoc fields.
- Text operations currently resolve ranges inside one paragraph and validate UTF-16 scalar boundaries; cross-block selection, structural transforms, inline marks, and grapheme validation must extend the Rust command model.
- Browser actions submit immutable DTOs and publish only Rust-verified revisions after persistence; the editor must preserve that ownership and durability rule.
- The current web build is framework-free TypeScript with a custom static build. Introducing the project-mandated React/Vite shell must retain deterministic pinned dependencies and the existing test/provenance gates.

### Integration Points

- Extend `flow-core` model/schema/anchor/transaction modules first, expose closed request/response DTOs through `flow-wasm`, then bind them to a React editor controller and semantic view.
- Keep IndexedDB as physical storage and reuse the Phase 1 create/open/apply/undo/redo/commit/recover lifecycle rather than creating a second editor state store in TypeScript.
- Preserve Phase 1 recovery, audit, provenance, localization, and browser tests while adding editor-focused Rust, unit, accessibility, IME, and Chromium coverage.

</code_context>

<specifics>
## Specific Ideas

- The vertical tracer should open a durable Ukrainian/English document, place a caret around combining marks and emoji, compose text through IME, apply a mark, split/merge a paragraph, insert a list/table/image/page break, undo/redo, reload, and recover the same canonical semantics.
- Unicode fixtures should cover Ukrainian apostrophes, decomposed combining sequences, surrogate pairs, emoji ZWJ sequences, regional indicators, punctuation, and mixed Ukrainian/English runs even though visual shaping and BiDi layout are Phase 3.
- Page breaks are shown as semantic separators in a continuous flow editor, making the phase honest about not yet having pagination.

</specifics>

<deferred>
## Deferred Ideas

- Glyph shaping, font fallback, BiDi ordering, language-aware line breaking, hyphenation, caret geometry from fragments, document-wide reflow, and page virtualization — Phase 3.
- Image crop/resize handles, floating/wrapping images, complex tables, merged cells, nested tables, and table fragmentation — later layout/editor expansion.
- Rich HTML/office clipboard import, comments, track changes, collaborative editing, and spell/grammar services — later product phases or backlog.
- Fillable-field authoring, PDF import/export, and voice capture/recognition remain their dedicated roadmap phases; Phase 2 only establishes mutation parity and reusable selection bookmarks.

</deferred>

---

*Phase: 2-Accessible Rich-Text Editing*
*Context gathered: 2026-08-25*

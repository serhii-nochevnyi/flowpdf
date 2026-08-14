# Architecture Patterns

**Domain:** Flow/PDF document platform  
**Researched:** 2026-08-14

## Recommended Architecture

```text
Input adapters (keyboard, IME, UI, voice, API)
    -> typed command bus
    -> immutable FlowDocument transactions
    -> computed style/box tree
    -> fragment tree + break tokens
    -> page display list
       -> browser renderer
       -> owned PDF writer

PDF bytes
    -> revision-aware COS object graph
    -> content stream interpreter
    -> PDF scene/display list with provenance
       -> native local PDF editor
       -> semantic reconstruction + confidence
          -> FlowDocument
```

## Canonical Models

| Model | Responsibility | Must not own |
|-------|----------------|--------------|
| FlowDocument AST | Semantics, styles, fields, assets, stable logical positions | Page coordinates and PDF object numbers |
| Resolved box tree | Computed styles and layout constraints | Persistent user data |
| Fragment tree | Lines, split blocks, pages, continuation tokens, caret geometry | Editing semantics |
| Display list | Renderer-neutral glyph/image/path/widget commands | Source document authority |
| PDF COS graph | Byte-oriented indirect objects, revisions, streams and references | Reflow semantics |
| PDF scene | Interpreted page graphics with operator provenance | Assumed paragraph or reading-order truth |

## Component Boundaries

| Component | Responsibility | Communicates With |
|-----------|----------------|-------------------|
| `flow-model` | Versioned AST, validation, migrations, stable IDs | transactions, persistence, layout |
| `transactions` | Typed mutations, preconditions, inverse operations | UI, voice, flow-model |
| `text-layout` | segmentation, BiDi, fallback, shaping, line opportunities | font service, page-layout |
| `page-layout` | boxes, fragmentation, pagination, incremental invalidation | text-layout, fragment-tree |
| `display-list` | Stable renderer-neutral paint commands and source maps | browser renderer, PDF writer |
| `pdf-syntax` | Lexer, parser, COS values, xref and lazy resolver | pdf-content, pdf-writer |
| `pdf-content` | graphics-state interpreter and PDF scene | pdf-reader, conversion, native editor |
| `pdf-writer` | deterministic serialization, fonts, resources, forms | display-list, forms |
| `conversion` | owned round-trip and external reconstruction | FlowDocument, PDF scene, OCR |
| `forms` | semantic fields, validation, anchors and widget resolution | FlowDocument, page-layout, PDF writer |
| `web-editor` | input host, page viewport, panels and accessibility DOM | WASM bridge and command bus |

## Core Patterns

### Writer-First Vertical Slice

Implement a complete authoring and export workflow before accepting arbitrary PDF syntax. This validates document semantics, typography, pagination, display-list design, font embedding, forms, and interoperability with controlled inputs.

### Break Tokens and Fixed-Point Repagination

Each layout operation receives an incoming continuation token and returns a fragment plus outgoing token. After an edit, repaginate from the first dirty page and stop when the outgoing token and carry-state signature match the prior layout.

### Provenance Everywhere

Maintain mappings in both directions:

```text
Flow text range <-> glyph cluster <-> fragment rectangle
PDF scene element <-> operator byte range <-> object/revision
Imported Flow node <-> source page/object/bounding boxes/confidence
```

### Controlled Loss

Every conversion returns a structured report. Unsupported content is explicit and preserved as an immutable island; low-confidence semantic inference requires review.

### Shared Command Boundary

The voice layer, editor toolbar and keyboard never mutate internal models directly. Commands are schema-validated, revision-checked, idempotent, undoable, and auditable.

## Data and Revision Flow

```text
User edit
  -> Command(documentRevision, targetAnchor, arguments)
  -> Transaction + inverse
  -> FlowDocument revision
  -> dirty-range calculation
  -> incremental layout
  -> page/display-list patches
  -> viewport update
  -> durable snapshot/event log
```

Export uses an immutable FlowDocument revision. A background worker completes full layout, validates the display list, emits PDF, then runs structural, extraction, form and visual checks.

## Security Boundaries

- PDF parsing, stream expansion, fonts, images and OCR are untrusted worker tasks.
- All loops need object, recursion, decoded-byte, pixel, page, time and memory limits.
- Active PDF JavaScript, launch actions, remote resources and embedded executables are disabled.
- Strict parse and recovery parse are separate modes; recovery is never silent.
- Native codecs execute in a restricted process or WASM sandbox where practical.
- Redaction requires a full rewrite and post-save search/extraction validation.

## Build Order

1. Flow schema and transactions.
2. Text and page layout with a display-list contract.
3. Browser editor and writer-first PDF export.
4. Forms, exact owned round-trip and voice command bus.
5. PDF syntax reader and page scene.
6. External semantic reconstruction and OCR.
7. Native PDF editing and broader compatibility.

## Sources

- [ISO 32000-2 PDF specification](https://pdfa.org/resource/iso-32000-2/)
- [qpdf object-stream design](https://qpdf.readthedocs.io/en/11.9/object-streams.html)
- [HarfBuzz shaping model](https://harfbuzz.github.io/what-is-harfbuzz.html)
- [Unicode Bidirectional Algorithm](https://unicode.org/reports/tr9/)
- [CSS Fragmentation](https://www.w3.org/TR/css-break-3/)


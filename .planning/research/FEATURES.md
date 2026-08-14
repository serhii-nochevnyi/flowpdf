# Feature Landscape

**Domain:** Semantic flow editor with owned PDF import/export and voice control  
**Researched:** 2026-08-14

## Table Stakes

| Feature | Why Expected | Complexity | First-milestone policy |
|---------|--------------|------------|------------------------|
| Rich text editing | Core authoring behavior | High | Paragraphs, headings, lists, inline marks |
| Document-wide reflow | Central product promise | Very high | Single-column deterministic pagination first |
| Undo/redo and autosave | Basic data safety | Medium | Transaction-level history and snapshots |
| PDF preview/export | Required delivery format | Very high | Owned writer for controlled documents |
| Selectable/searchable Unicode text | Interoperability and accessibility | High | Embedded fonts plus ToUnicode |
| Fillable fields | Explicit product requirement | High | Semantic fields mapped to AcroForm widgets |
| Page navigation and zoom | Required for long documents | Medium | Virtualized page viewport |
| External PDF viewing | Expected of a PDF product | Very high | Controlled reader subset, unsupported-content report |
| External PDF import | Needed for migration | Very high | Reconstruction with confidence, never silent guessing |
| Keyboard and accessible operation | Voice cannot be the only modality | High | Input host plus semantic DOM |
| Voice dictation and commands | Explicit differentiator | High | Push-to-talk, explicit dictation/command modes |

## Differentiators

| Feature | Value Proposition | Complexity | Policy |
|---------|-------------------|------------|--------|
| Exact round-trip for FlowPDF-generated PDF | Users can distribute PDF without losing editable source | Medium | Associated source payload or content-addressed source |
| Semantic and fidelity modes | Honest choice between reflow and page preservation | High | Explicit UI mode and conversion report |
| Confidence-scored import | Makes lossy reconstruction inspectable | High | Per-node provenance and review flags |
| Semantic form anchors | Fields move correctly after text reflow | High | Resolve widgets only after final pagination |
| Unified command bus | UI, keyboard, voice and API have identical undo/security behavior | Medium | All mutations are typed transactions |
| Opaque-island preservation | Complex PDF remains visually safe instead of corrupted | Very high | Preserve raw/scene fragment and mark read-only |
| Deterministic shared browser/PDF layout | Preview closely matches exported document | Very high | One layout/display-list core |

## Anti-Features

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|--------------------|
| Pretend arbitrary PDF import is lossless | Semantic information may not exist | Report confidence and preserve original |
| Reflow raw PDF operators globally | Operators are page painting commands, not document flow | Convert to FlowDocument and regenerate |
| Cover deleted text with a white rectangle | Old content remains searchable and recoverable | Rewrite/securely redact through a full save |
| Make HTML or canvas the canonical document | Runtime layout and positions are unstable | Own versioned AST and logical anchors |
| Commit partial speech recognition to the document | Interim STT changes and may hallucinate | Commit only final segments as transactions |
| Depend on PDF JavaScript for validation | Viewer behavior is inconsistent and often disabled | Keep validation in FlowDocument/application schema |
| Build all PDF features before user value | Compatibility surface is effectively unbounded | Versioned supported subset plus opaque fallback |

## Feature Dependencies

```text
Flow schema
  -> transactions and stable anchors
  -> text layout
  -> fragmentation and pagination
  -> display list
  -> browser preview
  -> owned PDF writer
  -> forms and exact owned round-trip

PDF syntax/parser
  -> page interpreter and PDF scene
  -> text/provenance extraction
  -> external PDF reconstruction
  -> native local editing

Command bus
  -> keyboard/UI editing
  -> voice dictation and commands
```

## MVP Recommendation

Prioritize the writer-first vertical slice:

1. Versioned FlowDocument, transactions and undo.
2. Ukrainian/English paragraph layout and deterministic pagination.
3. Shared display list and browser preview.
4. Owned PDF writer with selectable Cyrillic text.
5. Semantic fields mapped to basic AcroForm widgets.
6. Exact source recovery for owned PDFs.
7. Push-to-talk voice integration through the command bus.

Defer broad external PDF import until the owned flow-to-PDF invariant is proven.

## Sources

- [CSS Paged Media Level 3](https://www.w3.org/TR/css-page-3/)
- [CSS Fragmentation Level 3](https://www.w3.org/TR/css-break-3/)
- [Unicode Line Breaking Algorithm](https://unicode.org/reports/tr14/)
- [Unicode Text Segmentation](https://unicode.org/reports/tr29/)
- [PDF 2.0 Associated Files application note](https://pdfa.org/wp-content/uploads/2018/10/PDF20_AN002-AF.pdf)


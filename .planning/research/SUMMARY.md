# Project Research Summary

**Project:** FlowPDF  
**Domain:** Semantic flow editor, PDF engine, conversion and voice control  
**Researched:** 2026-08-14  
**Overall confidence:** HIGH for system boundaries and build order; MEDIUM for exact Rust text/font adapters until benchmarked

## Executive Summary

FlowPDF is feasible as an owned product when it is treated as a document platform with multiple explicit representations. FlowDocument is the semantic source of truth; the layout engine derives boxes and fragments; a shared display list drives browser preview and deterministic PDF generation; the PDF reader retains a separate byte-oriented object graph and page scene. Collapsing these layers would make either reflow or PDF fidelity unreliable.

The strongest implementation order is writer-first. A controlled `FlowDocument -> layout -> PDF` path proves rich editing, Unicode shaping, pagination, font embedding, forms, rendering parity, and exact owned round-trip before arbitrary external PDF complexity is introduced. External PDF import is a later reconstruction pipeline with confidence and provenance, not a guaranteed inverse transformation.

Rust with native and WebAssembly targets is the recommended core. A small TypeScript/React shell owns browser integration, while trusted standards primitives handle Unicode, OpenType, fonts, compression, images and cryptography. The project owns the document algorithms, PDF object semantics, conversion policy and validation gates rather than duplicating low-level codecs.

The dominant risks are typography parity, incremental reflow correctness, malformed-input security, font mapping, external-PDF reading order, form appearance interoperability, canvas accessibility, and scope expansion toward universal PDF support. These risks require executable invariants and golden corpora from the first phase.

## Key Findings

**Stack:** Rust stable pinned through `rust-toolchain.toml`, WASM through `wasm-bindgen`, React/TypeScript/Vite for the web shell, Unicode/HarfBuzz-compatible Rust primitives behind adapters, and an owned PDF parser/writer.  
**Architecture:** Flow AST, box/fragment tree, display list, PDF object graph and PDF scene are distinct models joined by versioned contracts.  
**Critical pitfall:** Attempting arbitrary PDF semantic editing before proving the controlled Flow-to-PDF path would consume the project in font, malformed-file and reading-order compatibility work.

## Implications for Roadmap

1. **Foundation and contracts** — repository, versioned Flow schema, transactions, command bus, geometry and test harness.
2. **Editable flow vertical slice** — input, selection, Ukrainian/English text, paragraphs and undo through a working browser.
3. **Deterministic reflow and pagination** — fragments, pages, viewport rendering and incremental/full equivalence.
4. **Owned PDF export and fields** — fonts, selectable text, images, links, AcroForm, validation and exact source recovery.
5. **Voice interaction** — dictation and allowlisted commands through the established transaction boundary.
6. **Controlled PDF reader and scene** — xref/object streams, filters, page interpreter, fonts, images and annotations.
7. **External reconstruction and hybrid fidelity** — blocks, reading order, confidence, OCR adapters and opaque islands.
8. **Native editing and hardening** — recognized scene editing, secure rewrite/redaction, encryption and broader compatibility.

**Phase ordering rationale:** Each phase introduces one new source of uncertainty only after its downstream contract already exists. The reader consumes the display-list and provenance models established by the writer path; reconstruction consumes a trusted scene; native editing consumes both parser provenance and writer serialization.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| System boundaries | HIGH | Direct consequence of PDF fixed-layout semantics and reflow requirements |
| Writer-first sequence | HIGH | Controlled input minimizes compatibility surface while proving user value |
| Rust/WASM core | HIGH | Fits portability, deterministic sharing and hostile-input constraints |
| Exact crate selection | MEDIUM | Requires build-size, performance, Unicode and shaping differential benchmarks |
| External PDF reconstruction quality | MEDIUM | Quality depends heavily on corpus and confidence/review UX |
| Broad native editing | MEDIUM | Feasible for a controlled subset; universal compatibility is open-ended |

## Gaps to Address

- Benchmark pure-Rust shaping/font stack against HarfBuzz and representative Ukrainian/RTL/CJK corpora.
- Decide whether browser painting remains Canvas2D or moves to glyph outlines/CanvasKit after display-list profiling.
- Quantify page-count, latency, memory and export targets on representative 20/100/200-page documents.
- Define the first reader whitelist and exact unsupported-content preservation mechanism.
- Define licensing policy for bundled and embedded fonts.
- Validate associated-source behavior, privacy and file-size tradeoffs across target viewers.

## Sources

- [ISO 32000-2 PDF specification hub](https://pdfa.org/resource/iso-32000-2/)
- [HarfBuzz documentation](https://harfbuzz.github.io/)
- [ICU User Guide](https://unicode-org.github.io/icu/userguide/)
- [Unicode Standard Annexes](https://www.unicode.org/reports/)
- [CSS Paged Media](https://www.w3.org/TR/css-page-3/)
- [wasm-bindgen guide](https://rustwasm.github.io/docs/wasm-bindgen/)
- [qpdf documentation](https://qpdf.readthedocs.io/en/stable/)


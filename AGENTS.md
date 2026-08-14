<!-- GSD:project-start source:PROJECT.md -->

## Project

**FlowPDF**

FlowPDF is a web-first document editor for Ukrainian- and English-language contracts, forms, reports, and similar professional documents. It combines a semantic rich-text flow format with a native PDF representation so users can edit text with document-wide reflow, add fillable fields, control editing by voice, and export interoperable PDF files without depending on a commercial PDF SDK.

The product owns its canonical document model, transaction system, layout and pagination engine, PDF writer, progressively expanded PDF reader/editor, and the conversion boundary between semantic flow documents and fixed-layout PDF pages.

**Core Value:** A user can edit semantic document text naturally, with all following content repaginating correctly, and export the result as a visually consistent, selectable, form-capable PDF.

### Constraints

- **Execution**: A single autonomous implementer owns research, design, implementation, tests, verification, and review — phases must remain small and recoverable.
- **Core technology**: Rust owns the canonical model, transactions, layout, invalidation, PDF syntax, reading, and writing; the same core targets native execution and WebAssembly.
- **Web technology**: React and TypeScript provide the web shell; Web Workers isolate layout and PDF work from the UI thread.
- **Low-level primitives**: HarfBuzz, ICU/ICU4X, vetted font libraries, image codecs, compression libraries, and cryptographic libraries are reused rather than reimplemented.
- **Independence**: No commercial PDF SDK is required at runtime. External engines may be used only as development references and differential validators.
- **Languages**: Ukrainian and English are first-class in v1; the architecture must not block additional scripts or bidirectional text.
- **Compatibility**: Chrome and Edge are the initial supported browsers; Safari and Firefox follow after the editor core stabilizes.
- **Scale target**: The initial interactive target is ordinary documents up to 100–200 pages, with viewport-first layout and background pagination.
- **Determinism**: Layout depends on pinned font binaries, engine versions, fixed-point geometry, and versioned hyphenation data.
- **Security**: Imported PDFs are hostile input. Parsing, decompression, fonts, images, OCR, and embedded actions require strict limits and sandboxing.
- **Accessibility**: The visual canvas/display-list layer must be accompanied by an input host and synchronized semantic DOM; voice is an additional modality, never the only one.
- **Data integrity**: Unsupported content is surfaced explicitly and preserved; the system must not invent or silently discard semantic structure.

<!-- GSD:project-end -->

<!-- GSD:stack-start source:research/STACK.md -->

## Technology Stack

## Version Policy

## Recommended Stack

### Core Document Engine

| Technology | Version policy | Purpose | Why |
|------------|----------------|---------|-----|
| Rust | Stable, pinned | AST, transactions, layout, display list, PDF syntax/read/write | Memory safety for hostile input and one core for native/WASM |
| Serde | Lockfile-pinned | Schema serialization and migrations | Mature typed interchange for JSON and compact formats |
| wasm-bindgen + web-sys | Lockfile-pinned | Rust/WASM to browser boundary and worker APIs | Official Rust/WASM binding path with generated Web API bindings |
| ICU4X / Unicode data adapters | Unicode-version pinned | Grapheme, word, line, script and locale behavior | Versioned Unicode algorithms suitable for portable builds |
| harfrust or equivalent HarfBuzz-compatible Rust shaper | Lockfile-pinned | OpenType shaping in native and WASM | Avoids separate C builds while preserving a HarfBuzz-derived shaping model |
| skrifa/ttf-parser class library | Lockfile-pinned | Font tables, outlines and metrics | Memory-safe font parsing with explicit bounds |
| Fixed-point geometry types | Owned | Deterministic layout coordinates | Prevents platform-dependent float drift in pagination |

### PDF Engine

| Technology | Version policy | Purpose | Why |
|------------|----------------|---------|-----|
| Owned Rust COS model/parser/writer | Project-versioned | PDF object graph, xref, streams, content and serialization | Runtime independence and retained provenance |
| flate2/miniz_oxide | Lockfile-pinned | Flate streams and predictors | Vetted compression primitive; PDF orchestration remains owned |
| jpeg-decoder / png class libraries | Lockfile-pinned | Common image decode/encode | Avoid custom codecs; enforce strict resource limits |
| OpenJPEG or sandboxed equivalent | Later phase | JPEG2000 | Complex native codec kept outside the core trust boundary |
| RustCrypto class libraries | Later phase | AES/hash/CMS primitives | Never implement cryptographic primitives in the PDF layer |
| qpdf, PDFium, MuPDF, veraPDF | CI/reference only | Structural, rendering and conformance comparison | Differential validation without runtime dependency |

### Web Application

| Technology | Version policy | Purpose | Why |
|------------|----------------|---------|-----|
| React | Stable, lockfile-pinned | Editor shell and panels | Mature component and accessibility ecosystem |
| TypeScript | Stable, lockfile-pinned | Browser application and typed WASM bridge | Keeps command and serialization contracts explicit |
| Vite | Stable, lockfile-pinned | SPA development/build | Smaller operational surface than a server-rendered framework for the editor spike |
| Web Workers | Browser platform | Layout, PDF parsing and export | Keeps hostile/heavy work off the UI thread |
| Canvas2D first, renderer adapter thereafter | Browser platform | Page painting | Fast bootstrap; adapter permits later CanvasKit/WebGL replacement |
| Hidden input host + semantic DOM | Browser platform | IME, clipboard, selection and accessibility | Canvas alone cannot provide a correct editor interaction model |
| IndexedDB/OPFS adapter | Browser platform | Local snapshots and assets | Offline-friendly spike without premature backend scope |
| Vitest + Playwright | Lockfile-pinned | Unit/integration/browser tests | Fast contracts plus real input/render behavior |

### Future Service Layer

| Technology | Version policy | Purpose | Why |
|------------|----------------|---------|-----|
| Rust service (Axum-class framework) | Stable, pinned later | Document API and native engine host | Reuses native core and reduces language count |
| PostgreSQL | Supported stable | Metadata, revisions and access control | Transactional document metadata |
| S3-compatible object storage | API-stable | PDF, FlowDocument and asset blobs | Content-addressed large-object storage |
| Durable job queue | Selected when server phase begins | OCR, import and export jobs | Avoids blocking request handlers |

## Alternatives Considered

| Category | Recommended | Alternative | Why Not Now |
|----------|-------------|-------------|-------------|
| Canonical format | Owned FlowDocument | HTML/CSS | Browser layout is not a deterministic cross-runtime source of truth |
| Core language | Rust | C++ | Larger memory-safety and tooling burden for hostile PDF/font parsing |
| Editor model | Owned transactions and anchors | DOM/contenteditable as truth | DOM positions are unstable across repagination and cannot model PDF provenance |
| Browser framework | React/Vite | Next.js | Server rendering adds little to the document-engine spike |
| Rendering | Renderer adapter, Canvas2D bootstrap | CanvasKit immediately | Large dependency before display-list and typography requirements are proven |
| PDF engine | Owned subset | Commercial SDK | Conflicts with product independence and control goals |
| Universal parser first | Writer-first | Reader-first | Arbitrary PDF compatibility would delay proof of the core value |

## Sources

- [ISO 32000-2 PDF specification hub](https://pdfa.org/resource/iso-32000-2/)
- [HarfBuzz responsibilities](https://harfbuzz.github.io/what-is-harfbuzz.html)
- [What HarfBuzz does not do](https://harfbuzz.github.io/what-harfbuzz-doesnt-do.html)
- [ICU boundary analysis](https://unicode-org.github.io/icu/userguide/boundaryanalysis/)
- [wasm-bindgen guide](https://rustwasm.github.io/docs/wasm-bindgen/)
- [wasm-bindgen Web Worker example](https://rustwasm.github.io/docs/wasm-bindgen/examples/wasm-in-web-worker.html)
- [qpdf design](https://qpdf.readthedocs.io/en/stable/design.html)

<!-- GSD:stack-end -->

<!-- GSD:conventions-start source:CONVENTIONS.md -->

## Conventions

Conventions not yet established. Will populate as patterns emerge during development.
<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->

## Architecture

Architecture not yet mapped. Follow existing patterns found in the codebase.
<!-- GSD:architecture-end -->

<!-- GSD:skills-start source:skills/ -->

## Project Skills

No project skills found. Add skills to any of: `.claude/skills/`, `.agents/skills/`, `.cursor/skills/`, `.github/skills/`, or `.codex/skills/` with a `SKILL.md` index file.
<!-- GSD:skills-end -->

<!-- GSD:workflow-start source:GSD defaults -->

## GSD Workflow Enforcement

Before using Edit, Write, or other file-changing tools, start work through a GSD command so planning artifacts and execution context stay in sync.

Use these entry points:

- `$gsd-quick` for small fixes, doc updates, and ad-hoc tasks
- `$gsd-debug` for investigation and bug fixing
- `$gsd-execute-phase` for planned phase work

Do not make direct repo edits outside a GSD workflow unless the user explicitly asks to bypass it.
<!-- GSD:workflow-end -->

<!-- GSD:profile-start -->

## Developer Profile

> Profile not yet configured. Run `$gsd-profile-user` to generate your developer profile.
> This section is managed by `generate-claude-profile` -- do not edit manually.
<!-- GSD:profile-end -->

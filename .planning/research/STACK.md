# Technology Stack

**Project:** FlowPDF  
**Researched:** 2026-08-14  
**Confidence:** HIGH for architecture and standards; MEDIUM for exact crate choices until the first benchmark spike

## Version Policy

Use the latest stable toolchain available at the start of each phase, then pin it in `rust-toolchain.toml`, `Cargo.lock`, and `package-lock.json`. Do not use floating runtime dependencies in production builds. The current workspace has Node.js 24.10.0 and npm 11.9.0; a Rust toolchain is not yet installed and is a Phase 1 bootstrap task.

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

The text subsystem must expose adapter traits (`Segmenter`, `BidiResolver`, `TextShaper`, `FontProvider`) so implementations can be compared against ICU and HarfBuzz reference behavior. HarfBuzz maps text runs to positioned glyphs but deliberately does not own BiDi, fallback, line breaking, or pagination.

### PDF Engine

| Technology | Version policy | Purpose | Why |
|------------|----------------|---------|-----|
| Owned Rust COS model/parser/writer | Project-versioned | PDF object graph, xref, streams, content and serialization | Runtime independence and retained provenance |
| flate2/miniz_oxide | Lockfile-pinned | Flate streams and predictors | Vetted compression primitive; PDF orchestration remains owned |
| jpeg-decoder / png class libraries | Lockfile-pinned | Common image decode/encode | Avoid custom codecs; enforce strict resource limits |
| OpenJPEG or sandboxed equivalent | Later phase | JPEG2000 | Complex native codec kept outside the core trust boundary |
| RustCrypto class libraries | Later phase | AES/hash/CMS primitives | Never implement cryptographic primitives in the PDF layer |
| qpdf, PDFium, MuPDF, veraPDF | CI/reference only | Structural, rendering and conformance comparison | Differential validation without runtime dependency |

The first writer emits a conservative unencrypted PDF 1.7-compatible subset while the object model remains capable of PDF 2.0 evolution. The reader initially accepts valid common files with xref tables/streams, object streams, common filters, Type0/TrueType fonts, images, annotations, and AcroForm.

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


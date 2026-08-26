# Phase 2: Accessible Rich-Text Editing - Research

**Researched:** 2026-08-26  
**Domain:** Rust-owned semantic rich-text editing, browser input/IME, React projection, and accessibility  
**Confidence:** MEDIUM

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Input, Selection, and IME

- **D-01:** The editor exposes one contiguous directional selection with `anchor` and `focus` endpoints expressed only as stable node ID, UTF-16 offset, and affinity. DOM paths, DOM ranges, absolute character indices, and pixel coordinates remain view-local and are never persisted. — **Reversibility:** costly — keyboard, UI, future voice, fields, source maps, and WASM DTOs depend on this position contract.
- **D-02:** A pinned Unicode grapheme segmenter in Rust is the authority for valid caret and edit boundaries. Normal keyboard/pointer navigation lands only on valid grapheme boundaries; malformed API/WASM targets return a stable error instead of silently snapping or splitting a cluster.
- **D-03:** Browser input uses a controlled input host together with a synchronized, visible semantic DOM. The DOM is projected from the last accepted Rust revision; `contenteditable`/DOM mutations are not document truth, and every committed change passes through the typed command service.
- **D-04:** IME composition is transient, visibly previewed, and noncanonical. `compositionend` produces one atomic transaction, cancellation produces none, and a changed base revision invalidates the composition bookmark rather than relocating text. Plain-text paste uses the same input path; arbitrary clipboard HTML is not imported as canonical structure in this phase.

#### Semantic Structure and Transformations

- **D-05:** Evolve the canonical schema through an explicit migration to a typed semantic block tree with stable IDs plus inline text leaves/runs and marks. HTML or browser DOM snapshots are prohibited as canonical content. — **Reversibility:** one-way — once schema v2 documents are persisted, the sequential migration and canonical byte contract must remain supported.
- **D-06:** First-class block kinds are paragraph, heading, ordered list, unordered list, list item, image, simple table, table row, table cell, and explicit page break. Lists and tables use semantic nesting; they are not encoded as styled text or opaque payloads.
- **D-07:** Bold, italic, underline, font family, font size, color, and language are inline range marks/attributes. Alignment, paragraph spacing, heading level, and list semantics are block attributes. A collapsed selection carries pending typing attributes; a mixed selection exposes an indeterminate formatting state.
- **D-08:** Split and merge rules are explicit and undoable: paragraphs preserve compatible style; a mid-heading split preserves heading semantics while Enter at the end creates a body paragraph; Enter in a list creates a sibling item and an empty item exits the list; adjacent compatible text blocks merge while preserving inline marks. Images, tables, and page breaks never merge implicitly.
- **D-09:** The document always retains an editable paragraph when its last editable block would otherwise be removed. Atomic blocks are selected or removed through explicit commands and visible controls, never by corrupting neighboring text ranges.

#### Formatting Controls and Editor Shell

- **D-10:** The Phase 2 UI is a page-neutral, centered word-processor surface. It has a persistent application bar for document identity/durability and undo/redo, a selection-aware formatting toolbar, a labeled insert menu, the document surface, and visible status/error regions. Simulated pages wait for the real Phase 3 layout engine.
- **D-11:** Every Phase 2 mutation has both a visible labeled control and a keyboard path. Standard undo/redo and bold/italic/underline shortcuts, text input, structural Enter/Backspace behavior, toolbar actions, and insert actions all compile to the same typed command bus. Toolbar use preserves the logical selection and returns focus to the editor.
- **D-12:** The React/TypeScript shell owns event translation, focus, physical browser I/O, localization, and rendering only. Rust owns selection validation, semantic mutations, migrations, transaction construction, undo/redo, and canonical serialization. The Phase 1 Foundation Inspector remains available as secondary diagnostics/test support rather than the primary product surface.

#### Accessible Navigation and Embedded Blocks

- **D-13:** The synchronized document tree uses native semantic elements for headings, paragraphs, ordered/unordered lists, tables, images, and page-break separators inside one labeled editor region. It is keyboard navigable and screen-reader readable without canvas or an ARIA-only replica; statuses use a polite live region and failures use atomic alerts with visible text.
- **D-14:** Image insertion accepts bounded PNG/JPEG assets through a visible file control, requires useful alt text or an explicit decorative choice, renders an inline preview, and exposes visible replace/remove actions. Cropping, freeform resizing, floating, and text wrap are deferred.
- **D-15:** Simple tables are bounded semantic tables with editable cells, an optional header row, keyboard cell navigation, and visible row/column add/remove controls. A page break is a labeled, focusable atomic separator; it carries semantic intent now and receives physical pagination behavior only in Phase 3.
- **D-16:** Ukrainian remains the default UI locale with key-identical English resources. Accessibility names, formatting state, live statuses, validation errors, and shortcuts are localized; document language metadata is distinct from interface locale.

### the agent's Discretion

- Exact React component boundaries, CSS class names, and token values, provided the UI-SPEC and Phase 1 accessibility patterns are preserved.
- The vetted Rust Unicode grapheme library selected after official-data compatibility, WASM size, and dependency-provenance checks; Unicode algorithms are reused, not reimplemented.
- Concrete bounded limits for image bytes, table dimensions, nesting, mark count, composition text, and command batch size, provided research documents the rationale and tests all failure codes.
- Whether the diagnostic inspector is exposed as a development-only route or a collapsible diagnostics panel, provided Phase 1 recovery/audit behavior remains testable.

### Deferred Ideas (OUT OF SCOPE)

- Glyph shaping, font fallback, BiDi ordering, language-aware line breaking, hyphenation, caret geometry from fragments, document-wide reflow, and page virtualization — Phase 3.
- Image crop/resize handles, floating/wrapping images, complex tables, merged cells, nested tables, and table fragmentation — later layout/editor expansion.
- Rich HTML/office clipboard import, comments, track changes, collaborative editing, and spell/grammar services — later product phases or backlog.
- Fillable-field authoring, PDF import/export, and voice capture/recognition remain their dedicated roadmap phases; Phase 2 only establishes mutation parity and reusable selection bookmarks.
</user_constraints>

<phase_requirements>
## Phase Requirements

The exact requirement statements below are quoted from the project requirements. [VERIFIED: `.planning/REQUIREMENTS.md:24-32,95-103`]

| ID | Description | Research Support |
|----|-------------|------------------|
| EDIT-01 | “User can place a caret and select text without splitting a Unicode grapheme cluster.” | Rust `GraphemeBoundaryMap`, pinned ICU4X data, UAX #29 fixture conformance, atomic-node edge sentinels, and malformed-boundary rejection. [ASSUMED] |
| EDIT-02 | “User can insert, replace, and delete Ukrainian and English text through keyboard and IME input.” | Native browser-event adapter, explicit cancel-aware composition state machine, one atomic `ReplaceSelection` commit, and Rust + browser + real-Chromium tests. [ASSUMED] |
| EDIT-03 | “User can split and merge paragraphs while preserving logical text order and compatible styles.” | Typed structural commands, compatibility matrix, document-order index, explicit affinity-aware anchor maps, and exact inverse operations. [ASSUMED] |
| EDIT-04 | “User can apply bold, italic, underline, font, size, color, language, alignment, spacing, and list formatting.” | Closed bounded `MarkSet`/block attributes plus Rust-owned, revision-bound pending typing attributes and mixed-state projection. [ASSUMED] |
| EDIT-05 | “User can create headings, paragraphs, ordered lists, unordered lists, images, simple tables, and explicit page breaks.” | Stable semantic block tree, staged binary assets, bounded atomic commands, native semantic DOM projection, and lossless schema-v2 migration. [ASSUMED] |
| QUAL-03 | “User can operate every document mutation available by voice through keyboard and visible UI controls.” | One semantic command catalog with keyboard/visible-control parity; voice can later emit the same commands without new mutation authority. [ASSUMED] |
| QUAL-04 | “Screen-reader users can navigate semantic document content, fields, status changes, errors, and confirmation prompts.” | Native document elements plus keyboard-navigable, non-authoring projections of existing Phase 1 field descriptors, one accessible document copy, localized dialogs/status/alerts, automated semantics, and an external AT UAT checkpoint. [ASSUMED] |
</phase_requirements>

## Project Constraints (from AGENTS.md)

The following actionable directives are project-authoritative. [VERIFIED: `AGENTS.md:13-26,128-140`]

- A single autonomous implementer owns research through review; work must remain small and recoverable.
- Rust owns the canonical model, transactions, layout/invalidation, PDF syntax, reading, and writing and must target native plus WebAssembly.
- React and TypeScript own the web shell; heavy layout/PDF work uses Web Workers.
- Reuse ICU/ICU4X, vetted font, image, compression, and cryptographic primitives; do not reimplement them.
- No commercial PDF SDK is a runtime dependency; external PDF engines may be used only as development references and differential validators.
- Ukrainian and English are first-class, without blocking additional scripts or bidirectional text.
- Chrome and Edge are the initial browser targets; Safari and Firefox follow only after the editor core stabilizes.
- The initial interactive scale target is ordinary documents up to 100–200 pages; viewport-first/background pagination remains Phase 3, while Phase 2 must avoid document-size-quadratic semantic projection and command paths.
- Preserve deterministic behavior through pinned font binaries, engine versions, fixed-point geometry, and versioned hyphenation/locale data; Phase 2 stores semantic values but does not implement Phase 3 layout.
- Treat imported input, fonts, images, and decompression as hostile and bounded.
- Keep an input host plus synchronized semantic DOM; voice is never the sole modality.
- Surface and preserve unsupported content; never invent or silently discard semantic structure.
- Use GSD workflow entry points before source edits; this research writes only the explicitly assigned research artifact.

## Summary

Phase 2 should be planned as a Rust-first extension of the Phase 1 vertical transaction path, not as a browser editor with a later serialization step. The current canonical value is explicitly schema version `1`; `FlowDocument` stores flat `content: Vec<ContentNode>`, and `ContentNodeKind` is exactly `Paragraph | Image`. [VERIFIED: `crates/flow-core/src/model/mod.rs:13,62-73,115-130`; exact source values: `pub const SCHEMA_VERSION: u32 = 1;`, `pub content: Vec<ContentNode>`, `pub enum ContentNodeKind { Paragraph, Image }`] Phase 2 must migrate that value to schema v2 without inventing image accessibility intent or snapping legacy field anchors, preserve stable IDs/source bytes, and make one immutable Rust/WASM result the only accepted editor snapshot. Existing Phase 1 field descriptors are preserved and exposed as keyboard-navigable, non-authoring semantic projections; field creation/editing/filling stays in its later phase. [ASSUMED] React renders and translates events; it does not parse canonical JSON into a second mutable truth. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]

The leading plan must prove a genuine vertical tracer: open/migrate a durable document, place a grapheme-safe logical caret, accept ordinary Ukrainian/English typing and one real Chromium IME composition, commit through Rust as one transaction, project a semantic paragraph in React, undo/redo, persist, reload, and recover the identical v2 semantics. [ASSUMED] Once that end-to-end seam is green, subsequent tasks can add the full structural-command matrix, formatting projection, lists, images, tables, page breaks, accessibility parity, bounds, and exhaustive tests without creating horizontal scaffolding that has never crossed WASM and IndexedDB.

Use ICU4X `icu_segmenter` with compiled data as the one grapheme authority. `GraphemeClusterSegmenter::segment_str` returns UTF-8 byte boundaries, so the core should create a cached-per-command boundary vector translated to UTF-16 offsets and reject any nonmember target. [VERIFIED: Context7 `/unicode-org/icu4x`; https://docs.rs/icu_segmenter/2.3.0/icu_segmenter/struct.GraphemeClusterSegmenter.html] Pin the crate and the tested Unicode data version independently: the segmenter source states its UAX #29 Unicode version, while the companion data crate separately records CLDR, ICU-export, and LSTM source tags. [VERIFIED: https://docs.rs/icu_segmenter/2.3.0/src/icu_segmenter/grapheme.rs.html; https://docs.rs/crate/icu_segmenter_data/2.3.0/source/Cargo.toml.orig]

**Primary recommendation:** build a lossless schema-v2 migration plus ICU grapheme validation and one paragraph-editing React/IME/durability tracer first; then expand only through closed Rust document/session commands, exact inverses, explicit text/atomic anchor mappings, staged binary assets, native document/field projection, and bounded resource tests. Dependency pins and ICU WASM cost remain conditional on the automated execution gates documented below. [ASSUMED]

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|--------------|----------------|-----------|
| Schema-v2 model, validation, migration | Rust core | WASM boundary | Canonical types and sequential migrations are project-owned Rust responsibilities; WASM only serializes closed DTOs. [VERIFIED: `AGENTS.md:15-18`; `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] |
| Grapheme boundaries and logical selection validation | Rust core | Browser adapter | Rust decides legal UTF-16 positions; the browser may request navigation but cannot snap or persist DOM positions. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] |
| Rich-text and structure mutation, inverse, undo/redo | Rust transaction service | WASM boundary | The current single publication point is `TransactionService::apply`; all modalities already enter `Command`. [VERIFIED: `crates/flow-core/src/transaction/mod.rs:24-40,328-337`; exact modality values: `Ui`, `Keyboard`, `Voice`, `Api`, `System`] |
| IME, `beforeinput`, paste, keyboard, focus | Browser/controller | Rust command service | Physical events and transient composition are browser state; committed meaning is a typed Rust command. [CITED: https://www.w3.org/TR/input-events-2/] |
| Accepted editor snapshot and formatting capabilities | Rust core | React external store | Rust calculates semantic/selection state; React subscribes to a stable immutable snapshot. [CITED: https://react.dev/reference/react/useSyncExternalStore] |
| Semantic document view and localized controls | React | Rust view DTO | React maps the trusted DTO to native DOM, one text copy, visible controls, live status, and alerts. [VERIFIED: `AGENTS.md:17,25`; `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-UI-SPEC.md`] |
| Existing Phase 1 field navigation | Rust view projection | React native controls | Rust preserves field descriptors/anchor status and orders valid fields at their semantic anchors; React exposes non-authoring, keyboard-navigable labeled controls or review summaries. [ASSUMED] |
| Durable snapshot/assets/recovery | Existing IndexedDB adapter | Rust canonical/commit DTO | Reuse the Phase 1 create/open/apply/commit/recover lifecycle; do not create an editor-local persistence store. [VERIFIED: `.planning/phases/FLOWPDF-01-durable-flow-foundation/01-06-SUMMARY.md`] |
| Image validation, staging, and metadata | Rust core + WASM binary staging seam | Browser file picker/IndexedDB commit | Browser supplies a bounded `Uint8Array`; Rust validates and returns an opaque revision-bound receipt, then redeems it into the same planned document+asset commit. [ASSUMED] |
| Shaping, BiDi, line breaking, pagination, caret geometry | Phase 3 engine | — | Explicitly outside this phase. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] |

## Standard Stack

### Core

| Library/runtime | Version | Purpose | Why Standard Here |
|-----------------|---------|---------|-------------------|
| Rust | `1.97.1` | Canonical schema, selection, transactions, validation, migration | This is the workspace-pinned Rust version and the locked authority boundary. [VERIFIED: `Cargo.toml:5-10`; exact value: `rust-version = "1.97.1"`] |
| `icu_segmenter` | candidate `=2.3.0`, `default-features = false`, `features = ["compiled_data"]` | Extended grapheme boundaries on native and WASM | ICU4X exposes an explicit grapheme segmenter and separately versioned Unicode data; adoption is conditional on the documented lockfile/provenance and numerical WASM-size gates. [VERIFIED: Context7 `/unicode-org/icu4x`; crates.io; package-legitimacy seam] |
| React / `react-dom` | `19.2.8` | UI shell, native semantic projection, external-store subscription | The packages are official React packages and passed the legitimacy gate; `useSyncExternalStore` is the supported external-store hook. [VERIFIED: npm registry; https://react.dev/reference/react/useSyncExternalStore] |
| TypeScript | existing `5.9.3` | Closed browser/WASM DTOs and event/controller code | Retain the workspace pin rather than combining the React migration with a compiler-major change. [VERIFIED: `package.json:21-27`; exact value: `"typescript": "5.9.3"`] |
| Vite | registry candidate `8.2.2`; execution selects an exact automatically approved pin | React SPA development and production build | Use the official React TypeScript setup and preserve the Phase 1 commands as compatibility gates. The observed candidate is not installation authority: the execution gate must prove official repository ownership, registry integrity/tarball, absence of deprecation/unsafe lifecycle scripts, and a locked resolution or reject it without an approval override. [CITED: https://vite.dev/guide/; https://github.com/vitejs/vite] |
| `image` | `=0.25.10`, `default-features = false`, `features = ["png", "jpeg"]` | Signature-aware PNG/JPEG decode and resource-bound validation | It supplies vetted readers/codecs and explicit decode limits; the crate passed the legitimacy gate. [VERIFIED: crates.io; https://docs.rs/image/0.25.10/image/] |

### Supporting

| Library/API | Version | Purpose | When to Use |
|-------------|---------|---------|-------------|
| `wasm-bindgen` / `serde-wasm-bindgen` | existing `0.2.108` / `0.6.5` | Narrow immutable Rust/WASM request-response DTOs | Extend the existing functions; do not expose mutable Rust editor handles. [VERIFIED: `Cargo.toml:11-19`; exact values: `wasm-bindgen = "=0.2.108"`, `serde-wasm-bindgen = "=0.6.5"`] |
| `@vitejs/plugin-react` | registry candidate `6.1.0`; execution selects an exact automatically approved pin | Official Vite React integration | Add only with an automatically provenance-approved Vite resolution; a failed gate blocks the install rather than requesting an approval exception. [CITED: https://vite.dev/guide/] |
| `@types/react` / `@types/react-dom` | registry candidates `19.2.18` / `19.2.5`; execution selects exact automatically approved pins | Type declarations | The official TypeScript React setup needs declarations, but candidate age/download assertions are not trust evidence; source/integrity/lockfile automation is authoritative. [CITED: https://github.com/DefinitelyTyped/DefinitelyTyped] |
| Vitest / browser Playwright provider | existing `4.1.11` | Rust-boundary-adjacent unit and browser component tests | Reuse named unit/browser projects. [VERIFIED: `package.json:21-27`; exact values: `"vitest": "4.1.11"`, `"@vitest/browser-playwright": "4.1.11"`] |
| Playwright | existing `1.57.0` | Chromium UI, accessibility, responsive, and real IME automation | Reuse the workspace browser binary and add a Chromium-only CDP IME verification lane. [VERIFIED: `package.json:21-27`; exact value: `"playwright": "1.57.0"`; https://playwright.dev/docs/api/class-browsercontext#browser-context-new-cdp-session] |
| Browser `beforeinput`, composition, clipboard APIs | standards + Chromium target | Translate physical input into controller events | Attach native listeners to the controlled input host; distinguish standard guarantees from browser observations. [CITED: https://www.w3.org/TR/input-events-2/; https://www.w3.org/TR/uievents/; https://developer.mozilla.org/en-US/docs/Web/API/Element/beforeinput_event] |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| ICU4X `icu_segmenter` | `unicode-segmentation` `1.13.3` | The narrower crate offers extended grapheme iteration and may be smaller, but it would establish a second Unicode authority before Phase 3 needs word/line segmentation. Use ICU4X for one explicit data-provenance path. [CITED: https://docs.rs/unicode-segmentation/1.13.3/unicode_segmentation/trait.UnicodeSegmentation.html] |
| Owned React/controller projection | ProseMirror, Lexical, Slate, TipTap | Their document/selection/transaction models would become a parallel truth or require continuous semantic translation; that contradicts the locked Rust authority. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] |
| Native semantic DOM | canvas editor | Canvas cannot satisfy the locked Phase 2 navigation/readability contract and Phase 3 owns geometry. [VERIFIED: `AGENTS.md:25`; `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] |
| Native input listeners | React `onBeforeInput` | React documents that its `onBeforeInput` is not the native event and attempts to polyfill it; the editor needs native `InputEvent.inputType`, `cancelable`, `isComposing`, and clipboard behavior. [CITED: https://react.dev/reference/react-dom/components/common] |
| Vite React shell | SSR framework | This local editor phase needs no server-rendering boundary; the project stack already fixes a React/Vite SPA. [VERIFIED: `AGENTS.md:61-72`] |

### Conditional Installation

The versions below are reproducible registry candidates, not a “latest is safe” assertion. The executor must first run a new fail-closed `scripts/verify-phase2-dependencies.mjs` task that queries the official npm/crates registries for the exact version, publish timestamp, repository owner allowlist, deprecation, tarball host, integrity, and lifecycle scripts; runs the package-legitimacy seam; materializes the lockfile; and verifies the resolved graph with the existing dependency-provenance and dependency-lock gates. A candidate that remains `SUS`/`SLOP` or mismatches official ownership is automatically rejected; the script may select an older exact release that passes all automated checks, but there is no manual approval override. [ASSUMED]

```bash
npm install --save-exact react@19.2.8 react-dom@19.2.8
npm install --save-dev --save-exact @types/react@19.2.18 @types/react-dom@19.2.5 vite@8.2.2 @vitejs/plugin-react@6.1.0
```

```toml
# crates/flow-core/Cargo.toml — recommended, not an existing value. [ASSUMED]
icu_segmenter = { version = "=2.3.0", default-features = false, features = ["compiled_data"] }
image = { version = "=0.25.10", default-features = false, features = ["png", "jpeg"] }
```

After candidate resolution, run `npm ci --ignore-scripts`, the explicitly required build steps, `cargo tree -e features`, `npm ls`, lockfile diff verification, and the numerical ICU WASM-size gate before accepting the dependency task. [ASSUMED]

### Registry Candidate Snapshot

These versions and publication timestamps were queried from the appropriate registry on 2026-08-25; timestamps are UTC. They are historical candidate observations only and must be revalidated by the execution-time official-registry, source-ownership, integrity, lockfile, and lifecycle-script gate. [VERIFIED: npm and crates.io registry queries]

| Package | Verified version | Published |
|---------|------------------|-----------|
| `react` | `19.2.8` | `2026-07-21T15:41:28.716Z` [VERIFIED: npm registry + package-legitimacy OK + official React source] |
| `react-dom` | `19.2.8` | `2026-07-21T15:41:41.267Z` [VERIFIED: npm registry + package-legitimacy OK + official React source] |
| `@types/react` | `19.2.18` | `2026-07-30T21:54:03.456Z`; candidate rejected unless the automated execution gate passes. [ASSUMED] |
| `@types/react-dom` | `19.2.5` | `2026-08-23T21:05:23.671Z`; candidate rejected unless the automated execution gate passes. [ASSUMED] |
| `vite` | `8.2.2` | `2026-08-20T04:14:39.107Z`; candidate rejected unless the automated execution gate passes. [ASSUMED] |
| `@vitejs/plugin-react` | `6.1.0` | `2026-08-20T02:49:46.306Z`; candidate rejected unless the automated execution gate passes. [ASSUMED] |
| `icu_segmenter` | `2.3.0` | `2026-08-13T23:30:08.159533Z` [VERIFIED: crates.io + package-legitimacy OK + official ICU4X source] |
| `image` | `0.25.10` | `2026-03-10T16:29:18.250581Z` [VERIFIED: crates.io + package-legitimacy OK + official image-rs source] |

The four `SUS` results cannot be waived: execution either proves and locks an automatically acceptable exact release or fails closed before installation. [ASSUMED]

## Package Legitimacy Audit

The table records the 2026-08-25 registry and `package-legitimacy` seam results used by this research. Download counts are volatile snapshots, not product requirements. [VERIFIED: npm/crates registries and `gsd-tools query package-legitimacy check`]

| Package | Registry | Age / observed use | Downloads at audit | Source repository | Verdict | Disposition |
|---------|----------|--------------------|--------------------|-------------------|---------|-------------|
| `icu_segmenter` | crates.io | published since 2021 | 475,672/week; 88,991 for 2.3.0 | `unicode-org/icu4x` | OK | Approved |
| `image` | crates.io | published since 2014 | 3.44M/week; 34.6M for 0.25.10 | `image-rs/image` | OK | Approved |
| `react` | npm | published since 2011 | ~170M/week | `facebook/react` | OK | Approved |
| `react-dom` | npm | published since 2014 | ~159M/week | `facebook/react` | OK | Approved |
| `@types/react` | npm | published since 2016 | No qualitative claim accepted; execution captures the official downloads-API number | `DefinitelyTyped/DefinitelyTyped` | SUS: too new | Candidate only — automated gate selects an acceptable exact release or blocks |
| `@types/react-dom` | npm | published since 2016 | No qualitative claim accepted; execution captures the official downloads-API number | `DefinitelyTyped/DefinitelyTyped` | SUS: too new | Candidate only — automated gate selects an acceptable exact release or blocks |
| `vite` | npm | published since 2020 | No qualitative claim accepted; execution captures the official downloads-API number | `vitejs/vite` | SUS: too new | Candidate only — automated gate selects an acceptable exact release or blocks |
| `@vitejs/plugin-react` | npm | published since 2021 | No qualitative claim accepted; execution captures the official downloads-API number | `vitejs/vite-plugin-react` | SUS: too new | Candidate only — automated gate selects an acceptable exact release or blocks |

Registry inspection reported no `postinstall` script for any recommended npm package. [VERIFIED: `npm view <package> scripts.postinstall`, 2026-08-25]

**Packages removed due to SLOP verdict:** none. [VERIFIED: package-legitimacy seam]  
**Packages flagged as suspicious (SUS):** `@types/react`, `@types/react-dom`, `vite`, `@vitejs/plugin-react`; they remain unapproved candidates until the automated official-registry/source/integrity/lockfile gate selects an acceptable exact release. No interactive approval may bypass the failure. [ASSUMED]

## Architecture Patterns

### System Architecture Diagram

```text
keyboard / pointer / IME / paste / toolbar / insert menu
                         |
                         v
      native event adapter + transient composition controller
                         |
              closed semantic Command DTO
                         |
                         v
  React EditorController ----> flow-wasm serialized boundary
       |                              |
       |                              v
       |                    Rust selection validation
       |                    + command compilation
       |                    + exact forward/inverse ops
       |                    + canonical schema-v2 validation
       |                              |
       |                  reject -----+----- accept
       |                    |                 |
       |                    v                 v
       |             visible alert    immutable OperationResult
       |                                      |
       |                         persist commit atomically
       |                         in existing IndexedDB path
       |                                      |
       +<---------- publish accepted EditorSnapshot only
                         |
              +----------+-----------+
              |                      |
              v                      v
      React native semantic DOM   status/toolbar/menu projection
      + logical selection bridge  + diagnostics inspector
```

The diagram is a recommended implementation flow constrained by the locked ownership split. [ASSUMED]

### Recommended Project Structure

These are recommended future paths, not claims about files already present. [ASSUMED]

```text
crates/flow-core/src/
├── model/                 # schema-v2 semantic tree and closed attributes
├── schema/                # v0/v1 legacy decoders, v1->v2 migration, bounds
├── anchor/                # document order, grapheme maps, selection mappings
├── transaction/           # semantic command compilation and exact inverses
├── editor_view/           # closed semantic/selection/capability DTO projection
└── asset/                 # bounded PNG/JPEG validation
crates/flow-core/tests/
├── grapheme_conformance.rs
├── schema_v2_migration.rs
├── rich_text_transactions.rs
├── rich_text_properties.rs
└── resource_limits.rs
web/src/editor/
├── editor-controller.ts   # accepted snapshot, command serialization, ephemeral UI
├── input-adapter.ts       # native beforeinput/composition/paste/keyboard events
├── selection-bridge.ts    # DOM range <-> logical selection, never persisted
├── editor-store.ts        # useSyncExternalStore-compatible stable snapshot
├── semantic-document.tsx  # native block projection
├── formatting-toolbar.tsx
├── insert-menu.tsx
├── embedded-blocks.tsx
└── editor-app.tsx
web/tests/
├── editor-controller.test.ts
├── editor-input.browser.test.ts
├── editor-accessibility.browser.test.ts
├── editor-parity.browser.test.ts
└── editor-responsive.browser.test.ts
fixtures/unicode/17.0.0/
└── GraphemeBreakTest.txt
```

### Pattern 1: Schema-v2 Stable Semantic Tree

**What:** make block identity stable while inline runs remain normalized values, not selection targets. Text positions continue to identify the editable text block plus UTF-16 offset; formatting may split/merge runs without moving anchors. Lists and tables own stable semantic container IDs. Empty editable blocks canonically use an empty run vector. Pending typing attributes are Rust-owned, revision-bound, noncanonical editor-session state—not React state and not persisted FlowDocument content. [ASSUMED]

**Recommended closed shape:** [ASSUMED]

```rust
// Design skeleton, not copied API. Every value below is a Phase 2 recommendation. [ASSUMED]
struct BlockNode { id: NodeId, kind: BlockKind }
enum BlockKind {
    Paragraph { attrs: ParagraphAttrs, runs: Vec<InlineRun> },
    Heading { level: u8, attrs: ParagraphAttrs, runs: Vec<InlineRun> },
    OrderedList { items: Vec<BlockNode> },
    UnorderedList { items: Vec<BlockNode> },
    ListItem { children: Vec<BlockNode> },
    Image { asset_id: AssetId, accessibility: ImageAccessibility },
    Table { header_rows: u16, rows: Vec<BlockNode> },
    TableRow { cells: Vec<BlockNode> },
    TableCell { children: Vec<BlockNode> },
    PageBreak,
}
struct InlineRun { text: String, marks: MarkSet }
enum ImageAccessibility {
    Described(String),
    Decorative,
    MissingLegacy,
}
enum FieldAnchorState {
    GraphemeSafe(LogicalPosition),
    LegacyInvalid { original: LogicalPosition, reason: LegacyAnchorReason },
    TargetDeleted { original: LogicalPosition, tombstone: TombstoneToken },
}
enum LegacyAnchorReason { NonGraphemeBoundary, MissingNode }
```

Use a typed `MarkSet` with exactly bold, italic, underline, font-family ID, size in millipoints, RGB color, and BCP-47 language; use typed paragraph alignment/spacing and heading/list attributes. Normalize by removing empty runs and merging adjacent equal mark sets. [ASSUMED] Do not perform Unicode normalization: canonical text bytes and user intent must remain unchanged while segmentation operates on the stored sequence. [CITED: https://unicode.org/reports/tr29/]

The existing v1 fields are exactly `schema_version`, `document_id`, `revision`, `locale`, `page_settings`, `styles`, `content`, `assets`, `fields`, and `provenance`; preserve all non-content fields through migration. [VERIFIED: `crates/flow-core/src/model/mod.rs:62-73`; verbatim field names quoted in this sentence] V1 `AssetDescriptor` fields are exactly `id`, `content_hash`, `media_type`, `byte_length`, and `alt_text`; v1 `FieldDescriptor` fields are exactly `id`, `name`, `label`, `anchor`, `kind`, `required`, `read_only`, `default_value`, and `options`. [VERIFIED: `crates/flow-core/src/model/mod.rs:132-154`; field names quoted verbatim] Preserve every v1 paragraph/image/asset/field ID and the original asset descriptor so existing references do not silently retarget. [ASSUMED]

Lossless v1→v2 image migration has one closed rule: nonempty v1 `alt_text` becomes `Described(original_text)`; empty v1 `alt_text` becomes `MissingLegacy`, never `Decorative`. New `InsertImage` commands may create only `Described(nonempty)` or `Decorative`; `MissingLegacy` is migration-only, remains visibly “needs description review,” and is never silently cleared. [ASSUMED] If a legacy asset is referenced by several image nodes, each migrated node receives the same explicit initial state while the original asset metadata/source record remains preserved. [ASSUMED]

Lossless field migration first checks the exact stored UTF-16 anchor against the migrated text and pinned grapheme map. A valid boundary becomes `GraphemeSafe(original)`; an offset inside a grapheme or a missing target becomes `LegacyInvalid { original, reason }`. It is never snapped, relocated, or discarded. [ASSUMED] Safe text edits may shift an invalid legacy scalar offset only through explicit anchor algebra; deletion of its target may create `TargetDeleted` only through the reversible transaction-preimage contract below, and no operation may promote either invalid state to `GraphemeSafe` without restoring its exact preimage or a later dedicated user-reviewed field command. [ASSUMED]

### Pattern 2: Explicit Sequential Migration with Legacy Types

The current migration registry has only the exact hop `MigrationStep::new(0, 1, migrate_v0_to_v1, validate_v1_migration_output)`. [VERIFIED: `crates/flow-core/src/schema/mod.rs:630-638`; exact expression quoted] Once current model types become v2, deserialize old bytes through private `LegacyFlowDocumentV0` and `LegacyFlowDocumentV1` structs; never make the v0→v1 decoder depend on the v2 `FlowDocument` type. [ASSUMED]

The plan must test three routes: v0→v1→v2, v1→v2, and canonical v2 no-op; each migration must append its provenance hop, preserve document/node/asset/field IDs, validate before publishing, and generate deterministic golden bytes/hash. [ASSUMED] Required migration fixtures are: nonempty image alt→`Described`; empty image alt→`MissingLegacy`; decomposed Ukrainian text with a field anchor on a valid cluster edge; the same text with an anchor between the base and combining mark→`LegacyInvalid(NonGraphemeBoundary)` with its exact original offset; and a missing-node field anchor→`LegacyInvalid(MissingNode)`. [ASSUMED] Keep the older migration fixtures permanently; schema v2 is a one-way compatibility promise. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]

### Pattern 3: Grapheme Map at the Rust Boundary

`GraphemeClusterSegmenter::segment_str` yields UTF-8 byte offsets, whereas the locked public position format uses UTF-16. [VERIFIED: Context7 `/unicode-org/icu4x`; https://docs.rs/icu_segmenter/2.3.0/icu_segmenter/struct.GraphemeClusterSegmenter.html; `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] Build one boundary vector per touched text block per command: translate every byte boundary through the existing UTF-8→UTF-16 helper, sort/deduplicate, and use binary search for validity and previous/next navigation. [ASSUMED]

The current helper explicitly validates only scalar boundaries: “This validates Unicode scalar boundaries only. Grapheme-safe editing is a later editor-layer responsibility.” [VERIFIED: `crates/flow-core/src/anchor/mod.rs:77-80`] Phase 2 should layer `resolve_grapheme_position` on top of byte/UTF-16 conversion and return a distinct stable grapheme-boundary error without snapping. [ASSUMED]

ICU4X `icu_segmenter` 2.3.0 states that its grapheme cluster segmenter is compatible with UAX #29 exact `Version 17.0.0`. [VERIFIED: https://docs.rs/icu_segmenter/2.3.0/src/icu_segmenter/grapheme.rs.html; value quoted verbatim] The companion `icu_segmenter_data` 2.3.0 source metadata separately records exact source tags `cldr = { tagged = "48.2.1" }`, `icuexport = { tagged = "release-78.1rc" }`, and `segmenter_lstm = { tagged = "v0.1.0" }`; record these values separately from the crate version. [VERIFIED: https://docs.rs/crate/icu_segmenter_data/2.3.0/source/Cargo.toml.orig; values quoted verbatim] The crate's exact feature facts are `default = [compiled_data, auto]` and `auto = [lstm]`. [VERIFIED: `cargo info icu_segmenter@2.3.0`, 2026-08-25; values quoted verbatim] Use `default-features = false` with only `compiled_data` because Phase 2 needs grapheme segmentation, not dictionary line/word segmentation, and confirm the resolved graph in Wave 0. [ASSUMED]

Vendor the official Unicode 17.0.0 `GraphemeBreakTest.txt`; the file observed in research identifies itself as `GraphemeBreakTest-17.0.0`, is 126,570 bytes, and has SHA-256 `e2d134d2c52919bace503ebb6a551c1855fe1a1faec18478c78fff254a1793ec`. [VERIFIED: https://www.unicode.org/Public/17.0.0/ucd/auxiliary/GraphemeBreakTest.txt, fetched and hashed 2026-08-25] Record the source URL, checksum, ICU4X crate version, and ICU4X tested Unicode/CLDR/ICU-export tags in a machine-readable provenance fixture. [ASSUMED]

### Pattern 4: Directional Selection and Structural Compatibility Matrix

Keep `Selection { anchor, focus }` directional in every DTO. Build a preorder `DocumentOrderIndex` in Rust over editable text blocks; normalize to ordered start/end only while compiling a command. [ASSUMED] The existing public endpoint shape is exactly `node_id`, `utf16_offset`, and `affinity`, whose affinity values are exactly `Forward` and `Backward`. [VERIFIED: `crates/flow-core/src/model/mod.rs:227-240`; verbatim values quoted]

Use the same public endpoint shape for atomic nodes with an explicit discriminated-by-node-kind sentinel contract. For image, whole-table, and page-break nodes, `utf16_offset = 0` is the edge before the atom and is valid only with `Forward`; `utf16_offset = 1` is the edge after the atom and is valid only with `Backward`. A forward whole-atom selection is `anchor = (atom, 0, Forward)`, `focus = (atom, 1, Backward)`; the reverse selection swaps those endpoints. Any other offset/affinity combination rejects without snapping. [ASSUMED] A collapsed atomic edge is a navigation position adjacent to the atom, never an interior text caret; selecting or removing the atom uses the complete 0→1 range. Table-cell text continues to use ordinary text-block UTF-16 positions, while 0/1 sentinels address only the whole table. [ASSUMED]

| Operation | Supported target | Required result |
|-----------|------------------|-----------------|
| Insert/replace/delete | Same text block; or compatible text endpoints in one structural container | Validate both grapheme boundaries; preserve leading-block semantics; delete covered compatible intermediates; return accepted caret. [ASSUMED] |
| Cross-block replacement | Paragraph↔paragraph, equal-level heading↔heading, or paragraph children of sibling items in the same list | Merge only when the compatibility matrix explicitly permits it; otherwise reject atomically with visible reason. [ASSUMED] |
| Split | Paragraph, heading, list-item paragraph, table-cell paragraph | Apply the exact D-08 Enter behavior; create stable IDs for new blocks and explicit old→new offset mapping. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] |
| Backspace/Delete merge | Adjacent compatible text blocks | Preserve inline marks, leading compatible block attributes, and an exact inverse subtree. [ASSUMED] |
| Range formatting | Any text-block span that does not traverse an unsupported atomic/container boundary | Split runs at grapheme-valid UTF-16 endpoints, apply one mark delta, normalize runs, expose mixed state. [ASSUMED] |
| Image/whole-table/page break | Exact atomic 0/1 sentinel range or explicit atomic command only | Validate the sentinel/affinity pair; never consume or merge through ordinary text deletion; always retain an editable paragraph. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] |

Treat a cross-block range that crosses an image, table, page break, or incompatible ancestor as unsupported in the MVP and return a deterministic reason; do not silently skip atomic content. [ASSUMED] This bounded rule satisfies text-flow editing while avoiding an underspecified arbitrary-subtree deletion language.

### Pattern 5: Semantic Commands Compile to Exact Operations

Expose semantic commands such as `ReplaceSelection`, `SplitTextBlock`, `MergeTextBlocks`, `SetInlineMarks`, `SetBlockAttributes`, `ToggleList`, `InsertImage`, `ReplaceImage`, `RemoveAtomicBlock`, `InsertTable`, `EditTable`, and `InsertPageBreak`; do not let the UI send raw node trees or arbitrary batches. [ASSUMED] Each compiler validates revision, selection, graphemes, structure, and limits, then emits forward operations, exact inverse operations containing removed runs/subtrees/attributes, explicit forward/inverse anchor mappings, and a bounded transaction-private anchor preimage for any owner whose target is removed. Removed subtree bytes alone are never sufficient for exact anchor restoration. [ASSUMED]

The current operations are exactly `CreateDocument`, `DeleteDocument`, `ReplaceText`, `SetNodeStyle`, `InsertNode`, `DeleteNode`, and `SetField`; the current anchor transformations are exactly `TextEdit` and `NodeInvalidated`. [VERIFIED: `crates/flow-core/src/transaction/mod.rs:136-169`; `crates/flow-core/src/anchor/mod.rs:159-176`; verbatim variant names quoted] Extend these closed enums for run replacement, split, merge, subtree insertion/removal, and attribute changes. Never infer focus restoration from whatever IDs remain after a mutation. [ASSUMED]

The current range compiler rejects cross-node ranges before resolving text: `if range.start.node_id != range.end.node_id { return Err(CommandError::InvalidRange); }`. [VERIFIED: `crates/flow-core/src/transaction/mod.rs:840-853`; exact code quoted] Replace that assumption with document-order and compatibility validation rather than bypassing the transaction service.

Lock the following anchor behavior in the operation contract before implementation. These are recommended Phase 2 mapping rules. [ASSUMED]

| Transform | Forward mapping | Exact inverse requirement |
|-----------|-----------------|---------------------------|
| Same-block replace `[s,e)` with inserted UTF-16 length `n` | Reuse current `TextEdit` affinity behavior; before `s` stays, after `e` shifts by `n-(e-s)`, interior anchors invalidate, endpoints follow affinity. [VERIFIED: current behavior in `crates/flow-core/src/anchor/mod.rs:220-284`] | Store removed marked runs verbatim; inverse replace restores them and produces its own mapping by applying the inverse operation. [ASSUMED] |
| Split block `A` at `s` into retained `A` and new `B` | Offsets `<s` remain on `A`; offsets `>s` map to `B` at `p-s`; exactly `s` with backward affinity stays `A@s`, forward affinity maps `B@0`. [ASSUMED] | Inverse merge names both IDs and restores the original block attributes/runs; never generate a replacement ID. [ASSUMED] |
| Merge adjacent `A` (length `a`) + `B` | Positions on `A` stay; every `B@p` maps to `A@(a+p)` with affinity preserved. [ASSUMED] | Inverse split stores `B`'s original ID, complete attributes/runs, and exact split offset `a`. [ASSUMED] |
| Compatible cross-block replace from `A@s` through `B@e` | `A` before `s` stays; `B` after `e` maps to `A@(s+n+p-e)`; anchors in removed suffix/intermediate blocks/removed prefix return explicit `Deleted(tombstone)`; accepted editor caret is returned separately at `A@(s+n)` with backward affinity. [ASSUMED] | Store every removed run/subtree with original IDs/order and both endpoint attributes **plus** one exact owner-keyed preimage entry for every affected selection/session/field anchor; inverse restores structure first, verifies it, then restores those endpoints exactly. [ASSUMED] |
| Mark/block-attribute change | Identity mapping; logical text positions and direction remain unchanged. [ASSUMED] | Store exact previous typed attribute/mark sets, including absence versus explicit value. [ASSUMED] |
| Explicit atomic removal | Anchors inside the removed atomic subtree return explicit `Deleted(tombstone)`; command result separately selects the adjacent/created editable paragraph. Persistent fields become `TargetDeleted { original, tombstone }` only if their exact prior state is recorded. [ASSUMED] | Store the full atomic subtree, original parent/index, asset reference, and original ID **and** the owner-keyed anchor preimage; inverse insertion must restore original IDs before applying the preimage to the selection/session/fields. [ASSUMED] |
| Explicit atomic insertion | Pre-existing text/atomic anchors remain identity-mapped; the inserted node exposes only its new exact 0/1 sentinel edges, and the command returns the required adjacent editable-paragraph caret explicitly. [ASSUMED] | Inverse removal names the same node ID/parent/index and purges no shared asset bytes unless the exact inverse persistence plan proves the asset became unreachable. [ASSUMED] |

Use a two-part reversible deletion contract. The public mapping result is closed and non-guessing: `Live(LogicalPosition)`, `Deleted { tombstone: TombstoneToken }`, or `Invalidated { reason }`; the opaque tombstone identifies only a transaction-local deletion slot and never authorizes retargeting. [ASSUMED] The inverse transaction privately stores an `AnchorPreimage` for every affected logical anchor with: deterministic tombstone token, deleted-root ID, exact owner identity (`SelectionAnchor`/`SelectionFocus` plus session generation, or `Field(field_id)`), exact pre-command `node_id + utf16_offset + affinity`, and the owner's complete prior anchor state. [ASSUMED]

Persistent field deletion policy is fail-closed: a delete may proceed only when Rust can record a reversible preimage for every affected field and atomically change it to `TargetDeleted { original, tombstone }`; otherwise the entire delete rejects before mutation. [ASSUMED] Undo first restores and validates the original subtree IDs/order, then consumes the transaction-private preimage to restore the exact prior field state (`GraphemeSafe`, `LegacyInvalid`, or an earlier `TargetDeleted`) and the exact active-session directional selection. Redo reapplies the same deterministic tombstone slots and explicit deleted states. After recovery creates a new `session_generation`, merely opening the document does not revive the historical selection; an explicit accepted `Undo` validates the stored original owner/generation and subtree, then rebinds the two transaction-owned endpoint roles to the new active generation while restoring their exact recorded logical positions. [ASSUMED] A current editor session is still not stored as canonical document state and session-only selection changes remain nonundoable; the durable transaction record carries only mutation-specific inverse preimage data required for deterministic undo/replay, not a current-session snapshot. [ASSUMED]

Bound preimages by the existing document field ceiling (exact `fields: 10_000`) plus at most two active selection endpoints, deduplicate by owner identity, and charge their encoded bytes to the existing exact `transaction_bytes: 8 * 1024 * 1024` ceiling; overflow, duplicate owners, token/root mismatch, or an endpoint outside the restored subtree rejects the transaction/recovery record. [VERIFIED: `crates/flow-core/src/schema/mod.rs:68-82`; values quoted verbatim] Serialize the private preimage and deterministic tombstone-slot table inside the integrity-checked transaction/recovery record so reload/replay can verify canonical deletion/undo/redo without exposing the preimage through the public editor DTO. [ASSUMED]

Do not try to algebraically invert a lossy forward `AnchorMapping`; replay the exact inverse operation, restore its verified private preimage, and collect the mapping produced in that direction, as the existing undo/redo path already obtains mapping from applied operations. [VERIFIED: `crates/flow-core/src/transaction/mod.rs:610-645`] Property tests must assert delete→undo→redo→undo equality for canonical bytes, IDs, deterministic tombstones, directional selection/session endpoints, every affected field anchor state, and history cursor. [ASSUMED]

### Pattern 6: Immutable Editor Snapshot, Not Canonical JSON Parsing in React

Extend `OperationResult` with a closed `EditorViewDto` containing the semantic block/field projection, directional selection, formatting state (`on | off | mixed` per mark), enabled capabilities with localized reason keys, pending typing attributes, history availability, limits, and durability/status metadata. [ASSUMED] The current response already groups exact fields `session`, `view`, and `commit`; the current session holds exact fields `canonical_json`, `canonical_hash`, `document_id`, `revision`, `next_command_target`, and `history`. [VERIFIED: `crates/flow-core/src/lib.rs:175-207`; verbatim field names quoted]

Add a Rust-owned noncanonical `EditorSessionState` bound to exact `document_id`, accepted document `revision`, and a nonpersisted `session_generation`. It owns the directional selection, pending typing marks, current block/mark projection, and capability reasons. `SetSelection` and collapsed-selection formatting are closed Rust session commands: they may advance only `session_generation`, never the document revision, transaction history, audit history, or IndexedDB state. [ASSUMED] Range formatting and text/structure mutations remain document commands; only they create transactions/revisions and become undoable. After reload/recovery, reconstruct default session state from the accepted document. After a document mutation, update session positions only from its explicit anchor mapping and reject stale session commands whose bound document revision no longer matches. [ASSUMED]

The controller stores the accepted returned object by reference and publishes a new object only after Rust acceptance and persistence. `useSyncExternalStore(subscribe, getSnapshot)` requires `subscribe` to return cleanup and `getSnapshot` to return a cached/stable identity while underlying data is unchanged; React may perform a second consistency read during a concurrent transition. [VERIFIED: Context7 `/react/react/v19.2.7`; https://react.dev/reference/react/useSyncExternalStore]

```ts
// Pattern adapted from official React external-store requirements. [CITED: https://react.dev/reference/react/useSyncExternalStore]
type EditorSnapshot = Readonly<{
  revision: number;
  document: ReadonlyArray<SemanticBlockDto>;
  selection: DirectionalSelectionDto;
  formatting: FormattingProjectionDto;
}>;

const snapshot = useSyncExternalStore(
  controller.subscribe,
  controller.getSnapshot,
);
```

Keep accepted canonical/editor state out of component `useState` and reducers. TypeScript retains only the exact accepted Rust `EditorViewDto` object plus physical browser concerns: transient menu/focus/hover state and composition event/candidate state. It does not compute pending marks, mixed formatting, capabilities, or a replacement selection. [ASSUMED]

### Pattern 7: Native Input/IME State Machine

Attach native `beforeinput`, `input`, `compositionstart`, `compositionupdate`, `compositionend`, `paste`, `keydown`, and selection listeners to the controlled host through a ref/effect. React explicitly documents that `onBeforeInput` is not the native `beforeinput` event and is implemented as a polyfill. [CITED: https://react.dev/reference/react-dom/components/common]

Use these controller states and transitions. The state names are recommended design values. [ASSUMED]

```text
Idle
  compositionstart -> Composing(baseRevision, logicalBookmark, candidate="", cancelRequested=false, browserCancelObserved=false)
  cancellable beforeinput(non-composition) -> translate -> one Rust command
  paste(text/plain) -> one ReplaceSelection command

Composing
  compositionupdate / insertCompositionText input -> replace transient candidate only
  Escape -> set cancelRequested=true; request host cancellation; wait for compositionend
  browser cancel sequence -> set browserCancelObserved=true
  compositionend when either cancel flag is true -> Idle, zero commands
  compositionend otherwise -> Committing(one ReplaceSelection(candidate, even when empty) at bookmark)
  accepted revision changes -> Invalidated -> restore accepted projection + alert

Committing
  Rust+persistence accept -> publish snapshot -> Idle
  Rust/persistence reject -> restore accepted snapshot -> alert -> Idle
```

Do not commit `insertCompositionText` events. Input Events Level 2 specifies composition-related `beforeinput`/`input` activity and identifies cases that are noncancelable; MDN also warns that `beforeinput` may be absent or noncancelable for IME, autocomplete, spell checking, password managers, and similar edits. [CITED: https://www.w3.org/TR/input-events-2/; https://developer.mozilla.org/en-US/docs/Web/API/Element/beforeinput_event] Treat unexpected or noncancelable DOM mutation as an observation: immediately restore the last accepted projection, update only the transient candidate if composing, and never scrape mutated DOM into a command. [ASSUMED]

UI Events orders composition start, updates, and end, and defines keyboard `isComposing` during an active composition. [CITED: https://www.w3.org/TR/uievents/; https://developer.mozilla.org/en-US/docs/Web/API/InputEvent/isComposing] Do not depend on canceling `compositionupdate` or `compositionend`. A local Chromium `143.0.7499.4` probe through Playwright `1.57.0` observed `compositionstart`, `compositionupdate`, noncancelable `beforeinput`/`input` with `inputType="insertCompositionText"`, and one eventual `compositionend`; Chromium reported some composition events cancelable differently from current W3C tables. [VERIFIED: local Playwright/CDP event probe, 2026-08-25] Therefore the standard defines the intended model, while Playwright target-browser tests define actual supported behavior. [ASSUMED]

Never infer cancellation from `compositionend.data === ""` alone. An empty noncancelled candidate can be a real replacement/deletion when the composition bookmark spans text; submit the one atomic `ReplaceSelection` command. [ASSUMED] Cancellation is established only by explicit controller state (`cancelRequested`), a tested browser-cancel event sequence (`browserCancelObserved`), or base-revision invalidation. A noncancelled empty candidate at a collapsed bookmark is an accepted semantic no-op: it creates no document revision/history entry, but the adapter still records exactly one completed command attempt for testability. [ASSUMED]

For the real IME browser lane, use Chromium CDP `Input.imeSetComposition` and `Input.insertText` through Playwright `browserContext.newCDPSession`; both CDP composition commands and the session are Chromium-specific/experimental, so keep this as a target-browser test, not an application dependency or cross-browser guarantee. [CITED: https://chromedevtools.github.io/devtools-protocol/tot/Input/; https://playwright.dev/docs/api/class-browsercontext#browser-context-new-cdp-session]

Paste reads only `event.clipboardData.getData("text/plain")`, checks the same size and grapheme/selection path, prevents default where cancelable, and submits one `ReplaceSelection`. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/ClipboardEvent/clipboardData] Ignore HTML entirely in Phase 2. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]

### Pattern 8: One Visible Semantic Document and a Short-Lived Input Proxy

Render one labeled editor region containing native `<p>`, `<h1>`–`<h6>`, `<ol>`, `<ul>`, `<li>`, `<figure>/<img>`, `<table>/<thead>/<tbody>/<th>/<td>`, and a labeled focusable separator for page breaks. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`; `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-UI-SPEC.md`] The input proxy must not mirror the whole document. Keep it empty outside active input and use it only to receive IME/keyboard events; the semantic projection remains the single accessible document text. [ASSUMED]

Phase 2 also projects every existing Phase 1 field descriptor without adding authoring or fill mutations. The current field-kind variants are exactly `Text`, `Checkbox`, `RadioGroup`, `Select`, `Signature`, and `Button`. [VERIFIED: `crates/flow-core/src/model/mod.rs:156-175`; variant names quoted verbatim] Rust orders `GraphemeSafe` fields at their logical anchors and emits a localized, focusable non-editing `FieldProjectionDto` containing stable ID, label/name, kind, required/read-only state, default-value summary, and options. React renders that DTO as a keyboard-focusable native/semantic group (`<span role="group" tabindex="0">` inline where valid, with visible label and read-only value text), not as a fillable widget and never as canonical HTML. [ASSUMED] `LegacyInvalid` and `TargetDeleted` fields remain preserved and appear in a labeled, keyboard-navigable “Fields needing placement review” section at the end of the editor region with the stable reason and original target summary; they are never placed at a guessed DOM position. Undo of a deletion returns a `TargetDeleted` descriptor to its exact preimage state before projection. [ASSUMED] This closes QUAL-04 navigation for already stored descriptors while Phase 5 retains field creation, editing, filling, validation, and PDF-widget behavior. [ASSUMED]

Project each editable block with its stable node ID and each inline span with a Rust-provided cumulative UTF-16 start, but treat those DOM attributes as view indexes only. On browser `selectionchange`, preserve direction from `Selection.anchorNode/anchorOffset` and `focusNode/focusOffset`, translate through the last accepted projection (never current `textContent`), and ask Rust to validate the exact candidate. [ASSUMED] After each accepted render, translate the returned logical anchor/focus back into DOM text-node offsets and restore the DOM range/focus; formatting runs may reconcile without changing logical positions. [ASSUMED]

Arrow, word, Home/End, Backspace, and Delete paths should submit navigation/edit intent to Rust rather than trust browser caret movement. Pointer hit-testing may choose the nearest member of the Rust-projected boundary list using the explicit pointer side/affinity, after which Rust validates it again; malformed API/WASM `SetSelection` values always reject without snapping. [ASSUMED] This distinction makes pointer placement an explicit view navigation policy while preserving D-02's fail-closed public boundary.

During composition, expose the candidate once: let the focused input proxy own the active candidate for input-method interaction and mark only the duplicate visual candidate span as accessibility-hidden; after acceptance, clear the proxy and render the committed text in the semantic tree. [ASSUMED] Never apply `aria-hidden` to the focusable input host itself. [CITED: https://www.w3.org/TR/wai-aria-1.2/#aria-hidden]

Use `role="status"` (polite, atomic semantics) for noncritical success/durability updates and `role="alert"` with visible text for failures; WAI-ARIA defines these roles' implicit live-region behavior. [CITED: https://www.w3.org/TR/wai-aria-1.2/#status; https://www.w3.org/TR/wai-aria-1.2/#alert] Use a labeled toolbar and deliberately managed roving or all-tab-stop focus per the UI contract, and restore the logical editor selection after toolbar commands. [CITED: https://www.w3.org/WAI/ARIA/apg/patterns/toolbar/]

Automated semantic assertions do not prove screen-reader editing quality. The required Edge-on-Windows plus Windows screen-reader UAT is an **unavailable external checkpoint in this workspace**, not a locally satisfied gate. [VERIFIED: local application/OS probes, 2026-08-26] Local execution can and must run Chromium keyboard/accessibility-tree checks and a macOS VoiceOver smoke pass, but those are fallback evidence only; the phase verification must record the external checkpoint as outstanding and must not claim final QUAL-04 support-matrix closure until that evidence exists. [ASSUMED]

### Pattern 9: Bounded Embedded Content and Structural Complexity

The following values are recommended MVP policy and must become named Rust constants, localized stable failures, boundary tests at max/max+1, and an editor-capability projection. They are not existing project values. [ASSUMED]

| Budget | Recommended maximum | Rationale / behavior |
|--------|---------------------|----------------------|
| Image encoded bytes | 8 MiB | Bounded upload and IndexedDB copy cost; reject before decode. [ASSUMED] |
| Image dimensions/pixels | 8,192×8,192 and 24,000,000 pixels | Prevent dimension bombs; apply both per-axis and total-pixel checks. [ASSUMED] |
| Image decoded allocation | 96 MiB | Covers 24M RGBA pixels without permitting unbounded decoder allocation. [ASSUMED] |
| Image formats | PNG and baseline/progressive JPEG by signature | Matches locked scope; browser MIME and filename are advisory only. [CITED: https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html] |
| Alt text | 4,096 UTF-8 bytes | Enough for useful descriptions while bounding canonical metadata. [ASSUMED] |
| Table | 50 rows × 20 columns, at most 1,000 cells | Bounded DOM/transaction cost; default insertion remains 2×2. [ASSUMED] |
| Nesting | Lists at most 8; nested tables prohibited | Prevent pathological recursion and explicitly keeps complex tables deferred. [ASSUMED] |
| Inline runs | 4,096 per text block after normalization | Bounds alternating-mark fragmentation; merge equal adjacent marks before checking. [ASSUMED] |
| Heading level | Exact integers 1–6 | Matches native `<h1>`–`<h6>` semantics; reject 0, 7, and nonintegers. [ASSUMED] |
| Font size | 6,000–288,000 millipoints inclusive (6–288 pt) | Store an integer fixed-point value; reject rather than clamp. [ASSUMED] |
| Text color | Exact uppercase `#RRGGBB` at the command boundary; canonical `[u8; 3]` RGB | Reject lowercase, shorthand, alpha, named colors, CSS functions, and out-of-range bytes; never persist arbitrary CSS. [ASSUMED] |
| Run language | Exact v2 allowlist `uk-UA` or `en-US` | Distinct from UI locale and document default; unknown new values reject rather than normalize. [ASSUMED] |
| Semantic font family | Exact v2 authoring IDs `noto-sans`, `noto-serif`, `noto-sans-mono` | Rust advertises the closed catalog; unknown new IDs reject and no browser CSS fallback becomes canonical. [ASSUMED] |
| Paragraph alignment | Exact values `start`, `center`, `end`, `justify` | Logical start/end remain script-extensible; reject physical aliases and arbitrary CSS. [ASSUMED] |
| Paragraph spacing | Before/after each 0–144,000 millipoints inclusive (0–144 pt) | Fixed-point integer values; reject negatives, overflow, fractions, and clamp-free out-of-range input. [ASSUMED] |
| Text touched per semantic command | 4 MiB | Aligns with the current v1 per-text-node limit so migrated content is not newly uneditable. [VERIFIED: `crates/flow-core/src/schema/mod.rs:68-82`; exact source expression: `text_node_bytes: 4 * 1024 * 1024`] |
| Composition candidate | 64 KiB UTF-8 and 32,768 UTF-16 code units | Bounds transient memory and eventual command size while allowing ordinary IME use. [ASSUMED] |
| Plain-text paste | 256 KiB UTF-8 before encoding | Leaves transport headroom beneath the separate serialized command-envelope maximum in the next row; pathological escaping still fails closed. [ASSUMED] |
| Semantic mutations / command envelope | 256 derived mutations and 1 MiB final serialized command DTO | Image bytes never enter this DTO; check actual encoded bytes, not just source-string length. [ASSUMED] |
| Asset staging pool | At most 2 live receipts, 16 MiB total encoded bytes, 5-minute receipt lifetime | Bounds duplicate/retry memory; expiry, cancellation, base-revision change, reload, and redemption release bytes deterministically. [ASSUMED] |

The existing persisted ceilings are exactly `canonical_bytes: 64 * 1024 * 1024`, `tree_depth: 128`, `semantic_nodes: 200_000`, `total_text_bytes: 32 * 1024 * 1024`, `text_node_bytes: 4 * 1024 * 1024`, `styles: 4_096`, `assets: 10_000`, `fields: 10_000`, `transaction_operations: 10_000`, `transaction_bytes: 8 * 1024 * 1024`, `recovery_records: 10_000`, and `recovery_bytes: 64 * 1024 * 1024`. [VERIFIED: `crates/flow-core/src/schema/mod.rs:68-82`; values quoted verbatim] Phase 2's tighter interaction budgets supplement, not replace, these canonical/recovery limits. [ASSUMED]

Use `image::ImageReader` to inspect/guess format and `image::Limits` before full decode, then validate decoded color-buffer size and hash the accepted original bytes in Rust. [CITED: https://docs.rs/image/0.25.10/image/struct.ImageReader.html; https://docs.rs/image/0.25.10/image/struct.Limits.html] Never trust `File.type`, extension, or a browser-created preview as validation. [CITED: https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html]

Image bytes use a dedicated noncanonical staging seam, never `InsertImage` JSON. The browser passes one bounded `Uint8Array` directly to a WASM `stage_asset`-class function. Rust checks encoded size/signature/dimensions/pixels/allocation, computes ID/hash/metadata, retains the bytes in a bounded revision-bound staging table, and returns an opaque one-use `AssetReceiptDto`; a receipt expires on cancel, base-revision change, reload, successful redemption, or bounded timeout. [ASSUMED] The semantic `InsertImage` command carries only that receipt plus the Rust-issued ID/hash/media/dimensions and `ImageAccessibility`; it cannot supply replacement bytes or metadata. Rust redeems the receipt once and returns document operations plus the asset record in the same planned persistence result; IndexedDB commits snapshot/transaction/audit/asset/head atomically before React publishes the accepted snapshot. [ASSUMED]

The current browser storage DTO spells asset fields exactly `recordFormatVersion`, `contentHash`, and `bytes`, with `bytes: readonly number[]`. [VERIFIED: `web/persistence/indexeddb-store.ts:120-124`; field/type text quoted verbatim] Phase 2 must version the physical asset envelope so new staged bytes cross WASM/persistence as `Uint8Array`/`ArrayBuffer`, while the recovery reader still accepts and verifies legacy number-array records; never expand 8 MiB bytes into the 1 MiB semantic command envelope. [ASSUMED]

Formatting migration follows the same no-invention rule: exact v1 `"Noto Sans"` maps to `noto-sans`; other legacy family strings remain an explicit `LegacyUnknown(original)` value that is visible but cannot be newly authored until a later reviewed mapping exists. [ASSUMED] Missing run-language marks inherit the preserved document language without writing a guessed direct mark. [ASSUMED]

Recommended new stable failures include grapheme boundary, incompatible selection, stale selection/composition, composition/paste limit, image type/dimension/decode limit, table/nesting/run limit, and semantic-command expansion limit; final exact code strings should be locked in the first schema/command contract task and thereafter tested verbatim. [ASSUMED]

### Anti-Patterns to Avoid

- **DOM or `contenteditable` as truth:** browser mutations and DOM ranges are view-local and unstable; project only from accepted Rust snapshots. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]
- **A parallel TypeScript AST/undo stack:** it creates two selection, validation, migration, and inverse authorities. Keep only immutable DTO projections in TypeScript. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]
- **UTF-16 scalar validation presented as grapheme safety:** surrogate-safe offsets can still split combining sequences, ZWJ emoji, or regional indicators. Use UAX #29 extended clusters in Rust. [CITED: https://unicode.org/reports/tr29/]
- **Snapping malformed API positions:** it hides caller bugs and can misplace future voice/field operations. Reject with a stable error. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]
- **Committing each composition update:** it duplicates text and destroys atomic undo. Keep candidate text transient and commit once on end. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]
- **Generic UI `Batch`/raw subtree commands:** they bypass semantic invariants and make exact inverses/anchor maps ambiguous. UI sends closed intent commands. [ASSUMED]
- **Run IDs as caret anchors:** ordinary formatting normalization would invalidate selection. Anchor text positions to stable editable block IDs. [ASSUMED]
- **Deleting across atomic blocks by accident:** cross-structure range commands must reject or use an explicit atomic command. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]
- **CSS font names as deterministic font identity:** Phase 2 stores stable family IDs/capabilities; Phase 3 maps them to pinned font binaries. [ASSUMED]
- **HTML paste/import:** it introduces sanitization and semantic-conversion scope explicitly deferred from Phase 2. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]
- **Visual-only accessibility tests:** semantic markup, keyboard parity, status announcement, focus return, and supported screen-reader UAT are separate gates. [ASSUMED]
- **Fake pages:** keep a continuous page-neutral surface until Phase 3 produces real fragments and pagination. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Unicode grapheme rules/data | Homegrown combining-mark/emoji regex | ICU4X `GraphemeClusterSegmenter` + official Unicode test data | UAX #29 contains script, prepend, extend, spacing-mark, RI, emoji-ZWJ, and data-version behavior that simple regexes miss. [CITED: https://unicode.org/reports/tr29/] |
| PNG/JPEG decoding | Custom header/parser/decoder | `image` with only PNG/JPEG features and explicit limits | Codec parsing is an untrusted-input boundary with allocation and malformed-input hazards. [CITED: https://docs.rs/image/0.25.10/image/] |
| React external-store consistency | Ad hoc render subscriptions | `useSyncExternalStore` | React supplies subscription cleanup and concurrent consistency checks when snapshot identity is stable. [CITED: https://react.dev/reference/react/useSyncExternalStore] |
| IME simulation as proof | Dispatch only synthetic `CompositionEvent`s | Playwright Chromium CDP IME lane plus controller unit tests | Synthetic events test translation code, not browser/IME event sequencing. [CITED: https://chromedevtools.github.io/devtools-protocol/tot/Input/] |
| Rich editor framework state | ProseMirror/Lexical/Slate document model | Owned Rust schema/transactions + React projection | A third-party editor model conflicts with locked canonical ownership and exact inverses. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] |
| ARIA replicas of native structure | Div/role trees for paragraphs/lists/tables | Native semantic HTML first | Native elements give stronger platform semantics and match the locked UI contract. [CITED: https://www.w3.org/WAI/ARIA/apg/practices/read-me-first/] |
| HTML sanitization/conversion | A partial sanitizer or HTML-to-Flow mapper | Plain-text-only paste this phase | Rich clipboard import is deferred and deceptively broad. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] |

**Key insight:** own the editor semantics and transaction algebra, but reuse Unicode data/algorithms, codecs, browser standards, React's store contract, and official conformance fixtures. [ASSUMED]

## Runtime State Inventory

This section is required because Phase 2 migrates already durable schema-v1 documents and changes the physical asset envelope; repository edits alone cannot update browser-resident IndexedDB state. [ASSUMED]

| Category | Items Found | Action Required |
|----------|-------------|-----------------|
| Stored data | IndexedDB database exact name `flowpdf-foundation`, database version `5`, record-store version `3`, and exact stores `snapshots-v3`, `transactions-v3`, `audits-v3`, `assets-v3`, `migration-sources-v3`, `storage-metadata`, and `document-heads`. [VERIFIED: `web/persistence/indexeddb-store.ts:154-169`; values quoted verbatim] Recovery reads exact collections `snapshots`, `transactions`, `audits`, `assets`, and `sources` plus one head. [VERIFIED: `web/persistence/indexeddb-store.ts:447-507`; collection names quoted verbatim] | Add compatibility decoding for v1 canonical snapshots and legacy number-array assets; establish a v2 canonical head only through a guarded atomic migration commit; retain immutable migration-source bytes and old record readability; never mutate/snap legacy anchors or delete old records as an incidental upgrade. Version transaction records to serialize integrity-checked deletion tombstone slots and bounded owner-keyed anchor preimages, and version only the physical asset record contract needed for `Uint8Array`; test cold open, corrupt/missing preimage rejection, migration, interruption, recovery, delete→undo→redo replay, and pre-Phase-2 fixtures. [ASSUMED] |
| Live service config | None found: Phase 2 introduces no deployed API, database service, dashboard, workflow, or remote editor configuration; persistence remains browser-local. [ASSUMED] | No external data migration. Keep any future service work out of this phase. [ASSUMED] |
| OS-registered state | None found: no launchd/systemd/task-scheduler registration is part of the repository or Phase 2 browser editor path. [ASSUMED] | No OS re-registration. The external Windows/Edge/AT environment below is test infrastructure, not migrated runtime state. [ASSUMED] |
| Secrets/env vars | No Phase 2 secret or renamed environment key was found. Existing build environment uses exact `CARGO_HOME`, `RUSTUP_HOME`, and `PATH` variables without a schema identifier. [VERIFIED: `scripts/check-phase1.mjs:12-22`; names quoted verbatim] | Preserve the existing pinned toolchain environment; introduce no secret-bearing asset receipt and persist no staging receipt across reload. [ASSUMED] |
| Build artifacts / installed packages / caches | The exact build command emits `target/wasm32-unknown-unknown/release/flow_wasm.wasm` and `web/generated`; the web builder recreates exact path `dist/web` and replaces its `generated` subtree. [VERIFIED: `package.json:12-13`; `scripts/build-web.mjs:10-29`; paths quoted verbatim] Browser module state and any bundler/compiler cache can retain the old WASM/schema contract until a clean reload/rebuild. [ASSUMED] | Rebuild release WASM/wasm-bindgen output, regenerate TypeScript bindings, clean/rebuild `dist/web`, invalidate derived build caches, and cold-reload the browser before migration/browser tests. These are rebuilds, not data migrations; do not delete user IndexedDB. [ASSUMED] |

The canonical runtime question is therefore answered explicitly: after source files change, old schema-v1 snapshots, transactions, migration sources, assets, recovery heads, generated WASM/JS, and loaded browser code still exist until the guarded migration/rebuild/reload paths run. [ASSUMED]

## Common Pitfalls

### Pitfall 1: Schema v1 Decoder Accidentally Becomes Schema v2

**What goes wrong:** changing `ContentNode` in place makes old fixtures deserialize using new assumptions, so v0→v1 may stop being a real historical hop. [ASSUMED]  
**Why it happens:** the current registry calls one v0→v1 function while the model module exposes only the current type. [VERIFIED: `crates/flow-core/src/schema/mod.rs:630-638`]  
**How to avoid:** freeze private legacy structs and golden bytes before changing current types; test v0→1→2, v1→2, v2 no-op. [ASSUMED]  
**Warning signs:** old fixture hashes change without a new migration hop, or v1 tests construct current v2 structs. [ASSUMED]

### Pitfall 2: Formatting Splits Selection Identity

**What goes wrong:** adding/removing a mark splits inline runs and invalidates a caret that targets a run ID. [ASSUMED]  
**Why it happens:** conflating storage chunks with semantic editing blocks. [ASSUMED]  
**How to avoid:** anchor to stable editable block ID + UTF-16 offset; normalize runs freely. [ASSUMED]  
**Warning signs:** formatting-only transactions need arbitrary selection relocation. [ASSUMED]

### Pitfall 3: Cross-Block Delete Has No Structural Algebra

**What goes wrong:** text deletion silently consumes a list/table/image, leaves invalid containers, or cannot undo exactly. [ASSUMED]  
**Why it happens:** extending a same-node string splice without a compatibility matrix and subtree inverse. [VERIFIED: current same-node guard in `crates/flow-core/src/transaction/mod.rs:840-853`]  
**How to avoid:** compile semantic operations through document order, explicit compatible endpoints, invariant checks, full removed subtree fragments, deterministic tombstone slots, and bounded owner-keyed preimages that restore selection/session/field anchors exactly. Subtree bytes alone do not restore invalidated anchor ownership. [ASSUMED]  
**Warning signs:** `DeleteText` directly mutates `Vec<BlockNode>`, undo reconstructs IDs, a field target is deleted without a reversible `TargetDeleted` preimage, or undo restores content but leaves selection/fields invalid. [ASSUMED]

### Pitfall 4: Composition Text Commits Twice

**What goes wrong:** `beforeinput`/`input` updates are committed and `compositionend` commits the same candidate again. [ASSUMED]  
**Why it happens:** assuming every browser text event represents final text. [CITED: https://www.w3.org/TR/input-events-2/]  
**How to avoid:** explicit composition state and cancel flags, transient candidate, one end transaction even when the noncancelled candidate is empty, and changed-revision invalidation. Never treat `compositionend.data === ""` as cancellation by itself. [ASSUMED]  
**Warning signs:** one IME word needs multiple undos, duplicate characters appear, or cancellation changes revision. [ASSUMED]

### Pitfall 5: `preventDefault` Is Treated as Universal

**What goes wrong:** a noncancelable IME/autocorrect event mutates the host and is mistaken for canonical state. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/Element/beforeinput_event]  
**How to avoid:** inspect `cancelable` and `isComposing`, observe `input`, then restore the last accepted projection; never scrape DOM. [ASSUMED]  
**Warning signs:** behavior differs between synthetic Vitest events and Chromium. [ASSUMED]

### Pitfall 6: Mutable Snapshot Identity Causes React Loops or Tearing

**What goes wrong:** `getSnapshot` creates a new object on every read, or an object mutates under the same identity. React may loop, show mismatched selection/toolbar state, or restart transitions. [CITED: https://react.dev/reference/react/useSyncExternalStore]  
**How to avoid:** cache immutable accepted snapshots by reference; publish only after commit. [ASSUMED]  
**Warning signs:** “The result of getSnapshot should be cached,” duplicate renders on unchanged state, or formatting lags selection. [CITED: https://react.dev/reference/react/useSyncExternalStore]

### Pitfall 7: Duplicate Accessibility Trees

**What goes wrong:** a hidden full-text input and visible semantic tree announce the document twice or expose conflicting focus. [ASSUMED]  
**How to avoid:** one semantic document copy; input proxy contains no full accepted text; composition candidate is exposed once; never hide a focusable host with `aria-hidden`. [CITED: https://www.w3.org/TR/wai-aria-1.2/#aria-hidden]  
**Warning signs:** accessibility snapshots contain duplicate paragraphs, or screen-reader browse/focus modes read different text. [ASSUMED]

### Pitfall 8: Image Header Validation Still Allows Allocation Bombs

**What goes wrong:** a valid small compressed file declares enormous dimensions or decodes beyond memory budget. [ASSUMED]  
**How to avoid:** enforce encoded bytes, signature, per-axis dimensions, pixel count, decoder allocation, decoded buffer size, and timeout/worker isolation in later heavy paths. [CITED: https://docs.rs/image/0.25.10/image/struct.Limits.html]  
**Warning signs:** validation calls full decode before reading limits, or trusts MIME/extension. [CITED: https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html]

### Pitfall 9: Phase 3 Scope Leaks into Phase 2

**What goes wrong:** fake lines/pages, CSS measurement, caret rectangles, BiDi visual arrows, or pagination block the semantic editor. [ASSUMED]  
**How to avoid:** keep logical order and semantic flow only; page break is intent; Phase 3 owns fragment geometry and visual movement. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]  
**Warning signs:** Phase 2 tests assert pixel coordinates or page counts. [ASSUMED]

## Code Examples

### ICU4X Extended Grapheme Boundaries

The constructor and iterator below are official ICU4X APIs; the byte→UTF-16 adapter and stable error are project code to add. [VERIFIED: Context7 `/unicode-org/icu4x`; https://docs.rs/icu_segmenter/2.3.0/icu_segmenter/struct.GraphemeClusterSegmenter.html]

```rust
use icu_segmenter::GraphemeClusterSegmenter;

fn grapheme_utf16_boundaries(text: &str) -> Result<Vec<u32>, AnchorError> {
    let segmenter = GraphemeClusterSegmenter::new();
    segmenter
        .segment_str(text)
        .map(|byte_offset| byte_offset_to_utf16(text, byte_offset))
        .collect()
}

fn require_grapheme_boundary(boundaries: &[u32], requested: u32) -> Result<(), AnchorError> {
    boundaries
        .binary_search(&requested)
        .map(|_| ())
        .map_err(|_| AnchorError::InvalidGraphemeBoundary)
}
```

The adapter/error names in the example are recommended, not existing source values. [ASSUMED]

### Native Event Registration Around a React Host

React recommends effects for synchronizing components with external systems; native `beforeinput` is required here because React's similarly named prop is polyfilled. [CITED: https://react.dev/reference/react/useEffect; https://react.dev/reference/react-dom/components/common]

```tsx
function InputHost({ controller }: { controller: EditorController }) {
  const hostRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    const host = hostRef.current
    if (host === null) return
    const onBeforeInput = (event: InputEvent) => controller.beforeInput(event)
    const onInput = (event: Event) => controller.input(event as InputEvent)
    const onPaste = (event: ClipboardEvent) => controller.paste(event)
    const onCompositionStart = (event: CompositionEvent) => controller.compositionStart(event)
    const onCompositionUpdate = (event: CompositionEvent) => controller.compositionUpdate(event)
    const onCompositionEnd = (event: CompositionEvent) => controller.compositionEnd(event)

    host.addEventListener('beforeinput', onBeforeInput)
    host.addEventListener('input', onInput)
    host.addEventListener('paste', onPaste)
    host.addEventListener('compositionstart', onCompositionStart)
    host.addEventListener('compositionupdate', onCompositionUpdate)
    host.addEventListener('compositionend', onCompositionEnd)
    return () => {
      host.removeEventListener('beforeinput', onBeforeInput)
      host.removeEventListener('input', onInput)
      host.removeEventListener('paste', onPaste)
      host.removeEventListener('compositionstart', onCompositionStart)
      host.removeEventListener('compositionupdate', onCompositionUpdate)
      host.removeEventListener('compositionend', onCompositionEnd)
    }
  }, [controller])

  return (
    <textarea
      ref={hostRef}
      aria-label={controller.inputLabel}
      value={controller.inputValue}
      onChange={(event) => controller.reactChangeFallback(event.currentTarget.value)}
    />
  )
}
```

This is a wiring pattern, not the complete Phase 2 accessibility/composition implementation. [ASSUMED]

### Real Chromium Composition Probe

Playwright exposes Chromium DevTools sessions, and CDP exposes `Input.imeSetComposition` plus `Input.insertText`. [CITED: https://playwright.dev/docs/api/class-browsercontext#browser-context-new-cdp-session; https://chromedevtools.github.io/devtools-protocol/tot/Input/]

```ts
const session = await page.context().newCDPSession(page)
await editorInput.focus()
await session.send('Input.imeSetComposition', {
  text: 'ї',
  selectionStart: 1,
  selectionEnd: 1,
})
await session.send('Input.insertText', { text: 'ї' })

await expect.poll(() => committedTransactions()).toBe(1)
await expect(editor).toContainText('ї')
```

The assertion helpers are project-test recommendations; the CDP calls are Chromium-only and experimental. [ASSUMED]

### Semantic Formatting Projection

Use native markup and expose toolbar state independently from document text. WAI-ARIA permits `aria-pressed="mixed"` for a tri-state toggle. [CITED: https://www.w3.org/TR/wai-aria-1.2/#aria-pressed]

```tsx
<button
  type="button"
  aria-pressed={bold === 'mixed' ? 'mixed' : bold === 'on'}
  onMouseDown={(event) => event.preventDefault()}
  onClick={() => controller.execute('toggleBold')}
>
  {messages.bold}
</button>
```

Preventing pointer-down focus loss is an implementation technique that still requires keyboard activation and explicit post-command focus/selection restoration tests. [ASSUMED]

## Recommended Planning Sequence

The phase is too coupled for horizontal “model first, entire UI later” planning. Use these small recoverable vertical increments. [ASSUMED]

1. **Dependency/provenance Wave 0:** run the fail-closed official-registry/source/integrity/lockfile gate, capture a release-WASM baseline, reject ICU if its measured raw/gzip delta exceeds the numerical budget, vendor the Unicode fixture/checksum, establish Vite without removing the Phase 1 inspector or gate, and add empty Phase 2 test lanes. No package is accepted through a human waiver. [ASSUMED]
2. **Lossless migration + leading tracer:** freeze private v1 records and migration fixtures, including empty legacy alt → `MissingLegacy` and invalid field anchors → `LegacyInvalid`; add schema v2 paragraph/run shape; Rust ICU boundary check; one directional caret; `ReplaceSelection`; immutable WASM editor/session DTO; React semantic paragraph; ordinary typing, plain paste, and one real IME commit/cancel path; atomic IndexedDB commit; undo/redo/reload/recovery. [ASSUMED]
3. **Text/selection algebra:** define text and atomic 0/1 sentinel positions, cross-block document order, directional selection, replace/delete, paragraph and heading split/merge, deterministic deletion tombstones, bounded owner-keyed preimages, and exact forward/inverse anchor restoration with delete→undo→redo property tests. [ASSUMED]
4. **Formatting/lists:** enforce the closed EDIT-04 values and bounds; keep selection, pending typing attributes, and capability projection in revision-bound Rust editor-session state; prove toolbar/shortcut parity and list enter/exit/nesting. [ASSUMED]
5. **Atomic structure and staged assets:** pass bounded `Uint8Array` bytes through the Rust/WASM staging seam; redeem an opaque one-use receipt into the atomic document+asset persistence plan; add simple table operations/navigation, page-break intent, and the always-editable-paragraph invariant. [ASSUMED]
6. **Accessibility and hardening:** project semantic content plus keyboard-navigable existing Phase 1 field descriptors without authoring/filling them; add focus restoration, one-copy composition behavior, locale parity, 320/1280 and 100–200-page contracts, resource max/max+1, migration/replay, and the complete Phase 1 regression gate. Run local VoiceOver+Chromium fallback evidence and leave the unavailable Edge/Windows/screen-reader UAT checkpoint explicitly outstanding. [ASSUMED]

Each increment should end at the same publication boundary: command → Rust validation/transaction → persistence → immutable snapshot → semantic UI. [ASSUMED]

## State of the Art

| Old/insufficient approach | Current recommended approach | Evidence point | Impact for Phase 2 |
|---------------------------|------------------------------|----------------|--------------------|
| Scalar/surrogate-safe caret only | UAX #29 extended grapheme boundaries with pinned Unicode fixture | ICU4X 2.3.0 data tooling separates library and data versions. [VERIFIED: Context7 `/unicode-org/icu4x`] | Exact Rust boundary validation and repeatable native/WASM conformance. [ASSUMED] |
| Uncontrolled `contenteditable` | Controlled input proxy + accepted semantic DOM projection | Input Events/IME edits are not uniformly cancelable. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/Element/beforeinput_event] | Browser DOM cannot become canonical truth. [ASSUMED] |
| React component state mirroring a core store | Cached immutable snapshot through `useSyncExternalStore` | Official React 19 contract. [VERIFIED: Context7 `/react/react/v19.2.7`] | Concurrent reads see a consistent revision/selection/toolbar tuple. [ASSUMED] |
| Synthetic composition events only | Controller unit tests plus real target-Chromium CDP composition | Playwright CDP session and Chromium Input domain. [CITED: https://playwright.dev/docs/api/class-browsercontext#browser-context-new-cdp-session] | Detects actual event ordering and atomic commit behavior. [ASSUMED] |
| ARIA-only editor replica | Native semantic document plus minimal state ARIA | WAI-ARIA recommends native semantics where available. [CITED: https://www.w3.org/WAI/ARIA/apg/practices/read-me-first/] | Screen-reader browse navigation follows real document structure. [ASSUMED] |
| Browser MIME/filename image trust | Signature, dimension, pixel, allocation, decoded-size validation in Rust | OWASP file-upload defense in depth and `image` limits. [CITED: https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html] | Bounded hostile-image failure before publication. [ASSUMED] |

**Deprecated/outdated for this phase:** React `onBeforeInput` as if it were the native event. [CITED: https://react.dev/reference/react-dom/components/common] Also prohibited are DOM mutation scraping, full-document hidden input mirrors, selection anchored to inline-run IDs, and fake page geometry. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] Do not introduce `document.execCommand`; it would make the DOM mutation engine an editing authority. [ASSUMED]

## Assumptions Log

The user authorized recommended defaults. These assumptions are bounded implementation choices for the planner to lock; none expands Phase 3 or later product scope. [VERIFIED: parent task context]

| # | Claim group | Section(s) | Risk if Wrong / bounded response |
|---|-------------|------------|----------------------------------|
| A1 | The dependency gate plus lossless leading tracer and six-increment plan are the smallest recoverable vertical sequence. | Summary; Recommended Planning Sequence | Reorder tasks only if dependency analysis preserves the same end-to-end first tracer and numerical gates. |
| A2 | Editable block IDs, not inline-run IDs, are the correct stable text anchor; empty text blocks use zero runs, while pending marks and selection remain Rust-owned noncanonical session state. | Patterns 1 and 6 | Changing this after persisted v2 documents would be costly; lock with migration/anchor/session goldens first. |
| A3 | Private legacy v0/v1 structs, `MissingLegacy` image accessibility, `LegacyInvalid` field anchors, and permanent route/edge migration fixtures are the safe lossless v2 transition. | Patterns 1 and 2 | A different decoder isolation can work only if old canonical bytes/values remain deterministic, visible, and unsnapped. |
| A4 | Per-command grapheme boundary vectors and byte→UTF-16 conversion are performant enough for the current 4 MiB text-block ceiling. | Pattern 3 | Benchmark pathological touched blocks; cache by immutable block/revision if needed, without adding TS authority. |
| A5 | MVP cross-block replace supports compatible text endpoints in one container and rejects atomic/incompatible crossings. | Pattern 4 | If user research requires arbitrary subtree deletion, specify a separate explicit semantic command instead of weakening invariants. |
| A6 | The atomic 0/1 edge sentinel contract, semantic command catalog, deterministic tombstones, bounded owner-keyed deletion preimages, exact subtree/anchor inverses, and split/merge mappings are sufficient for Phase 2. | Patterns 4 and 5 | Add a closed command only when a UI/keyboard requirement demonstrates it; never expose raw batch/tree mutation, subtree-only undo, or ambiguous anchor relocation. |
| A7 | Rust-owned `EditorSessionState` includes revision-bound selection, pending typing attributes, formatting/capabilities and session generation; `EditorViewDto` projects it, and React caches only the exact accepted DTO plus physical composition state. | Pattern 6 | Adjust DTO shape via contract tests, but never persist/undo session-only changes or parse canonical JSON into parallel editor state. |
| A8 | The explicit-cancel Idle/Composing/Committing/Invalidated machine and unexpected-input restoration work for target Chromium behavior; empty `compositionend.data` is not cancellation. | Pattern 7 | Real Chromium tests must cover empty commit/deletion, explicit cancel, and invalidation; unsupported sequences fail visibly rather than relocating text. |
| A9 | An empty short-lived input proxy plus one semantic document copy and a nonauthoring field projection are screen-reader workable. | Pattern 8; Open Questions | Exercise local VoiceOver early and adapt focus/labeling without duplicating canonical text or adopting contenteditable truth. |
| A10 | The exact EDIT-04 catalog plus image/table/nesting/run/composition/256-KiB-paste/1-MiB-command/staging limits are appropriate MVP defaults. | Pattern 9 | Max/max+1 tests and surfaced capability/error reasons make each limit independently changeable before v1 release. |
| A11 | The recommended new error categories should become stable codes in the first command-contract task. | Pattern 9 | Exact strings remain to be locked; tests must prevent later accidental renames. |
| A12 | Native semantic elements, keyboard coverage, automated Chromium semantics, and local VoiceOver form fallback evidence are necessary but do not satisfy the unavailable Edge/Windows/screen-reader external checkpoint. | Pattern 8; Validation Architecture | Record the checkpoint as outstanding; never promote fallback evidence into a full supported-matrix claim. |
| A13 | Vite can replace the custom web entry/build as primary editor build while keeping the inspector and Phase 1 gates as secondary diagnostics. | Standard Stack; Planning Sequence | Run both paths during migration; remove no existing gate until equivalent evidence is green. |
| A14 | The exact Phase 2 font-family IDs `noto-sans`, `noto-serif`, and `noto-sans-mono` can be stored without resolving Phase 3 binaries/shaping. | Pattern 9; Anti-Patterns | Unknown new IDs reject; legacy unknown strings remain explicit rather than silently falling back. |
| A15 | Registry snapshots are only candidate inputs; an exact package release is accepted solely after automated official-source, integrity, lifecycle, legitimacy, resolved-lockfile, and provenance gates. | Standard Stack; Version Verification Snapshot | If every candidate fails, the dependency task blocks and retains the current build path; no human waiver converts failure into acceptance. |
| A16 | A bounded Rust/WASM asset staging table with one-use opaque receipts is the correct way to keep image bytes out of the semantic DTO while preserving an atomic document+asset commit. | Pattern 9; Security Domain | Expiry/replay/revision/max/max+1 and recovery tests must prove no orphan publication or byte substitution. |
| A17 | Existing Phase 1 field descriptors can be keyboard/semantically navigated in Phase 2 through valid-anchor inline groups and an explicit review section for `LegacyInvalid`, without implementing field authoring/filling. | Pattern 8; Validation Architecture | If an assistive technology cannot navigate the projection, change its native semantics/focus pattern, not the canonical descriptor or Phase 5 scope. |
| A18 | ICU `compiled_data` is acceptable only when its measured release-WASM delta is at most 512 KiB raw and 160 KiB gzip over the Phase 1 baseline. | Standard Stack; Validation Architecture | Exceeding either threshold automatically rejects that dependency configuration and triggers a smaller provider/feature strategy; do not waive the gate. |

## Open Questions

1. **What exact input-proxy/focus pattern works best with the supported screen-reader/browser matrix?**
   - What we know: the locked design requires controlled browser input and one visible native semantic document, and a focusable host must not be `aria-hidden`. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`; https://www.w3.org/TR/wai-aria-1.2/#aria-hidden]
   - What's unclear: automated DOM/accessibility snapshots cannot predict browse-mode and composition speech across all AT combinations. [ASSUMED]
   - Recommendation: prove the one-paragraph and existing-field tracer with local Chromium keyboard/accessibility-tree automation and a VoiceOver smoke pass before scaling components; retain the proxy architecture but adapt focus/labeling from observed evidence. Record Edge-on-Windows plus Windows-screen-reader UAT as an unavailable external checkpoint, not a local pass. [ASSUMED]

2. **How should each v1 `style_id` map into v2 block defaults and inline marks?**
   - What we know: v1 `StyleDefinition` has exact fields `id`, `name`, `font_family`, and `font_size_millipoints`; `ContentNode` has `style_id`. [VERIFIED: `crates/flow-core/src/model/mod.rs:106-123`; verbatim field names quoted]
   - What's unclear: whether v2 retains reusable style references in addition to direct overrides. [ASSUMED]
   - Recommendation: preserve the style table and `style_id` as a block default reference, create one inline run with no direct overrides, and make computed formatting a Rust projection; lock with current-fixture migration golden. [ASSUMED]

3. **Should CDP IME automation be a separate script or a Vitest browser test?**
   - What we know: the existing Vitest browser provider exposes real Chromium, but raw CDP is straightforward in a standalone Playwright context. [VERIFIED: `vitest.config.ts`; https://playwright.dev/docs/api/class-browsercontext#browser-context-new-cdp-session]
   - Recommendation: use `scripts/verify-ime-chromium.mjs` as a deterministic target-browser lane and keep synthetic controller cases in Vitest; both run under `check:phase2`. [ASSUMED]

4. **How should object-URL preview lifetime be managed?**
   - What we know: accepted bytes live in the existing durable asset path, while browser previews are noncanonical. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]
   - Recommendation: controller owns object URLs, revokes them on replacement/removal/unmount, and regenerates them only from accepted stored bytes; test leak-free lifecycle. [ASSUMED]

## Environment Availability

The audit below was executed in the workspace on 2026-08-26. [VERIFIED: local command probes]

| Dependency | Required By | Available | Version | Fallback |
|------------|-------------|-----------|---------|----------|
| Node.js | Vite/build/tests | ✓ | `24.10.0` | — |
| npm | exact package installation | ✓ | `11.9.0` | — |
| Rust toolchain | core/native/WASM tests | ✓ with workspace environment | `rustc 1.97.1`, `cargo 1.97.1` | use the existing `RUSTUP_HOME`, `CARGO_HOME`, and PATH pattern |
| `wasm-bindgen` CLI | browser artifact | ✓ | `0.2.108` | — |
| Playwright | browser/IME testing | ✓ | `1.57.0` | — |
| Chrome for Testing | local target-Chromium behavior and accessibility automation | ✓ | `143.0.7499.4` | — |
| Microsoft Edge | required supported-browser UAT | ✗ | `/Applications/Microsoft Edge.app` absent | external Edge-on-Windows checkpoint; Chromium automation is fallback evidence only |
| Windows + Windows screen reader | required supported assistive-technology UAT | ✗ on this macOS host | — | unavailable external checkpoint; no local equivalence claim |
| macOS VoiceOver | local assistive-technology fallback smoke evidence | ✓ | system application present | fallback does not close the Windows/Edge checkpoint |
| Vite | React shell | partially | `8.2.2` transitive, not a direct dependency | declare only an exact release accepted by the automated dependency gate |
| React / `react-dom` | editor UI | ✗ direct dependencies | — | install only an exact release accepted by the automated dependency gate |
| `@vitejs/plugin-react` | React Vite build | ✗ | — | declare only an exact release accepted by the automated dependency gate |
| `icu_segmenter` | Rust grapheme authority | ✗ workspace dependency | candidate registry `2.3.0` | accept only after official-source/lockfile provenance and 512-KiB-raw/160-KiB-gzip delta gates |
| `image` | bounded PNG/JPEG | ✗ workspace dependency | candidate registry `0.25.10` | add only an exact release accepted by the automated gate, with PNG/JPEG features only |
| Unicode 17 grapheme fixture | conformance | ✗ | official file available | vendor exact official file plus source/checksum metadata in Wave 0 |
| Git/macOS architecture | reproducibility context | ✓ | Git `2.50.1`, macOS `26.5.1`, arm64 | — |

The package manifest pins exact Node/npm values `"node": "24.10.0"` and `"npm": "11.9.0"`; local probes match. [VERIFIED: `package.json:6-10`; exact values quoted; local command probes]

**Missing dependencies with no full-equivalence fallback:** Microsoft Edge on Windows and a Windows screen reader. They block the final external QUAL-04/support-matrix evidence claim, although they do not block local implementation or automated semantics. [VERIFIED: local OS/application probes, 2026-08-26]  
**Missing dependencies with bounded fallback:** local Chromium automation plus macOS VoiceOver can exercise keyboard, semantic tree, field projection, status/error, and focus behavior; it must remain labeled fallback evidence. Vite is available transitively for inspection only but must be declared directly after the automated dependency gate; the Unicode fixture can be vendored from the pinned official URL. [VERIFIED: local `npm ls`/filesystem probes; https://www.unicode.org/Public/17.0.0/ucd/auxiliary/GraphemeBreakTest.txt]

## Validation Architecture

Nyquist validation is enabled because `.planning/config.json` does not set `workflow.nyquist_validation` to `false`. [VERIFIED: `.planning/config.json`] The Definition of Done requires implementation, automated tests, corpus fixtures, resource/security checks, and observable verification in the same revision. [VERIFIED: `.planning/REQUIREMENTS.md:115-117`; exact sentence: “A v1 requirement is complete only when implementation, automated tests, relevant corpus fixtures, security/resource-limit checks, and observable user verification all pass in the same committed revision.”]

### Test Framework

| Property | Value |
|----------|-------|
| Rust framework | Built-in Rust tests + existing `proptest = "=1.8.0"`. [VERIFIED: `Cargo.toml:11-19`; exact value quoted] |
| Browser/unit framework | Vitest `4.1.11`, `@vitest/browser-playwright` `4.1.11`, Playwright `1.57.0`, real Chromium. [VERIFIED: `package.json:21-27`; exact values quoted] |
| Existing config | `vitest.config.ts`: named `unit`, `inspector-unit`, and `browser` projects; browser files are serialized in single-process Chromium. [VERIFIED: `vitest.config.ts`] |
| Existing regression gate | `node scripts/check-phase1.mjs`, which runs provenance, pins, Node regressions, boundary contract, fmt, Clippy, all Rust tests, WASM, TypeScript, unit, accessibility, browser, recovery, and deterministic replay. [VERIFIED: `scripts/check-phase1.mjs`] |
| Quick run command | `npm run check:phase2:quick` — Wave 0 must add a fail-fast focused gate. [ASSUMED] |
| Full suite command | `npm run check:phase2` — Wave 0 must add a superset of the complete Phase 1 gate. [ASSUMED] |

All abbreviated `cargo` commands below run with the existing pinned prefix `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH`; plain `cargo` is not on this workspace shell's default PATH. [VERIFIED: local command probes; `scripts/check-phase1.mjs`]

### Phase Requirements → Test Map

The test paths and Phase 2 scripts in this table are Wave 0 recommendations unless marked existing. [ASSUMED]

| Req ID | Behavior | Test Type | Automated fast command | Full-gate evidence | File Exists? |
|--------|----------|-----------|------------------------|--------------------|--------------|
| EDIT-01 | Every accepted caret/range endpoint is an extended-grapheme UTF-16 boundary on native and WASM; invalid API endpoints reject without snap | Unicode conformance + unit/property + boundary DTO | `cargo test --locked -p flow-core --test grapheme_conformance` | official Unicode fixture, native/WASM parity, Ukrainian/emoji corpus, random edit sequences | ❌ Wave 0 |
| EDIT-02 | Keyboard insert/replace/delete, 256-KiB-bounded plain paste, IME empty/nonempty commit, explicit cancel/invalidation, one transaction per composition | Rust transaction + browser keyboard/paste + real Chromium IME | `cargo test --locked -p flow-core --test rich_text_transactions replace_selection && npx --no-install vitest run --project browser web/tests/editor-input.browser.test.ts && node scripts/verify-ime-chromium.mjs` | Rust operation/inverse/revision assertions, keyboard and paste browser cases, CDP sequence log, undo/reload/recovery | ❌ Wave 0 |
| EDIT-03 | Paragraph/heading/list split and compatible merge preserve order/styles/marks; deletion inverses restore subtree plus exact owner-keyed selection/session/field-anchor preimages | Unit + property + recovery/replay | `cargo test --locked -p flow-core --test rich_text_transactions split_merge` | generated structural sequences, tombstone/preimage bounds and corruption cases, delete→undo→redo→undo endpoint equality, v2 canonical replay | ❌ Wave 0 |
| EDIT-04 | Closed heading 1–6, font 6,000–288,000 millipoints, uppercase `#RRGGBB`, `uk-UA`/`en-US`, three semantic font IDs, alignment/spacing bounds, pending/mixed state, shortcut/toolbar equality | Rust bounds/session unit/property + browser interaction | `npx --no-install vitest run --project browser web/tests/editor-formatting.browser.test.ts` | reject/clamp-free boundary matrix, exact command DTO equality, session-only changes make no document revision/undo record, locale keys, reload resets session state | ❌ Wave 0 |
| EDIT-05 | Create/edit/remove every block kind with invariants, bounded staged assets, and atomic persistence | Rust integration/property + WASM binary seam + browser semantic/keyboard | `cargo test --locked -p flow-core --test rich_text_transactions embedded_blocks` | image/staging max/max+1, receipt replay/expiry/revision/substitution, atomic asset+document commit, table/list structure, page break, always-editable paragraph, native elements | ❌ Wave 0 |
| QUAL-03 | Every Phase 2 mutation has a visible labeled control and keyboard path that sends the same semantic command | Command-catalog contract + browser parity | `npx --no-install vitest run --project browser web/tests/editor-parity.browser.test.ts` | enumerate command catalog vs UI and keyboard registry; no mutation exceptions | ❌ Wave 0 |
| QUAL-04 | Keyboard/semantic navigation of document content and existing Phase 1 field descriptors (including `LegacyInvalid`/`TargetDeleted` review items), localized labels, focus return, status, alert, modal confirmation, one text copy; no field authoring/filling | Browser accessibility + keyboard + local AT fallback + external AT checkpoint | `npx --no-install vitest run --project browser web/tests/editor-accessibility.browser.test.ts` | automated tree at 320/1280, valid-field inline order, invalid/deleted-field review order/reason and exact undo restoration, live-region/focus assertions, local VoiceOver smoke record; Edge/Windows/screen-reader UAT remains explicitly outstanding | ❌ Wave 0 |

### Mandatory Cross-Cutting Tests

- **Migration goldens:** v0→v1→v2, v1→v2, v2 no-op; nonempty legacy alt → `Described`, empty legacy alt → `MissingLegacy`/needs-review and never decorative; valid decomposed-grapheme field anchor remains exact; interior/nonboundary and missing-node anchors preserve the original target as `LegacyInvalid` and never snap; stable document/block/asset/field IDs, old/current canonical bytes/hash, and provenance hop sequence. [ASSUMED]
- **Transaction algebra:** every command's forward operations followed by exact inverse returns byte-identical canonical v2 and exact owner-keyed directional selection/session/field anchors; cover atomic 0/1 affinity sentinels, insertion adjacent to atom, split, merge, cross-block replace, nested/whole subtree deletion, deterministic `Deleted(tombstone)` postimages, preimage max/max+1, and delete→undo→redo→undo after generated mixed sequences. [ASSUMED]
- **Recovery/replay:** commit a schema-v2 editor transaction, including staged image redemption and deletion preimages, interrupt before/after each persistence stage, prove no published document without its asset and no orphan accepted asset, reject missing/corrupt/duplicate-owner/token-root-mismatched preimages, recover the correct accepted revision, and replay delete→undo→redo twice with identical hash/view/selection/field anchors. [ASSUMED]
- **WASM boundary:** reject unknown fields, invalid enum variants, excessive nesting/bytes, invalid UTF-16/grapheme endpoints, stale revisions, raw arbitrary subtree/batch attempts, and any client-supplied tombstone token or private anchor preimage; only Rust transaction planning creates them. [ASSUMED]
- **Dependency and numerical WASM gate:** `scripts/verify-phase2-dependencies.mjs` must capture official registry/repository/tarball/integrity/deprecation/lifecycle evidence for every exact package and compare the resolved lockfile. `scripts/verify-wasm-size.mjs` builds the unchanged Phase 1 release-WASM baseline and the `icu_segmenter + compiled_data` candidate under identical toolchain/flags, records raw byte and deterministic gzip byte sizes in JSON, and fails when the candidate delta is **>512 KiB raw or >160 KiB gzip**. Either failure automatically rejects the candidate configuration; there is no human-approval bypass. [ASSUMED]
- **UI contracts and scale:** all 108 approved UI-state probes remain represented; 320px and 1280px layouts retain labeled controls, editor focus, errors/status, and no fake-page behavior. Add a generated 100-page and 200-page semantic document to verify bounded initial projection/viewport-first interaction without introducing Phase 3 layout. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-UI-SPEC.md`; project 100–200-page constraint is in `AGENTS.md`]
- **Field accessibility:** valid existing descriptors follow Rust logical order and are keyboard reachable with label, kind, required/read-only state and value summary; `LegacyInvalid`/`TargetDeleted` anchors remain reachable in a labeled review group with original target/reason, and deletion undo restores the exact earlier state; no create/edit/fill command exists in Phase 2. [ASSUMED]
- **Locale parity:** Ukrainian/English key sets, `lang`, all accessible names, error/status text, shortcut help, image/table dialogs, and formatting states match. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`]
- **Security limits:** zero, one, max, max+1, declared-vs-decoded mismatch, malformed PNG/JPEG, run fragmentation normalization, deep nesting, large composition/paste, command expansion, anchor-preimage owner count/encoded bytes, duplicate owners, and tombstone/root mismatch. [ASSUMED]
- **Regression:** Phase 1 inspector, immutable DTO, IndexedDB durability, audit redaction, WASM build, provenance, and deterministic replay remain green. [VERIFIED: Phase 1 summaries and `scripts/check-phase1.mjs`]

### Sampling Rate

- **Per task commit:** run the smallest relevant Rust test or Vitest file plus `npm run typecheck`; schema/transaction tasks also run their property target. [ASSUMED]
- **Per wave merge:** `npm run check:phase2:quick`, including fmt, focused Clippy, core Phase 2 tests, TypeScript, controller tests, one browser tracer, and IME probe. [ASSUMED]
- **Phase gate:** `npm run check:phase2` must run the unchanged Phase 1 gate plus full Rust all-target tests, release WASM and its numerical delta gate, all unit/browser suites, keyboard/paste/CDP IME, Unicode conformance, migration/recovery deterministic replay twice including deletion-preimage verification, automated dependency provenance/lockfile verification, and contract probes. [ASSUMED]
- **Observable UAT gate:** locally record keyboard-only full tracer, Chromium responsive use, Ukrainian/English switch, field navigation, and VoiceOver semantic/status/error navigation. Separately record Edge-on-Windows plus Windows-screen-reader UAT as **unavailable/outstanding**; it blocks the final support-matrix/QUAL-04 evidence claim but must not be reported as a passed local gate. [ASSUMED]

### Wave 0 Gaps

- [ ] `fixtures/unicode/17.0.0/GraphemeBreakTest.txt` and adjacent provenance/checksum metadata — official UAX #29 conformance corpus. [ASSUMED]
- [ ] `crates/flow-core/tests/grapheme_conformance.rs` — Unicode fixture parser plus UTF-8/UTF-16/native/WASM boundary equality. [ASSUMED]
- [ ] `crates/flow-core/tests/schema_v2_migration.rs` — deterministic routes plus nonempty/empty legacy-alt and valid/interior/missing-node field-anchor fixtures. [ASSUMED]
- [ ] `crates/flow-core/tests/rich_text_transactions.rs` — command matrix, 0/1 anchors, deterministic deletion tombstones, bounded owner-keyed preimages, corrupt/missing-preimage rejection, and exact delete→undo→redo→undo selection/session/field restoration. [ASSUMED]
- [ ] `crates/flow-core/tests/rich_text_properties.rs` — generated text/format/structure sequences and canonical equality. [ASSUMED]
- [ ] `crates/flow-core/tests/resource_limits.rs` — every new maximum and stable failure, including 256-KiB paste, 1-MiB DTO, and 2-receipt/16-MiB/5-minute staging pool. [ASSUMED]
- [ ] `crates/flow-core/tests/asset_staging.rs` and WASM boundary cases — `Uint8Array` input, opaque receipt metadata, replay/expiry/cancel/revision/reload, byte substitution, and atomic persistence plan. [ASSUMED]
- [ ] `web/tests/editor-controller.test.ts` — external-store identity, serialization, rejection, composition model. [ASSUMED]
- [ ] `web/tests/editor-input.browser.test.ts` and `scripts/verify-ime-chromium.mjs` — ordinary input/paste plus real Chromium composition. [ASSUMED]
- [ ] `web/tests/editor-formatting.browser.test.ts`, `editor-parity.browser.test.ts`, `editor-accessibility.browser.test.ts`, and `editor-responsive.browser.test.ts` — include exact EDIT-04 bounds, valid/legacy-invalid Phase 1 field navigation, and 100/200-page responsiveness fixtures. [ASSUMED]
- [ ] `scripts/verify-phase2-dependencies.mjs` and `scripts/verify-wasm-size.mjs` — fail-closed official-registry/source/integrity/lockfile gate plus recorded 512-KiB-raw/160-KiB-gzip ICU delta gate. [ASSUMED]
- [ ] `tests/contracts/phase2-boundary.test.mjs` — Rust/React ownership, no contenteditable/innerHTML, no Phase 3 geometry, no client-generated/exposed private deletion preimages, exact dependency pins, and command catalog parity. [ASSUMED]
- [ ] Vite React config/entry plus test setup that preserves the inspector and existing named Vitest projects. [ASSUMED]
- [ ] `scripts/check-phase2.mjs`, `check:phase2`, and `check:phase2:quick` — fail-fast Phase 2 superset gate. [ASSUMED]

## Security Domain

Security enforcement is enabled because `.planning/config.json` does not explicitly set it to `false`. [VERIFIED: `.planning/config.json`] ASVS 5.0 reorganized frontend, validation, and file controls into V1–V5; this table intentionally uses ASVS 5.0 numbering rather than the older ASVS 4 template. [VERIFIED: https://github.com/OWASP/ASVS/tree/master/5.0]

### Applicable ASVS 5.0 Categories

| ASVS category | Applies | Phase 2 control |
|---------------|---------|-----------------|
| V1 Encoding and Sanitization | yes | Render document/alt/status content as React text/native attributes only; no `innerHTML`, dynamic code, HTML paste, SVG, CSS, or template input. [CITED: https://github.com/OWASP/ASVS/blob/master/5.0/en/0x10-V1-Encoding-and-Sanitization.md] |
| V2 Validation and Business Logic | yes | Closed Rust enums/DTOs, allowlisted attributes, revision/grapheme/structure checks, documented limits, exact invariant failures at the trusted core. [VERIFIED: https://github.com/OWASP/ASVS/blob/master/5.0/docs_en/OWASP_Application_Security_Verification_Standard_5.0.0_en.flat.json] |
| V3 Web Frontend Security | yes | Safe text rendering, no DOM truth, CSP-ready static build, browser capability documentation, and one accepted snapshot. [VERIFIED: https://github.com/OWASP/ASVS/blob/master/5.0/en/0x12-V3-Web-Frontend-Security.md] |
| V4 API and Web Service | partially | No HTTP service in this phase; the WASM boundary is nevertheless treated as an untrusted closed API with strict deserialization and bounded messages. [ASSUMED] |
| V5 File Handling | yes | PNG/JPEG allowlist, signature and decode validation, byte/dimension/pixel/allocation limits, content hash, local object-URL lifecycle. [VERIFIED: https://github.com/OWASP/ASVS/blob/master/5.0/docs_en/OWASP_Application_Security_Verification_Standard_5.0.0_en.flat.json; https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html] |
| V6 Authentication | no | Phase 2 is local editor scope and adds no account boundary. [VERIFIED: `.planning/PROJECT.md`] |
| V7 Session Management | no | No authentication/session system is introduced. [VERIFIED: `.planning/PROJECT.md`] |
| V8 Authorization | no | No multi-user or authorization boundary is introduced; collaboration is later scope. [VERIFIED: `.planning/PROJECT.md`] |
| V11 Cryptography | partially | Add no cryptographic primitive; retain existing vetted canonical/content hash path for assets and snapshots. [VERIFIED: `AGENTS.md:18`; `.planning/PROJECT.md`] |
| V14 Data Protection | yes | Document content stays local in this phase; diagnostics/errors must not include raw document or image content; revoke preview URLs. [ASSUMED] |
| V15 Secure Coding and Architecture | yes | Rust authority, immutable snapshots, dependency provenance, deny-unknown DTOs, and fail-closed publication boundary. [VERIFIED: `AGENTS.md`; Phase 1 summaries] |
| V16 Security Logging and Error Handling | yes | Stable localized visible errors, privacy-safe diagnostics, no candidate/document text in analytics/logs. [ASSUMED] |

### Known Threat Patterns for the Phase 2 Stack

| Pattern | STRIDE | Standard mitigation / verification |
|---------|--------|------------------------------------|
| Document text/alt text becomes executable HTML, URL, style, or script | Tampering / Elevation | Render text nodes and closed typed attributes; prohibit `dangerouslySetInnerHTML`, HTML clipboard, SVG, arbitrary CSS/URL values; CSP contract test. [CITED: https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html] |
| Browser DOM mutation forges canonical document state | Tampering | Restore accepted projection after unexpected input; only Rust transaction acceptance plus persistence publishes a revision. [ASSUMED] |
| Stale selection/composition lands in changed text | Tampering | Base revision, stable IDs, exact grapheme endpoints, invalidated composition bookmark, stable rejection. [VERIFIED: `.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-CONTEXT.md`] |
| Replayed/double IME or UI command | Tampering | One committing state, command IDs, duplicate-command rejection, disable conflicting commands while in flight, exact audit. [VERIFIED: current `DuplicateCommand` / `FLOW_DUPLICATE_COMMAND` in `crates/flow-core/src/transaction/mod.rs:341-385`; values quoted] |
| Malformed/huge canonical DTO or deep tree exhausts memory/CPU | Denial of Service | Deny unknown fields, canonical bytes/depth/nodes/text/transaction limits, checked arithmetic, fail before cloning/recursing. [VERIFIED: `crates/flow-core/src/schema/mod.rs:16-82`] |
| Forged or incomplete deletion tombstone/preimage restores an anchor under the wrong owner or node | Tampering / Information Disclosure | Rust alone creates deterministic transaction-local slots; integrity-cover owner kind/ID, exact endpoint/state, deleted-root ID and token; enforce uniqueness/count/byte limits; validate the restored subtree before applying; expose only opaque `Deleted` state; fail recovery closed on mismatch. [ASSUMED] |
| Compressed image causes decode/allocation bomb | Denial of Service | Encoded/signature/dimension/pixel/allocation/decoded-size checks in Rust using a vetted decoder; max/max+1 and malformed corpus. [CITED: https://docs.rs/image/0.25.10/image/struct.Limits.html] |
| Forged, replayed, stale, or leaked staged-asset receipt substitutes bytes or publishes a dangling image | Tampering / Denial of Service | Opaque one-use revision-bound receipt, Rust-issued ID/hash/metadata, 2-receipt/16-MiB/5-minute staging bounds, expiry on cancel/reload/revision/redeem, constant-time receipt lookup, and one guarded IndexedDB document+transaction+audit+asset+head commit before publication. [ASSUMED] |
| Mark alternation or table/list nesting causes render/transaction explosion | Denial of Service | Normalize adjacent marks, cap runs/cells/nesting/mutation expansion, project capability reasons before action. [ASSUMED] |
| Object URLs retain removed private assets | Information Disclosure | Controller ownership and `URL.revokeObjectURL` on replace/remove/unmount; no raw bytes in diagnostics. [CITED: https://developer.mozilla.org/en-US/docs/Web/API/URL/revokeObjectURL_static] |
| Dependency substitution or unsafe lifecycle script | Tampering / Elevation | Exact pins, registry+official-source+legitimacy gate, lockfile review, postinstall inspection, provenance artifact, retained Phase 1 verifier. [VERIFIED: Package Legitimacy Audit; `scripts/check-phase1.mjs`] |
| Accessibility state differs from visible mutation availability | Spoofing / Denial of Service | Generate command capabilities once from Rust; assert visible/keyboard catalog parity and truthful disabled/mixed states. [ASSUMED] |

The untrusted-input boundary is the Rust DTO/asset decoder, not the TypeScript type system: TypeScript types disappear at runtime, so all enums, IDs, ranges, nesting, byte budgets, and media metadata must be revalidated in Rust before allocation/mutation/publication. [ASSUMED]

## Sources

### Primary (HIGH confidence)

- Context7 `/react/react/v19.2.7` — `useSyncExternalStore` subscription, immutable/cached snapshot, and concurrent consistency behavior. [VERIFIED: Context7]
- Context7 `/unicode-org/icu4x` — `GraphemeClusterSegmenter::segment_str`, compiled data, and independent Unicode/CLDR/ICU data tags. [VERIFIED: Context7]
- [Unicode Standard Annex #29](https://unicode.org/reports/tr29/) and [Unicode 17 GraphemeBreakTest](https://www.unicode.org/Public/17.0.0/ucd/auxiliary/GraphemeBreakTest.txt) — extended clusters and official conformance data. [VERIFIED: official Unicode sources]
- [React `useSyncExternalStore`](https://react.dev/reference/react/useSyncExternalStore), [common DOM event props](https://react.dev/reference/react-dom/components/common), and [state snapshots](https://react.dev/learn/state-as-a-snapshot). [VERIFIED: official React documentation]
- [Input Events Level 2](https://www.w3.org/TR/input-events-2/) and [UI Events](https://www.w3.org/TR/uievents/) — event model and composition ordering. [VERIFIED: W3C specifications]
- [WAI-ARIA 1.2](https://www.w3.org/TR/wai-aria-1.2/) and [APG toolbar pattern](https://www.w3.org/WAI/ARIA/apg/patterns/toolbar/) — state/live-region/toolbar semantics. [VERIFIED: W3C/WAI sources]
- [Playwright CDP session](https://playwright.dev/docs/api/class-browsercontext#browser-context-new-cdp-session) and [Chromium Input domain](https://chromedevtools.github.io/devtools-protocol/tot/Input/) — target-browser IME automation. [VERIFIED: official Playwright/Chromium documentation]
- [`image` 0.25.10 docs](https://docs.rs/image/0.25.10/image/) — decoder and resource-limit APIs. [VERIFIED: official crate documentation/source]
- [OWASP ASVS 5.0](https://github.com/OWASP/ASVS/tree/master/5.0), [XSS Prevention](https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html), and [File Upload](https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html). [VERIFIED: official OWASP sources]
- In-repository sources opened this session: `AGENTS.md`, `Cargo.toml`, `package.json`, `vitest.config.ts`, `scripts/check-phase1.mjs`, `crates/flow-core/src/{model,schema,anchor,transaction,lib}.rs`, `crates/flow-wasm/src/lib.rs`, Phase 1 summaries, Phase 2 context/UI spec, project/requirements/state. [VERIFIED: repository source inspection]

### Secondary (MEDIUM confidence)

- MDN [`beforeinput`](https://developer.mozilla.org/en-US/docs/Web/API/Element/beforeinput_event), [`InputEvent.isComposing`](https://developer.mozilla.org/en-US/docs/Web/API/InputEvent/isComposing), [`ClipboardEvent.clipboardData`](https://developer.mozilla.org/en-US/docs/Web/API/ClipboardEvent/clipboardData), and [`URL.revokeObjectURL`](https://developer.mozilla.org/en-US/docs/Web/API/URL/revokeObjectURL_static) — browser API guidance cross-checked against standards/target Chromium. [CITED: MDN]
- Local Playwright event probe on Chrome for Testing 143.0.7499.4 — target-environment observation, not a cross-browser specification. [VERIFIED: local probe, 2026-08-25]

### Tertiary (LOW confidence)

- None used as authority; all product-policy defaults are explicitly `[ASSUMED]` and listed in the Assumptions Log.

## Metadata

**Confidence breakdown:**

- Standard stack: **MEDIUM** — official React/ICU4X/Vite/Playwright/image sources identify the intended APIs, but candidate versions remain conditional until the automated official-registry/source/integrity/lockfile gate and numerical release-WASM measurement both pass.
- Architecture: **MEDIUM-HIGH** — locked decisions and current source inspection ground the ownership/migration/durability seams; the new schema-v2, session-state, anchor, and staged-asset contracts remain recommendations until their Wave 0 fixtures pass.
- Browser/IME behavior: **MEDIUM** — standards plus a real target-Chromium probe support the controller model; Edge-on-Windows and Windows-screen-reader UAT are unavailable external evidence and remain outstanding.
- Pitfalls/security: **MEDIUM-HIGH** — grounded in current code seams, W3C/browser behavior, OWASP ASVS 5.0, and decoder-limit APIs, with recommended receipt and interaction bounds still requiring max/max+1 tests.
- Numeric MVP limits: **MEDIUM** — explicit, testable defaults including 256-KiB paste, staged-asset bounds, and 512-KiB-raw/160-KiB-gzip ICU deltas; measured evidence may force a stricter implementation before acceptance.

**Research date:** 2026-08-26  
**Valid until:** 2026-09-02 for registry-candidate observations; architectural/standards findings remain valid until a locked contract, measured gate, or cited standard changes.

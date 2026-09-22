# FlowPDF

FlowPDF is a web-first document editor for Ukrainian- and English-language
contracts, forms, reports, and similar professional documents. Its intended
core value is natural editing of semantic document text, with deterministic
reflow and a visually consistent, selectable, form-capable PDF export.
The intended product combines semantic rich-text flow, document-wide reflow,
fillable forms, and voice control without requiring a commercial PDF SDK.

This repository is in active development. It currently contains the durable
document-engine foundation, a development editor shell, and a small browser
Foundation Inspector; it is not yet a complete PDF editor and should not be
evaluated as production-ready editing software.

## Purpose and architecture

FlowPDF keeps a semantic `FlowDocument` as the source of truth. The document
model is separate from the representations that will be derived from it:

```text
FlowDocument
    -> Rust transactions, anchors, and canonical serialization
    -> Rust-owned fixed-point layout, pagination, and derived fragments
    -> versioned WASM boundary and revision-safe Web Worker scheduling
    -> browser page viewport plus synchronized semantic accessibility DOM
    -> Rust-owned semantic form validation and derived widget projection
    -> owned fixed-layout PDF objects, export, and visual preview adapter
```

Rust owns the document model, revision checks, transactions, recovery rules,
logical positions, and the native/WASM semantic boundary. React and TypeScript
provide the browser shell, controls, physical browser I/O, and presentation.
Layout work is already derived from canonical bytes through the Rust/WASM
boundary and scheduled away from the UI thread through a revision-aware Web
Worker adapter. IndexedDB is the current browser durability adapter. The page
viewport is a projection of accepted Rust geometry; the semantic DOM remains
the authoring and accessibility surface.

The canonical model is deliberately not the DOM and is not a PDF page tree.
This keeps reflow, undo/redo, accessibility projections, and future PDF
conversion from depending on unstable browser coordinates. Imported or
unsupported PDF content is intended to remain explicitly marked and
preserved; the future reconstruction path is best effort and must not claim
lossless semantic recovery where it cannot prove it.

## What is implemented

The current tree includes:

- a versioned FlowDocument schema with canonical inline runs, structured block
  nodes, stable IDs, deterministic canonical bytes, hashes, and migration
  fixtures;
- Rust transaction lifecycle support with revision preconditions, undo/redo,
  anchor transformations, recovery, provenance, and privacy-preserving audit
  projections;
- a Rust/WASM bridge and a React/TypeScript Foundation Inspector that exercises
  document creation, mutation, recovery, and browser persistence through
  IndexedDB;
- pinned ICU4X grapheme-boundary validation for UTF-16 browser positions,
  exact atomic-node edges, and a Rust-owned noncanonical editor session/view
  projection in the current Phase 2 working tree;
- deterministic fixed-point text layout with explicit Noto Sans/font and
  Ukrainian hyphenation provenance, ICU line segmentation, bidi runs, shaping,
  UTF-8/UTF-16 cluster ranges, and fail-closed unsupported-glyph paths;
- schema-v3 section/page settings, bounded static header/footer runs,
  deterministic fragment pagination, table-row overflow diagnostics, and
  conservative incremental/full-reflow equivalence checks;
- a closed string-only Rust/WASM layout protocol, revision/hash-bound worker
  scheduling with cancellation and stale-result rejection, and an accessible
  page viewport that renders accepted Rust coordinates beside the semantic DOM;
- a revision/hash/catalog-bound Rust PDF display-list projection with source
  UTF-8/UTF-16 ranges, shaped glyph placements, page order, repeated-band
  provenance, bounded unsupported-content diagnostics, and deterministic
  admitted TrueType subset/ToUnicode resource preparation;
- a bounded revision-owned PNG/JPEG resource adapter with content-identity
  deduplication, fixed-point image rectangles, deterministic PDF metadata,
  outline and internal-link records, and an explicit support matrix that
  excludes active/external actions;
- a versioned private owned-export manifest/source envelope with canonical
  payload hash binding and exact Rust recovery classification; source payloads
  are redacted from request/result debug output;
- a bounded JSON-only Rust/WASM PDF export/recovery protocol with a
  revision-aware single-flight browser worker, manifest/byte-hash guards,
  explicit abort/stale diagnostics, and caller-owned immutable download state;
- a visual-only virtualized PDF preview adapter with bounded zoom, page
  navigation, source-backed search, selection projection, and announced
  export status; the semantic editor remains the only authored/accessibility
  surface;
- Rust-owned validation for the current semantic field vocabulary and a
  revision/hash-bound anchor-to-widget projection over accepted fixed-point
  display lists, including deterministic identities/tab order and explicit
  review entries for invalid, deleted, or unmapped anchors;
- a noncanonical Rust form session bound to document identity/revision/hash,
  with immutable fill/clear operations, explicit value overrides separate from
  authored defaults, read-only enforcement, and deterministic serialization;
- a versioned Rust/WASM form-session action boundary plus a separate guarded
  `form-sessions-v1` IndexedDB adapter for accepted current values, with source
  identity and generation conflict checks kept outside canonical recovery;
- accessible native text/textarea, checkbox, radio-group, and select controls
  for already-authored valid fields, coordinated by the source-bound session;
  clear/default behavior, localized loading/errors, and explicit non-editable
  signature/button states are covered without changing canonical revisions;
- an accessible descriptor-configuration disclosure for already-authored valid
  fields, routed through the reversible Rust `SetField` transaction while
  preserving field identity and anchors; it edits current metadata, flags,
  typed defaults, and option metadata but does not insert or delete fields;
- an accessible place-at-caret action for already-authored valid fields that
  uses the accepted Rust logical selection and the same reversible `SetField`
  path; non-collapsed selections and review-region fields remain fenced;
- a Rust-owned, parity-catalogued `InsertField` transaction and localized
  accessible action that creates one typed default text field at an accepted
  collapsed caret, with exact undo/redo and durable recovery evidence;
- a Rust-owned, parity-catalogued `RemoveField` transaction with explicit
  confirmation, exact indexed undo/redo/recovery, and localized accessible
  removal controls for valid projected fields; review-region fields remain
  read-only;
- a Rust-owned, parity-catalogued `MoveField` transaction that reorders valid
  semantic fields by canonical tab order with exact undo/redo/recovery, derived
  editor/widget/PDF order, and localized earlier/later controls; review-region
  fields remain read-only;
- a bounded AcroForm structure adapter that converts an accepted session-aware
  widget projection into deterministic field dictionaries, page widgets,
  values/defaults, options, flags, and catalog/page references in the owned
  PDF writer, with derived normal appearances, shared built-in Helvetica
  resources, explicit `/DR`/`/DA`/`NeedAppearances false`, and closed checkbox
  and radio state dictionaries; this is not a full-Unicode or target-viewer
  compatibility claim;
- an explicit core export option for selected-field flattening: accepted field
  appearances can be emitted as fixed-point page content while unselected
  fields retain editable AcroForm widgets; selecting every field omits the
  AcroForm catalog entry, and the private source envelope still recovers the
  canonical document exactly. The optional source-bound form plan and
  `flattenedFieldIds` selection are also admitted by the Rust/WASM export
  envelope; the browser now offers source-bound selection intent without
  moving flattening authority out of Rust;
- a closed Rust/WASM form-projection endpoint that reuses the validated layout
  request, builds the display list in Rust, returns session-aware fixed-point
  widget geometry and review entries, and derives an optional source-bound
  `PdfFormPlan` with a Rust verifier; the result is consumed by the typed
  browser adapter and editor projection surface;
- a typed revision-safe browser adapter and single-flight scheduler for that
  projection endpoint, bound to the accepted layout result hash and optional
  source session; it publishes only Rust-verified derived responses and feeds
  the visual overlay, accessible summary, and source-bound selection surface;
- an `EditorApp`-integrated projection surface that requests only against the
  current accepted layout/session, paints fixed-point widget overlays on the
  visual page viewport, exposes a separate accessible widget/review summary,
  and offers explicit select-all/clear/per-field flatten intent without
  mutating semantic or session state;
- a reusable `createPdfExportRequest` builder that copies accepted source and
  layout identities, Rust-produced page bounds, and verified form-plan/
  flatten-selection data into the exact opaque Rust PDF wire payload, failing
  closed on stale identity combinations;
- an `EditorController` default that uses this builder when a caller provides
  a PDF scheduler without a custom factory; explicit factories remain
  authoritative, and worker/font-catalog ownership remains with the caller;
- a real-Chromium PDF preview smoke that exercises that controller default,
  validates the captured source/layout-bound request, and retains stale export
  suppression;
- caller-owned `EditorControllerDependencies` composition through `EditorApp`
  and `mountEditorApp`, so scheduler/WASM seams can be supplied without
  duplicating controller construction;
- a `createWasmPdfExportScheduler` composition helper that joins the existing
  Rust-verifying WASM adapter to the revision-aware scheduler while preserving
  caller-owned worker and font-catalog decisions;
- unit evidence for the closed PDF worker scope message loop, including
  accepted-result and cooperative-cancellation routing without stale publish;
- Phase 1 and Phase 2 foundation fixtures, Unicode 17 grapheme conformance
  data, dependency provenance checks, Rust tests, TypeScript checks, browser
  tests for the implemented foundation and Phase 3 layout path, and a Phase 4
  local/reference validation gate that keeps unavailable external evidence
  explicit.

These capabilities provide deterministic engine contracts and an inspection
surface; they do not yet constitute a finished rich-text editor or PDF
product. Schema support for images, lists, tables, page breaks, fields, and
styles should not be read as a claim that all corresponding UI operations or
PDF behavior are already implemented.

## Planned capabilities

The roadmap is intentionally staged. Remaining work includes, among other
things:

- broader accessible paragraph and structured rich-text editing with complete
  keyboard, IME, clipboard, and visible-control coverage;
- broader script/font coverage, advanced layout constraints, and production
  pagination hardening beyond the admitted fixtures and bounded Phase 3 path;
- end-to-end PDF text/image content streams, production font/layout catalog
  wiring, and target-viewer validation on top of the current owned
  preview/export contracts;
- a bounded PDF reader, with selectable text, reproducibility evidence,
  explicit unsupported-content reporting, and an exact owned-source round trip
  when the source payload is available;
- target-viewer behavior and external PDF form import for the verified
  form-field export path; the core and Rust/WASM envelope can explicitly
  flatten selected accepted fields into derived page content while retaining
  unselected widgets and the recoverable source, but this does not claim
  general text/image flattening or target-viewer compatibility;
- voice dictation and commands through the same revision-checked transaction
  boundary;
- controlled external-PDF reconstruction/OCR and later native editing of
  supported PDF scene islands.

Phase 2 implementation plans and the Phase 3 local implementation gate are
complete, but the explicitly required Microsoft Edge on Windows plus Windows
screen-reader evidence remains outstanding. All six Phase 4 implementation
plans are now executed: the first five slices add a deterministic bounded Rust
COS/page envelope, a revision-bound display list, compact TrueType/ToUnicode
resource inputs, bounded PNG/JPEG resource preparation, typed
metadata/outline/internal-link support with explicit exclusions for active and
external actions, a private reproducibility manifest and exact owned-source
recovery path, plus a revision-safe WASM/worker export adapter and
visual-only virtualized preview; the sixth adds the reproducible local/
reference gate. The local Phase 4 gate passes its rows twice, while the four
external PDF/reference rows are `unavailable` because this checkout has no
Phase 4 PDF fixture or matching qpdf/Poppler/target-viewer environment. End-
to-end PDF text/image content streams, production runtime catalog wiring,
target-viewer validation, and general PDF import remain planned. Complete
assistive-technology validation, voice control, and production hardening are
not delivered by this repository state.
See the
[roadmap](.planning/ROADMAP.md) and [project constraints](.planning/PROJECT.md)
for authoritative scope and sequencing.

## Repository map

- [`crates/flow-core`](crates/flow-core) — canonical Rust model, schema,
  transactions, anchors, recovery, and editor-session authority.
- [`crates/flow-wasm`](crates/flow-wasm) — wasm-bindgen exports for the browser
  boundary.
- [`web`](web) — React/TypeScript browser shell, Foundation Inspector, and
  IndexedDB persistence adapter.
- [`fixtures`](fixtures) — canonical documents, migrations, Unicode data, and
  recovery recipes.
- [`scripts`](scripts) — locked build, provenance, evidence, inspector, and
  verification scripts.
- [`config`](config) — dependency provenance configuration.
- [`artifacts`](artifacts) — retained benchmark and provenance evidence.
- [`.planning`](.planning) — project decisions, roadmap, state, research, and
  executable phase plans.

Useful entry points include [`package.json`](package.json),
[`Cargo.toml`](Cargo.toml), [`rust-toolchain.toml`](rust-toolchain.toml), the
[dependency provenance configuration](config/dependency-provenance.json), the
[web build script](scripts/build-web.mjs), the
[local inspector server](scripts/serve-inspector.mjs), and the
[WASM tool verifier](scripts/verify-wasm-bindgen-tool.mjs).

## Prerequisites

The checked-in toolchain policy is exact:

- Node.js `24.10.0`;
- npm `11.9.0`;
- Rust `1.97.1` with the `wasm32-unknown-unknown` target;
- `rustfmt` and `clippy` for that Rust toolchain;
- `wasm-bindgen-cli` `0.2.108`.

The repository's verification commands use the prepared workspace-local
toolchain under `work/toolchains/rustup` and `work/toolchains/cargo`, not an
arbitrary global Cargo installation. The configured binary target is
`aarch64-apple-darwin`. Dependency verification also checks the approved
lockfiles, successful tracked provenance, and the Cargo installation receipt
at `work/toolchains/cargo/.crates2.json`; rebuilding a same-version binary or
using another platform does not automatically create approved evidence.

This workspace currently assumes that the local Rust/WASM and pinned
Chromium toolchain have already been prepared. There is not yet a portable
fresh-clone bootstrap command: `npm ci` installs JavaScript dependencies but
does not install the pinned Rust toolchain, `wasm-bindgen-cli`, or the browser
binary. Generated WASM under `web/generated`, browser binaries and local
toolchains under `work`, `node_modules`, Rust `target`, and `dist` are local
build outputs and are not source prerequisites to commit.

## Run the current inspector

From the repository root:

```sh
npm ci
npm run inspector
```

The inspector is served locally at
[`http://127.0.0.1:4173`](http://127.0.0.1:4173). Set
`FLOWPDF_INSPECTOR_PORT` to use another local port. The inspector command
builds the web output first. If using Vite directly, build the WASM module
before starting it because `npm run dev` does not generate WASM:

```sh
npm run build:wasm
npm run dev
```

Keep development servers bound to local interfaces. `npm run inspector` serves
the Foundation Inspector; `npm run dev` serves the current development editor
shell. Neither is a production-ready full PDF editor.

## Checks

The most useful commands in the prepared environment are:

```sh
npm run build:wasm   # build Rust/WASM and generate web/generated
npm run build:web    # build WASM, assemble dist/web, and typecheck
npm run typecheck    # TypeScript without emitting files
npm run test:unit    # unit and inspector-unit suites
npm run test:browser # pinned Chromium browser suites
npm test             # all configured Vitest projects
npm run check        # the full Phase 1 evidence and regression gate
npm run check:phase2 # the full Phase 2 validation gate
npm run check:phase3:smoke # Phase 3 manifest/diagnostic contract smoke
npm run check:phase3       # Phase 3 local gate; runs exact rows in fixed order
npm run check:phase4:smoke # Phase 4 manifest/diagnostic contract smoke
npm run check:phase4       # Phase 4 local gate plus explicit reference rows
npm run check:phase4:smoke && npm run check:phase4 && npm run check:phase4
```

`npm run test:browser` requires the pinned Chromium installation under
`work/playwright`. `npm run check` is broader than a local source-only check:
it validates retained evidence and live dependency provenance as well as Rust,
WASM, TypeScript, unit, and browser gates, so it can be network- and
environment-sensitive. `npm run check:phase3` additionally covers the
deterministic layout, pagination, worker, viewport, migration, and scale
lanes; its external AT status remains explicit rather than substituted by
local Chromium evidence. The terminal Phase 3 verification is two consecutive
full gate runs after the smoke contract.

For a direct Rust workspace check using the repository-local toolchain:

```sh
RUSTUP_HOME=./work/toolchains/rustup \
CARGO_HOME=./work/toolchains/cargo \
PATH=./work/toolchains/cargo/bin:$PATH \
cargo test --locked --workspace --all-targets
```

The planning files record the exact focused commands and retained evidence for
each phase. Start with [`STATE.md`](.planning/STATE.md), then inspect the
relevant plan under [`.planning/phases`](.planning/phases).

## Development status

Phase 1, Durable Flow Foundation, is complete. Phase 2 implementation plans
are complete with external Windows/Edge/screen-reader closure still open.
Phase 3, Deterministic Reflow and Pagination, has completed its six local
implementation plans and dual-run gate. Phase 4 has completed its six local
implementation plans and dual-run local gate; its qpdf/Poppler/target-viewer
reference rows remain unavailable, so the phase is not marked complete. Phase
5 has twenty completed local slices for validation/projection,
noncanonical fill state, effective-value projection, bounded AcroForm
field/widget structure, the Rust/WASM plus guarded IndexedDB session boundary,
accessible controls, descriptor configuration, safe anchor placement,
Rust-owned default text-field insertion, semantic field removal, and canonical
tab-order authoring, bounded derived AcroForm appearance resources and state
streams, explicit core export-time flattening of selected fields, and the
source-bound Rust/WASM form-plan/selection and display-list-backed projection
envelopes, plus a typed revision-safe browser projection adapter/scheduler,
an EditorApp-integrated visual/read-only projection surface, and explicit
source-bound selection controls propagated to the PDF export factory.
The latest slice centralizes that callback payload construction in a tested
Rust-compatible builder; it does not activate production font-catalog or
worker wiring.
The controller now uses that builder by default when a PDF scheduler is
provided without a custom factory, while preserving the explicit custom-factory
path.
The real-Chromium PDF preview smoke now exercises that default path and checks
the captured source/layout-bound request before the fixture scheduler runs.
The editor shell exposes the same caller-owned controller dependencies through
`EditorApp` and `mountEditorApp`; it still does not instantiate production
workers or font catalogs.
The generated-WASM smoke now crosses the complete request-builder,
WASM-adapter, scheduler, and accepted-result path through this helper.
The worker message loop is covered separately with a fake scope; it is not yet
connected to the active application entry.
The latest slice also verifies an ordinary request against the generated
Rust/WASM `export_pdf` and response verifier.
Target-viewer behavior and external form import remain unimplemented; the
current implementation still makes no claim of complete Unicode/PDF
compatibility, commercial-SDK parity, or production readiness.

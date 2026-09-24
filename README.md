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

Imported PDF bytes
    -> bounded, read-only classic-xref reader and supported scene
        -> confidence-scored FlowDocument candidate for side-by-side review
        -> [planned] Rust-owned edit session and fresh full-document rewrite
```

Rust owns the document model, revision checks, transactions, recovery rules,
logical positions, and the native/WASM semantic boundary. React and TypeScript
provide the browser shell, controls, physical browser I/O, and presentation.
Layout work is already derived from canonical bytes through the Rust/WASM
boundary and scheduled away from the UI thread through a revision-aware Web
Worker adapter. IndexedDB is the current browser durability adapter. The page
viewport is a projection of accepted Rust geometry; the semantic DOM remains
the authoring and accessibility surface.

The canonical model is deliberately not the DOM and is not an imported PDF page
tree. This keeps reflow, undo/redo, and accessibility projections independent
of unstable browser coordinates. Imported PDFs follow a separate bounded,
read-only path today: reconstruction is best effort, confidence-scored, and
reviewed beside an immutable source. Native PDF editing is still planned; an
accepted architecture decision requires a Rust-owned source-bound session and
a fresh rewrite, with unsafe or unrepresentable inputs refused rather than
silently flattened.

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
  projection established by the completed Phase 2 local implementation plans;
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
- a bounded voice-command resolver and browser voice-input seam over the
  existing command boundary; this does not prove access to a real browser
  speech service or external assistive-technology behavior;
- a bounded read-only classic-xref PDF reader, supported-scene projection, and
  explicit unsupported-content diagnostics; this is not a universal PDF
  parser or native editing implementation;
- confidence-scored single-column reconstruction candidates, immutable source
  storage, side-by-side review, and an OCR adapter seam; no OCR engine or
  external recognition provider is bundled;
- a Phase 9 native-editing investigation and accepted ADR; executable plans
  and native-PDF editing code have not yet been delivered.

These capabilities provide deterministic engine contracts and an inspection
surface; they do not yet constitute a finished rich-text editor or PDF
product. Schema support for images, lists, tables, page breaks, fields, and
styles should not be read as a claim that all corresponding UI operations or
PDF behavior are already implemented.

## Remaining work and evidence boundaries

Implementation plans for Phases 2–8 are locally complete, but that is not the
same as closing every release criterion. Microsoft Edge/Windows
assistive-technology observations remain outstanding for the accessibility
work; target-viewer and external form-import evidence remains open for forms;
and external speech-service, qpdf/Poppler, target-viewer, OCR-provider, and
corpus evidence is unavailable where the phase closures say so. The repository
does not claim broad PDF compatibility or production readiness.

The owned PDF export path still has bounded content/resource and viewer
coverage; Phase 4's external PDF-reference rows are unavailable locally. Phase
7 reads only its controlled, unencrypted classic-xref subset. Phase 8 produces
reviewable best-effort reconstruction and exposes an OCR adapter, not a
bundled OCR engine or provider. Phase 9's investigation and architecture
decision are accepted, but its executable plans and implementation have not
started. The first planned end-to-end native-edit path is a plain text-note
annotation through a Rust-owned source/revision-bound session and fresh rewrite;
broader text, form, page, and redaction operations require explicit bounds and
verification before being described as supported.

See the [roadmap](.planning/ROADMAP.md), [project constraints](.planning/PROJECT.md),
and [current state](.planning/STATE.md) for authoritative scope and sequencing.

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
npm run check:phase4:release # Phase 4 strict release evidence gate
npm run check:phase5:smoke # Phase 5 manifest, closure, and diagnostic smoke
npm run check:phase5       # Phase 5 local form, PDF, worker, and build gate
npm run check:phase6:smoke # Phase 6 manifest/closure/diagnostic smoke
npm run check:phase6       # Phase 6 local voice and regression gate
npm run check:phase7:smoke # Phase 7 reader manifest/diagnostic smoke
npm run check:phase7       # Phase 7 controlled reader local gate
npm run check:phase8:smoke # Phase 8 reconstruction manifest/diagnostic smoke
npm run check:phase8       # Phase 8 local reconstruction and regression gate
npm run check:planning     # planning head, plan, and state projection check
npm run check:release      # ordered release gate with dependency de-duplication
```

`npm run test:browser` requires the pinned Chromium installation under
`work/playwright`. `npm run check` validates checked-in dependency evidence and
the Phase 1 Rust, WASM, TypeScript, unit, browser, and recovery lanes without
refreshing tracked reports. Use `npm run refresh:provenance`,
`npm run refresh:phase2-dependencies`, `npm run refresh:wasm-size`, or
`npm run refresh:recovery-benchmark` when the corresponding evidence must be
renewed. `npm run check:phase3`
additionally covers the deterministic layout, pagination, worker, viewport,
migration, and scale lanes; its external AT status remains explicit rather than
substituted by local Chromium evidence. Phase 4 reference rows are reported as
unavailable locally and become release blockers under
`npm run check:phase4:release`. Phase 6–8 checks validate their bounded local
contracts; they do not substitute for unavailable external service,
PDF-reference, viewer, OCR-provider, corpus, or accessibility evidence.

`npm run check:release` is the aggregate strict gate currently wired for
Phases 1–5, not a single complete release gate for every roadmap phase. It
requires a synchronized clean working tree, `.planning/STATE.md`, and a closed
Phase 5 plan inventory, then runs Phase 1, Phase 2/3 in dependency-child mode,
the strict Phase 4 evidence gate, Phase 5, and final tree/state/diff checks.
Each child has a bounded timeout and emits a redacted diagnostic tail on
failure. Run the separate Phase 6–8 gates for their local contracts; required
external Edge/Windows accessibility, PDF-reference, viewer, speech-service,
and OCR evidence still must be observed on the relevant target.

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

- Phases 1–4: local implementation plans/gates are recorded; Phase 2/3 external
  accessibility evidence and Phase 4 external PDF-reference/viewer evidence
  remain open.
- Phase 5: local forms implementation and gate complete; target-viewer and
  external form-import evidence remain pending.
- Phase 6: local voice-command/input seam complete; real speech-service and
  external accessibility evidence remain pending.
- Phase 7: bounded read-only reader/scene complete locally; external PDF,
  target-viewer, and accessibility evidence remain unavailable.
- Phase 8: confidence-scored reconstruction/review and OCR adapter seam
  complete locally; OCR provider/corpus and external PDF/viewer/accessibility
  evidence remain unavailable.
- Phase 9: investigation closed and ADR accepted; executable plans and code
  have not started. It is scoped around a complete bounded imported-PDF graph,
  Rust-owned source/revision-bound sessions, fresh rewrites, explicit refusal
  boundaries, and independently verified redaction.

The current shell and engine are development software. No phase status here
claims general PDF compatibility, full accessibility certification, or
production readiness.

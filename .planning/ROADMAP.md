# Roadmap: FlowPDF

## Overview

FlowPDF progresses through nine dependency-ordered vertical capabilities. The roadmap first proves a durable semantic editor and deterministic reflow, then establishes owned PDF export and forms, adds voice through the same command boundary, and only then accepts external PDF complexity through a secure reader, reconstruction pipeline, and controlled native editing. Each phase must preserve the writer-first invariants and adds no promise of universal PDF compatibility.

## Phases

**Phase Numbering:**

- Integer phases are planned milestone work.
- Decimal phases are urgent insertions between planned phases.

- [x] **Phase 1: Durable Flow Foundation** - Users can create, save, recover, inspect, undo, and safely target versioned FlowDocuments. (completed 2026-08-25)
- [ ] **Phase 2: Accessible Rich-Text Editing** - Users can edit semantic Ukrainian and English content through correct keyboard, IME, UI, and assistive-technology interactions.
- [ ] **Phase 3: Deterministic Reflow and Pagination** - Document edits reshape, fragment, and repaginate predictably with incremental/full equivalence.
- [ ] **Phase 4: Owned PDF Preview and Export** - Users can preview and export selectable, reproducible PDFs and recover the exact owned source.
- [x] **Phase 5: Semantic Fillable Forms** - Local implementation complete 2026-09-22; target-viewer and external form-import evidence remain explicit release inputs.
- [ ] **Phase 6: Voice Dictation and Commands** - Users can dictate and invoke safe, undoable document commands through explicit voice modes.
- [ ] **Phase 7: Secure PDF Reader and Scene** - Users can open the controlled PDF subset through a bounded parser and inspect supported page content and warnings.
- [ ] **Phase 8: External Reconstruction and OCR** - Users can review and accept confidence-scored FlowDocument reconstruction while preserving the original and opaque content.
- [ ] **Phase 9: Controlled Native PDF Editing** - Users can edit supported PDF scene islands, page objects, annotations and forms with secure rewrite/redaction behavior.

## Phase Details

### Phase 1: Durable Flow Foundation

**Goal**: As a document author, I want to create, save, reopen, migrate, mutate, undo, redo, and recover a versioned FlowDocument, so that later editor inputs and renderers can rely on durable semantic content and auditable revisions.
**Mode:** mvp
**Depends on**: Nothing (first phase)
**Requirements**: FLOW-01, FLOW-02, FLOW-03, FLOW-04, FLOW-05, EDIT-06, EDIT-07, QUAL-08
**Success Criteria** (what must be TRUE):

  1. User can create, save, reopen and migrate a FlowDocument without losing semantic content, styles, fields or assets.
  2. User can undo and redo committed mutations and a stale command fails without changing a different target.
  3. User can recover the last durable revision after a reload or worker failure.
  4. User can inspect the revision, provenance and privacy-preserving audit entries behind the current document.

**Plans**: 6/6 plans executed

- [x] 01-01-PLAN.md
- [x] 01-02-PLAN.md
- [x] 01-03-PLAN.md
- [x] 01-04-PLAN.md
- [x] 01-05-PLAN.md
- [x] 01-06-PLAN.md

**UI hint**: yes

### Phase 2: Accessible Rich-Text Editing

**Goal**: As a document author, I want to edit structured Ukrainian and English text, so that I can create accessible documents.
**Mode:** mvp
**Depends on**: Phase 1
**Requirements**: EDIT-01, EDIT-02, EDIT-03, EDIT-04, EDIT-05, QUAL-03, QUAL-04
**Success Criteria** (what must be TRUE):

  1. User can place a caret, select and edit text without splitting visible grapheme clusters.
  2. User can author paragraphs, headings, lists, images, simple tables and page breaks and apply supported inline/block styles.
  3. Keyboard, visible controls and synchronized semantic accessibility content expose every implemented mutation.
  4. Screen-reader users can navigate semantic content, fields, statuses and errors without relying on the page canvas alone.

**Plans**: 18/18 plans executed

Plans:

- [x] 02-01-PLAN.md — Admit and lock exact Phase 2 dependencies without lifecycle execution.
- [x] 02-02-PLAN.md — Pin Unicode conformance data, numerical WASM growth, and retained boundaries.
- [x] 02-03-PLAN.md — Execute the one-way legacy freeze and every schema-v2 migration route.
- [x] 02-04-PLAN.md — Establish Rust grapheme, atomic-position, and editor-session authority.
- [x] 02-05-PLAN.md — Prove the core/WASM/persistence/minimal-React paragraph tracer.
- [x] 02-06-PLAN.md — Add controlled input, directional selection, paste, and real Chromium IME.
- [x] 02-07-PLAN.md — Implement exact structural and deletion/preimage/recovery algebra.
- [x] 02-08-PLAN.md — Attach durable visible and keyboard structural product paths.
- [x] 02-09-PLAN.md — Implement closed formatting, pending state, and list semantics in Rust.
- [x] 02-10-PLAN.md — Expose accessible formatting/list controls and parity.
- [x] 02-11-PLAN.md — Implement complete table headers/removal and page-break semantics in Rust.
- [x] 02-12-PLAN.md — Expose native table/page-break controls, semantics, and confirmation.
- [x] 02-13-PLAN.md — Implement complete image lifecycle, BLAKE3 identity, and shared reachability.
- [x] 02-14-PLAN.md — Carry image lifecycle through WASM, atomic storage, and accessible controls.
- [x] 02-15-PLAN.md — Generate exhaustive command parity and retained boundary contracts.
- [x] 02-16-PLAN.md — Keep existing fields and the editor shell semantically accessible.
- [x] 02-17-PLAN.md — Close the exact 108-pair UI contract and honest AT evidence.
- [x] 02-18-PLAN.md — Add fast scale feedback and the deterministic dual-run release gate.

Phase 2 implementation plans are complete and the deterministic release gate is
green. Final Phase 2 closure remains open until the explicitly required
Edge/Windows screen-reader evidence is observed on that platform; local
Chromium/macOS evidence is not a substitute.

**UI hint**: yes

### Phase 3: Deterministic Reflow and Pagination

**Goal**: Users see language-correct text and all following content repaginate consistently while the active page remains interactive.
**Mode:** mvp
**Depends on**: Phase 2
**Requirements**: LAYO-01, LAYO-02, LAYO-03, LAYO-04, LAYO-05, LAYO-06, LAYO-07, LAYO-08
**Success Criteria** (what must be TRUE):

  1. Supported Unicode text has correct shaping, fallback, direction, line breaking and Ukrainian hyphenation behavior.
  2. An edit that changes block height moves and repaginates all following content across configured pages and sections.
  3. Headers, footers, keep constraints, widow/orphan fallback and simple table row fragmentation behave deterministically.
  4. Incremental reflow is structurally and visually identical to full reflow for the same revision.
  5. User can continue working on the active viewport while later pages paginate in a worker.

**Plans**: 6/6 local plans executed

Plans:

- [x] 03-01-PLAN.md — Build the deterministic fixed-point text-layout tracer.
- [x] 03-02-PLAN.md — Admit schema-v3 sections and static header/footer settings.
- [x] 03-03-PLAN.md — Build fragment-tree pagination and bounded constraints.
- [x] 03-04-PLAN.md — Prove incremental/full reflow equivalence.
- [x] 03-05-PLAN.md — Expose revision-safe WASM and worker layout scheduling.
- [x] 03-06-PLAN.md — Deliver the accessible page viewport and Phase 3 gate.

All six Phase 3 implementation plans and the deterministic local gate are
complete. Phase-level closure remains explicit about the inherited Phase 2
Microsoft Edge on Windows plus Windows-screen-reader checkpoint; local
Chromium/macOS evidence is not a substitute.
**UI hint**: yes

### Phase 4: Owned PDF Preview and Export

**Goal**: Users can preview and export a reproducible, selectable PDF from a fixed FlowDocument revision and recover the exact owned source.
**Mode:** mvp
**Depends on**: Phase 3
**Requirements**: PDFX-01, PDFX-02, PDFX-03, PDFX-04, PDFX-05, PDFX-06, PDFX-07, PDFX-08, QUAL-07
**Success Criteria** (what must be TRUE):

  1. User can navigate, zoom, select and search a virtualized preview derived from the canonical display list.
  2. Exported PDF preserves supported page geometry, text, images, paths, links, metadata and outlines with embedded subset fonts and Unicode mappings.
  3. Browser and target-viewer rendering pass the defined geometry and visual-difference thresholds.
  4. Reopening an owned PDF restores an identical canonical FlowDocument when its source payload is available.
  5. Every export records reproducibility inputs and passes structural, extraction, visual and form validation gates.

**Plans**: 6/6 plans executed

- [x] 04-01-PLAN.md — Establish bounded COS values and a deterministic PDF page envelope.
- [x] 04-02-PLAN.md — Connect shaped layout to embedded fonts, Unicode mappings, and selectable text.
- [x] 04-03-PLAN.md — Add bounded images, metadata, outlines/links, and unsupported-content reporting.
- [x] 04-04-PLAN.md — Add reproducibility manifests and exact owned-source recovery.
- [x] 04-05-PLAN.md — Expose revision-safe export and the accessible virtualized PDF preview.
- [x] 04-06-PLAN.md — Run the Phase 4 structural/extraction/visual release gate honestly.

Phase 4 is planned from the Phase 3 fixed-point/display-list contracts. Plans
04-01 through 04-05 provide the bounded syntax/page-envelope foundation,
revision-bound display list, deterministic TrueType/ToUnicode resource
preparation, bounded PNG/JPEG resource preparation, typed metadata,
outline/internal-link and unsupported-feature reporting, exact owned-source
recovery, and a revision-safe browser export/preview adapter. Plan 04-06 adds
the exact local/reference release gate and passes all local rows twice in the
prepared environment; its four external PDF/reference rows remain
`unavailable` because no checked-in fixture or matching tool/viewer is
available. End-to-end PDF text/image content streams, target-viewer
compatibility, and general PDF import remain unclaimed.
**UI hint**: yes

### Phase 5: Semantic Fillable Forms

**Goal**: Users can design and fill semantic fields whose PDF widgets remain valid after document reflow.
**Mode:** mvp
**Depends on**: Phase 4
**Requirements**: FORM-01, FORM-02, FORM-03, FORM-04, FORM-05, FORM-06, FORM-07
**Success Criteria** (what must be TRUE):

  1. User can add and configure every supported field type, validation rule, option and tab position.
  2. Semantic field anchors resolve to the correct page and rectangle after any supported repagination.
  3. User can fill, validate, import and export values independently from visual PDF appearances.
  4. Exported fields and appearance states behave consistently in the target viewer matrix.
  5. Flattening is explicit, selected, validated and never silently replaces the editable source.

**Plans**: 26/26 local plans executed; local gate passed; phase-level release evidence remains open

- [x] 05-01-PLAN.md — Add Rust-owned field-value validation and deterministic
  display-list anchor-to-widget projection with explicit review paths.
- [x] 05-02-PLAN.md — Add a revision/hash-bound noncanonical Rust form-value
  session with immutable fill, clear, and effective-value operations.
- [x] 05-03-PLAN.md — Connect validated session values to a versioned
  session-aware widget projection while preserving authored defaults.
- [x] 05-04-PLAN.md — Emit bounded deterministic AcroForm field/widget
  structure through the owned PDF COS writer without appearance claims.
- [x] 05-05-PLAN.md — Expose the Rust/WASM form-session protocol and persist
  accepted current values in a guarded dedicated IndexedDB store.
- [x] 05-06-PLAN.md — Add accessible native controls for already-authored valid
  fields through the Rust-owned session coordinator.
- [x] 05-07-PLAN.md — Expose accessible descriptor configuration for
  already-authored valid fields through the reversible Rust `SetField` path.
- [x] 05-08-PLAN.md — Add safe accessible anchor placement for already-authored
  valid fields through the accepted Rust caret and `SetField` path.
- [x] 05-09-PLAN.md — Add parity-closed Rust-owned default text-field insertion
  at an accepted collapsed caret with localized accessible controls.
- [x] 05-10-PLAN.md — Add confirmed Rust-owned semantic field removal with an
  exact indexed inverse, accessible localized controls, and source rebind fences.
- [x] 05-11-PLAN.md — Add Rust-owned semantic field tab-order authoring with
  derived editor/widget/PDF order and accessible localized earlier/later controls.
- [x] 05-12-PLAN.md — Add bounded deterministic AcroForm normal appearance
  streams, shared resources, and validated checkbox/radio appearance states.
- [x] 05-13-PLAN.md — Add explicit export-time flattening of selected accepted
  fields into derived page content while retaining unselected widgets and the
  recoverable owned source.
- [x] 05-14-PLAN.md — Carry the source-bound form plan and explicit flatten
  selection through the closed Rust/WASM export envelope.
- [x] 05-15-PLAN.md — Expose Rust-owned display-list-backed form projection
  and optional plan derivation through a verified string-only WASM boundary.
- [x] 05-16-PLAN.md — Add a revision-safe browser adapter and scheduler for
  the verified Rust/WASM form projection response.
- [x] 05-17-PLAN.md — Integrate verified form projection into the editor as a
  reflow-safe visual overlay and accessible read-only review summary.
- [x] 05-18-PLAN.md — Add explicit accessible flatten selection controls and
  carry the source-bound selection to the PDF export factory.
- [x] 05-19-PLAN.md — Centralize the Rust-compatible browser PDF request
  builder and prove form-plan/flatten-selection payload propagation.
- [x] 05-20-PLAN.md — Prove the ordinary shared PDF request builder against
  the generated Rust/WASM export and response verifier.
- [x] 05-21-PLAN.md — Use the shared PDF request builder as the controller
  default while preserving explicit custom factories.
- [x] 05-22-PLAN.md — Prove the controller default PDF builder in the real
  Chromium preview path.
- [x] 05-23-PLAN.md — Expose caller-owned controller dependencies at the
  editor shell boundary.
- [x] 05-24-PLAN.md — Compose the WASM PDF engine and scheduler at one
  caller-owned seam.
- [x] 05-25-PLAN.md — Verify the PDF worker message loop at its scope
  boundary.
- [x] 05-26-PLAN.md — Wire the Rust-verified font catalog and production
  layout/PDF worker entries without creating a browser-side authority.

The twenty-six Phase 5 slices validate the existing semantic field vocabulary,
derive revision/hash-bound fixed-point widget projections, keep explicit
noncanonical fill overrides separate from authored defaults through to the
derived widget, emit bounded AcroForm field/widget dictionaries from an
accepted projection, carry accepted current values through a versioned
Rust/WASM boundary into a guarded dedicated IndexedDB store, expose accessible
native controls, configure descriptors, place already-authored valid fields
through Rust `SetField`, insert a typed default text field through the
parity-catalogued Rust `InsertField` path, and remove valid fields through the
confirmed `RemoveField` path. Valid fields can also be reordered through the
parity-catalogued `MoveField` path; editor and form/PDF projections derive
contiguous tab order from the canonical field vector without persisting a
tab-order property, and the owned writer emits derived bounded normal
appearance streams with explicit AcroForm resources/defaults and button state
dictionaries. The final slice adds an explicit core export selection that
translates selected accepted appearances into fixed-point page content,
retains unselected editable widgets, omits `/AcroForm` only when all fields are
selected, and keeps exact owned-source recovery. The final projection slice
reuses validated layout execution, builds display-list-backed widget geometry,
and carries session-effective values, review entries, and an optional
source-bound plan through a verified Rust/WASM response. The slices do not
claim target-viewer compatibility or external form import; those remain
separately planned and verified. The browser now exposes source-bound
per-field/select-all/clear flatten intent and passes it to the opaque export
factory without becoming a second semantic or PDF authority.
The source-bound form plan, explicit selection, and projection response now
cross the closed Rust/WASM export boundary without moving semantic authority
into the browser. A typed browser adapter now binds that response to the
accepted layout result hash and optional session. The EditorApp now schedules
the verified projection against the accepted layout/session, shows a visual
fixed-point widget overlay, exposes an accessible read-only widget/review
summary, and passes explicit selection intent to the opaque export factory.
The request-construction slice then centralizes the exact Rust-compatible
opaque payload, including accepted page bounds, reproducibility identities, and
verified form-plan/flatten-selection data, while leaving production font and
worker wiring caller-owned.
The next smoke executes the ordinary builder output through generated WASM
`export_pdf` and verifies the Rust-produced response identity.
The controller integration slice then uses the same builder by default when a
caller supplies a PDF scheduler without a custom factory; custom factories
remain authoritative and no production worker or font catalog is created.
The browser preview smoke now exercises that default through the existing
caller-owned scheduler and checks the captured source/layout-bound request
while retaining stale export suppression.
The shell composition slice exposes the same caller-owned controller
dependencies through `EditorApp` and `mountEditorApp` without creating
production workers or font catalogs.
The WASM composition slice adds a small helper that joins the existing
Rust-verifying adapter to the revision-aware scheduler, and the generated-WASM
smoke now covers the complete request-to-accepted-result path.
The worker-boundary slice verifies accepted and cooperative-cancelled outbound
messages with a fake scope, and the final wiring slice activates the
repository-owned production layout/PDF worker entries with a bounded
Rust-verified Noto Sans and Ukrainian hyphenation catalog. The real Chromium
smoke proves the default runtime path on a supported-glyph fixture; the sample
document's deliberately unsupported emoji/non-BMP path remains fail-closed.
No target-viewer compatibility or external PDF form import is claimed: the
available environment has no matching external viewer/tool fixture, and
external form import remains outside the implemented local scope.
**UI hint**: yes

### Phase 6: Voice Dictation and Commands

**Goal**: Users can dictate and operate the editor by voice without bypassing transaction, confirmation, accessibility or privacy boundaries.
**Mode:** mvp
**Depends on**: Phase 5
**Requirements**: VOIC-01, VOIC-02, VOIC-03, VOIC-04, VOIC-05, VOIC-06
**Success Criteria** (what must be TRUE):

  1. User explicitly controls push-to-talk and can always distinguish dictation from command mode.
  2. Interim recognition never mutates the document; final dictation becomes exactly one undoable transaction.
  3. Allowlisted commands resolve structured intents against the expected selection and document revision.
  4. Ambiguous or high-risk operations require the configured preview/confirmation and provide visible and accessible feedback.
  5. Stopping voice input ends microphone capture and default telemetry retains no raw audio or sensitive transcript text.

**Plans**: 4/6 executed; 2 remaining

- [x] 06-01-PLAN.md — Add the Rust-owned bounded voice-intent boundary.
- [x] 06-02-PLAN.md — Route final dictation through one revision-safe
  transaction.
- [x] 06-03-PLAN.md — Add the ephemeral push-to-talk recognition state machine.
- [x] 06-04-PLAN.md — Expand allowlisted commands to navigation, formatting,
  history, and fields.
- [ ] 06-05-PLAN.md — Expose accessible voice controls, preview, confirmation,
  and feedback.
- [ ] 06-06-PLAN.md — Close the voice phase with privacy and regression gates.
**UI hint**: yes

### Phase 7: Secure PDF Reader and Scene

**Goal**: Users can open and inspect a controlled PDF subset through a bounded, revision-aware reader that never hides unsupported or active content.
**Mode:** mvp
**Depends on**: Phase 4
**Requirements**: PDFI-01, PDFI-02, PDFI-03, PDFI-05, QUAL-05, QUAL-06
**Success Criteria** (what must be TRUE):

  1. User can open valid supported PDFs and view recognized text, images, paths, clipping, forms, links and annotations.
  2. Extracted text and scene elements retain glyph geometry and source object/operator provenance when available.
  3. Unsupported filters, fonts, operators, mappings, forms and active actions appear in a conversion/support report.
  4. Hostile or over-budget files terminate with a bounded diagnostic rather than hanging the UI or service.
  5. PDF JavaScript, launch actions, executables and external resource actions never execute during import or preview.

**Plans**: TBD
**UI hint**: yes

### Phase 8: External Reconstruction and OCR

**Goal**: Users can turn supported external or scanned PDF content into a reviewable FlowDocument without losing the immutable source or obscuring uncertainty.
**Mode:** mvp
**Depends on**: Phase 7
**Requirements**: PDFI-04, PDFI-06, PDFI-07, PDFI-08, QUAL-01, QUAL-02
**Success Criteria** (what must be TRUE):

  1. User can reconstruct supported single-column pages into semantic blocks with source mappings and per-node confidence.
  2. Unsupported visual content remains visible as an immutable opaque island when safe preservation is possible.
  3. User can compare original and reconstructed documents side by side and review OCR or semantic uncertainty before accepting it.
  4. The original PDF remains immutable and the UI never labels lossy reconstruction as exact.

**Plans**: TBD
**UI hint**: yes

### Phase 9: Controlled Native PDF Editing

**Goal**: Users can directly modify recognized PDF page objects and perform secure page and redaction operations within the supported scene subset.
**Mode:** mvp
**Depends on**: Phase 7
**Requirements**: NPDF-01, NPDF-02, NPDF-03, NPDF-04, NPDF-05
**Success Criteria** (what must be TRUE):

  1. User can edit supported annotations, form widgets, text islands, images and simple graphics while unsupported scene content remains intact.
  2. User receives an actionable warning instead of corrupted output when fonts, transforms, clipping or resource constraints make a local edit unsafe.
  3. User can reorder, rotate, insert, duplicate and delete pages while retaining supported resources and annotations.
  4. Secure redaction uses a full rewrite and passes post-save object, text and visual checks.

**Plans**: TBD
**UI hint**: yes

## Progress

**Execution Order:** Phase 1 -> 2 -> 3 -> 4 -> 5 -> 6; Phase 7 follows Phase 4 and unlocks Phases 8 and 9. With one executor, phases run sequentially in numeric order unless an inserted phase is recorded.

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Durable Flow Foundation | 6/6 | Complete    | 2026-08-25 |
| 2. Accessible Rich-Text Editing | 18/18 | In Progress (external AT checkpoint) |  |
| 3. Deterministic Reflow and Pagination | 6/6 | In Progress (external AT checkpoint) |  |
| 4. Owned PDF Preview and Export | 6/6 | In Progress (reference evidence unavailable) |  |
| 5. Semantic Fillable Forms | 26/26 executed | Complete locally (release evidence pending) | 2026-09-22 |
| 6. Voice Dictation and Commands | 2/6 executed | In Progress |  |
| 7. Secure PDF Reader and Scene | 0/TBD | Not started | - |
| 8. External Reconstruction and OCR | 0/TBD | Not started | - |
| 9. Controlled Native PDF Editing | 0/TBD | Not started | - |

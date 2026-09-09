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
- [ ] **Phase 5: Semantic Fillable Forms** - Users can author validated semantic fields that become interoperable AcroForm widgets after pagination.
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

**Plans**: 3/18 plans executed

Plans:

- [x] 02-01-PLAN.md — Admit and lock exact Phase 2 dependencies without lifecycle execution.
- [x] 02-02-PLAN.md — Pin Unicode conformance data, numerical WASM growth, and retained boundaries.
- [x] 02-03-PLAN.md — Execute the one-way legacy freeze and every schema-v2 migration route.
- [ ] 02-04-PLAN.md — Establish Rust grapheme, atomic-position, and editor-session authority.
- [ ] 02-05-PLAN.md — Prove the core/WASM/persistence/minimal-React paragraph tracer.
- [ ] 02-06-PLAN.md — Add controlled input, directional selection, paste, and real Chromium IME.
- [ ] 02-07-PLAN.md — Implement exact structural and deletion/preimage/recovery algebra.
- [ ] 02-08-PLAN.md — Attach durable visible and keyboard structural product paths.
- [ ] 02-09-PLAN.md — Implement closed formatting, pending state, and list semantics in Rust.
- [ ] 02-10-PLAN.md — Expose accessible formatting/list controls and parity.
- [ ] 02-11-PLAN.md — Implement complete table headers/removal and page-break semantics in Rust.
- [ ] 02-12-PLAN.md — Expose native table/page-break controls, semantics, and confirmation.
- [ ] 02-13-PLAN.md — Implement complete image lifecycle, BLAKE3 identity, and shared reachability.
- [ ] 02-14-PLAN.md — Carry image lifecycle through WASM, atomic storage, and accessible controls.
- [ ] 02-15-PLAN.md — Generate exhaustive command parity and retained boundary contracts.
- [ ] 02-16-PLAN.md — Keep existing fields and the editor shell semantically accessible.
- [ ] 02-17-PLAN.md — Close the exact 108-pair UI contract and honest AT evidence.
- [ ] 02-18-PLAN.md — Add fast scale feedback and the deterministic dual-run release gate.

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

**Plans**: TBD
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

**Plans**: TBD
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

**Plans**: TBD
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

**Plans**: TBD
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
| 2. Accessible Rich-Text Editing | 3/18 | In Progress|  |
| 3. Deterministic Reflow and Pagination | 0/TBD | Not started | - |
| 4. Owned PDF Preview and Export | 0/TBD | Not started | - |
| 5. Semantic Fillable Forms | 0/TBD | Not started | - |
| 6. Voice Dictation and Commands | 0/TBD | Not started | - |
| 7. Secure PDF Reader and Scene | 0/TBD | Not started | - |
| 8. External Reconstruction and OCR | 0/TBD | Not started | - |
| 9. Controlled Native PDF Editing | 0/TBD | Not started | - |

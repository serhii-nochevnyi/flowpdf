# Requirements: FlowPDF

**Defined:** 2026-08-14  
**Core Value:** A user can edit semantic document text naturally, with all following content repaginating correctly, and export the result as a visually consistent, selectable, form-capable PDF.

## User Stories

- As a document author, I can edit Ukrainian and English text as a flowing document instead of manually repositioning content on every PDF page.
- As a form designer, I can anchor fillable fields to semantic content and trust them to follow pagination.
- As a PDF user, I can distinguish exact owned round-trip from best-effort external reconstruction before I edit.
- As a keyboard, assistive-technology, or voice user, I can invoke the same safe, undoable document operations.
- As a security-conscious user, I keep the original imported PDF intact and receive explicit warnings about unsupported or uncertain content.

## v1 Requirements

### FlowDocument

- [x] **FLOW-01**: User can create a new versioned FlowDocument with page settings, locale, styles, content, assets, and fields.
- [x] **FLOW-02**: User can save a FlowDocument and reopen it without semantic, style, field, or asset loss.
- [x] **FLOW-03**: User can open a document created by an older supported schema version through deterministic migrations.
- [x] **FLOW-04**: User can recover the last durable document revision after an editor refresh or worker failure.
- [x] **FLOW-05**: User can inspect the document revision and conversion provenance used for a preview or export.

### Editing

- [ ] **EDIT-01**: User can place a caret and select text without splitting a Unicode grapheme cluster.
- [ ] **EDIT-02**: User can insert, replace, and delete Ukrainian and English text through keyboard and IME input.
- [ ] **EDIT-03**: User can split and merge paragraphs while preserving logical text order and compatible styles.
- [ ] **EDIT-04**: User can apply bold, italic, underline, font, size, color, language, alignment, spacing, and list formatting.
- [ ] **EDIT-05**: User can create headings, paragraphs, ordered lists, unordered lists, images, simple tables, and explicit page breaks.
- [x] **EDIT-06**: User can undo and redo every committed content, style, structure, and field operation as an atomic transaction.
- [x] **EDIT-07**: User receives a deterministic conflict instead of a misplaced edit when a command targets a stale revision or invalid anchor.

### Layout and Pagination

- [ ] **LAYO-01**: User sees Unicode text shaped with correct glyph ordering, clusters, fallback fonts, and direction for supported scripts.
- [ ] **LAYO-02**: User sees language-appropriate line-break and Ukrainian hyphenation opportunities without changing stored text.
- [ ] **LAYO-03**: User sees following content move and repaginate after an edit changes a block's size.
- [ ] **LAYO-04**: User can configure page size, orientation, margins, sections, static headers, and static footers.
- [ ] **LAYO-05**: User can keep headings with following content and apply widow, orphan, and keep-together constraints with deterministic fallback.
- [ ] **LAYO-06**: User sees simple tables paginate between rows and repeat configured header rows.
- [ ] **LAYO-07**: User gets the same fragment tree and rendered pages from incremental reflow as from a full reflow of the same revision.
- [ ] **LAYO-08**: User can continue editing the active page while remaining pages paginate in a worker and update progressively.

### Preview and PDF Export

- [ ] **PDFX-01**: User can preview virtualized document pages with zoom, page navigation, selection, and search.
- [ ] **PDFX-02**: User can export an unencrypted standards-conformant PDF from an immutable FlowDocument revision.
- [ ] **PDFX-03**: Exported Ukrainian and English text is selectable, searchable, and extractable in logical order.
- [ ] **PDFX-04**: Exported PDF embeds licensed subset fonts and valid Unicode mappings for all emitted glyphs.
- [ ] **PDFX-05**: Exported PDF preserves supported images, paths, links, page geometry, metadata, and outlines from the FlowDocument.
- [ ] **PDFX-06**: Browser preview and PDF rendering use the same layout/display-list geometry and pass the defined visual-difference threshold.
- [ ] **PDFX-07**: User can reopen a FlowPDF-generated PDF and recover the exact source FlowDocument revision when its source payload is available.
- [ ] **PDFX-08**: User receives structural, text-extraction, visual, and form-validation results for every completed export.

### Fillable Fields

- [ ] **FORM-01**: User can add text, multiline, checkbox, radio-group, select/combo, date, number, email, and signature-placeholder fields.
- [ ] **FORM-02**: User can set a field's stable name, label, default value, required, read-only, options, validation, and tab order.
- [ ] **FORM-03**: User can anchor a field before, after, inside, or relative to semantic FlowDocument content.
- [ ] **FORM-04**: User sees field widgets move to the correct page and rectangle after repagination.
- [ ] **FORM-05**: User can fill, clear, validate, import, and export field values independently of the PDF appearance layer.
- [ ] **FORM-06**: Exported AcroForm fields have explicit appearances and consistent states in target PDF viewers.
- [ ] **FORM-07**: User can flatten selected fields into page content only through an explicit, separately undoable/exported operation.

### Voice Interaction

- [x] **VOIC-01**: User can explicitly start and stop push-to-talk dictation and see a persistent microphone-state indicator.
- [x] **VOIC-02**: User sees interim recognition as non-document ghost text and only final recognition becomes one undoable insert transaction.
- [x] **VOIC-03**: User can switch explicitly between dictation mode and command mode so spoken document text is not executed as a command.
- [x] **VOIC-04**: User can invoke allowlisted navigation, formatting, editing, undo/redo, and field commands through structured intents.
- [x] **VOIC-05**: User must preview or confirm destructive, ambiguous, export, flatten, and signing-related voice operations according to risk policy.
- [x] **VOIC-06**: User receives visible, accessible, and optional spoken feedback when a voice command succeeds, fails, or needs disambiguation.

### Controlled PDF Import

- [ ] **PDFI-01**: User can open a valid unencrypted PDF from the supported subset without loading the full object graph eagerly.
- [ ] **PDFI-02**: User can view supported text, images, paths, transforms, clipping, forms, links, and annotations through the internal PDF scene.
- [ ] **PDFI-03**: User can extract mapped Unicode text with glyph positions and source object/operator provenance where the PDF provides enough information.
- [ ] **PDFI-04**: User can convert supported single-column PDF pages into FlowDocument blocks with per-node confidence and source mapping.
- [ ] **PDFI-05**: User receives an explicit report for uncertain reading order, missing Unicode mapping, unsupported operators, fonts, filters, forms, and active content.
- [ ] **PDFI-06**: User sees unsupported visual content preserved as an immutable opaque island when safe preservation is possible.
- [ ] **PDFI-07**: User can compare the original PDF and reconstructed FlowDocument side by side before accepting the conversion.
- [ ] **PDFI-08**: User can submit scanned pages through an OCR adapter and review low-confidence recognized text before it becomes editable content.

### Native PDF Editing

- [ ] **NPDF-01**: User can add, edit, move, and remove supported annotations and AcroForm widgets without converting the document to Flow mode.
- [ ] **NPDF-02**: User can replace text within a recognized editable PDF text island and receive a warning when font or layout constraints prevent a safe edit.
- [ ] **NPDF-03**: User can add, replace, move, and remove supported page images and simple graphic objects within recognized scene boundaries.
- [ ] **NPDF-04**: User can reorder, rotate, insert, duplicate, and delete pages while retaining supported page resources and annotations.
- [ ] **NPDF-05**: User can perform secure redaction only through a full rewrite that passes post-save text, object, and visual validation.

### Safety, Accessibility and Operability

- [ ] **QUAL-01**: User's original imported PDF remains immutable and available as the source for every derived document.
- [ ] **QUAL-02**: User is never told a lossy external reconstruction is exact and can inspect all conversion warnings before editing.
- [ ] **QUAL-03**: User can operate every document mutation available by voice through keyboard and visible UI controls.
- [ ] **QUAL-04**: Screen-reader users can navigate semantic document content, fields, status changes, errors, and confirmation prompts.
- [ ] **QUAL-05**: User receives bounded failure instead of a hung tab or service when a PDF exceeds object, depth, decoded-byte, image, page, memory, or time limits.
- [ ] **QUAL-06**: Active PDF JavaScript, launch actions, embedded executables, and external resource actions are disabled during import and preview.
- [ ] **QUAL-07**: User can reproduce an export from its FlowDocument revision, font hashes, engine version, locale data, and export options.
- [x] **QUAL-08**: User can inspect a concise audit history of document mutations without raw microphone audio or sensitive transcript analytics being retained by default.

## Acceptance Criteria

- A representative 100-page FlowDocument remains editable while a change on page 1 repaginates following pages in the background.
- Ukrainian text containing combining marks, apostrophes, punctuation, ligatures, emoji, and mixed Latin text preserves caret, selection, extraction, and round-trip behavior.
- Property-based edit sequences produce identical incremental and full fragment trees.
- A generated PDF passes the project's structural checker, text extraction comparison, target-viewer form tests, and configured visual-diff threshold.
- `FlowDocument A -> PDF -> recovered FlowDocument B` yields identical canonical semantic hashes for owned PDFs.
- Importing unsupported or hostile PDF input never silently discards content or bypasses resource limits.
- Every voice mutation produces a validated command, revision check, atomic transaction, inverse operation, and user-visible result.

## Definition of Done

A v1 requirement is complete only when implementation, automated tests, relevant corpus fixtures, security/resource-limit checks, and observable user verification all pass in the same committed revision. Browser-only visual success, parser-only syntax success, or task completion without requirement verification is insufficient.

## v2 Requirements

### Advanced Layout

- **ADVL-01**: User can create multi-column sections with deterministic cross-column fragmentation.
- **ADVL-02**: User can use page footnotes whose callouts and footnote areas converge without pagination cycles.
- **ADVL-03**: User can use floating figures with text exclusion regions.
- **ADVL-04**: User can split complex tables with rowspans and colspans across pages.
- **ADVL-05**: User can author vertical-writing documents.

### Broader PDF Compatibility

- **COMP-01**: User can open encrypted PDFs supported by the Standard Security Handler.
- **COMP-02**: User can preserve and validate incremental PDF revisions and allowed signed-document changes.
- **COMP-03**: User can generate and validate tagged PDF with a defined PDF/UA conformance target.
- **COMP-04**: User can export archival variants with a defined PDF/A conformance target.
- **COMP-05**: User can inspect optional content groups and broader color/transparency models.
- **COMP-06**: User can use full-text reconstruction for common multi-column, table, header/footer, and footnote layouts.

### Collaboration and Deployment

- **COLL-01**: Multiple authorized users can collaboratively edit a FlowDocument with deterministic conflict resolution.
- **COLL-02**: Administrators can deploy the full service stack in a documented self-hosted environment.
- **COLL-03**: Users can complete cryptographic signing with certificate validation, timestamping, and long-term validation data.

## Out of Scope

| Feature | Reason |
|---------|--------|
| XFA authoring/runtime | Separate legacy platform with high complexity and inconsistent viewer support |
| Arbitrary embedded PDF JavaScript | Security and interoperability risk; application validation replaces it |
| Multimedia, 3D and geospatial PDF | Not required for the document/form core value |
| Pixel-identical external PDF semantic round-trip | Source semantics may never have existed in the PDF |
| Zero-dependency Unicode/font/codec/crypto implementation | High-risk reinvention without product value |
| Always-listening voice mode | Ambient activation and privacy risks outweigh v1 value |
| Universal malformed-PDF repair | Open-ended compatibility surface; add evidence-driven repairs only |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| FLOW-01 | Phase 1 | Complete |
| FLOW-02 | Phase 1 | Complete |
| FLOW-03 | Phase 1 | Complete |
| FLOW-04 | Phase 1 | Complete |
| FLOW-05 | Phase 1 | Complete |
| EDIT-01 | Phase 2 | Pending |
| EDIT-02 | Phase 2 | Pending |
| EDIT-03 | Phase 2 | Pending |
| EDIT-04 | Phase 2 | Pending |
| EDIT-05 | Phase 2 | Pending |
| EDIT-06 | Phase 1 | Complete |
| EDIT-07 | Phase 1 | Complete |
| LAYO-01 | Phase 3 | Pending |
| LAYO-02 | Phase 3 | Pending |
| LAYO-03 | Phase 3 | Pending |
| LAYO-04 | Phase 3 | Pending |
| LAYO-05 | Phase 3 | Pending |
| LAYO-06 | Phase 3 | Pending |
| LAYO-07 | Phase 3 | Pending |
| LAYO-08 | Phase 3 | Pending |
| PDFX-01 | Phase 4 | Pending |
| PDFX-02 | Phase 4 | Pending |
| PDFX-03 | Phase 4 | Pending |
| PDFX-04 | Phase 4 | Pending |
| PDFX-05 | Phase 4 | Pending |
| PDFX-06 | Phase 4 | Pending |
| PDFX-07 | Phase 4 | Pending |
| PDFX-08 | Phase 4 | Pending |
| FORM-01 | Phase 5 | Pending |
| FORM-02 | Phase 5 | Pending |
| FORM-03 | Phase 5 | Pending |
| FORM-04 | Phase 5 | Pending |
| FORM-05 | Phase 5 | Pending |
| FORM-06 | Phase 5 | Pending |
| FORM-07 | Phase 5 | Pending |
| VOIC-01 | Phase 6 | Complete |
| VOIC-02 | Phase 6 | Complete |
| VOIC-03 | Phase 6 | Complete |
| VOIC-04 | Phase 6 | Complete |
| VOIC-05 | Phase 6 | Complete |
| VOIC-06 | Phase 6 | Complete |
| PDFI-01 | Phase 7 | Pending |
| PDFI-02 | Phase 7 | Pending |
| PDFI-03 | Phase 7 | Pending |
| PDFI-04 | Phase 8 | Pending |
| PDFI-05 | Phase 7 | Pending |
| PDFI-06 | Phase 8 | Pending |
| PDFI-07 | Phase 8 | Pending |
| PDFI-08 | Phase 8 | Pending |
| NPDF-01 | Phase 9 | Pending |
| NPDF-02 | Phase 9 | Pending |
| NPDF-03 | Phase 9 | Pending |
| NPDF-04 | Phase 9 | Pending |
| NPDF-05 | Phase 9 | Pending |
| QUAL-01 | Phase 8 | Pending |
| QUAL-02 | Phase 8 | Pending |
| QUAL-03 | Phase 2 | Pending |
| QUAL-04 | Phase 2 | Pending |
| QUAL-05 | Phase 7 | Pending |
| QUAL-06 | Phase 7 | Pending |
| QUAL-07 | Phase 4 | Pending |
| QUAL-08 | Phase 1 | Complete |

**Coverage:**

- v1 requirements: 62 total
- Mapped to phases: 62
- Unmapped: 0 ✓

---
*Requirements defined: 2026-08-14*  
*Last updated: 2026-08-14 after roadmap creation*

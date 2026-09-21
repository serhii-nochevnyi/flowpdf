---
gsd_state_version: "1.0"
milestone: v1.0
current_phase: 5
current_phase_name: Semantic Fillable Forms
status: executing
stopped_at: Completed 05-06-PLAN.md; the Phase 5 Rust/WASM session boundary and accessible controls for already-authored fields are green, while field authoring UI/descriptor insertion, AcroForm appearances/viewer evidence, follow-on form plans, four external Phase 4 PDF/reference rows, and the inherited Phase 2/3 external AT checkpoint remain open
last_updated: "2026-09-21T22:17:41Z"
last_activity: 2026-09-22
last_activity_desc: Completed Phase 5 Plan 05-06 source-bound coordinator, accessible native controls, localized status projection, and Chromium evidence
state_head: 12a2d39
progress:
  total_phases: 9
  completed_phases: 1
  total_plans: 42
  completed_plans: 42
milestone_name: milestone
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-08-25)

**Core value:** A user can edit semantic document text naturally, repaginate following content, and export a visually consistent, selectable, form-capable PDF.
**Current focus:** Phase 5 — Semantic Fillable Forms (six local slices complete; field authoring UI/descriptor insertion, appearances/viewer evidence, and follow-on plans open)

## Current Position

Phase: 5 (Semantic Fillable Forms) — EXECUTING
Plan: 6 executed; follow-on plans to be decomposed
Status: 05-06 complete; Rust validates the current field vocabulary, derives deterministic revision/hash-bound fixed-point widget projections with explicit invalid/deleted/unmapped review, exposes separate authored defaults/effective session values, emits bounded deterministic AcroForm field/widget COS structure, and exposes a versioned Rust/WASM session action boundary with guarded `form-sessions-v1` IndexedDB persistence. The browser now renders accessible native controls for already-authored valid fields through a source-bound coordinator. Field authoring UI/descriptor insertion, appearance streams, flattening, target-viewer evidence, and external form import remain open. Phase 4 external PDF/reference rows and Phase 2/3 external Edge/Windows screen-reader evidence remain outstanding.
Last activity: 2026-09-22 — Phase 5 Plan 05-06 source-bound coordinator, accessible native controls, localized status projection, and Chromium evidence completed

Progress: Phase 2 — 18/18 implementation plans complete; Phase 3 — 6/6 local implementation plans executed; Phase 4 — 6/6 plans executed; Phase 5 — 6/6 local plans executed with field authoring/descriptor insertion, appearance/viewer, flattening, import, and follow-on plans still to be decomposed. Phase 2/3 phase-level closure remains honest about inherited external AT evidence, and Phase 4 remains open for unavailable external PDF/reference evidence.

## Performance Metrics

**Velocity:**

- Total plans completed: 42
- Average duration: not comparable (Plan 05 was a multi-session closeout)
- Total execution time: multi-session

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 1 | 6 | - | - |
| 2 | 18 | multi-session | - |
| 3 | 6 | multi-session | - |
| 4 | 6 | planned | - |
| 5 | 6 executed (follow-on TBD) | same session | - |

**Recent Trend:** Schema-v2 tree, lossless migration, editor accessibility, exact UI coverage, deterministic semantic scale, and the dual-run release gate remain green. Plan 02-18 closes all Phase 2 implementation plans with 7/7 requirements, 33/33 edges, 108/108 UI owners, and 3/3 flagged prohibitions; Edge/Windows screen-reader UAT remains explicitly outstanding before Phase 2 closure. Phase 3 now has all six local implementation plans complete, including Rust typography/pagination, schema-v3 sections, incremental equivalence, revision-safe workers, the accessible viewport, and a two-run local gate; Phase 3 phase-level closure does not invent the inherited external AT result. Phase 4 now has all six local plans complete and a two-run local gate, while structural/extraction/raster/target-viewer evidence remains explicitly unavailable.
**Per-Plan Metrics:**

| Plan | Duration | Tasks | Files |
|------|----------|-------|-------|
| Phase 1 P01 | 40 min | 3 tasks | 16 files |
| Phase 1 P02 | 27min | 2 tasks | 17 files |
| Phase 1 P03 | 35min | 3 tasks | 13 files |
| Phase 1 P04 | 30min | 3 tasks | 13 files |
| Phase FLOWPDF-01 P05 | 7d-multi-session | 3 tasks | 18 files |
| Phase FLOWPDF-01 P06 | 46min | 3 tasks | 15 files |
| Phase FLOWPDF-02 P01 | 17m 7s | 2 tasks | 11 files |
| Phase FLOWPDF-02 P02 | multi-session | 2 tasks | 7 files |
| Phase FLOWPDF-02 P03 | multi-session | 2 tasks | 28 files |
| Phase FLOWPDF-02 P04 | resumed multi-session | 2 tasks | 7 files |
| Phase FLOWPDF-02 P05 | resumed multi-session | 2 tasks | 16 files |
| Phase FLOWPDF-02 P06 | resumed multi-session | 2 tasks | 14 files |
| Phase FLOWPDF-02 P07 | resumed multi-session | 2 tasks | 9 files |
| Phase FLOWPDF-02 P08 | resumed multi-session | 2 tasks | 12 files |
| Phase FLOWPDF-02 P09 | resumed multi-session | 2 tasks | 8 files |
| Phase FLOWPDF-02 P10 | resumed multi-session | 2 tasks | 19 files |
| Phase FLOWPDF-02 P11 | resumed multi-session | 2 tasks | 8 files |
| Phase FLOWPDF-02 P12 | resumed multi-session | 2 tasks | 12 files |
| Phase FLOWPDF-02 P13 | resumed multi-session | 2 tasks | 12 files |
| Phase FLOWPDF-02 P14 | resumed multi-session | 2 tasks | 13 files |
| Phase FLOWPDF-02 P15 | resumed multi-session | 2 tasks | 8 files |
| Phase FLOWPDF-02 P16 | resumed multi-session | 2 tasks | 14 files |
| Phase FLOWPDF-02 P17 | resumed multi-session | 2 tasks | 9 files |
| Phase FLOWPDF-02 P18 | resumed multi-session | 2 tasks | 14 files |
| Phase FLOWPDF-03 P01 | same session | 2 tasks | 11 files |
| Phase FLOWPDF-03 P02 | same session | 2 tasks | 13 files |
| Phase FLOWPDF-03 P03 | same session | 2 tasks | 5 files |
| Phase FLOWPDF-03 P04 | same session | 2 tasks | 5 files |
| Phase FLOWPDF-04 P01 | same session | 2 tasks | 3 files |
| Phase FLOWPDF-04 P02 | same session | 2 tasks | 7 files |
| Phase FLOWPDF-04 P03 | same session | 2 tasks | 7 files |
| Phase FLOWPDF-04 P04 | same session | 2 tasks | 7 files |
| Phase FLOWPDF-04 P05 | same session | 2 tasks | 16 files |
| Phase FLOWPDF-04 P06 | same session | 2 tasks | 10 files |
| Phase FLOWPDF-05 P05 | same session | 2 tasks | 9 files |
| Phase FLOWPDF-05 P06 | same session | 2 tasks | 10 files |

## Accumulated Context

### Decisions

Decisions are logged in `.planning/PROJECT.md`.

- FlowDocument is canonical for reflow; PDF remains a derived or native fixed-layout representation.
- Rust is the sole semantic authority on native and WASM targets; TypeScript constructs DTOs, performs physical browser I/O, and renders verified results.
- UI, keyboard, API, and future voice input share one typed transactional command service with revision checks, deterministic undo/redo, and explicit anchor invalidation.
- Browser durability is acknowledged only after one atomic IndexedDB transaction completes and Rust revalidates the recovered record set; exact retries converge and divergent writes conflict.
- Canonical UTF-8 bytes, UTF-16 public positions, contiguous schema migrations, provenance, and closed audit projections are versioned and deterministic; hashes prove consistency rather than external authenticity.
- [Phase 2]: Candidate fallback is allowed only across predeclared exact versions; missing or contradictory official evidence is terminal.
- [Phase 2]: npm legitimacy uses exact-release age; crate legitimacy uses package inception age while retaining exact-release evidence.
- [Phase 2]: ICU segmentation is locked to compiled_data and image decoding to PNG/JPEG with default Cargo features disabled.
- [Phase 2]: WASM size evidence binds stable provenance identities and verified executable bytes; live observation timestamps do not affect determinism.
- [Phase 2]: Frozen legacy decoders precede schema changes; v2 uses one canonical inline-run tree with retained block style defaults.
- [Phase 2]: Stored migration-only review states remain explicit; new authoring rejects them and plain commands reject unsupported rich targets atomically.
- [Phase 2]: ICU byte boundaries are translated to UTF-16 in one bounded linear pass; editor sessions remain noncanonical, revision-bound, and immutable at the WASM boundary.
- [Phase 2]: ReplaceSelection validates directional grapheme endpoints in Rust, persists only through the existing guarded planner, and publishes React text only after durable commit plus Rust re-query.
- [Phase 2]: Structural keyboard and visible controls translate to the same closed Rust commands; accepted capability projection and durable re-query, never DOM mutation, determine the semantic result.
- [Phase 2]: Split node identity is deterministic from the accepted document context for cross-modality canonical parity; command and audit identities remain independent.
- [Phase 2]: Recursive list/table DTO projection is available, while sibling list-item split/merge semantics remain scoped to Plan 02-09.
- [Phase 2]: Formatting and list mutations are closed Rust commands with exact identity-mapped inverse children; collapsed inline attributes remain revision-bound session state.
- [Phase 2]: Authoring accepts only bounded heading/font/spacing values and uppercase #RRGGBB colors; legacy unknown fonts are retained only for migration/read compatibility.
- [Phase 2]: List nesting is capped at depth eight, with continuation, empty-item exit, indent, and outdent validated in Rust before publication.
- [Phase 2]: Native formatting/list controls project accepted Rust state and share the typed controller command bus; React owns only physical disclosure and focus state.
- [Phase 2]: DOM selection restoration uses `Selection.setBaseAndExtent` so directional anchor/focus order remains intact across accepted React renders.
- [Phase 2]: Durable replay validates the complete closed formatting/list command vocabulary before acknowledging a transaction.
- [Phase 2]: Structural insertion uses explicit Rust parent/index placement; page breaks are atomic separators followed by an editable paragraph, and ordinary text deletion/merge cannot consume them.
- [Phase 2]: Simple tables remain bounded to 50 rows, 20 columns, and 1,000 cells, with an optional first header row, Rust-owned row-major focus, and confirmation-gated whole-table/last-dimension removal.
- [Phase 2]: Structural table/page-break commands are audit/replay-closed and use exact subtree preimages so durable recovery restores canonical content and header state.
- [Phase 2]: Native structural projection consumes Rust placement, limits, header state, confirmation metadata, and table-cell focus order; React does not infer semantic coordinates from DOM order.
- [Phase 2]: Image bytes enter Rust only through bounded binary staging; canonical assets use the existing BLAKE3 identity, receipts are one-use revision-bound secrets, and semantic DTOs never carry raw bytes or staging digests.
- [Phase 2]: Image replacement/removal retains physical records until a future persistence policy proves safe collection with undo/recovery retention; current commits prioritize no premature shared-byte deletion.
- [Phase 2]: The closed Rust mutation catalog is the sole source of command capability parity; visible, keyboard, and future voice routes share intent, risk, confirmation, validation, and undo semantics.
- [Phase 2]: Physical image bytes and hashes remain outside semantic command DTOs, while phase-aware contracts retain immutable WASM, safe native DOM, and deferred layout/PDF/voice/backend exclusions.
- [Phase 2]: Existing fields remain Rust-owned, read-only editor projections; valid fields are ordered by logical anchors and invalid/deleted targets remain in an explicit review region with exact descriptor state.
- [Phase 2]: The editor shell exposes one semantic document copy, one polite status, a separate atomic alert, and native keyboard-reachable field groups; local browser evidence does not substitute for external Windows/Edge/screen-reader evidence.
- [Phase 2]: Semantic scale recipes remain bounded and layout/PDF-free; the release runner derives all 36 validation tasks, fails fast with safe diagnostics, and pins direct Browser Mode steps to the checked-in Playwright cache.
- [Phase 2]: Vitest 4 filters use `--testNamePattern`; GitHub repository fallback explicitly requests HTML after API rate limiting while provenance remains fail-closed.
- [Phase 3]: Layout geometry begins with checked 1/64-point `LayoutUnit` values; public request/result DTOs carry scalar identities and hashes rather than floats or raw provider state.
- [Phase 3]: Text layout is derived from canonical source text and bound to source revision/hash, explicit byte-backed font catalog identity, hyphenation identity, and engine version; it never becomes canonical document state.
- [Phase 3]: The admitted typography fixture is checked-in Noto Sans Regular with OFL text and SHA-256 provenance; unsupported glyphs, malformed fonts, duplicate identities, and missing/mismatched Ukrainian data fail closed without host-font discovery.
- [Phase 3]: ICU grapheme/line segmentation, Unicode bidi, RustyBuzz shaping, and versioned Ukrainian hyphenation are the first adapters; pagination, schema-v3 sections, workers, and PDF output remain later plans.
- [Phase 3]: Schema-v3 stores page geometry, ordered semantic section boundaries, and bounded typed static header/footer runs; it never stores derived fragments, glyphs, display-list bytes, or coordinates.
- [Phase 3]: The frozen v0/v1 decoder and source/hash gate remain unchanged; a private canonical v2 envelope admits exactly one v2-to-v3 migration hop with deterministic default section settings and preserved provenance.
- [Phase 3]: The first section is the document-start boundary and mirrors top-level page settings; later boundaries reference ordered top-level content nodes and reject duplicate, missing, or out-of-order anchors before publication.
- [Phase 3]: Pagination is a borrowed, revision/hash-bound derived result using fixed-point page geometry, stable source ranges, explicit break reasons, repeated static bands, and bounded diagnostics; canonical FlowDocument state is never mutated.
- [Phase 3]: Simple tables split only between rows; configured first header rows repeat as derived fragments, while overlarge rows and impossible constraints publish explicit overflow/fallback diagnostics rather than retrying indefinitely.
- [Phase 3]: Full pagination remains the executable equivalence oracle; changed or ambiguous revisions fall back to it, while only an exact verified no-op cache may be reused.
- [Phase 3]: Carry signatures bind revision/hash, layout settings, font/hyphenation identities, boundary, and accepted page prefix; stale, forged, cross-fingerprint, and unknown inputs cannot publish reused results.
- [Phase 3]: The WASM layout boundary is string-only and versioned; bounded canonical/font/data ingress is consumed by Rust, while results expose only derived fragments, privacy-safe diagnostics, identities, and Rust-verified hashes.
- [Phase 3]: A single-flight worker scheduler aborts superseded requests and atomically publishes only complete results whose request ID, source revision/hash, settings, font/data identities, and result hash still match; editor/session input remains independent.
- [Phase 3]: The page viewport is a derived visual projection over accepted Rust fixed-point fragments; the semantic DOM/input host remains the single authoring and accessibility surface.
- [Phase 3]: The executable Phase 3 gate covers exactly the local LAYO-01..08 rows, retains Phase 1/2 local regression lanes, emits allowlisted diagnostics, and records inherited Edge/Windows AT as unavailable/outstanding rather than substituting local Chromium evidence.
- [Phase 4]: Owned PDF output begins with a bounded Rust COS graph and fixed-point page envelope; legal page-tree `/Parent` back-references are allowed, arbitrary cycles/dangling references are rejected, and the first slice makes no selectable-text or compatibility claim.
- [Phase 4]: PDF/preview display data is a derived revision/hash/catalog-bound Rust projection; source ranges, glyph clusters, direction, fixed-point placements, and repeated-band provenance remain attached to every emitted text item.
- [Phase 4]: The first admitted PDF font profile is a deterministic bounded TrueType subset with `.notdef`/composite dependencies, compact glyph IDs, fixed metrics, format-12 cmap, and explicit ToUnicode mappings; TTC/CFF/malformed tables and unsupported glyph coverage fail closed.
- [Phase 4]: PDF image resources reuse revision-owned asset hashes and admission limits, deduplicate by stable content identity, and carry fixed-point display-list rectangles without copying bytes into semantic DTOs or diagnostics.
- [Phase 4]: Export structure is a closed typed subset: escaped metadata, deterministic internal outlines/links, and an explicit support report; JavaScript, launch/external actions, arbitrary annotations/paths, fields, and encryption are not inferred or emitted.
- [Phase 4]: Every owned export carries a versioned manifest and bounded private canonical-source envelope; exact recovery requires payload, canonical hash, source identity, revision, schema, and manifest checks, while external PDFs remain unavailable rather than best-effort exact.
- [Phase 4]: Browser PDF export is a string-only, bounded, single-flight derived protocol; request/source/layout/manifest identities and Rust byte verification must pass before immutable download state is published, and text revisions cancel/clear stale exports.
- [Phase 4]: The PDF preview is a visual-only projection of accepted Rust page geometry with a bounded virtualization window, source-backed search, selection projection, and status announcements; semantic DOM/input remains the sole authoring and accessibility surface.
- [Phase 4]: The release gate has twelve exact local rows and four separate reference rows; missing PDF fixtures/tools/viewers are `unavailable`, never a local or Chromium substitute, and the phase checkbox remains open until those evidence lanes are observed.
- [Phase 5]: Semantic fields remain canonical `FieldDescriptor`/anchor data; the first forms slice validates values and derives revision/hash-bound fixed-point widgets from accepted display-list source ranges, while invalid/deleted/unmapped anchors enter explicit review and no page/widget state is persisted.
- [Phase 5]: Plans 05-01 through 05-03 deliberately kept durable current-value records, AcroForm dictionaries/appearances, flattening, external-PDF form import, and target-viewer compatibility out of the projection/session slices; Plan 05-04 adds only bounded field/widget structure, with the remaining items still follow-on contracts.
- [Phase 5]: Form fill state is a noncanonical document-ID/revision/hash-bound override map; absent overrides resolve to authored defaults, set/clear operations are immutable generation changes, and read-only/forged/stale state fails closed. Durable browser persistence remains separate from canonical recovery and accepts only Rust-returned session DTOs.
- [Phase 5]: Version-2 derived widgets carry both authored `default_value` and effective session `value`; the default-only resolver remains deterministic, while session-aware projection rejects invalid session identity before placement and never changes canonical bytes.
- [Phase 5]: The first PDF form export slice accepts only a source-bound, hash-checked `PdfFormPlan`, emits bounded AcroForm field/widget/page/catalog structure, and keeps appearance streams, viewer behavior, import, and flattening as explicit follow-on evidence.
- [Phase 5]: The form-session action protocol is versioned and closed at Rust/WASM: Start, Validate, SetValue, and ClearValue decode one canonical source and return only a validated immutable session, never canonical mutations or PDF state.
- [Phase 5]: `form-sessions-v1` is a separate IndexedDB object store keyed by document identity/revision/source hash; exact retries are idempotent, divergent writes require the next generation, and canonical recovery records never include session values.
- [Phase 5]: The browser form-session coordinator subscribes to accepted editor snapshots, clears stale UI session state on source identity changes, serializes accepted mutations, and leaves canonical revision/history ownership with the editor controller.
- [Phase 5]: Already-authored valid fields use native semantic controls for text, checkbox, radio, and select input; Rust descriptor metadata remains authoritative, signature/button controls stay explicitly non-editable, and review fields remain read-only.

### Pending Todos

- Verify and pin the fixed Vitest Browser Mode family through the dependency-provenance workflow before any non-local Browser Mode exposure; see Phase 1 `deferred-items.md`.

### Blockers/Concerns

- The Rust text/font stack requires an early differential benchmark against HarfBuzz reference behavior.
- The refreshed WASM size artifact now names its current-source probe semantics explicitly and passes the numeric Phase 2 budget; it does not claim a fresh Phase-1-compatible delta. Keep the historical Phase 1 admission evidence unchanged in git history. See 02-03-SUMMARY.md.
- External Microsoft Edge on Windows plus a Windows screen-reader run remains unavailable/outstanding; Phase 2 is not marked complete by local evidence.

## Deferred Items

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| Compatibility | Advanced layout, encryption, signatures, PDF/A, PDF/UA and broad malformed-PDF repair | v2 | Initialization |
| Security | Verify and pin the fixed Vitest Browser Mode family | Open | Phase 1 Plan 06 |

## Session Continuity

Last session: 2026-09-21T22:17:41Z
Stopped at: Completed 05-06-PLAN.md; Phase 5 accessible controls and session coordination are green, Phase 4 reference PDF rows are unavailable, and the inherited Phase 2/3 external AT checkpoint remains outstanding
Resume file: None

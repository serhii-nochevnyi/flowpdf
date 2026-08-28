# Phase 2 Multi-Source Coverage Audit

**Revised:** 2026-08-28
**Plan graph:** 18 sequential vertical plans, 36 tasks, waves 1–18; every plan modifies at most 9 files.

## GOAL coverage

| Goal / observable truth | Status | Revised plan/task coverage |
|---|---|---|
| As a document author, I want to edit structured Ukrainian and English text, so that I can create accessible documents. | COVERED | Exact roadmap story is preserved across the plan set; production tracer 02-05, expansion 02-06..17, final gate 02-18. |
| Grapheme-safe caret/selection/editing | COVERED | 02-02-01, 02-04-01..02, 02-05-01, 02-06-01..02. |
| Paragraphs, headings, lists, images, complete simple tables/header/remove, page breaks and formatting | COVERED | 02-07..14. |
| Keyboard, visible controls, and synchronized semantic accessibility for every mutation | COVERED | product slices 02-06, 02-08, 02-10, 02-12, 02-14; exhaustive parity 02-15. |
| Screen-reader navigation of content, fields, statuses, errors, and confirmations | COVERED LOCALLY / EXTERNAL OUTSTANDING | 02-16..17; Edge/Windows/screen-reader remains unavailable/outstanding. |

## REQ coverage

| Requirement | Status | Revised plan/task coverage |
|---|---|---|
| EDIT-01 | COVERED | 02-02-01, 02-03-01..02, 02-04-01..02, 02-05-01, 02-06-01, 02-07-01. |
| EDIT-02 | COVERED | 02-05-01..02, 02-06-01..02, 02-08. |
| EDIT-03 | COVERED | 02-07-01..02, 02-08-01..02, 02-09-02. |
| EDIT-04 | COVERED | 02-09-01..02, 02-10-01..02. |
| EDIT-05 | COVERED | 02-03-02, 02-11..14 including optional table header, RemoveTable, ReplaceImage, SetImageAccessibility, and RemoveImage. |
| QUAL-03 | COVERED | per-slice visible/keyboard assertions and exhaustive 02-15-01; P2-01 remains flagged until execution. |
| QUAL-04 | COVERED LOCALLY / EXTERNAL EVIDENCE OUTSTANDING | native semantics throughout; field/a11y/UI/AT closure 02-16..17; external platform remains outstanding. |

Requirement equality: **7 planned / 7 required / 0 extra / 0 missing**.

## CONTEXT decision coverage

| Decision | Status | Revised plan/task coverage |
|---|---|---|
| D-01 directional nodeId/UTF-16/affinity positions | COVERED | 02-04-01..02, 02-05-01, 02-06-01, 02-07. |
| D-02 pinned Rust grapheme authority/no snapping | COVERED | 02-01..02 dependency/data gates, 02-04-01, 02-06-02, 02-07-01. |
| D-03 controlled input + accepted semantic DOM | COVERED | 02-05-02, 02-06-01..02, 02-15-02, 02-16-02. |
| D-04 transient explicit-cancel IME/plain-text paste | COVERED | 02-06-02. |
| D-05 one-way schema v2 + explicit migration | COVERED | 02-03-01 executable one-way freeze before mutation; 02-03-02 executes all routes before persistence/tracer. |
| D-06 closed semantic block kinds | COVERED | 02-03-02, 02-07..14. |
| D-07 inline/block formatting + pending/mixed state | COVERED | 02-04-02, 02-09, 02-10. |
| D-08 split/merge/Enter/list rules | COVERED | 02-07..09. |
| D-09 editable paragraph + explicit atomic commands | COVERED | 02-07, 02-11..14. |
| D-10 centered page-neutral product UI | COVERED | 02-05-02, 02-10, 02-12, 02-14, 02-16..17. |
| D-11 visible/keyboard one command bus | COVERED | every browser slice; exhaustive 02-15-01. |
| D-12 Rust/React ownership + retained Inspector | COVERED | 02-02-02, 02-05..06, 02-08, 02-15-02. |
| D-13 native semantic tree/live status/alerts | COVERED | 02-05-02, 02-06, 02-08, 02-12, 02-14, 02-16. |
| D-14 bounded accessible PNG/JPEG full lifecycle | COVERED | 02-13 core staging/Insert/Replace/SetAccessibility/Remove/reachability; 02-14 WASM/storage/UI/confirmation/recovery. |
| D-15 bounded simple tables/header/remove + page break | COVERED | 02-11 core optional header/SetTableHeaderRow/RemoveTable/inverses; 02-12 native thead/th scope/controls/confirmation. |
| D-16 Ukrainian default/key-identical English | COVERED | browser UI plans 02-05..17; exact state/locale closure 02-17. |

Decision equality: **16 planned / 16 locked / 0 missing**.

## RESEARCH coverage

| Research feature/constraint | Status | Revised plan/task coverage |
|---|---|---|
| Package legitimacy/provenance/integrity, exact pins, no lifecycle bypass | COVERED | 02-01. |
| Official Unicode data + one numerical `verify-wasm-size.mjs` | COVERED | 02-02. |
| Frozen private legacy decoders and sequential migration/no-op | COVERED | 02-03-01..02. |
| ICU4X grapheme map, atomic sentinels, directional Rust session | COVERED | 02-04. |
| Production core/WASM/persistence/minimal React tracer | COVERED | 02-05. |
| Native input/selection/IME state machine | COVERED | 02-06; resolved research Q1/Q3. |
| Exact structural algebra/tombstones/private preimage recovery | COVERED | 02-07..08. |
| Closed formatting/list session and UI | COVERED | 02-09..10. |
| Complete simple table/page-break semantics | COVERED | 02-11..12. |
| Hostile images, staging, canonical BLAKE3, shared reachability, full lifecycle | COVERED | 02-13..14; resolved Q4. |
| One atomic document+transaction+audit+assets+head commit | COVERED | 02-14-01. |
| Capability/parity and retained boundaries | COVERED | 02-15. |
| Existing fields/native accessibility and external evidence honesty | COVERED LOCALLY / OUTSTANDING EXTERNALLY | 02-16..17. |
| 100–200-page bounded semantic scale and every Phase 1 regression | COVERED | 02-18. |
| Resolved style migration choice | COVERED | research Q2 -> 02-03-02. |

## Spec-less explicit coverage

| Source set | Expected | Planned | Missing/backstop/dismissed | Location |
|---|---:|---:|---:|---|
| Probe edge predicates | 33 | 33 | 0 | must_haves: EDIT-01 4 in 02-04; EDIT-02 5 in 02-06; EDIT-03 6 in 02-07; EDIT-04 7 in 02-09; EDIT-05 7 across 02-11/13; QUAL-03 3 in 02-15; QUAL-04 1 in 02-16. |
| UI surface/state considerations | 108 | 108 | 0 | 02-17-01 exact 108/108/0/0/0 generator/contract; 02-17-02 concrete browser/evidence owners. |
| Descriptor-less prohibitions | 3 | 3 | 0 | P2-02 in 02-13, P2-01 in 02-15, P2-03 in 02-16; all `unverified`, `flagged: true`, original test/judgment tier, no check descriptor. |

## Runtime state coverage

| State class | Plan |
|---|---|
| Stored IndexedDB snapshots/transactions/recovery/assets | 02-03 migration, 02-05 tracer, 02-07 recovery, 02-13..14 asset lifecycle, 02-18 regression. |
| Live service configuration | None in Phase 2. |
| OS-registered state | None. |
| Secrets/environment variables | None; dependency verification uses credential-free official metadata. |
| Build/WASM/generated caches | 02-01..02 rebuild and measurement; no data migration. |

## Explicit exclusions (not gaps)

- Phase 3 shaping, fallback, BiDi, line breaking, hyphenation, fragment caret geometry, reflow, pagination, and virtualization.
- Later image crop/resize/floating/wrap and complex/merged/nested/fragmented tables.
- Later rich HTML/office clipboard, comments, track changes, collaboration, spelling/grammar.
- Dedicated roadmap phases for field authoring/filling, PDF import/export, voice capture/recognition, and backend services. Phase 2 keeps mutation parity and navigation of existing fields only.

## Audit result

All GOAL, REQ, RESEARCH, and non-deferred CONTEXT items are covered by executable revised plans. There are no planner-authorized scope reductions, missing source items, or unowned review findings.

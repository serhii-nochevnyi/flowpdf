# Phase 2 — Plan Checker Findings

**Checked:** 2026-08-28
**Verdict:** ISSUES FOUND — revision required before execution

## Blockers

1. `02-02-01` labels the schema-v2 boundary `costly` and compiles its freeze test with `--no-run`; D-05 is one-way and the legacy byte/hash/decoder gate must execute before schema/model mutation. Migration route tests must execute before v2 persistence/tracer work.
2. `02-VALIDATION.md` contains stale/unplanned/future-wave commands: `verify-unicode-corpus.mjs`, `check-release-wasm-size.mjs`, and rich-text tests before their owning plan. Every row must exactly match its task's `<automated>` command and ownership; the canonical size script is `scripts/verify-wasm-size.mjs`.
3. D-14/EDIT-05 lack Rust-owned image replacement, description/accessibility editing, and removal. Add `ReplaceImage`, `SetImageAccessibility`, and `RemoveImage` with capabilities, exact inverses, asset reachability, atomic persistence, visible keyboard-accessible controls, confirmation, and core/browser tests.
4. D-15 lacks optional header-row state and whole-table removal. Add insertion/header state, `SetTableHeaderRow`, `RemoveTable`, exact inverse/capability/confirmation, and native `thead`/`th scope` verification.
5. `02-RESEARCH.md` questions Q1–Q4 are not formally marked resolved. Rename the section and record the selected answer plus implementing plan/task for each.
6. The image plan calls canonical asset hashing SHA-256, conflicting with live `canonical::asset_hash` and the `blake3:` contract. Retain BLAKE3 as canonical `content_hash`; any receipt digest must be separately named and noncanonical.
7. Plan 03 modifies 16 files. Split core/WASM/persistence/minimal paragraph tracer from input-host/selection/IME expansion.
8. Plan 08 modifies 21 files. Split parity/boundary verification, existing-field/accessibility/UI closure, and final scale/regression gate.

## Warnings

1. Other plans modify 10–14 files; reduce further where ownership boundaries permit without breaking vertical slices.
2. The final release task uses only the full ≤120-second suite for immediate feedback; add a fast targeted smoke verifier and retain the full terminal gate separately.

## Already Passing

- Requirement IDs 7/7, D-01–D-13 and D-16, linear acyclic dependency graph, and vertical tracer intent.
- Task XML 23/23, threat models, artifact inventories, tombstone/`AnchorPreimage`, Rust session ownership, IME empty-data semantics, formatting bounds, staged binary persistence, existing-field projection, and Phase 1 boundary intent.
- Coverage equality: edge 33/33, UI 108/108, prohibitions 3/3.
- External Edge/Windows/screen-reader evidence is honestly outstanding; no human approval or external-service API matrix exists.

## Revision Resolution — 2026-08-28

| Finding | Status | Exact revised resolution |
|---|---|---|
| B1 one-way schema-v2 gate was `costly` and compile-only | RESOLVED | **02-03-01** modifies only frozen legacy decoder/test/fixtures and executes `legacy_freeze_gate` without `--no-run` before mutation. **02-03-02** carries `reversibility="one-way"` for the already user-chosen/locked D-05 decision, mutates current model/schema only after that gate, then executes `migration_routes` for v0→v1→v2, v1→v2, and v2 no-op plus schema/persistence tests before dependency **02-04** and all persistence/tracer work. No new human checkpoint re-asks the already locked choice. |
| B2 VALIDATION stale/future-wave commands and inconsistent script names | RESOLVED | `02-VALIDATION.md` now has **36/36** rows equal to the decoded owning `<automated>` commands; every referenced file is prior-wave or created by the same task. Unicode uses only `scripts/verify-unicode-corpus.mjs`; size uses only `scripts/verify-wasm-size.mjs`; no rich-text test is called before its owner. |
| B3 missing image replacement/accessibility editing/removal lifecycle | RESOLVED | **02-13-02** owns Rust `ReplaceImage`, `SetImageAccessibility`, `RemoveImage`, capabilities/risk/confirmation, exact inverses, shared/reachable asset rules, atomic plan and recovery tests. **02-14-01** owns WASM/IndexedDB atomicity; **02-14-02** owns visible keyboard-accessible Replace/Change description/Delete controls, confirmation/focus, and browser/recovery lifecycle. |
| B4 missing table optional header and whole-table removal | RESOLVED | **02-11-02** owns insertion header state, `SetTableHeaderRow`, `RemoveTable`, exact inverses/capabilities/confirmation, last-row/column escalation, and Rust tests. **02-12-01** verifies native `thead`/`th scope=col`; **02-12-02** verifies controls, confirmation/focus, parity, undo/reload/recovery. |
| B5 RESEARCH Q1–Q4 unresolved | RESOLVED | Heading is `## Open Questions (RESOLVED)`; Q1 maps to **02-06-01/02, 02-16-02, 02-17-02**; Q2 to **02-03-02**; Q3 to **02-06-02, 02-18-02**; Q4 to **02-14-02**. |
| B6 image plan changed canonical hash to SHA-256 | RESOLVED | **02-13-01** requires the existing `canonical::asset_hash` lowercase `blake3:` contract and tests rejection of canonical SHA-256 substitution. Any optional receipt-only value is separately named noncanonical `staging_digest`; **02-14-01** re-verifies legacy/new physical records against canonical BLAKE3. |
| B7 old Plan 03 had 16 files | RESOLVED | Split into **02-05** core/WASM/persistence/minimal React paragraph tracer (9 files) and **02-06** accepted controller/input-host/selection/IME expansion (8 files). |
| B8 old Plan 08 had 21 files | RESOLVED | Split into **02-15** parity/boundary (6 files), **02-16** existing-field/semantic accessibility (7), **02-17** exact UI/AT closure (8), and **02-18** scale/regression terminal gate (7). |
| W1 six other plans had 10–14 files | RESOLVED | Re-sliced the full graph into **18 plans**, each with **≤9 files**: dependencies, Unicode/size, migration, grapheme/session, tracer, input, structural core/UI, formatting core/UI, table core/UI, image core/persistence+UI, parity, field/a11y, UI/AT, final scale/gate. |
| W2 terminal task lacked fast feedback | RESOLVED | **02-18-01** creates `check:phase2:smoke`; terminal **02-18-02** runs the exact command `npm run check:phase2:smoke && npm run check:phase2 && npm run check:phase2`, retaining both full release passes. |

**Resolution equality:** 8/8 blockers resolved, 2/2 warnings resolved, 0 deferred, 0 rejected.

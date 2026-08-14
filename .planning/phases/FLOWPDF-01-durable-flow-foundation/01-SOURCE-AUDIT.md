# Phase 1 — Multi-Source Coverage Audit

**Audited:** 2026-08-14  
**Result:** complete — no unplanned source item

## Goal and Requirements

| Source | ID | Feature / requirement | Plan(s) | Status | Notes |
|---|---|---|---|---|---|
| GOAL | — | Versioned, recoverable FlowDocument and transactional foundation | 01-02 through 01-06 | COVERED | Early real-browser skeleton, then semantic expansions and final proof. |
| REQ | FLOW-01 | Create versioned document with settings, locale, styles, content, assets, fields | 01-02, 01-03, 01-06 | COVERED | Full typed schema and browser sample. |
| REQ | FLOW-02 | Save/reopen without loss | 01-02, 01-03, 01-05, 01-06 | COVERED | Canonical/asset and native/browser persistence evidence. |
| REQ | FLOW-03 | Deterministic supported migrations | 01-03, 01-05, 01-06 | COVERED | Pure golden registry and migration replay boundary. |
| REQ | FLOW-04 | Recover last durable revision after refresh/worker failure | 01-02, 01-05, 01-06 | COVERED | Core verified replay plus actual IndexedDB interruption/reload. |
| REQ | FLOW-05 | Inspect revision and provenance | 01-02, 01-05, 01-06 | COVERED | Exact revision/create/migration lineage and unavailable-export disclosure. |
| REQ | EDIT-06 | Atomic undo/redo for committed operation categories | 01-04, 01-06 | COVERED | Same command boundary plus stateful properties/browser controls. |
| REQ | EDIT-07 | Deterministic stale/invalid conflict | 01-04, 01-06 | COVERED | Strict revision/anchor validation and unchanged-state proof. |
| REQ | QUAL-08 | Concise privacy-minimized audit history | 01-02, 01-05, 01-06 | COVERED | Closed Rust audit DTO and browser/accessibility absence checks. |

## Locked Decisions

| Source | ID | Decision | Plan(s) | Status |
|---|---|---|---|---|
| CONTEXT | D-01 | Canonical deterministic JSON/schemaVersion/stable IDs/no coordinates | 01-02, 01-03 | COVERED |
| CONTEXT | D-02 | Hash-addressed external asset bytes | 01-02, 01-03, 01-05 | COVERED |
| CONTEXT | D-03 | Package/binary derived; semantic model remains truth | 01-03 | COVERED |
| CONTEXT | D-04 | Pure sequential golden migrations; future version fails | 01-03, 01-05 | COVERED |
| CONTEXT | D-05 | Typed command → atomic immutable transaction/inverse | 01-02, 01-04 | COVERED |
| CONTEXT | D-06 | One boundary for every modality; no adapter mutation | 01-02, 01-04, 01-06 | COVERED |
| CONTEXT | D-07 | Stale/invalid/duplicate/invariant failures are non-mutating | 01-04, 01-06 | COVERED |
| CONTEXT | D-08 | Undo/redo are revisioned transactions restoring hashes | 01-04, 01-06 | COVERED |
| CONTEXT | D-09 | Public nodeId + UTF-16 offset + affinity | 01-04 | COVERED |
| CONTEXT | D-10 | Explicit surviving mappings; deleted unmapped is invalid | 01-04 | COVERED |
| CONTEXT | D-11 | No DOM/absolute/page/PDF canonical anchors | 01-03, 01-04, 01-06 | COVERED |
| CONTEXT | D-12 | Rust DocumentStore snapshot/log port; adapters physical only | 01-02, 01-05 | COVERED |
| CONTEXT | D-13 | IndexedDB newest-valid snapshot + verified idempotent replay | 01-02, 01-05, 01-06 | COVERED |
| CONTEXT | D-14 | Policy/benchmark snapshot cadence; correctness interval-independent | 01-05 | COVERED |
| CONTEXT | D-15 | Linked redacted audit, no raw audio/text default retention | 01-02, 01-05, 01-06 | COVERED |

## Research Features and Constraints

| Source | Feature / constraint | Plan(s) | Status |
|---|---|---|---|
| RESEARCH | Rust core + narrow WASM DTO ownership map | 01-01, 01-02, 01-03, 01-04, 01-05, 01-06 | COVERED |
| RESEARCH | Official package legitimacy before installs | 01-01 | COVERED — automated official registry/repo gate replaces prohibited human checkpoint and records a blocker on failure. |
| RESEARCH | Typed structs/ordered collections/one compact canonical serializer/BLAKE3 diagnostic | 01-03 | COVERED |
| RESEARCH | Sequential migrations and new replay boundary | 01-03, 01-05 | COVERED |
| RESEARCH | Pre-state inverses, explicit UTF-16 conversions, no guessed target | 01-04 | COVERED |
| RESEARCH | Native IndexedDB transaction lifetime/completion correctness | 01-02, 01-06 | COVERED |
| RESEARCH | Bounded typed decode/recovery work | 01-03, 01-05 | COVERED |
| RESEARCH | Separate transaction record versus audit DTO | 01-05 | COVERED |
| RESEARCH | Ukrainian/English, combining, emoji, non-BMP fixtures | 01-02, 01-03, 01-04 | COVERED |
| RESEARCH | Property/golden/fake adapter plus real browser validation | 01-03 through 01-06 | COVERED |
| RESEARCH | No React/Vite application, runtime IDB wrapper, layout/PDF/forms/voice/backend/auth | 01-02, 01-06 boundary tests | COVERED |
| UI-SPEC | Complete Foundation Inspector design/copy/accessibility/state contract | 01-02, 01-06 | COVERED |

Deferred ideas and later-phase capabilities are excluded, not gaps.

## Spec-Less Edge Probe Audit

Command executed:

```bash
node /Users/serhii/.codex/gsd-core/bin/lib/edge-probe.cjs work/phase1-edge-requirements.json
```

The probe returned 36 applicable, 36 unresolved classified rows. Autonomous resolution authored a concrete acceptance truth for every row; none required a backstop and none was dismissed.

| Requirement | Surfaced categories | Authored location | Count |
|---|---|---|---:|
| FLOW-01 | adjacency, empty, encoding, ordering, idempotency, concurrency | 01-03 `must_haves.truths` | 6 |
| FLOW-02 | adjacency, empty, encoding, ordering, idempotency, concurrency | 01-05 `must_haves.truths` | 6 |
| FLOW-03 | idempotency, concurrency | 01-03 `must_haves.truths` | 2 |
| FLOW-04 | idempotency, concurrency | 01-05 `must_haves.truths` | 2 |
| FLOW-05 | adjacency, empty, ordering, idempotency, concurrency | 01-06 `must_haves.truths` | 5 |
| EDIT-06 | adjacency, empty, ordering, idempotency, concurrency | 01-04 `must_haves.truths` | 5 |
| EDIT-07 | empty, encoding, idempotency, concurrency | 01-04 `must_haves.truths` | 4 |
| QUAL-08 | adjacency, empty, encoding, ordering, idempotency, concurrency | 01-05 `must_haves.truths` | 6 |

**No-silent-drop equality:** 36 surfaced = 36 authored truths + 0 backstops + 0 flagged assumptions + 0 dismissals.

## Prohibition Recall → Precision Audit

Stage 1 adversarially recalled 78 raw must-NOT candidates across the eight requirements. Stage 2 removed 69 routine correctness/hygiene candidates already owned by the edge tests or code review and referred five canon items to the applicable threat models. Four bespoke privacy/transparency/safety/data-integrity prohibitions remained and were authored descriptor-less as required for autonomous spec-less fallback.

| Requirement | Kept bespoke prohibition | Category | Authored location |
|---|---|---|---|
| FLOW-04 | Never label an unverified/gapped/corrupt revision recovered or durable | transparency / safety | 01-05 `must_haves.prohibitions` |
| FLOW-05 | Never fabricate PDF preview/export provenance | transparency | 01-06 `must_haves.prohibitions` |
| EDIT-07 | Never silently rebase/guess/move a stale or invalid target while reporting success | safety / data integrity | 01-04 `must_haves.prohibitions` |
| QUAL-08 | Never retain/expose semantic text, command arguments, raw audio, or transcripts in default audit/analytics/inspection | privacy | 01-05 `must_haves.prohibitions` |

Canon-referral breadcrumbs:

- Generic dependency supply-chain spoofing/tampering → Plan 01 threat model `T-01-SC`/`T-01-02`; not minted.
- Generic malformed/oversized input and injection-like decode concerns → Plan 03/05 ASVS V5 threat registers; not minted.
- Generic persisted-record tampering/denial of service → Plan 05 threat model `T-05-01`/`T-05-02`; not minted.
- Generic GDPR/data-retention/consent canon → Plan 05 information-disclosure threat and project privacy requirement; not minted as a second prohibition.
- Generic DOM injection/accessibility security hygiene → Plan 06 threat model/tests; not minted.

**Precision equality:** 78 recalled = 69 routine drops + 5 canon referrals + 4 kept.  
**Projection equality:** 4 kept = 4 authored descriptor-less prohibitions + 0 silently dropped.  
No prohibition is auto-dismissed; absent descriptors intentionally dispose as flagged-unverified downstream.

